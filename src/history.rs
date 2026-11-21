//! Local history. Positions and durations are milliseconds.
//! Callers must serialize concurrent load/save operations.
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAX_ENTRIES: usize = 200;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 4096;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub watched_at_ms: Option<u64>,
}

pub fn is_valid_video_id(video_id: &str) -> bool {
    video_id.len() == 11
        && video_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

pub fn youtube_video_id(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let after_scheme = if let Some(rest) = trimmed.strip_prefix("https://") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        rest
    } else if trimmed
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
    {
        &trimmed[8..]
    } else if trimmed
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
    {
        &trimmed[7..]
    } else {
        return None;
    };

    let (authority, rest) = match after_scheme.find(['/', '?', '#']) {
        Some(index) => (&after_scheme[..index], &after_scheme[index..]),
        None => (after_scheme, ""),
    };
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let host = if let Some(stripped) = authority.strip_suffix(":443") {
        stripped
    } else if let Some(stripped) = authority.strip_suffix(":80") {
        stripped
    } else if authority.contains(':') {
        return None;
    } else {
        authority
    };
    let host = host.to_ascii_lowercase();

    let before_fragment = rest.split('#').next().unwrap_or("");
    let (path, query) = match before_fragment.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (before_fragment, None),
    };

    let candidate = match host.as_str() {
        "youtu.be" | "www.youtu.be" => {
            let segment = path.strip_prefix('/')?;
            let segment = segment.strip_suffix('/').unwrap_or(segment);
            if segment.is_empty() || segment.contains('/') {
                return None;
            }
            segment
        }
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            if path == "/watch" || path == "/watch/" {
                let query = query?;
                let mut found: Option<&str> = None;
                for pair in query.split('&').filter(|pair| !pair.is_empty()) {
                    let (key, value) = pair.split_once('=')?;
                    if key == "v" {
                        if value.is_empty() {
                            return None;
                        }
                        if let Some(previous) = found {
                            if previous != value {
                                return None;
                            }
                        } else {
                            found = Some(value);
                        }
                    }
                }
                found?
            } else {
                let segment = ["/shorts/", "/embed/", "/live/", "/v/"]
                    .into_iter()
                    .find_map(|prefix| path.strip_prefix(prefix))?;
                let segment = segment.strip_suffix('/').unwrap_or(segment);
                if segment.is_empty() || segment.contains('/') {
                    return None;
                }
                segment
            }
        }
        "youtube-nocookie.com" | "www.youtube-nocookie.com" => {
            let segment = path.strip_prefix("/embed/")?;
            let segment = segment.strip_suffix('/').unwrap_or(segment);
            if segment.is_empty() || segment.contains('/') {
                return None;
            }
            segment
        }
        _ => return None,
    };

    if is_valid_video_id(candidate) {
        Some(candidate.to_owned())
    } else {
        None
    }
}

pub fn youtube_watch_url(video_id: &str) -> Option<String> {
    if is_valid_video_id(video_id) {
        Some(format!("https://www.youtube.com/watch?v={video_id}"))
    } else {
        None
    }
}

pub fn is_near_end(position_ms: u64, duration_ms: Option<u64>) -> bool {
    let Some(duration_ms) = duration_ms else {
        return false;
    };
    if duration_ms == 0 || position_ms >= duration_ms {
        return true;
    }
    let remaining_ms = duration_ms.saturating_sub(position_ms);
    if u128::from(position_ms) * 100 >= u128::from(duration_ms) * 95 {
        return true;
    }
    if duration_ms >= 30_000 && remaining_ms <= 5_000 {
        return true;
    }
    if duration_ms >= 5_000 && remaining_ms <= 2_000 {
        return true;
    }
    false
}

pub fn should_resume(position_ms: u64, duration_ms: Option<u64>) -> Option<u64> {
    if position_ms == 0 || is_near_end(position_ms, duration_ms) {
        None
    } else {
        Some(position_ms)
    }
}

