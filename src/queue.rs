use crate::search::Video;
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::io::{self, Read};
use std::path::Path;

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_TEXT_BYTES: usize = 4096;

pub fn should_advance(ended: bool, handled: &mut bool, has_items: bool) -> bool {
    if !ended {
        *handled = false;
        return false;
    }
    if *handled {
        return false;
    }
    *handled = true;
    has_items
}

#[derive(Clone)]
pub struct Queue {
    videos: Vec<Video>,
    entry_ids: Vec<u64>,
    limit: usize,
    next_entry_id: u64,
}

impl Queue {
    pub fn new(limit: usize) -> Self {
        Self {
            videos: Vec::new(),
            entry_ids: Vec::new(),
            limit,
            next_entry_id: 1,
        }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }
    pub fn len(&self) -> usize {
        self.videos.len()
    }
    pub fn is_empty(&self) -> bool {
        self.videos.is_empty()
    }
    pub fn videos(&self) -> &[Video] {
        &self.videos
    }

    /// Stable identity for one queue occurrence. Duplicate URLs have different IDs.
    pub fn entry_id(&self, index: usize) -> Option<u64> {
        self.entry_ids.get(index).copied()
    }

    pub fn first_entry_id(&self) -> Option<u64> {
        self.entry_id(0)
    }

    pub fn load(path: &Path, limit: usize) -> io::Result<Self> {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::new(limit)),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "fila inválida");
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(invalid());
        }
        let root: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        Self::from_value_with_validator(root, limit, invalid)
    }

    /// Decodes a queue value embedded in a user-data backup.
    pub fn from_backup_value(value: Value, limit: usize) -> io::Result<Self> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "fila invÃ¡lida");
        Self::from_value_with_validator(value, limit, invalid)
    }

    fn from_value_with_validator<F>(root: Value, limit: usize, invalid: F) -> io::Result<Self>
    where
        F: Fn() -> io::Error + Copy,
    {
        let version = root.get("version").and_then(Value::as_u64);
        if !matches!(version, None | Some(1 | 2)) {
            return Err(invalid());
        }
        let entries = root
            .get("videos")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?;
        if entries.len() > limit {
            return Err(invalid());
        }
        let mut queue = Self::new(limit);
        let mut seen_ids = HashSet::with_capacity(entries.len());
        for entry in entries {
            let text = |key| -> io::Result<String> {
                let value = entry.get(key).and_then(Value::as_str).ok_or_else(invalid)?;
                if value.len() > MAX_TEXT_BYTES {
                    return Err(invalid());
                }
                Ok(value.to_owned())
            };
            let video = Video {
                title: text("title")?,
                channel: text("channel")?,
                url: text("url")?,
                thumbnail: text("thumbnail")?,
                duration: text("duration")?,
            };
            if video.url.is_empty() {
                return Err(invalid());
            }
            let entry_id = match entry.get("id") {
                Some(value) => value.as_u64().filter(|id| *id > 0).ok_or_else(invalid)?,
                None if version == Some(2) => return Err(invalid()),
                None => queue
                    .entry_ids
                    .last()
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(1),
            };
            if !seen_ids.insert(entry_id) {
                return Err(invalid());
            }
            queue.videos.push(video);
            queue.entry_ids.push(entry_id);
        }
        queue.next_entry_id = queue
            .entry_ids
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(invalid)?;
        Ok(queue)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let bytes = serde_json::to_vec(&self.to_backup_value())?;
        if bytes.len() as u64 > MAX_FILE_BYTES
            || self.videos.iter().any(|v| {
                [&v.title, &v.channel, &v.url, &v.thumbnail, &v.duration]
                    .iter()
                    .any(|s| s.len() > MAX_TEXT_BYTES)
                    || v.url.is_empty()
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fila excede os limites de armazenamento",
            ));
        }
        crate::history::write_atomic(path, &bytes)
    }

    /// Version 2 adds a stable occurrence ID while retaining the `videos` array shape.
    pub fn to_backup_value(&self) -> Value {
        let videos: Vec<_> = self
            .videos
            .iter()
            .zip(&self.entry_ids)
            .map(|(video, id)| {
                json!({
                    "id": id,
                    "title": video.title,
                    "channel": video.channel,
                    "url": video.url,
                    "thumbnail": video.thumbnail,
                    "duration": video.duration,
                })
            })
            .collect();
        json!({"version": 2, "videos": videos})
    }

    pub fn add(&mut self, video: Video) -> Result<(), Video> {
        if self.videos.len() >= self.limit {
            return Err(video);
        }
        let entry_id = self.allocate_entry_id();
        self.videos.push(video);
        self.entry_ids.push(entry_id);
        Ok(())
    }

    fn allocate_entry_id(&mut self) -> u64 {
        loop {
            let candidate = self.next_entry_id.max(1);
            self.next_entry_id = candidate.wrapping_add(1).max(1);
            if !self.entry_ids.contains(&candidate) {
                return candidate;
            }
        }
    }

    pub fn remove(&mut self, index: usize) -> Option<Video> {
        if index >= self.videos.len() {
            return None;
        }
        self.entry_ids.remove(index);
        Some(self.videos.remove(index))
    }

    /// Removes the exact occurrence confirmed by a player load event.
    pub fn remove_entry(&mut self, entry_id: u64) -> Option<Video> {
        let index = self.entry_ids.iter().position(|id| *id == entry_id)?;
        self.remove(index)
    }

    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.videos.len() || to >= self.videos.len() {
            return false;
        }
        if from != to {
            let video = self.videos.remove(from);
            let entry_id = self.entry_ids.remove(from);
            self.videos.insert(to, video);
            self.entry_ids.insert(to, entry_id);
        }
        true
    }

    #[cfg(test)]
    pub fn next(&mut self) -> Option<Video> {
        self.remove(0)
    }
}

