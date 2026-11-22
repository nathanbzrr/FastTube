//! Generic background task primitives enforcing monotonic request-ID staleness
//! filtering, cooperative cancellation, and automatic cancel-on-drop.
//!
//! `AsyncTask<T>` models a one-shot background request (replacing ad-hoc
//! `rx + cancel + next_request_id + active_request_id` quadruplets).
//! `AsyncStreamTask<T>` models a multi-message streaming job (progressive
//! feeds, live chat batches, or paged playlist syncs).

use crate::process::Cancellation;
use eframe::egui;
use std::fmt;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

/// Result of polling an [`AsyncTask<T>`] without losing visibility into
/// unexpected worker panics or channel disconnects.
#[derive(Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum AsyncTaskPoll<T> {
    /// No background task is currently active.
    Idle,
    /// A background task is still running and has not delivered a matching result yet.
    Pending,
    /// The active request completed and delivered its value.
    Ready(T),
    /// The worker exited or panicked without delivering a result for the active request.
    Disconnected,
}

/// Triggers an `egui` repaint when dropped unless explicitly disarmed.
///
/// Declared before the channel sender inside spawned closures so that on panic
/// unwind, the sender drops first (disconnecting the receiver) and the repaint
/// fires immediately afterward.
struct RepaintGuard {
    ctx: Option<egui::Context>,
}

impl RepaintGuard {
    fn new(ctx: Option<egui::Context>) -> Self {
        Self { ctx }
    }

    fn disarm(&mut self) {
        self.ctx = None;
    }
}

impl Drop for RepaintGuard {
    fn drop(&mut self) {
        if let Some(ctx) = self.ctx.take() {
            ctx.request_repaint();
        }
    }
}

/// One-shot background job that enforces monotonic `request_id` validation,
/// cooperative [`Cancellation`], and cancel-on-drop semantics by construction.
#[allow(dead_code)]
pub struct AsyncTask<T> {
    next_request_id: u64,
    active_request_id: Option<u64>,
    cancel: Option<Cancellation>,
    rx: Option<Receiver<(u64, T)>>,
}

impl<T> Default for AsyncTask<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> fmt::Debug for AsyncTask<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncTask")
            .field("active_request_id", &self.active_request_id)
            .field("is_running", &self.is_running())
            .finish()
    }
}

impl<T> Drop for AsyncTask<T> {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[allow(dead_code)]
impl<T> AsyncTask<T> {
    pub fn new() -> Self {
        Self {
            next_request_id: 1,
            active_request_id: None,
            cancel: None,
            rx: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.rx.is_some()
    }

    pub fn is_active(&self) -> bool {
        self.active_request_id.is_some()
    }

    pub fn active_request_id(&self) -> Option<u64> {
        self.active_request_id
    }

    pub fn next_request_id(&self) -> u64 {
        self.next_request_id
    }

    /// Signals cancellation to any in-flight worker and clears active state
    /// without resetting the monotonic `request_id` sequence.
    pub fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.active_request_id = None;
        self.rx = None;
    }

    /// Cancels any previous task and reserves the next monotonic `request_id`
    /// before a worker is spawned (or for synchronous unit tests).
    pub fn begin_reserved(&mut self) -> u64 {
        self.cancel();
        let request_id = self.allocate_request_id();
        self.active_request_id = Some(request_id);
        request_id
    }

    #[cfg(test)]
    pub fn reserve_id_for_test(&mut self, request_id: u64) {
        self.cancel();
        if request_id >= self.next_request_id {
            self.next_request_id = request_id
                .checked_add(1)
                .expect("AsyncTask request_id counter exhausted");
        }
        self.active_request_id = Some(request_id);
    }