impl Entry {
    fn valid(&self) -> bool {
        is_valid_video_id(&self.video_id)
            && !self.title.trim().is_empty()
            && self.title.len() <= MAX_TEXT_BYTES
            && self.channel.len() <= MAX_TEXT_BYTES
            && self
                .duration_ms
                .is_none_or(|duration| duration > 0 && self.position_ms <= duration)
    }

    pub fn resume_position_ms(&self) -> Option<u64> {
        should_resume(self.position_ms, self.duration_ms)
    }

    pub fn is_near_end(&self) -> bool {
        is_near_end(self.position_ms, self.duration_ms)
    }

    pub fn is_short(&self) -> bool {
        if self.duration_ms.is_some_and(|d| d > 0 && d <= 62_000) {
            return true;
        }
        let title_lower = self.title.to_lowercase();
        let channel_lower = self.channel.to_lowercase();
        title_lower.contains("#shorts")
            || title_lower.contains("#short")
            || title_lower.contains("shorts")
            || channel_lower.contains("shorts")
    }

    pub fn is_podcast(&self) -> bool {
        if self.duration_ms.is_some_and(|d| d >= 1_500_000) {
            return true;
        }
        const KEYWORDS: &[&str] = &[
            "podcast",
            "cortes",
            "entrevista",
            "episódio",
            "episodio",
            "papo",
            "conversa",
            "achismos",
            "flow",
            "inteligência ltda",
            "podpah",
            "monark",
            "debate",
        ];
        entry_matches_keywords(&self.title, &self.channel, KEYWORDS)
    }

    pub fn is_music(&self) -> bool {
        const KEYWORDS: &[&str] = &[
            "music",
            "música",
            "musica",
            "song",
            "clipe",
            "official video",
            "live from",
            "vevo",
            "acoustic",
            "acústico",
            "violão",
            "guitar",
            "cover",
            "lyrics",
            "letra",
            "album",
            "álbum",
            "feat",
            "ft.",
            "banda",
            "remix",
            "soundtrack",
        ];
        if entry_matches_keywords(&self.title, &self.channel, &["asmr"]) {
            return false;
        }
        entry_matches_keywords(&self.title, &self.channel, KEYWORDS)
    }
}

