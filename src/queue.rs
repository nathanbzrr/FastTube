use crate::search::Video;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub fn label_pt(self) -> &'static str {
        match self {
            Self::Off => "Repetir: Desligado",
            Self::All => "Repetir: Fila toda",
            Self::One => "Repetir: Vídeo atual",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::Off => "Repeat: Off",
            Self::All => "Repeat: All",
            Self::One => "Repeat: One",
        }
    }
}

#[derive(Clone)]
pub struct Queue {
    videos: Vec<Video>,
    entry_ids: Vec<u64>,
    queued_video_ids: HashSet<String>,
    /// Tracks counts only for keys with 2+ occurrences so duplicate URLs
    /// preserve `queued_video_ids` membership until the final copy is removed.
    duplicate_key_counts: HashMap<String, usize>,
    limit: usize,
    next_entry_id: u64,
    repeat_mode: RepeatMode,
}

impl Queue {
    pub fn new(limit: usize) -> Self {
        Self {
            videos: Vec::new(),
            entry_ids: Vec::new(),
            queued_video_ids: HashSet::new(),
            duplicate_key_counts: HashMap::new(),
            limit,
            next_entry_id: 1,
            repeat_mode: RepeatMode::Off,
        }
    }

    pub fn repeat_mode(&self) -> RepeatMode {
        self.repeat_mode
    }

    pub fn set_repeat_mode(&mut self, repeat_mode: RepeatMode) {
        self.repeat_mode = repeat_mode;
    }

    #[allow(dead_code)]
    pub fn cycle_repeat_mode(&mut self) -> RepeatMode {
        self.repeat_mode = self.repeat_mode.cycle();
        self.repeat_mode
    }

    fn video_key(url: &str) -> String {
        crate::history::youtube_video_id(url).unwrap_or_else(|| url.to_owned())
    }

    fn index_insert(&mut self, url: &str) {
        let key = Self::video_key(url);
        if !self.queued_video_ids.insert(key.clone()) {
            *self.duplicate_key_counts.entry(key).or_insert(1) += 1;
        }
    }