    /// Spawns a background worker bound to a previously reserved `request_id`.
    pub fn spawn_reserved<F>(&mut self, request_id: u64, ctx: egui::Context, work: F)
    where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> T + Send + 'static,
    {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        if request_id >= self.next_request_id {
            self.next_request_id = request_id
                .checked_add(1)
                .expect("AsyncTask request_id counter exhausted");
        }
        let cancel = Cancellation::new();
        let (tx, rx) = mpsc::channel();
        self.active_request_id = Some(request_id);
        self.cancel = Some(cancel.clone());
        self.rx = Some(rx);
        self.spawn_worker(None, Some(ctx), request_id, cancel, tx, work);
    }

    /// Cancels any previous in-flight task, allocates the next monotonic
    /// `request_id`, and spawns a background worker that wakes `ctx` on delivery.
    pub fn spawn<F>(&mut self, ctx: egui::Context, work: F) -> u64
    where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> T + Send + 'static,
    {
        self.spawn_inner(None, Some(ctx), work)
    }

    /// Alias of [`Self::spawn`] for clarity at call sites that use cooperative cancellation.
    pub fn spawn_with_cancel<F>(&mut self, ctx: egui::Context, work: F) -> u64
    where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> T + Send + 'static,
    {
        self.spawn(ctx, work)
    }

    /// Like [`Self::spawn`], passing the allocated monotonic `request_id` into `work`.
    pub fn spawn_with_id_and_cancel<F>(&mut self, ctx: egui::Context, work: F) -> u64
    where
        T: Send + 'static,
        F: FnOnce(u64, Cancellation) -> T + Send + 'static,
    {
        let (request_id, cancel, tx) = self.begin_manual();
        self.spawn_worker(None, Some(ctx), request_id, cancel, tx, move |cancel| {
            work(request_id, cancel)
        });
        request_id
    }

    /// Like [`Self::spawn`], assigning `thread_name` to the spawned OS thread.
    pub fn spawn_named<F>(&mut self, thread_name: &str, ctx: egui::Context, work: F) -> u64
    where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> T + Send + 'static,
    {
        self.spawn_inner(Some(thread_name), Some(ctx), work)
    }

    /// Spawns a background worker without requesting an `egui` repaint
    /// (suitable for headless unit tests and early startup pre-warm tasks).
    pub fn spawn_without_repaint<F>(&mut self, work: F) -> u64
    where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> T + Send + 'static,
    {
        self.spawn_inner(None, None, work)
    }

    /// Cancels any previous task, allocates the next monotonic `request_id`,
    /// and returns `(request_id, cancel, tx)` for callers that manage thread
    /// creation manually.
    pub fn begin_manual(&mut self) -> (u64, Cancellation, Sender<(u64, T)>) {
        self.cancel();
        let request_id = self.allocate_request_id();
        let cancel = Cancellation::new();
        let (tx, rx) = mpsc::channel();
        self.active_request_id = Some(request_id);
        self.cancel = Some(cancel.clone());
        self.rx = Some(rx);
        (request_id, cancel, tx)
    }

    /// Cancels any previous task and attaches an externally created channel and
    /// `request_id`, advancing `next_request_id` if needed to preserve strict
    /// monotonicity.
    pub fn attach_receiver(
        &mut self,
        request_id: u64,
        cancel: Cancellation,
        rx: Receiver<(u64, T)>,
    ) {
        self.cancel();
        if request_id >= self.next_request_id {
            self.next_request_id = request_id
                .checked_add(1)
                .expect("AsyncTask request_id counter exhausted");
        }
        self.active_request_id = Some(request_id);
        self.cancel = Some(cancel);
        self.rx = Some(rx);
    }

    /// Non-blocking poll that discards stale messages and returns the active
    /// request's value once ready.
    pub fn poll(&mut self) -> Option<T> {
        match self.poll_event() {
            AsyncTaskPoll::Ready(value) => Some(value),
            AsyncTaskPoll::Idle | AsyncTaskPoll::Pending | AsyncTaskPoll::Disconnected => None,
        }
    }