fn entry_matches_keywords(title: &str, channel: &str, keywords_lower: &[&str]) -> bool {
    let title_lower = title.to_lowercase();
    let channel_lower = channel.to_lowercase();
    keywords_lower
        .iter()
        .any(|kw| title_lower.contains(kw) || channel_lower.contains(kw))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MergeSummary {
    pub added: usize,
    pub conflicts: usize,
    pub progress_updated: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordTickOutcome {
    pub changed: bool,
    pub ranking_changed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    entries: Vec<Entry>,
}

impl History {
    /// Adds missing videos and keeps the larger playback progress on conflicts.
    pub fn merged(&self, imported: &History) -> (History, MergeSummary) {
        let mut merged = self.clone();
        let mut summary = MergeSummary::default();
        for entry in imported.entries.iter().rev() {
            match merged.get(&entry.video_id).cloned() {
                Some(mut current) if current.position_ms >= entry.position_ms => {
                    summary.conflicts += 1;
                    if entry.watched_at_ms > current.watched_at_ms {
                        current.watched_at_ms = entry.watched_at_ms;
                        let _ = merged.record(current);
                    }
                }
                Some(current) => {
                    summary.conflicts += 1;
                    summary.progress_updated += 1;
                    let mut updated = entry.clone();
                    updated.watched_at_ms = current.watched_at_ms.max(entry.watched_at_ms);
                    let _ = merged.record(updated);
                }
                None => {
                    if merged.record(entry.clone()).is_ok() {
                        summary.added += 1;
                    }
                }
            }
        }
        (merged, summary)
    }

    /// Entries are ordered from most recently recorded to oldest.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn get(&self, video_id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.video_id == video_id)
    }
    pub fn resume_position_ms(&self, video_id: &str) -> Option<u64> {
        self.get(video_id).and_then(Entry::resume_position_ms)
    }
    /// Replaces an existing video and moves it to the front.
    pub fn record(&mut self, entry: Entry) -> io::Result<()> {
        if !entry.valid() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid history entry",
            ));
        }
        if let Some(first) = self.entries.first_mut()
            && first.video_id == entry.video_id
        {
            *first = entry;
            return Ok(());
        }
        self.entries.retain(|old| old.video_id != entry.video_id);
        self.entries.insert(0, entry);
        self.entries.truncate(MAX_ENTRIES);
        Ok(())
    }
    /// Fast path for active playback ticks: updates the front entry in-place when
    /// `video_id` already leads history, reporting whether recommendation-relevant
    /// state changed so callers do not rebuild feed rankings every video frame.
    pub fn record_active_tick(&mut self, entry: Entry) -> io::Result<RecordTickOutcome> {
        if !entry.valid() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid history entry",
            ));
        }
        if let Some(first) = self.entries.first_mut()
            && first.video_id == entry.video_id
        {
            if *first == entry {
                return Ok(RecordTickOutcome {
                    changed: false,
                    ranking_changed: false,
                });
            }
            let ranking_changed = first.title != entry.title
                || first.channel != entry.channel
                || first.duration_ms.is_some() != entry.duration_ms.is_some()
                || first.is_near_end() != entry.is_near_end()
                || first.resume_position_ms().is_some() != entry.resume_position_ms().is_some();
            *first = entry;
            return Ok(RecordTickOutcome {
                changed: true,
                ranking_changed,
            });
        }
        self.record(entry)?;
        Ok(RecordTickOutcome {
            changed: true,
            ranking_changed: true,
        })
    }
    pub fn remove(&mut self, video_id: &str) -> bool {
        let previous = self.entries.len();
        self.entries.retain(|entry| entry.video_id != video_id);
        previous != self.entries.len()
    }
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoryFilter {
    #[default]
    All,
    Videos,
    Shorts,
    Podcasts,
    Music,
    InProgress,
    Completed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DateGroup {
    Today,
    Yesterday,
    ThisWeek,
    Older,
}

impl DateGroup {
    pub fn label_pt(self) -> &'static str {
        match self {
            Self::Today => "Hoje",
            Self::Yesterday => "Ontem",
            Self::ThisWeek => "Esta semana",
            Self::Older => "Mais antigos",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::ThisWeek => "This week",
            Self::Older => "Older",
        }
    }

    pub fn label(self, pt: bool) -> &'static str {
        if pt { self.label_pt() } else { self.label_en() }
    }
}

#[cfg(test)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WatchStats {
    pub total_videos: usize,
    pub completed_count: usize,
    pub top_channels: Vec<(String, usize)>,
}

pub fn filter_entries<'a>(
    entries: &'a [Entry],
    query: &str,
    filter: HistoryFilter,
) -> Vec<&'a Entry> {
    let q = query.trim().to_lowercase();
    entries
        .iter()
        .filter(|entry| {
            let matches_filter = match filter {
                HistoryFilter::All => true,
                HistoryFilter::Videos => !entry.is_short(),
                HistoryFilter::Shorts => entry.is_short(),
                HistoryFilter::Podcasts => entry.is_podcast(),
                HistoryFilter::Music => entry.is_music(),
                HistoryFilter::InProgress => !entry.is_near_end(),
                HistoryFilter::Completed => entry.is_near_end(),
            };
            if !matches_filter {
                return false;
            }
            if q.is_empty() {
                return true;
            }
            entry.title.to_lowercase().contains(&q) || entry.channel.to_lowercase().contains(&q)
        })
        .collect()
}

pub fn date_group_at(
    watched_at_ms: Option<u64>,
    now_ms: u64,
    utc_offset_minutes: i32,
) -> DateGroup {
    let Some(ts_ms) = watched_at_ms.filter(|&ts| ts > 0) else {
        return DateGroup::Older;
    };
    const DAY_MS: i128 = 86_400_000;
    let offset_ms = i128::from(utc_offset_minutes) * 60_000;
    let now_day = (i128::from(now_ms) + offset_ms).div_euclid(DAY_MS);
    let entry_day = (i128::from(ts_ms) + offset_ms).div_euclid(DAY_MS);
    let diff = now_day - entry_day;
    if diff <= 0 {
        DateGroup::Today
    } else if diff == 1 {
        DateGroup::Yesterday
    } else if diff <= 6 {
        DateGroup::ThisWeek
    } else {
        DateGroup::Older
    }
}