#[cfg(test)]
mod tests {
    use super::Queue;
    use crate::search::Video;

    fn video(title: char) -> Video {
        Video {
            title: title.to_string(),
            channel: String::new(),
            url: title.to_string(),
            thumbnail: String::new(),
            duration: String::new(),
        }
    }

    fn titles(queue: &Queue) -> Vec<char> {
        queue
            .videos()
            .iter()
            .map(|video| video.title.chars().next().unwrap())
            .collect()
    }

    #[test]
    fn fifo_preserves_duplicates_and_removes_next() {
        let mut queue = Queue::new(3);
        assert!(queue.add(video('a')).is_ok());
        assert!(queue.add(video('b')).is_ok());
        assert!(queue.add(video('a')).is_ok());
        assert_eq!(titles(&queue), ['a', 'b', 'a']);
        assert_eq!(queue.next().unwrap().title, 'a'.to_string());
        assert_eq!(queue.next().unwrap().title, 'b'.to_string());
        assert_eq!(queue.next().unwrap().title, 'a'.to_string());
        assert!(queue.next().is_none());
        assert!(queue.is_empty());
    }

    #[test]
    fn limit_returns_rejected_video_and_space_can_be_reused() {
        let mut queue = Queue::new(1);
        assert_eq!(queue.limit(), 1);
        assert!(queue.add(video('a')).is_ok());
        let rejected = match queue.add(video('b')) {
            Err(video) => video,
            Ok(()) => panic!(),
        };
        assert_eq!(rejected.title, 'b'.to_string());
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.remove(0).unwrap().title, 'a'.to_string());
        assert!(queue.add(rejected).is_ok());
        assert_eq!(titles(&queue), ['b']);
    }

    #[test]
    fn zero_limit_and_invalid_indices_are_safe() {
        let mut queue = Queue::new(0);
        assert!(queue.add(video('a')).is_err());
        assert!(queue.remove(0).is_none());
        assert!(queue.remove(usize::MAX).is_none());
        assert!(!queue.reorder(0, 0));
        assert!(queue.next().is_none());
        assert!(queue.is_empty());
    }

    #[test]
    fn reorder_uses_final_indices_and_invalid_moves_change_nothing() {
        let mut queue = Queue::new(4);
        for title in ['a', 'b', 'c', 'd'] {
            assert!(queue.add(video(title)).is_ok());
        }
        assert!(queue.reorder(0, 3));
        assert_eq!(titles(&queue), ['b', 'c', 'd', 'a']);
        assert!(queue.reorder(2, 0));
        assert_eq!(titles(&queue), ['d', 'b', 'c', 'a']);
        assert!(queue.reorder(1, 1));
        assert!(!queue.reorder(4, 0));
        assert!(!queue.reorder(0, 4));
        assert!(!queue.reorder(usize::MAX, usize::MAX));
        assert_eq!(titles(&queue), ['d', 'b', 'c', 'a']);
        assert_eq!(queue.remove(1).unwrap().title, 'b'.to_string());
        assert_eq!(titles(&queue), ['d', 'c', 'a']);
    }

    #[test]
    fn persistence_round_trip_and_corrupt_file_is_preserved() {
        let dir = std::env::temp_dir().join(format!("fasttube-queue-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("queue.json");
        let mut queue = Queue::new(3);
        queue.add(video('a')).unwrap();
        queue.add(video('a')).unwrap();
        queue.add(video('b')).unwrap();
        queue.save(&path).unwrap();
        assert_eq!(titles(&Queue::load(&path, 3).unwrap()), ['a', 'a', 'b']);
        assert!(Queue::load(&path, 2).is_err());
        std::fs::write(&path, b"broken").unwrap();
        assert!(Queue::load(&path, 3).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn version_one_queue_migrates_and_version_two_preserves_occurrence_ids() {
        let legacy = serde_json::json!({
            "version": 1,
            "videos": [
                {"title":"first","channel":"","url":"same","thumbnail":"","duration":""},
                {"title":"second","channel":"","url":"same","thumbnail":"","duration":""}
            ]
        });
        let legacy_path =
            std::env::temp_dir().join(format!("fasttube-queue-v1-{}.json", std::process::id()));
        std::fs::write(&legacy_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let queue = Queue::load(&legacy_path, 4).unwrap();
        let first_id = queue.entry_id(0).unwrap();
        let second_id = queue.entry_id(1).unwrap();
        std::fs::remove_file(&legacy_path).unwrap();
        assert_ne!(first_id, second_id);
        let encoded = queue.to_backup_value();
        assert_eq!(encoded["version"], 2);
        let restored = Queue::from_backup_value(encoded, 4).unwrap();
        assert_eq!(restored.entry_id(0), Some(first_id));
        assert_eq!(restored.entry_id(1), Some(second_id));
        let v2_path =
            std::env::temp_dir().join(format!("fasttube-queue-v2-{}.json", std::process::id()));
        restored.save(&v2_path).unwrap();
        let from_disk = Queue::load(&v2_path, 4).unwrap();
        assert_eq!(from_disk.entry_id(0), Some(first_id));
        assert_eq!(from_disk.entry_id(1), Some(second_id));
        std::fs::remove_file(v2_path).unwrap();
        assert!(Queue::from_backup_value(serde_json::json!({"version":2,"videos":[{"title":"x","channel":"","url":"same","thumbnail":"","duration":""}]}), 4).is_err());
    }

    #[test]
    fn removal_by_occurrence_id_targets_duplicate_after_reorder() {
        let mut queue = Queue::new(4);
        for title in ['a', 'b', 'a'] {
            let mut item = video(title);
            item.url = "https://example.test/duplicate".into();
            queue.add(item).unwrap();
        }
        let first_id = queue.entry_id(0).unwrap();
        let second_id = queue.entry_id(1).unwrap();
        let targeted_id = queue.entry_id(2).unwrap();
        assert!(queue.reorder(2, 0));
        assert_eq!(queue.remove_entry(targeted_id).unwrap().title, "a");
        assert_eq!(queue.entry_id(0), Some(first_id));
        assert_eq!(queue.entry_id(1), Some(second_id));
        assert!(queue.remove_entry(targeted_id).is_none());
    }
    #[test]
    fn end_of_video_advances_only_once_until_playback_resumes() {
        let mut handled = false;
        assert!(super::should_advance(true, &mut handled, true));
        assert!(!super::should_advance(true, &mut handled, true));
        assert!(!super::should_advance(false, &mut handled, true));
        assert!(!super::should_advance(true, &mut handled, false));
        assert!(!super::should_advance(true, &mut handled, true));
        assert!(!super::should_advance(false, &mut handled, true));
        assert!(super::should_advance(true, &mut handled, true));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackEvent {
    StartFile {
        playlist_entry_id: u64,
    },
    FileLoaded,
    EndFile {
        playlist_entry_id: u64,
        failed: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueLoadResult {
    None,
    Confirmed { playlist_entry_id: u64 },
    Failed { playlist_entry_id: u64 },
}

#[derive(Default)]
pub struct QueuePlaybackCorrelation {
    pending: bool,
    pending_entry_id: Option<u64>,
    started_entries: VecDeque<u64>,
}

impl QueuePlaybackCorrelation {
    pub fn begin(&mut self) {
        self.started_entries.clear();
        self.pending = true;
        self.pending_entry_id = None;
    }

    pub fn cancel(&mut self) {
        self.pending = false;
        self.pending_entry_id = None;
    }

    pub fn handle(&mut self, event: PlaybackEvent) -> QueueLoadResult {
        match event {
            PlaybackEvent::StartFile { playlist_entry_id } => {
                self.started_entries.push_back(playlist_entry_id);
                if self.pending && self.pending_entry_id.is_none() {
                    self.pending_entry_id = Some(playlist_entry_id);
                }
                QueueLoadResult::None
            }
            PlaybackEvent::FileLoaded => {
                let Some(playlist_entry_id) = self.started_entries.pop_front() else {
                    return QueueLoadResult::None;
                };
                if self.pending_entry_id == Some(playlist_entry_id) {
                    self.cancel();
                    QueueLoadResult::Confirmed { playlist_entry_id }
                } else {
                    QueueLoadResult::None
                }
            }
            PlaybackEvent::EndFile {
                playlist_entry_id,
                failed,
            } => {
                if failed && self.pending_entry_id == Some(playlist_entry_id) {
                    self.cancel();
                    QueueLoadResult::Failed { playlist_entry_id }
                } else {
                    QueueLoadResult::None
                }
            }
        }
    }
}
#[cfg(test)]
mod playback_correlation_tests {
    use super::{PlaybackEvent, QueueLoadResult, QueuePlaybackCorrelation};

    #[test]
    fn duplicate_urls_are_distinguished_by_playlist_entry_id() {
        let mut state = QueuePlaybackCorrelation::default();
        state.begin();
        assert_eq!(
            state.handle(PlaybackEvent::StartFile {
                playlist_entry_id: 11
            }),
            QueueLoadResult::None
        );
        assert_eq!(
            state.handle(PlaybackEvent::StartFile {
                playlist_entry_id: 12
            }),
            QueueLoadResult::None
        );
        assert_eq!(
            state.handle(PlaybackEvent::FileLoaded),
            QueueLoadResult::Confirmed {
                playlist_entry_id: 11
            }
        );
    }

    #[test]
    fn stale_end_and_reordered_completion_do_not_confirm_another_entry() {
        let mut state = QueuePlaybackCorrelation::default();
        state.begin();
        state.handle(PlaybackEvent::StartFile {
            playlist_entry_id: 21,
        });
        assert_eq!(
            state.handle(PlaybackEvent::EndFile {
                playlist_entry_id: 20,
                failed: false,
            }),
            QueueLoadResult::None
        );
        assert_eq!(
            state.handle(PlaybackEvent::EndFile {
                playlist_entry_id: 21,
                failed: false,
            }),
            QueueLoadResult::None
        );
        assert_eq!(
            state.handle(PlaybackEvent::FileLoaded),
            QueueLoadResult::Confirmed {
                playlist_entry_id: 21
            }
        );
        assert_eq!(
            state.handle(PlaybackEvent::FileLoaded),
            QueueLoadResult::None
        );
    }

    #[test]
    fn failed_or_manually_replaced_load_remains_unconfirmed() {
        let mut failed = QueuePlaybackCorrelation::default();
        failed.begin();
        failed.handle(PlaybackEvent::StartFile {
            playlist_entry_id: 31,
        });
        assert_eq!(
            failed.handle(PlaybackEvent::EndFile {
                playlist_entry_id: 31,
                failed: true,
            }),
            QueueLoadResult::Failed {
                playlist_entry_id: 31
            }
        );
        assert_eq!(
            failed.handle(PlaybackEvent::FileLoaded),
            QueueLoadResult::None
        );

        let mut manual = QueuePlaybackCorrelation::default();
        manual.begin();
        manual.handle(PlaybackEvent::StartFile {
            playlist_entry_id: 41,
        });
        manual.cancel();
        assert_eq!(
            manual.handle(PlaybackEvent::FileLoaded),
            QueueLoadResult::None
        );
    }
}