    /// Non-blocking poll that distinguishes idle, pending, ready, and unexpected
    /// worker disconnect outcomes.
    pub fn poll_event(&mut self) -> AsyncTaskPoll<T> {
        let Some(active_id) = self.active_request_id else {
            self.clear_state();
            return AsyncTaskPoll::Idle;
        };
        let Some(rx) = self.rx.as_ref() else {
            return AsyncTaskPoll::Idle;
        };

        let outcome = loop {
            match rx.try_recv() {
                Ok((id, value)) if id == active_id => break AsyncTaskPoll::Ready(value),
                Ok(_) => continue,
                Err(TryRecvError::Empty) => break AsyncTaskPoll::Pending,
                Err(TryRecvError::Disconnected) => break AsyncTaskPoll::Disconnected,
            }
        };

        if matches!(
            outcome,
            AsyncTaskPoll::Ready(_) | AsyncTaskPoll::Disconnected
        ) {
            self.clear_state();
        }
        outcome
    }

    /// Non-blocking poll that validates both the channel envelope ID and an
    /// inner payload `request_id` against `active_request_id`, discarding stale
    /// items while keeping the active task alive.
    pub fn poll_matching_id<F>(&mut self, mut extract_id: F) -> AsyncTaskPoll<T>
    where
        F: FnMut(&T) -> u64,
    {
        let Some(active_id) = self.active_request_id else {
            self.clear_state();
            return AsyncTaskPoll::Idle;
        };
        let Some(rx) = self.rx.as_ref() else {
            return AsyncTaskPoll::Idle;
        };

        let outcome = loop {
            match rx.try_recv() {
                Ok((id, value)) if id == active_id && extract_id(&value) == active_id => {
                    break AsyncTaskPoll::Ready(value);
                }
                Ok(_) => continue,
                Err(TryRecvError::Empty) => break AsyncTaskPoll::Pending,
                Err(TryRecvError::Disconnected) => break AsyncTaskPoll::Disconnected,
            }
        };

        if matches!(
            outcome,
            AsyncTaskPoll::Ready(_) | AsyncTaskPoll::Disconnected
        ) {
            self.clear_state();
        }
        outcome
    }

    fn spawn_inner<F>(
        &mut self,
        thread_name: Option<&str>,
        ctx: Option<egui::Context>,
        work: F,
    ) -> u64
    where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> T + Send + 'static,
    {
        let (request_id, cancel, tx) = self.begin_manual();
        self.spawn_worker(thread_name, ctx, request_id, cancel, tx, work);
        request_id
    }

    fn spawn_worker<F>(
        &self,
        thread_name: Option<&str>,
        ctx: Option<egui::Context>,
        request_id: u64,
        cancel: Cancellation,
        tx: Sender<(u64, T)>,
        work: F,
    ) where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> T + Send + 'static,
    {
        let task = move || {
            let mut repaint = RepaintGuard::new(ctx);
            let sender = tx;
            if cancel.cancelled() {
                repaint.disarm();
                return;
            }
            let value = work(cancel.clone());
            if cancel.cancelled() {
                repaint.disarm();
                return;
            }
            if sender.send((request_id, value)).is_err() {
                repaint.disarm();
            }
        };
        let mut builder = thread::Builder::new();
        if let Some(name) = thread_name {
            builder = builder.name(name.to_owned());
        }
        let _ = builder.spawn(task);
    }

    fn allocate_request_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id = id
            .checked_add(1)
            .expect("AsyncTask request_id counter exhausted");
        id
    }

    fn clear_state(&mut self) {
        self.active_request_id = None;
        self.cancel = None;
        self.rx = None;
    }
}

/// Batch drained from an [`AsyncStreamTask<T>`] during a single UI frame.
#[derive(Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub struct AsyncStreamPoll<T> {
    pub items: Vec<T>,
    pub finished: bool,
}

impl<T> Default for AsyncStreamPoll<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            finished: false,
        }
    }
}

/// Request-scoped stream emitter passed to [`AsyncStreamTask::spawn_stream`]
/// workers so every emitted item carries the active `request_id`, respects
/// cancellation, and wakes the UI thread.
#[allow(dead_code)]
pub struct AsyncStreamSender<T> {
    request_id: u64,
    cancel: Cancellation,
    tx: Sender<(u64, T)>,
    ctx: Option<egui::Context>,
}