#[cfg(test)]
pub fn compute_watch_stats(entries: &[Entry]) -> WatchStats {
    use std::collections::HashMap;

    let total_videos = entries.len();
    let completed_count = entries.iter().filter(|entry| entry.is_near_end()).count();
    let mut channel_counts: HashMap<String, (String, usize)> = HashMap::new();
    for entry in entries {
        let trimmed = entry.channel.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_lowercase();
        let slot = channel_counts
            .entry(key)
            .or_insert_with(|| (trimmed.to_owned(), 0));
        slot.1 += 1;
    }
    let mut ranked: Vec<(String, usize)> = channel_counts.into_values().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(3);
    WatchStats {
        total_videos,
        completed_count,
        top_channels: ranked,
    }
}

pub fn current_unix_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(windows)]
pub fn local_utc_offset_minutes() -> i32 {
    #[repr(C)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    #[repr(C)]
    struct TimeZoneInformation {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: SystemTime,
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: SystemTime,
        daylight_bias: i32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetTimeZoneInformation(lp_time_zone_information: *mut TimeZoneInformation) -> u32;
    }
    let mut info = std::mem::MaybeUninit::<TimeZoneInformation>::zeroed();
    let mode = unsafe { GetTimeZoneInformation(info.as_mut_ptr()) };
    if mode == u32::MAX {
        return 0;
    }
    let info = unsafe { info.assume_init() };
    let extra = match mode {
        1 => info.standard_bias,
        2 => info.daylight_bias,
        _ => 0,
    };
    -(info.bias.saturating_add(extra))
}

#[cfg(not(windows))]
pub fn local_utc_offset_minutes() -> i32 {
    0
}

/// Suggested local path; no directory is created until save.
pub fn history_path() -> io::Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))?;
    let path = PathBuf::from(local);
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LOCALAPPDATA must be absolute",
        ));
    }
    Ok(path.join("FastTube").join("history.json"))
}

/// Missing files yield empty history; malformed files return InvalidData unchanged.
pub fn load(path: &Path) -> io::Result<History> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(History::default()),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(invalid_data());
    }
    let root: Value = serde_json::from_slice(&bytes).map_err(|_| invalid_data())?;
    if root.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(invalid_data());
    }
    let values = root
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(invalid_data)?;
    if values.len() > MAX_ENTRIES {
        return Err(invalid_data());
    }
    let mut entries = Vec::with_capacity(values.len());
    let mut seen = HashSet::new();
    for value in values {
        let text = |key| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(invalid_data)
        };
        let duration_ms = match value.get("duration_ms") {
            Some(Value::Null) => None,
            Some(value) => Some(value.as_u64().ok_or_else(invalid_data)?),
            None => None,
        };
        let watched_at_ms = match value.get("watched_at_ms") {
            Some(Value::Null) | None => None,
            Some(value) => Some(value.as_u64().ok_or_else(invalid_data)?),
        };
        let entry = Entry {
            video_id: text("video_id")?,
            title: text("title")?,
            channel: text("channel")?,
            position_ms: value
                .get("position_ms")
                .and_then(Value::as_u64)
                .ok_or_else(invalid_data)?,
            duration_ms,
            watched_at_ms,
        };
        if !entry.valid() || !seen.insert(entry.video_id.clone()) {
            return Err(invalid_data());
        }
        entries.push(entry);
    }
    Ok(History { entries })
}

fn invalid_data() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid local video history")
}