    fn index_remove(&mut self, url: &str) {
        let key = Self::video_key(url);
        if let Some(count) = self.duplicate_key_counts.get_mut(&key) {
            if *count > 2 {
                *count -= 1;
            } else {
                self.duplicate_key_counts.remove(&key);
            }
        } else {
            self.queued_video_ids.remove(&key);
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

    #[allow(dead_code)]
    pub fn queued_video_ids(&self) -> &HashSet<String> {
        &self.queued_video_ids
    }

    /// O(1) membership check by validated YouTube video ID.
    #[allow(dead_code)]
    pub fn contains_video_id(&self, video_id: &str) -> bool {
        self.queued_video_ids.contains(video_id)
    }

    /// O(1) membership check accepting either a raw video ID or a full URL.
    #[allow(dead_code)]
    pub fn contains_video(&self, video_id_or_url: &str) -> bool {
        if self.queued_video_ids.contains(video_id_or_url) {
            return true;
        }
        crate::history::youtube_video_id(video_id_or_url)
            .is_some_and(|id| self.queued_video_ids.contains(&id))
    }

    #[allow(dead_code)]
    pub fn is_queued(&self, video_id_or_url: &str) -> bool {
        self.contains_video(video_id_or_url)
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

    #[allow(dead_code)]
    pub fn load_with_fingerprint(
        path: &Path,
        limit: usize,
    ) -> io::Result<(Self, Option<crate::history::FileFingerprint>)> {
        let queue = Self::load(path, limit)?;
        Ok((queue, crate::history::file_fingerprint(path)))
    }

    /// Decodes a queue value embedded in a user-data backup.
    pub fn from_backup_value(value: Value, limit: usize) -> io::Result<Self> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "fila inválida");
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
        let repeat_mode = match root.get("repeat_mode") {
            None | Some(Value::Null) => RepeatMode::Off,
            Some(Value::String(raw)) => match raw.trim().to_ascii_lowercase().as_str() {
                "off" => RepeatMode::Off,
                "all" => RepeatMode::All,
                "one" => RepeatMode::One,
                _ => return Err(invalid()),
            },
            Some(_) => return Err(invalid()),
        };
        let entries = root
            .get("videos")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?;
        if entries.len() > limit {
            return Err(invalid());
        }
        let mut queue = Self::new(limit);
        queue.repeat_mode = repeat_mode;
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
            queue.index_insert(&video.url);
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

    pub fn serialize_bytes(&self) -> io::Result<Vec<u8>> {
        if self.videos.iter().any(|v| {
            [&v.title, &v.channel, &v.url, &v.thumbnail, &v.duration]
                .iter()
                .any(|s| s.len() > MAX_TEXT_BYTES)
                || v.url.is_empty()
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fila excede os limites de armazenamento",
            ));
        }
        #[derive(Serialize)]
        struct QueueEntryRef<'a> {
            id: u64,
            title: &'a str,
            channel: &'a str,
            url: &'a str,
            thumbnail: &'a str,
            duration: &'a str,
        }
        #[derive(Serialize)]
        struct QueueFile<'a> {
            version: u8,
            #[serde(default)]
            repeat_mode: RepeatMode,
            videos: Vec<QueueEntryRef<'a>>,
        }
        let videos = self
            .videos
            .iter()
            .zip(&self.entry_ids)
            .map(|(video, &id)| QueueEntryRef {
                id,
                title: &video.title,
                channel: &video.channel,
                url: &video.url,
                thumbnail: &video.thumbnail,
                duration: &video.duration,
            })
            .collect();
        let bytes = serde_json::to_vec(&QueueFile {
            version: 2,
            repeat_mode: self.repeat_mode,
            videos,
        })?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fila excede os limites de armazenamento",
            ));
        }
        Ok(bytes)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let bytes = self.serialize_bytes()?;
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
        json!({
            "version": 2,
            "repeat_mode": self.repeat_mode,
            "videos": videos
        })
    }

    pub fn add(&mut self, video: Video) -> Result<(), Video> {
        if self.videos.len() >= self.limit {
            return Err(video);
        }
        let entry_id = self.allocate_entry_id();
        self.index_insert(&video.url);
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
        let removed = self.videos.remove(index);
        self.index_remove(&removed.url);
        Some(removed)
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

    pub fn move_item(&mut self, from_index: usize, to_index: usize) -> bool {
        self.reorder(from_index, to_index)
    }

    pub fn move_to_top(&mut self, index: usize) -> bool {
        self.reorder(index, 0)
    }

    pub fn move_to_bottom(&mut self, index: usize) -> bool {
        if self.videos.is_empty() {
            return false;
        }
        self.reorder(index, self.videos.len() - 1)
    }

    pub fn index_of_entry(&self, entry_id: u64) -> Option<usize> {
        self.entry_ids.iter().position(|id| *id == entry_id)
    }

    pub fn shuffle(&mut self) -> bool {
        let mut buf = [0u8; 8];
        let seed = if getrandom::fill(&mut buf).is_ok() {
            u64::from_le_bytes(buf)
        } else {
            self.next_entry_id ^ ((self.videos.len() as u64) << 32)
        };
        self.shuffle_with_seed(seed)
    }

    pub fn shuffle_with_seed(&mut self, seed: u64) -> bool {
        let len = self.videos.len();
        if len <= 1 {
            return false;
        }
        let before_ids = self.entry_ids.clone();
        let mut state = if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        };
        for i in (1..len).rev() {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let rand = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
            let j = (rand as usize) % (i + 1);
            if i != j {
                self.videos.swap(i, j);
                self.entry_ids.swap(i, j);
            }
        }
        if self.entry_ids == before_ids {
            self.videos.swap(0, len - 1);
            self.entry_ids.swap(0, len - 1);
        }
        true
    }

    /// Removes later duplicate occurrences of the same video while preserving entry IDs.
    pub fn deduplicate(&mut self) -> usize {
        let mut seen = HashSet::with_capacity(self.videos.len());
        let mut kept_videos = Vec::with_capacity(self.videos.len());
        let mut kept_ids = Vec::with_capacity(self.entry_ids.len());
        let original_len = self.videos.len();
        for (video, entry_id) in self.videos.drain(..).zip(self.entry_ids.drain(..)) {
            let key = Self::video_key(&video.url);
            if seen.insert(key) {
                kept_videos.push(video);
                kept_ids.push(entry_id);
            }
        }
        self.videos = kept_videos;
        self.entry_ids = kept_ids;
        self.queued_video_ids = seen;
        self.duplicate_key_counts.clear();
        original_len - self.videos.len()
    }

    /// Replaces the entire queue contents with fresh occurrence IDs while keeping
    /// the video index synchronized and respecting `self.limit`.
    #[allow(dead_code)]
    pub fn replace_videos(&mut self, videos: impl IntoIterator<Item = Video>) -> usize {
        self.clear();
        let mut added = 0usize;
        for video in videos {
            if self.add(video).is_err() {
                break;
            }
            added += 1;
        }
        added
    }

    pub fn clear(&mut self) -> usize {
        let count = self.videos.len();
        self.videos.clear();
        self.entry_ids.clear();
        self.queued_video_ids.clear();
        self.duplicate_key_counts.clear();
        count
    }

    pub fn total_duration_ms(&self) -> (u64, usize) {
        let mut total_ms = 0u64;
        let mut known_count = 0usize;
        for video in &self.videos {
            if let Some(ms) = crate::parse_duration_ms(&video.duration) {
                total_ms = total_ms.saturating_add(ms);
                known_count += 1;
            }
        }
        (total_ms, known_count)
    }

    pub fn format_duration_summary(&self) -> String {
        let len = self.videos.len();
        let count_label = if len == 1 {
            "1 vídeo".to_owned()
        } else {
            format!("{len} vídeos")
        };
        let (total_ms, known_count) = self.total_duration_ms();
        if len == 0 || known_count == 0 || total_ms == 0 {
            return count_label;
        }
        if total_ms < 60_000 {
            return format!("{count_label} • ~<1m");
        }
        let total_mins = (total_ms + 30_000) / 60_000;
        let hours = total_mins / 60;
        let mins = total_mins % 60;
        let dur_text = if hours > 0 && mins > 0 {
            format!("~{hours}h {mins}m")
        } else if hours > 0 {
            format!("~{hours}h")
        } else {
            format!("~{mins}m")
        };
        format!("{count_label} • {dur_text}")
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

    #[test]
    fn move_to_top_shuffle_deduplicate_clear_and_duration_summary_work() {
        let mut queue = Queue::new(10);
        let mut v1 = video('a');
        v1.url = "https://www.youtube.com/watch?v=abcdefghijk".into();
        v1.duration = "1:00:00".into();
        let mut v2 = video('b');
        v2.url = "https://youtu.be/lmnopqrstuv".into();
        v2.duration = "24:00".into();
        let mut v3 = video('c');
        v3.url = "https://youtu.be/abcdefghijk".into();
        v3.duration = "10:00".into();
        let mut v4 = video('d');
        v4.url = "https://www.youtube.com/watch?v=zzzzzzzzzzz".into();
        v4.duration = "AO VIVO".into();

        queue.add(v1).unwrap();
        queue.add(v2).unwrap();
        queue.add(v3).unwrap();
        queue.add(v4).unwrap();

        let id_b = queue.entry_id(1).unwrap();
        let id_d = queue.entry_id(3).unwrap();
        assert_eq!(queue.deduplicate(), 1);
        assert_eq!(titles(&queue), ['a', 'b', 'd']);
        assert_eq!(queue.entry_id(1), Some(id_b));
        assert_eq!(queue.entry_id(2), Some(id_d));
        assert_eq!(queue.format_duration_summary(), "3 vídeos • ~1h 24m");

        assert!(queue.move_to_top(2));
        assert_eq!(titles(&queue), ['d', 'a', 'b']);
        assert_eq!(queue.entry_id(0), Some(id_d));

        let ids_before: Vec<u64> = (0..queue.len()).filter_map(|i| queue.entry_id(i)).collect();
        assert!(queue.shuffle_with_seed(42));
        let ids_after: Vec<u64> = (0..queue.len()).filter_map(|i| queue.entry_id(i)).collect();
        assert_ne!(ids_before, ids_after);
        let mut sorted_before = ids_before;
        let mut sorted_after = ids_after;
        sorted_before.sort_unstable();
        sorted_after.sort_unstable();
        assert_eq!(sorted_before, sorted_after);

        assert_eq!(queue.clear(), 3);
        assert!(queue.is_empty());
        assert!(!queue.shuffle_with_seed(1));
        assert_eq!(queue.format_duration_summary(), "0 vídeos");
    }

    #[test]
    fn queued_video_index_stays_consistent_across_mutations_duplicates_and_load() {
        let mut queue = Queue::new(10);
        let yt_a = Video {
            title: "A".into(),
            channel: "Ch".into(),
            url: "https://www.youtube.com/watch?v=abcdefghijk".into(),
            thumbnail: String::new(),
            duration: "3:00".into(),
        };
        let yt_a_short = Video {
            title: "A dup".into(),
            channel: "Ch".into(),
            url: "https://youtu.be/abcdefghijk".into(),
            thumbnail: String::new(),
            duration: "3:00".into(),
        };
        let yt_b = Video {
            title: "B".into(),
            channel: "Ch".into(),
            url: "https://www.youtube.com/watch?v=lmnopqrstuv".into(),
            thumbnail: String::new(),
            duration: "4:00".into(),
        };

        assert!(!queue.contains_video("abcdefghijk"));
        assert!(!queue.is_queued(&yt_a.url));

        queue.add(yt_a.clone()).unwrap();
        queue.add(yt_a_short.clone()).unwrap();
        queue.add(yt_b.clone()).unwrap();

        let first_a_id = queue.entry_id(0).unwrap();
        let second_a_id = queue.entry_id(1).unwrap();
        assert_ne!(first_a_id, second_a_id);
        assert!(queue.contains_video_id("abcdefghijk"));
        assert!(queue.is_queued("https://youtu.be/abcdefghijk"));
        assert!(queue.contains_video(&yt_b.url));

        // Reorder preserves index membership and occurrence IDs.
        assert!(queue.reorder(2, 0));
        assert!(queue.contains_video_id("abcdefghijk"));
        assert!(queue.contains_video_id("lmnopqrstuv"));

        // Removing one duplicate occurrence keeps the video indexed until the last copy is removed.
        assert!(queue.remove_entry(first_a_id).is_some());
        assert!(queue.contains_video_id("abcdefghijk"));
        assert!(queue.remove_entry(second_a_id).is_some());
        assert!(!queue.contains_video_id("abcdefghijk"));
        assert!(queue.contains_video_id("lmnopqrstuv"));

        // Round-trip save & load rebuilds index and preserves occurrence IDs.
        let path = std::env::temp_dir().join(format!(
            "fasttube-queue-index-test-{}.json",
            std::process::id()
        ));
        let b_id = queue.entry_id(0).unwrap();
        queue.save(&path).unwrap();
        let (mut loaded, fp) = Queue::load_with_fingerprint(&path, 10).unwrap();
        assert!(fp.is_some());
        assert_eq!(loaded.entry_id(0), Some(b_id));
        assert!(loaded.contains_video_id("lmnopqrstuv"));
        assert!(!loaded.contains_video_id("abcdefghijk"));
        let _ = std::fs::remove_file(&path);

        // replace_videos and clear keep index exact.
        assert_eq!(loaded.replace_videos([yt_a.clone(), yt_a_short]), 2);
        assert!(loaded.contains_video_id("abcdefghijk"));
        assert!(!loaded.contains_video_id("lmnopqrstuv"));
        assert_eq!(loaded.deduplicate(), 1);
        assert!(loaded.contains_video_id("abcdefghijk"));
        loaded.clear();
        assert!(!loaded.contains_video_id("abcdefghijk"));
        assert!(loaded.queued_video_ids().is_empty());
    }

    #[test]
    fn repeat_mode_persists_in_queue_json_and_legacy_files_default_to_off() {
        use super::RepeatMode;

        let dir =
            std::env::temp_dir().join(format!("fasttube-queue-repeat-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("queue.json");

        let mut queue = Queue::new(4);
        queue.add(video('a')).unwrap();
        queue.set_repeat_mode(RepeatMode::All);
        assert_eq!(queue.repeat_mode(), RepeatMode::All);
        queue.save(&path).unwrap();

        let loaded = Queue::load(&path, 4).unwrap();
        assert_eq!(loaded.repeat_mode(), RepeatMode::All);
        assert_eq!(titles(&loaded), ['a']);

        let backup = loaded.to_backup_value();
        assert_eq!(backup["repeat_mode"], "all");
        let from_backup = Queue::from_backup_value(backup, 4).unwrap();
        assert_eq!(from_backup.repeat_mode(), RepeatMode::All);

        // Legacy v2 file without repeat_mode field loads with RepeatMode::Off.
        let legacy_v2 = serde_json::json!({
            "version": 2,
            "videos": [
                {"id": 1, "title": "a", "channel": "", "url": "a", "thumbnail": "", "duration": ""}
            ]
        });
        std::fs::write(&path, serde_json::to_vec(&legacy_v2).unwrap()).unwrap();
        let legacy_loaded = Queue::load(&path, 4).unwrap();
        assert_eq!(legacy_loaded.repeat_mode(), RepeatMode::Off);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn move_item_and_move_to_bottom_preserve_occurrence_ids_and_video_index() {
        let mut queue = Queue::new(6);
        for ch in ['a', 'b', 'c', 'd'] {
            let mut v = video(ch);
            v.url = format!("https://www.youtube.com/watch?v={ch}bcdefghijk");
            queue.add(v).unwrap();
        }
        let id_a = queue.entry_id(0).unwrap();
        let id_b = queue.entry_id(1).unwrap();
        let id_c = queue.entry_id(2).unwrap();
        let id_d = queue.entry_id(3).unwrap();

        assert!(queue.move_item(0, 2));
        assert_eq!(titles(&queue), ['b', 'c', 'a', 'd']);
        assert_eq!(queue.entry_id(0), Some(id_b));
        assert_eq!(queue.entry_id(1), Some(id_c));
        assert_eq!(queue.entry_id(2), Some(id_a));
        assert_eq!(queue.entry_id(3), Some(id_d));

        assert!(queue.move_to_bottom(0));
        assert_eq!(titles(&queue), ['c', 'a', 'd', 'b']);
        assert_eq!(queue.entry_id(3), Some(id_b));
        assert!(queue.contains_video_id("abcdefghijk"));
        assert!(queue.contains_video_id("bbcdefghijk"));
        assert!(queue.contains_video_id("cbcdefghijk"));
        assert!(queue.contains_video_id("dbcdefghijk"));
        assert!(!queue.move_item(9, 0));
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