impl<T> Clone for AsyncStreamSender<T> {
    fn clone(&self) -> Self {
        Self {
            request_id: self.request_id,
            cancel: self.cancel.clone(),
            tx: self.tx.clone(),
            ctx: self.ctx.clone(),
        }
    }
}

impl<T> fmt::Debug for AsyncStreamSender<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncStreamSender")
            .field("request_id", &self.request_id)
            .field("cancelled", &self.cancel.cancelled())
            .finish()
    }
}

#[allow(dead_code)]
impl<T> AsyncStreamSender<T> {
    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.cancelled()
    }

    /// Sends one item tagged with this stream's `request_id`. Returns `false`
    /// if the stream was cancelled or the receiver has been dropped.
    pub fn send(&self, item: T) -> bool {
        if self.cancel.cancelled() {
            return false;
        }
        if self.tx.send((self.request_id, item)).is_ok() {
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint();
            }
            true
        } else {
            false
        }
    }
}

/// Multi-message background stream enforcing monotonic `request_id` filtering,
/// cooperative [`Cancellation`], and cancel-on-drop semantics.
#[allow(dead_code)]
pub struct AsyncStreamTask<T> {
    next_request_id: u64,
    active_request_id: Option<u64>,
    cancel: Option<Cancellation>,
    rx: Option<Receiver<(u64, T)>>,
}

impl<T> Default for AsyncStreamTask<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> fmt::Debug for AsyncStreamTask<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncStreamTask")
            .field("active_request_id", &self.active_request_id)
            .field("is_running", &self.is_running())
            .finish()
    }
}

impl<T> Drop for AsyncStreamTask<T> {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[allow(dead_code)]
impl<T> AsyncStreamTask<T> {
    pub fn new() -> Self {
        Self {
            next_request_id: 1,
            active_request_id: None,
            cancel: None,
            rx: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.rx.is_some()
    }

    pub fn active_request_id(&self) -> Option<u64> {
        self.active_request_id
    }

    pub fn next_request_id(&self) -> u64 {
        self.next_request_id
    }

    /// Signals cancellation to any in-flight stream worker and clears active
    /// state without resetting the monotonic `request_id` sequence.
    pub fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.active_request_id = None;
        self.rx = None;
    }

    /// Cancels any previous stream, allocates the next monotonic `request_id`,
    /// and returns `(request_id, cancel, tx)` for manual thread management.
    pub fn begin_manual(&mut self) -> (u64, Cancellation, Sender<(u64, T)>) {
        self.cancel();
        let request_id = self.allocate_request_id();
        let cancel = Cancellation::new();
        let (tx, rx) = mpsc::channel();
        self.active_request_id = Some(request_id);
        self.cancel = Some(cancel.clone());
        self.rx = Some(rx);
        (request_id, cancel, tx)
    }

    /// Cancels any previous stream and attaches an externally created channel
    /// and `request_id`, advancing `next_request_id` if needed to preserve
    /// strict monotonicity.
    pub fn attach_receiver(
        &mut self,
        request_id: u64,
        cancel: Cancellation,
        rx: Receiver<(u64, T)>,
    ) {
        self.cancel();
        if request_id >= self.next_request_id {
            self.next_request_id = request_id
                .checked_add(1)
                .expect("AsyncStreamTask request_id counter exhausted");
        }
        self.active_request_id = Some(request_id);
        self.cancel = Some(cancel);
        self.rx = Some(rx);
    }

    /// Cancels any previous stream, allocates the next monotonic `request_id`,
    /// and spawns a streaming worker that wakes `ctx` on each emitted item and
    /// upon completion.
    pub fn spawn_stream<F>(&mut self, ctx: egui::Context, work: F) -> u64
    where
        T: Send + 'static,
        F: FnOnce(Cancellation, AsyncStreamSender<T>) + Send + 'static,
    {
        self.spawn_stream_inner(None, Some(ctx), work)
    }

