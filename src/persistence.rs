//! Coalescing single-threaded background persistence worker for local JSON stores.
//!
//! Each domain slot (`History`, `Library`, `Queue`) keeps only the latest pending
//! snapshot. Rapid updates while a write is in flight coalesce in place so disk
//! I/O and `file.sync_all()` never block the UI thread and never accumulate an
//! unbounded queue.
//!
//! External-modification detection compares `(mtime, len)` via `FileFingerprint`
//! against the last known good load/save. When disk metadata differs, the slot's
//! validator runs before writing so corrupt JSON files on disk are preserved
//! untouched and reported to the UI via `poll_errors()` / `poll_detailed_errors()`.

use crate::history::{self, FileFingerprint, History};
use crate::library::{self, Library};
use crate::queue::Queue;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

#[allow(dead_code)]
pub type DiskValidator = Box<dyn Fn(&Path) -> io::Result<()> + Send + 'static>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum PersistenceSlot {
    History,
    Library,
    Queue,
}

impl PersistenceSlot {
    const COUNT: usize = 3;

    const fn index(self) -> usize {
        match self {
            Self::History => 0,
            Self::Library => 1,
            Self::Queue => 2,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::History => "history",
            Self::Library => "library",
            Self::Queue => "queue",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub struct PersistenceError {
    pub slot: PersistenceSlot,
    pub path: PathBuf,
    pub message: String,
    pub corrupt_file: bool,
}

enum SlotPayload {
    History(History),
    Library(Library),
    Queue(Queue),
    Raw {
        bytes: Vec<u8>,
        validator: Option<DiskValidator>,
    },
}

struct PendingWrite {
    slot: PersistenceSlot,
    path: PathBuf,
    payload: SlotPayload,
    expected_fingerprint: Option<FileFingerprint>,
}

#[derive(Default)]
struct SharedState {
    slots: [Option<PendingWrite>; PersistenceSlot::COUNT],
    in_flight: bool,
    last_known: [Option<(PathBuf, Option<FileFingerprint>)>; PersistenceSlot::COUNT],
    errors: Vec<PersistenceError>,
    completed_writes: u64,
    shutdown: bool,
}

struct WorkerShared {
    state: Mutex<SharedState>,
    work_cv: Condvar,
    idle_cv: Condvar,
}

/// Dedicated coalescing persistence worker. Dropping the worker flushes every
/// pending slot cleanly before joining the background thread.
#[allow(dead_code)]
pub struct PersistenceWorker {
    shared: Arc<WorkerShared>,
    handle: Option<JoinHandle<()>>,
}

impl Default for PersistenceWorker {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl PersistenceWorker {
    pub fn new() -> Self {
        let shared = Arc::new(WorkerShared {
            state: Mutex::new(SharedState::default()),
            work_cv: Condvar::new(),
            idle_cv: Condvar::new(),
        });
        let bg = Arc::clone(&shared);
        let handle = thread::Builder::new()
            .name("fasttube-persistence".to_owned())
            .spawn(move || worker_loop(bg))
            .ok();
        Self { shared, handle }
    }

    /// Records the last known-good on-disk fingerprint after loading a store or
    /// changing its backing path so subsequent saves can skip full re-parsing
    /// unless the file changes externally.
    pub fn record_loaded_fingerprint(
        &self,
        slot: PersistenceSlot,
        path: PathBuf,
        fingerprint: Option<FileFingerprint>,
    ) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.last_known[slot.index()] = Some((path, fingerprint));
    }

    /// Returns the most recently observed good fingerprint for `slot`, updated
    /// after every successful background write.
    pub fn last_fingerprint(&self, slot: PersistenceSlot) -> Option<FileFingerprint> {
        let state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.last_known[slot.index()]
            .as_ref()
            .and_then(|(_, fp)| *fp)
    }

    pub fn schedule_history(
        &self,
        path: PathBuf,
        history: History,
        expected_fingerprint: Option<FileFingerprint>,
    ) {
        self.enqueue(PendingWrite {
            slot: PersistenceSlot::History,
            path,
            payload: SlotPayload::History(history),
            expected_fingerprint,
        });
    }

    pub fn schedule_library(
        &self,
        path: PathBuf,
        library: Library,
        expected_fingerprint: Option<FileFingerprint>,
    ) {
        self.enqueue(PendingWrite {
            slot: PersistenceSlot::Library,
            path,
            payload: SlotPayload::Library(library),
            expected_fingerprint,
        });
    }

    pub fn schedule_queue(
        &self,
        path: PathBuf,
        queue: Queue,
        expected_fingerprint: Option<FileFingerprint>,
    ) {
        self.enqueue(PendingWrite {
            slot: PersistenceSlot::Queue,
            path,
            payload: SlotPayload::Queue(queue),
            expected_fingerprint,
        });
    }

    /// Schedules a pre-serialized payload with an optional external-change
    /// corrupt-file validator hook.
    pub fn schedule_bytes(
        &self,
        slot: PersistenceSlot,
        path: PathBuf,
        bytes: Vec<u8>,
        expected_fingerprint: Option<FileFingerprint>,
        validator: Option<DiskValidator>,
    ) {
        self.enqueue(PendingWrite {
            slot,
            path,
            payload: SlotPayload::Raw { bytes, validator },
            expected_fingerprint,
        });
    }

    /// Non-blocking drain of formatted persistence error messages for UI display.
    pub fn poll_errors(&self) -> Vec<String> {
        self.poll_detailed_errors()
            .into_iter()
            .map(|err| err.message)
            .collect()
    }

    /// Non-blocking drain of structured persistence errors (slot, path, corrupt flag).
    pub fn poll_detailed_errors(&self) -> Vec<PersistenceError> {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        std::mem::take(&mut state.errors)
    }

    pub fn has_pending(&self) -> bool {
        let state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.in_flight || state.slots.iter().any(Option::is_some)
    }

    pub fn completed_writes(&self) -> u64 {
        let state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.completed_writes
    }

    /// Blocks until every currently scheduled snapshot and any in-flight write
    /// have completed.
    pub fn flush(&self) {
        if self.handle.is_none() {
            self.drain_inline();
            return;
        }
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        while state.in_flight || state.slots.iter().any(Option::is_some) {
            state = self
                .shared
                .idle_cv
                .wait(state)
                .unwrap_or_else(|poison| poison.into_inner());
        }
    }

    fn enqueue(&self, job: PendingWrite) {
        if self.handle.is_none() {
            // Fallback when thread spawn fails: run synchronously so writes are never lost.
            let expected = {
                let state = self
                    .shared
                    .state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                resolve_expected(&state.last_known[job.slot.index()], &job)
            };
            let outcome = execute_write(&job, expected);
            let mut state = self
                .shared
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            apply_outcome(&mut state, job.slot, job.path, outcome);
            return;
        }
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let slot_idx = job.slot.index();
        state.slots[slot_idx] = Some(job);
        self.shared.work_cv.notify_one();
    }

    fn drain_inline(&self) {
        loop {
            let next = {
                let mut state = self
                    .shared
                    .state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                take_next_slot(&mut state.slots).map(|job| {
                    let expected = resolve_expected(&state.last_known[job.slot.index()], &job);
                    (job, expected)
                })
            };
            let Some((job, expected)) = next else {
                break;
            };
            let outcome = execute_write(&job, expected);
            let mut state = self
                .shared
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            apply_outcome(&mut state, job.slot, job.path, outcome);
        }
    }
}

impl Drop for PersistenceWorker {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            {
                let mut state = self
                    .shared
                    .state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                state.shutdown = true;
                self.shared.work_cv.notify_all();
            }
            let _ = handle.join();
        } else {
            self.drain_inline();
        }
    }
}

fn take_next_slot(
    slots: &mut [Option<PendingWrite>; PersistenceSlot::COUNT],
) -> Option<PendingWrite> {
    for slot in slots.iter_mut() {
        if let Some(job) = slot.take() {
            return Some(job);
        }
    }
    None
}

fn resolve_expected(
    last_known: &Option<(PathBuf, Option<FileFingerprint>)>,
    job: &PendingWrite,
) -> Option<FileFingerprint> {
    if let Some((known_path, known_fp)) = last_known
        && known_path == &job.path
    {
        return *known_fp;
    }
    job.expected_fingerprint
}

fn apply_outcome(
    state: &mut SharedState,
    slot: PersistenceSlot,
    path: PathBuf,
    outcome: Result<Option<FileFingerprint>, PersistenceError>,
) {
    match outcome {
        Ok(new_fp) => {
            state.last_known[slot.index()] = Some((path, new_fp));
            state.completed_writes = state.completed_writes.saturating_add(1);
        }
        Err(err) => {
            state.errors.push(err);
        }
    }
}

fn worker_loop(shared: Arc<WorkerShared>) {
    loop {
        let next_job = {
            let mut state = shared
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            loop {
                if let Some(job) = take_next_slot(&mut state.slots) {
                    state.in_flight = true;
                    let expected = resolve_expected(&state.last_known[job.slot.index()], &job);
                    break Some((job, expected));
                }
                state.in_flight = false;
                shared.idle_cv.notify_all();
                if state.shutdown {
                    break None;
                }
                state = shared
                    .work_cv
                    .wait(state)
                    .unwrap_or_else(|poison| poison.into_inner());
            }
        };

        let Some((job, expected)) = next_job else {
            break;
        };

        let outcome = execute_write(&job, expected);
        {
            let mut state = shared
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            apply_outcome(&mut state, job.slot, job.path, outcome);
            state.in_flight = false;
            if state.slots.iter().all(Option::is_none) {
                shared.idle_cv.notify_all();
            }
        }
    }
}

fn execute_write(
    job: &PendingWrite,
    expected: Option<FileFingerprint>,
) -> Result<Option<FileFingerprint>, PersistenceError> {
    let make_err = |error: io::Error| PersistenceError {
        slot: job.slot,
        path: job.path.clone(),
        corrupt_file: error.kind() == io::ErrorKind::InvalidData,
        message: format!(
            "Failed to persist {} at '{}': {error}",
            job.slot.label(),
            job.path.display()
        ),
    };

    // Fast external-change guard: skip reading/parsing disk JSON when `(mtime, len)`
    // still matches our last known good load/save.
    let current_fp = history::file_fingerprint(&job.path);
    if current_fp != expected && current_fp.is_some() {
        validate_existing_disk_file(job).map_err(make_err)?;
    }

    let bytes = serialize_payload(&job.payload).map_err(make_err)?;
    history::write_atomic(&job.path, &bytes).map_err(make_err)?;
    Ok(history::file_fingerprint(&job.path))
}

fn validate_existing_disk_file(job: &PendingWrite) -> io::Result<()> {
    match &job.payload {
        SlotPayload::History(_) => history::load(&job.path).map(|_| ()),
        SlotPayload::Library(_) => library::load(&job.path).map(|_| ()),
        SlotPayload::Queue(queue) => Queue::load(&job.path, queue.limit()).map(|_| ()),
        SlotPayload::Raw {
            validator: Some(validate),
            ..
        } => validate(&job.path),
        SlotPayload::Raw {
            validator: None, ..
        } => Ok(()),
    }
}

fn serialize_payload(payload: &SlotPayload) -> io::Result<Vec<u8>> {
    match payload {
        SlotPayload::History(hist) => history::serialize_bytes(hist),
        SlotPayload::Library(lib) => library::serialize_bytes(lib),
        SlotPayload::Queue(queue) => queue.serialize_bytes(),
        SlotPayload::Raw { bytes, .. } => Ok(bytes.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::Entry as HistoryEntry;
    use crate::search::Video;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;

    static TEST_DIR_SEQ: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fasttube-persistence-{prefix}-{}-{}",
            std::process::id(),
            TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_history_entry(n: usize, pos_ms: u64) -> HistoryEntry {
        HistoryEntry {
            video_id: format!("{n:011}"),
            title: format!("Video {n}"),
            channel: "Channel".into(),
            position_ms: pos_ms,
            duration_ms: Some(120_000),
            watched_at_ms: Some(1_700_000_000_000 + pos_ms),
            ..Default::default()
        }
    }

    fn sample_video(id: &str) -> Video {
        Video {
            title: format!("Title {id}"),
            channel: "Channel".into(),
            url: format!("https://www.youtube.com/watch?v={id}"),
            thumbnail: String::new(),
            duration: "3:00".into(),
        }
    }

    #[test]
    fn persistence_worker_coalesces_rapid_updates_to_latest_snapshot() {
        let dir = temp_dir("coalesce");
        let path = dir.join("history.json");
        let gate_path = dir.join("gate.json");
        fs::write(&gate_path, b"{}").unwrap();

        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);

        let worker = PersistenceWorker::new();
        // Hold the worker busy inside a validator hook so rapid history schedules
        // deterministically coalesce in the History slot before it gets picked up.
        worker.schedule_bytes(
            PersistenceSlot::Library,
            gate_path,
            b"{\"ok\":true}".to_vec(),
            None,
            Some(Box::new(move |_| {
                let _ = entered_tx.send(());
                let _ = release_rx.lock().unwrap().recv();
                Ok(())
            })),
        );
        entered_rx.recv().unwrap();

        let mut hist = History::default();
        for i in 1..=25u64 {
            hist.record(sample_history_entry(1, i * 1_000)).unwrap();
            worker.schedule_history(path.clone(), hist.clone(), None);
        }
        let _ = release_tx.send(());
        worker.flush();

        assert!(worker.poll_errors().is_empty());
        // 1 gate write + 1 coalesced history write = 2 total writes instead of 26.
        assert_eq!(worker.completed_writes(), 2);
        let loaded = history::load(&path).unwrap();
        assert_eq!(loaded.get("00000000001").unwrap().position_ms, 25_000);
        assert!(worker.last_fingerprint(PersistenceSlot::History).is_some());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn persistence_worker_atomic_write_roundtrips_history_library_and_queue() {
        let dir = temp_dir("roundtrip");
        let hist_path = dir.join("history.json");
        let lib_path = dir.join("library.json");
        let queue_path = dir.join("queue.json");

        let mut hist = History::default();
        hist.record(sample_history_entry(7, 42_000)).unwrap();

        let mut lib = Library::default();
        lib.toggle_watch_later(sample_video("abcdefghijk"));

        let mut queue = Queue::new(10);
        queue.add(sample_video("lmnopqrstuv")).unwrap();

        let worker = PersistenceWorker::new();
        worker.schedule_history(hist_path.clone(), hist.clone(), None);
        worker.schedule_library(lib_path.clone(), lib.clone(), None);
        worker.schedule_queue(queue_path.clone(), queue.clone(), None);
        worker.flush();

        assert!(worker.poll_errors().is_empty());
        assert_eq!(history::load(&hist_path).unwrap(), hist);
        assert_eq!(library::load(&lib_path).unwrap(), lib);
        let loaded_queue = Queue::load(&queue_path, 10).unwrap();
        assert_eq!(loaded_queue.videos(), queue.videos());
        assert_eq!(loaded_queue.entry_id(0), queue.entry_id(0));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn persistence_worker_preserves_externally_corrupted_file_and_reports_error() {
        let dir = temp_dir("corrupt");
        let path = dir.join("history.json");

        let mut hist1 = History::default();
        hist1.record(sample_history_entry(1, 5_000)).unwrap();

        let worker = PersistenceWorker::new();
        worker.schedule_history(path.clone(), hist1.clone(), None);
        worker.flush();
        assert!(worker.poll_errors().is_empty());

        // Simulate external corruption on disk after a good save.
        fs::write(&path, b"{corrupt").unwrap();

        let mut hist2 = hist1;
        hist2.record(sample_history_entry(1, 15_000)).unwrap();
        worker.schedule_history(path.clone(), hist2, None);
        worker.flush();

        let errors = worker.poll_detailed_errors();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].slot, PersistenceSlot::History);
        assert!(errors[0].corrupt_file);
        // Corrupt file must remain untouched on disk!
        assert_eq!(fs::read(&path).unwrap(), b"{corrupt");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn persistence_worker_flushes_pending_writes_cleanly_on_drop() {
        let dir = temp_dir("drop-flush");
        let path = dir.join("history.json");

        let mut hist = History::default();
        hist.record(sample_history_entry(9, 77_000)).unwrap();

        {
            let worker = PersistenceWorker::new();
            worker.schedule_history(path.clone(), hist.clone(), None);
            // Drop without explicit flush().
        }

        let loaded = history::load(&path).unwrap();
        assert_eq!(loaded, hist);

        let _ = fs::remove_dir_all(dir);
    }
}