/// Syncs a temporary file alongside the destination before replacing it.
pub fn save(path: &Path, history: &History) -> io::Result<()> {
    let entries: Vec<Value> = history
        .entries
        .iter()
        .map(|entry| {
            json!({
                "video_id": entry.video_id, "title": entry.title, "channel": entry.channel,
                "position_ms": entry.position_ms, "duration_ms": entry.duration_ms,
                "watched_at_ms": entry.watched_at_ms,
            })
        })
        .collect();
    let bytes = serde_json::to_vec(&json!({ "version": 1, "entries": entries }))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "history exceeds file size limit",
        ));
    }
    write_atomic(path, &bytes)
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
        })?;
    fs::create_dir_all(parent)?;
    let (temporary, mut file) = loop {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".fasttube-history-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0x1 | 0x8) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(number: usize) -> Entry {
        Entry {
            video_id: format!("{number:011}"),
            title: "Video".into(),
            channel: "Channel".into(),
            position_ms: 12,
            duration_ms: Some(100),
            watched_at_ms: None,
        }
    }
    fn test_path() -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "fasttube-history-test-{}-{}",
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ))
            .join("history.json")
    }
    #[test]
    fn missing_duration_and_unicode_urls_are_safe() {
        let path = test_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"version":1,"entries":[{"video_id":"00000000001","title":"Video","channel":"","position_ms":42}]}"#).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.entries()[0].duration_ms, None);
        assert_eq!(loaded.entries()[0].watched_at_ms, None);
        assert_eq!(youtube_video_id("aaaaaaaéaaaa"), None);
        assert_eq!(youtube_video_id("aaaaaaéaaaa"), None);
        assert!(!is_near_end(u64::MAX / 2, Some(u64::MAX)));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn deduplicates_limits_and_validates() {
        let mut history = History::default();
        for number in 0..=MAX_ENTRIES {
            history.record(entry(number)).unwrap();
        }
        assert_eq!(history.entries().len(), MAX_ENTRIES);
        assert!(history.get(&entry(0).video_id).is_none());
        let mut updated = entry(1);
        updated.position_ms = 90;
        history.record(updated.clone()).unwrap();
        assert_eq!(history.entries()[0], updated);
        assert_eq!(history.entries().len(), MAX_ENTRIES);
        let before = history.clone();
        updated.position_ms = 101;
        assert_eq!(
            history.record(updated).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(history, before);
        assert!(history.remove(&entry(1).video_id));
        history.clear();
        assert!(history.entries().is_empty());
    }
    #[test]
    fn roundtrip_and_replace() {
        let path = test_path();
        assert_eq!(load(&path).unwrap(), History::default());
        let mut history = History::default();
        let mut e1 = entry(1);
        e1.watched_at_ms = Some(1_700_000_000_000);
        history.record(e1).unwrap();
        save(&path, &history).unwrap();
        history.record(entry(2)).unwrap();
        save(&path, &history).unwrap();
        assert_eq!(load(&path).unwrap(), history);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn invalid_files_are_preserved() {
        let path = test_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let duplicate = json!({"video_id":"00000000001","title":"Video","channel":"","position_ms":1,"duration_ms":null});
        let malformed = json!({"version":1,"entries":[duplicate.clone(),duplicate]})
            .to_string()
            .into_bytes();
        for bytes in [
            b"broken".to_vec(),
            json!({"version":2,"entries":[]}).to_string().into_bytes(),
            malformed,
            vec![b' '; MAX_FILE_BYTES as usize + 1],
        ] {
            fs::write(&path, &bytes).unwrap();
            assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn validates_youtube_video_urls_and_rejects_other_sources() {
        for (url, expected) in [
            ("https://www.youtube.com/watch?v=dQw4w9WgXcQ", "dQw4w9WgXcQ"),
            (
                "http://youtube.com/watch?feature=shared&v=abc_DEF-123&t=42s",
                "abc_DEF-123",
            ),
            (
                "https://m.youtube.com/watch?v=0123456789A#t=10",
                "0123456789A",
            ),
            (
                "https://music.youtube.com/watch?v=_-aBcDeFgHi",
                "_-aBcDeFgHi",
            ),
            ("https://youtu.be/dQw4w9WgXcQ?si=test", "dQw4w9WgXcQ"),
            ("https://www.youtube.com/shorts/dQw4w9WgXcQ", "dQw4w9WgXcQ"),
            ("https://www.youtube.com/embed/dQw4w9WgXcQ", "dQw4w9WgXcQ"),
            ("https://www.youtube.com/live/dQw4w9WgXcQ", "dQw4w9WgXcQ"),
            ("https://www.youtube.com/v/dQw4w9WgXcQ", "dQw4w9WgXcQ"),
            (
                "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
                "dQw4w9WgXcQ",
            ),
        ] {
            assert_eq!(
                youtube_video_id(url).as_deref(),
                Some(expected),
                "url={url}"
            );
        }

        for invalid in [
            "",
            "dQw4w9WgXcQ",
            "ftp://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://example.com/watch?v=dQw4w9WgXcQ",
            "https://vimeo.com/12345678901",
            "https://notyoutube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com.evil.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com@evil.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com/playlist?list=PL12345678901",
            "https://www.youtube.com/results?search_query=dQw4w9WgXcQ",
            "https://www.youtube.com/@channel",
            "https://www.youtube.com/watch?v=short",
            "https://www.youtube.com/watch?v=too_long_video_id",
            "https://www.youtube.com/watch?v=bad!char123",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&v=0123456789A",
            "https://youtu.be/dQw4w9WgXcQ/extra",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ/extra",
        ] {
            assert_eq!(youtube_video_id(invalid), None, "invalid={invalid}");
        }
    }
    #[test]
    fn avoids_resuming_at_zero_or_near_end() {
        assert_eq!(should_resume(0, None), None);
        assert_eq!(should_resume(0, Some(120_000)), None);
        assert_eq!(should_resume(25_000, None), Some(25_000));
        assert_eq!(should_resume(25_000, Some(120_000)), Some(25_000));
        assert_eq!(should_resume(110_000, Some(120_000)), Some(110_000));
        assert_eq!(should_resume(115_000, Some(120_000)), None);
        assert_eq!(should_resume(118_000, Some(120_000)), None);
        assert_eq!(should_resume(120_000, Some(120_000)), None);
        assert_eq!(should_resume(96, Some(100)), None);
    }

    #[test]
    fn merge_preserves_greater_local_progress_and_applies_newer_imported_progress() {
        let mut local = History::default();
        let mut higher_local = entry(1);
        higher_local.position_ms = 80;
        local.record(higher_local.clone()).unwrap();
        let mut imported = History::default();
        let mut lower_imported = entry(1);
        lower_imported.position_ms = 40;
        imported.record(lower_imported).unwrap();
        let mut newer_imported = entry(2);
        newer_imported.position_ms = 60;
        imported.record(newer_imported.clone()).unwrap();
        let (merged, summary) = local.merged(&imported);
        assert_eq!(merged.get(&higher_local.video_id), Some(&higher_local));
        assert_eq!(merged.get(&newer_imported.video_id), Some(&newer_imported));
        assert_eq!(summary.added, 1);
        assert_eq!(summary.conflicts, 1);
        assert_eq!(summary.progress_updated, 0);
    }

    #[test]
    fn record_active_tick_skips_ranking_churn_on_routine_position_ticks() {
        let mut history = History::default();

        let mut e = entry(1);
        e.position_ms = 5_000;
        e.duration_ms = Some(120_000);
        let first = history.record_active_tick(e.clone()).unwrap();
        assert!(first.changed);
        assert!(first.ranking_changed);

        e.position_ms = 15_000;
        let second = history.record_active_tick(e.clone()).unwrap();
        assert!(second.changed);
        assert!(!second.ranking_changed);

        // No-op identical tick does not even mark history dirty.
        let same = history.record_active_tick(e.clone()).unwrap();
        assert!(!same.changed);
        assert!(!same.ranking_changed);

        // Crossing near-end threshold counts as a ranking-relevant completion signal.
        e.position_ms = 116_000;
        let completed = history.record_active_tick(e).unwrap();
        assert!(completed.changed);
        assert!(completed.ranking_changed);
    }

    #[test]
    fn filters_entries_groups_dates_and_computes_stats() {
        const DAY: u64 = 86_400_000;
        let now = 20 * DAY + 12 * 3_600_000;
        assert_eq!(
            date_group_at(Some(now - 3_600_000), now, 0),
            DateGroup::Today
        );
        assert_eq!(date_group_at(Some(now - DAY), now, 0), DateGroup::Yesterday);
        assert_eq!(
            date_group_at(Some(now - 4 * DAY), now, 0),
            DateGroup::ThisWeek
        );
        assert_eq!(
            date_group_at(Some(now - 10 * DAY), now, 0),
            DateGroup::Older
        );
        assert_eq!(date_group_at(None, now, 0), DateGroup::Older);

        let entries = vec![
            Entry {
                video_id: "aaaaaaaaaaa".into(),
                title: "Rust Async".into(),
                channel: "Canal A".into(),
                position_ms: 98,
                duration_ms: Some(100),
                watched_at_ms: Some(now),
            },
            Entry {
                video_id: "bbbbbbbbbbb".into(),
                title: "Outro Vídeo".into(),
                channel: "Canal B".into(),
                position_ms: 40,
                duration_ms: Some(100),
                watched_at_ms: Some(now - DAY),
            },
            Entry {
                video_id: "ccccccccccc".into(),
                title: "Tokio Guia".into(),
                channel: "canal a".into(),
                position_ms: 99,
                duration_ms: Some(100),
                watched_at_ms: Some(now - 3 * DAY),
            },
            Entry {
                video_id: "ddddddddddd".into(),
                title: "Podcast".into(),
                channel: "Canal C".into(),
                position_ms: 10,
                duration_ms: Some(100),
                watched_at_ms: None,
            },
        ];

        assert_eq!(filter_entries(&entries, "", HistoryFilter::All).len(), 4);
        assert_eq!(
            filter_entries(&entries, "", HistoryFilter::Completed).len(),
            2
        );
        assert_eq!(
            filter_entries(&entries, "", HistoryFilter::InProgress).len(),
            2
        );
        assert_eq!(
            filter_entries(&entries, "canal b", HistoryFilter::All).len(),
            1
        );
        assert_eq!(
            filter_entries(&entries, "tokio", HistoryFilter::Completed).len(),
            1
        );

        let category_entries = vec![
            Entry {
                video_id: "11111111111".into(),
                title: "Rust Tutorial Completo".into(),
                channel: "Dev Channel".into(),
                position_ms: 120_000,
                duration_ms: Some(600_000),
                watched_at_ms: Some(now),
            },
            Entry {
                video_id: "22222222222".into(),
                title: "Clip rápido #shorts".into(),
                channel: "Shorts Canal".into(),
                position_ms: 15_000,
                duration_ms: Some(30_000),
                watched_at_ms: Some(now),
            },
            Entry {
                video_id: "33333333333".into(),
                title: "Podcast Tecnologia Episódio 10".into(),
                channel: "Tech Talk".into(),
                position_ms: 400_000,
                duration_ms: Some(2_400_000),
                watched_at_ms: Some(now),
            },
            Entry {
                video_id: "44444444444".into(),
                title: "Acoustic Guitar Cover".into(),
                channel: "Music Studio".into(),
                position_ms: 90_000,
                duration_ms: Some(210_000),
                watched_at_ms: Some(now),
            },
            Entry {
                video_id: "55555555555".into(),
                title: "Haircut asmr".into(),
                channel: "klara".into(),
                position_ms: 300_000,
                duration_ms: Some(660_000),
                watched_at_ms: Some(now),
            },
        ];
        assert_eq!(
            filter_entries(&category_entries, "", HistoryFilter::Videos).len(),
            4
        );
        assert_eq!(
            filter_entries(&category_entries, "", HistoryFilter::Shorts).len(),
            1
        );
        assert_eq!(
            filter_entries(&category_entries, "", HistoryFilter::Podcasts).len(),
            1
        );
        assert_eq!(
            filter_entries(&category_entries, "", HistoryFilter::Music).len(),
            1
        );

        let stats = compute_watch_stats(&entries);
        assert_eq!(stats.total_videos, 4);
        assert_eq!(stats.completed_count, 2);
        assert_eq!(stats.top_channels[0], ("Canal A".into(), 2));
        assert_eq!(stats.top_channels.len(), 3);
    }
}