    /// Like [`Self::spawn_stream`], assigning `thread_name` to the OS thread.
    pub fn spawn_stream_named<F>(&mut self, thread_name: &str, ctx: egui::Context, work: F) -> u64
    where
        T: Send + 'static,
        F: FnOnce(Cancellation, AsyncStreamSender<T>) + Send + 'static,
    {
        self.spawn_stream_inner(Some(thread_name), Some(ctx), work)
    }

    /// Spawns a streaming worker without requesting `egui` repaints (suitable
    /// for headless unit tests).
    pub fn spawn_stream_without_repaint<F>(&mut self, work: F) -> u64
    where
        T: Send + 'static,
        F: FnOnce(Cancellation, AsyncStreamSender<T>) + Send + 'static,
    {
        self.spawn_stream_inner(None, None, work)
    }

    /// Drains all currently queued items matching `active_request_id`,
    /// discarding stale IDs and marking `finished = true` when the sender
    /// disconnects.
    pub fn poll_batch(&mut self) -> AsyncStreamPoll<T> {
        let Some(active_id) = self.active_request_id else {
            self.clear_state();
            return AsyncStreamPoll::default();
        };
        let Some(rx) = self.rx.as_ref() else {
            self.clear_state();
            return AsyncStreamPoll::default();
        };

        let mut items = Vec::new();
        let mut finished = false;
        loop {
            match rx.try_recv() {
                Ok((id, value)) if id == active_id => items.push(value),
                Ok(_) => continue,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }

        if finished {
            self.clear_state();
        }
        AsyncStreamPoll { items, finished }
    }

    fn spawn_stream_inner<F>(
        &mut self,
        thread_name: Option<&str>,
        ctx: Option<egui::Context>,
        work: F,
    ) -> u64
    where
        T: Send + 'static,
        F: FnOnce(Cancellation, AsyncStreamSender<T>) + Send + 'static,
    {
        let (request_id, cancel, tx) = self.begin_manual();
        let emitter_ctx = ctx.clone();
        let task = move || {
            let mut repaint = RepaintGuard::new(ctx);
            let sender = AsyncStreamSender {
                request_id,
                cancel: cancel.clone(),
                tx,
                ctx: emitter_ctx,
            };
            if cancel.cancelled() {
                repaint.disarm();
                return;
            }
            work(cancel.clone(), sender);
            if cancel.cancelled() {
                repaint.disarm();
            }
        };
        let mut builder = thread::Builder::new();
        if let Some(name) = thread_name {
            builder = builder.name(name.to_owned());
        }
        let _ = builder.spawn(task);
        request_id
    }

    fn allocate_request_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id = id
            .checked_add(1)
            .expect("AsyncStreamTask request_id counter exhausted");
        id
    }

    fn clear_state(&mut self) {
        self.active_request_id = None;
        self.cancel = None;
        self.rx = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    struct NonDebugPayload(&'static str);

    #[test]
    fn async_task_monotonic_request_ids_discard_stale_results() {
        let mut task = AsyncTask::<NonDebugPayload>::default();
        assert!(!task.is_running());
        assert_eq!(task.active_request_id(), None);
        assert_eq!(task.next_request_id(), 1);
        assert!(matches!(task.poll_event(), AsyncTaskPoll::Idle));
        assert!(format!("{task:?}").contains("is_running: false"));

        let (id1, _c1, _tx1) = task.begin_manual();
        assert_eq!(id1, 1);
        assert_eq!(task.active_request_id(), Some(1));
        assert_eq!(task.next_request_id(), 2);
        assert!(task.is_running());

        task.cancel();
        assert!(!task.is_running());
        assert_eq!(task.active_request_id(), None);
        assert_eq!(task.next_request_id(), 2);

        let (id2, _c2, tx2) = task.begin_manual();
        assert_eq!(id2, 2);
        assert_eq!(task.active_request_id(), Some(2));
        assert_eq!(task.next_request_id(), 3);

        tx2.send((id1, NonDebugPayload("stale-old"))).unwrap();
        tx2.send((999, NonDebugPayload("stale-other"))).unwrap();
        assert!(matches!(task.poll_event(), AsyncTaskPoll::Pending));
        assert!(task.is_running());

        tx2.send((id1, NonDebugPayload("stale-before-match")))
            .unwrap();
        tx2.send((id2, NonDebugPayload("fresh"))).unwrap();
        let delivered = task.poll().expect("matching request should be delivered");
        assert_eq!(delivered.0, "fresh");
        assert!(!task.is_running());
        assert_eq!(task.active_request_id(), None);
        assert_eq!(task.next_request_id(), 3);

        // Attaching a higher external ID keeps future monotonic IDs collision-free.
        let (ext_tx, ext_rx) = mpsc::channel();
        task.attach_receiver(10, Cancellation::new(), ext_rx);
        assert_eq!(task.active_request_id(), Some(10));
        assert_eq!(task.next_request_id(), 11);
        ext_tx.send((10, NonDebugPayload("attached"))).unwrap();
        assert_eq!(task.poll().unwrap().0, "attached");

        let (id_after, _c_after, _tx_after) = task.begin_manual();
        assert_eq!(id_after, 11);
        assert_eq!(task.next_request_id(), 12);
    }

    #[test]
    fn async_task_supersede_and_drop_cancel_previous_worker() {
        let cancel_on_drop;
        {
            let mut task = AsyncTask::<u32>::new();
            let (id1, cancel1, tx1) = task.begin_manual();
            assert_eq!(id1, 1);
            assert!(!cancel1.cancelled());

            // Starting a second request cancels the first and drops its receiver.
            let (id2, cancel2, _tx2) = task.begin_manual();
            assert_eq!(id2, 2);
            assert!(cancel1.cancelled());
            assert!(!cancel2.cancelled());
            assert!(tx1.send((id1, 100)).is_err());

            // Superseding a spawned worker also signals cancellation deterministically.
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let observed_cancel = Arc::new(AtomicBool::new(false));
            let observed_clone = Arc::clone(&observed_cancel);

            let id3 = task.spawn_without_repaint(move |cancel| {
                let _ = entered_tx.send(());
                let _ = release_rx.recv();
                observed_clone.store(cancel.cancelled(), Ordering::SeqCst);
                42
            });
            assert_eq!(id3, 3);
            assert!(cancel2.cancelled());
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();

            let (id4, cancel4, _tx4) = task.begin_manual();
            assert_eq!(id4, 4);
            assert!(!cancel4.cancelled());
            let _ = release_tx.send(());

            let deadline = Instant::now() + Duration::from_secs(5);
            while !observed_cancel.load(Ordering::SeqCst) {
                assert!(
                    Instant::now() < deadline,
                    "superseded worker did not observe cancellation"
                );
                thread::sleep(Duration::from_millis(1));
            }

            cancel_on_drop = cancel4;
            assert!(!cancel_on_drop.cancelled());
        }

        // Dropping AsyncTask cancels the in-flight worker automatically.
        assert!(cancel_on_drop.cancelled());
    }

    #[test]
    fn async_task_handles_worker_disconnect_cleanly() {
        let mut task = AsyncTask::<String>::new();
        let (id1, _cancel1, tx1) = task.begin_manual();
        assert_eq!(id1, 1);
        tx1.send((999, "stale".to_owned())).unwrap();
        drop(tx1);

        assert_eq!(task.poll_event(), AsyncTaskPoll::Disconnected);
        assert!(!task.is_running());
        assert_eq!(task.active_request_id(), None);
        assert_eq!(task.next_request_id(), 2);
        assert_eq!(task.poll_event(), AsyncTaskPoll::Idle);

        // `poll()` also clears running state cleanly on disconnect.
        let (id2, _cancel2, tx2) = task.begin_manual();
        assert_eq!(id2, 2);
        drop(tx2);
        assert_eq!(task.poll(), None);
        assert!(!task.is_running());
        assert_eq!(task.active_request_id(), None);
    }

    #[test]
    fn async_task_spawn_without_repaint_delivers_result_and_clears_running_state() {
        let mut task = AsyncTask::<usize>::new();
        let req_id = task.spawn_without_repaint(|cancel| {
            assert!(!cancel.cancelled());
            7 * 6
        });
        assert_eq!(req_id, 1);
        assert!(task.is_running());
        assert_eq!(task.active_request_id(), Some(1));
        assert_eq!(task.next_request_id(), 2);

        let deadline = Instant::now() + Duration::from_secs(5);
        let value = loop {
            match task.poll_event() {
                AsyncTaskPoll::Ready(v) => break v,
                AsyncTaskPoll::Pending => {
                    assert!(Instant::now() < deadline, "timed out waiting for worker");
                    thread::sleep(Duration::from_millis(1));
                }
                other => panic!("unexpected poll outcome: {other:?}"),
            }
        };

        assert_eq!(value, 42);
        assert!(!task.is_running());
        assert_eq!(task.active_request_id(), None);
        assert_eq!(task.next_request_id(), 2);
        assert_eq!(task.poll_event(), AsyncTaskPoll::Idle);
    }

    #[test]
    fn async_stream_task_delivers_multiple_batches_filters_stale_and_marks_finished_on_disconnect()
    {
        let mut stream = AsyncStreamTask::<&'static str>::new();
        assert!(!stream.is_running());
        assert_eq!(stream.next_request_id(), 1);
        assert_eq!(
            stream.poll_batch(),
            AsyncStreamPoll {
                items: Vec::new(),
                finished: false,
            }
        );

        let (id1, _c1, tx1) = stream.begin_manual();
        assert_eq!(id1, 1);
        assert!(stream.is_running());
        assert_eq!(stream.active_request_id(), Some(1));

        tx1.send((0, "stale-0")).unwrap();
        tx1.send((id1, "batch-1a")).unwrap();
        tx1.send((999, "stale-999")).unwrap();
        tx1.send((id1, "batch-1b")).unwrap();
        assert_eq!(
            stream.poll_batch(),
            AsyncStreamPoll {
                items: vec!["batch-1a", "batch-1b"],
                finished: false,
            }
        );
        assert!(stream.is_running());

        tx1.send((id1, "batch-2a")).unwrap();
        tx1.send((777, "stale-777")).unwrap();
        drop(tx1);
        assert_eq!(
            stream.poll_batch(),
            AsyncStreamPoll {
                items: vec!["batch-2a"],
                finished: true,
            }
        );
        assert!(!stream.is_running());
        assert_eq!(stream.active_request_id(), None);
        assert_eq!(stream.next_request_id(), 2);

        // Once finished, subsequent idle polls report empty + not finished.
        assert_eq!(
            stream.poll_batch(),
            AsyncStreamPoll {
                items: Vec::new(),
                finished: false,
            }
        );

        // Background streaming worker delivers all items and terminates cleanly.
        let id2 = stream.spawn_stream_without_repaint(|cancel, sender| {
            assert_eq!(sender.request_id(), 2);
            assert!(!sender.is_cancelled());
            for item in ["s1", "s2", "s3"] {
                if cancel.cancelled() || !sender.send(item) {
                    return;
                }
            }
        });
        assert_eq!(id2, 2);

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut collected = Vec::new();
        loop {
            let batch = stream.poll_batch();
            collected.extend(batch.items);
            if batch.finished {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for stream completion"
            );
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(collected, vec!["s1", "s2", "s3"]);
        assert!(!stream.is_running());
        assert_eq!(stream.next_request_id(), 3);

        // Dropping an active stream cancels its worker automatically.
        let drop_cancel;
        {
            let mut active_stream = AsyncStreamTask::<u8>::new();
            let (_id, cancel, _tx) = active_stream.begin_manual();
            drop_cancel = cancel;
            assert!(!drop_cancel.cancelled());
        }
        assert!(drop_cancel.cancelled());
    }
}
