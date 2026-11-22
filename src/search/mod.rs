mod channel;
mod filters;
mod playlist;
mod query;
#[cfg(test)]
mod tests;

pub use channel::*;
pub use filters::*;
pub use playlist::*;
pub(crate) use playlist::{
    cached_playback_metadata, is_allowed_subtitle_url, prime_playback_metadata_cache,
    sanitize_subtitle_tracks,
};
pub use query::*;

use channel::parse_youtube_channel_url;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const QUERY_TIMEOUT: Duration = Duration::from_secs(45);

const BROWSER_FEED_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone, Debug, PartialEq)]
pub struct StoryboardFragment {
    pub url: String,
    pub duration_secs: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoryboardSpec {
    pub rows: u32,
    pub columns: u32,
    pub thumb_width: u32,
    pub thumb_height: u32,
    pub fragments: Vec<StoryboardFragment>,
}

impl StoryboardSpec {
    /// Resolves the storyboard sheet URL and normalized UV rectangle for a timestamp.
    pub fn lookup(&self, time_secs: f64) -> Option<(&str, eframe::egui::Rect)> {
        if self.rows == 0
            || self.columns == 0
            || self.fragments.is_empty()
            || !time_secs.is_finite()
        {
            return None;
        }
        let target = time_secs.max(0.0);
        let mut elapsed = 0.0;
        let per_sheet = (self.rows as usize) * (self.columns as usize);
        if per_sheet == 0 {
            return None;
        }
        for (idx, frag) in self.fragments.iter().enumerate() {
            let dur = frag.duration_secs.max(1.0);
            let is_last = idx + 1 == self.fragments.len();
            if target < elapsed + dur || is_last {
                let local = (target - elapsed).clamp(0.0, dur);
                let frame_idx = ((local / dur) * (per_sheet as f64))
                    .floor()
                    .clamp(0.0, (per_sheet - 1) as f64) as usize;
                let col = (frame_idx % self.columns as usize) as f32;
                let row = (frame_idx / self.columns as usize) as f32;
                let u0 = col / self.columns as f32;
                let v0 = row / self.rows as f32;
                let u1 = (col + 1.0) / self.columns as f32;
                let v1 = (row + 1.0) / self.rows as f32;
                return Some((
                    frag.url.as_str(),
                    eframe::egui::Rect::from_min_max(
                        eframe::egui::pos2(u0, v0),
                        eframe::egui::pos2(u1, v1),
                    ),
                ));
            }
            elapsed += dur;
        }
        None
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PlaybackExtraInfo {
    pub is_live: bool,
    pub title: Option<String>,
    pub channel: Option<String>,
    pub channel_id: Option<String>,
    pub channel_url: Option<String>,
    pub description: Option<String>,
    pub concurrent_view_count: Option<u64>,
    pub storyboard: Option<StoryboardSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Video {
    pub title: String,
    pub channel: String,
    pub url: String,
    pub thumbnail: String,
    pub duration: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchResultKind {
    #[default]
    Video,
    Short,
    Channel,
    Playlist,
    Movie,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VideoMetadata {
    pub view_count: Option<u64>,
    pub upload_date: Option<String>,
    pub description: Option<String>,
    pub channel_id: Option<String>,
    pub channel_url: Option<String>,
    pub kind: SearchResultKind,
    pub is_live: bool,
    pub published_time_text: Option<String>,
    pub view_count_text: Option<String>,
    pub channel_avatar_url: Option<String>,
    pub channel_verified: bool,
    pub badges: Vec<String>,
    pub playlist_count: Option<String>,
    pub subscriber_count_text: Option<String>,
    pub upcoming_start_unix: Option<i64>,
}

impl VideoMetadata {
    /// Fills any missing (`None`) metadata fields from `other` without overwriting populated fields.
    pub(crate) fn merge_missing_from(&mut self, other: &Self) {
        if self.view_count.is_none() {
            self.view_count = other.view_count;
        }
        if self.upload_date.is_none() {
            self.upload_date = other.upload_date.clone();
        }
        if self.description.is_none() {
            self.description = other.description.clone();
        }
        if self.channel_id.is_none() {
            self.channel_id = other.channel_id.clone();
        }
        if self.channel_url.is_none() {
            self.channel_url = other.channel_url.clone();
        }
        if self.published_time_text.is_none() {
            self.published_time_text = other.published_time_text.clone();
        }
        if self.view_count_text.is_none() {
            self.view_count_text = other.view_count_text.clone();
        }
        if self.channel_avatar_url.is_none() {
            self.channel_avatar_url = other.channel_avatar_url.clone();
        }
        if self.playlist_count.is_none() {
            self.playlist_count = other.playlist_count.clone();
        }
        if self.subscriber_count_text.is_none() {
            self.subscriber_count_text = other.subscriber_count_text.clone();
        }
        if self.upcoming_start_unix.is_none() {
            self.upcoming_start_unix = other.upcoming_start_unix;
        }
        if other.channel_verified {
            self.channel_verified = true;
        }
        if self.badges.is_empty() && !other.badges.is_empty() {
            self.badges = other.badges.clone();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResult {
    pub video: Video,
    pub metadata: VideoMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HomeChannel {
    pub title: String,
    pub channel_id: String,
    pub avatar_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct HomeFeed {
    pub items: Vec<SearchResult>,
    pub playlists: Vec<String>,
    pub channels: Vec<HomeChannel>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitleTrack {
    pub language: String,
    pub automatic: bool,
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VideoPlaybackMetadata {
    pub heights: Vec<u32>,
    pub subtitles: Vec<SubtitleTrack>,
    pub is_live: bool,
    pub title: Option<String>,
    pub channel: Option<String>,
    pub channel_id: Option<String>,
    pub channel_url: Option<String>,
    pub description: Option<String>,
    pub concurrent_view_count: Option<u64>,
    pub storyboard: Option<StoryboardSpec>,
}

const SUBTITLE_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

fn percent_encode_query_component(raw: &str) -> String {
    let mut encoded = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else if byte == b' ' {
            encoded.push('+');
        } else {
            use std::fmt::Write;
            let _ = write!(&mut encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChannelTarget {
    ChannelId(String),
    Handle(String),
}

impl Default for ChannelTarget {
    fn default() -> Self {
        Self::Handle("@channel".to_owned())
    }
}

impl ChannelTarget {
    pub fn canonical_url(&self) -> String {
        match self {
            Self::ChannelId(id) => format!("https://www.youtube.com/channel/{id}"),
            Self::Handle(handle) => format!("https://www.youtube.com/{handle}"),
        }
    }

    pub fn featured_tab_url(&self) -> String {
        format!("{}/featured", self.canonical_url())
    }

    pub fn videos_tab_url(&self) -> String {
        format!("{}/videos", self.canonical_url())
    }

    pub fn shorts_tab_url(&self) -> String {
        format!("{}/shorts", self.canonical_url())
    }

    pub fn streams_tab_url(&self) -> String {
        format!("{}/streams", self.canonical_url())
    }

    pub fn playlists_tab_url(&self) -> String {
        format!("{}/playlists", self.canonical_url())
    }

    pub fn about_tab_url(&self) -> String {
        format!("{}/about", self.canonical_url())
    }

    pub fn search_url(&self, query: &str) -> Option<String> {
        let base = format!("{}/search", self.canonical_url());
        let mut url = url::Url::parse(&base).ok()?;
        url.query_pairs_mut().append_pair("query", query.trim());
        Some(url.into())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelExternalLink {
    pub title: String,
    pub url: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChannelShelfKind {
    #[default]
    Videos,
    Shorts,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelHomeShelf {
    pub title: String,
    pub subtitle: Option<String>,
    pub kind: ChannelShelfKind,
    pub playlist_id: Option<String>,
    pub items: Vec<SearchResult>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChannelVideosSort {
    #[default]
    Latest,
    Popular,
    Oldest,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelPlaylist {
    pub id: String,
    pub title: String,
    pub url: String,
    pub thumbnail: String,
    pub item_count: Option<u64>,
    pub description: Option<String>,
    pub privacy: crate::playlist_sync::PlaylistPrivacy,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelAboutInfo {
    pub channel_id: Option<String>,
    pub title: Option<String>,
    pub handle: Option<String>,
    pub canonical_url: Option<String>,
    pub description: Option<String>,
    pub subscriber_count: Option<u64>,
    pub view_count: Option<u64>,
    pub subscriber_count_text: Option<String>,
    pub video_count_text: Option<String>,
    pub view_count_text: Option<String>,
    pub country: Option<String>,
    pub joined_date_text: Option<String>,
    pub links: Vec<ChannelExternalLink>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub verified: Option<bool>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelVideosBatch {
    pub videos: Vec<SearchResult>,
    pub start: usize,
    pub end: usize,
    pub has_more: bool,
    pub reached_max_limit: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChannelPage {
    pub target: ChannelTarget,
    pub channel_id: Option<String>,
    pub title: Option<String>,
    pub handle: Option<String>,
    pub canonical_url: String,
    pub description: Option<String>,
    pub description_preview: Option<String>,
    pub subscriber_count: Option<u64>,
    pub view_count: Option<u64>,
    pub subscriber_count_text: Option<String>,
    pub video_count_text: Option<String>,
    pub view_count_text: Option<String>,
    pub country: Option<String>,
    pub joined_date_text: Option<String>,
    pub links: Vec<ChannelExternalLink>,
    pub more_links_label: Option<String>,
    pub about_continuation: Option<String>,
    pub featured_video: Option<SearchResult>,
    pub home_shelves: Vec<ChannelHomeShelf>,
    pub videos_popular_continuation: Option<String>,
    pub videos_oldest_continuation: Option<String>,
    pub has_home_tab: bool,
    pub has_videos_tab: bool,
    pub has_shorts_tab: bool,
    pub has_streams_tab: bool,
    pub has_playlists_tab: bool,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub verified: Option<bool>,
    pub tags: Vec<String>,
    pub videos: Vec<SearchResult>,
    pub initial_playlists: Vec<ChannelPlaylist>,
    pub tab_endpoints: Vec<crate::innertube::ChannelTabEndpoint>,
    pub sort_chips: Vec<crate::innertube::ChannelSortChip>,
    pub next_continuation: Option<String>,
    pub community_posts: Vec<crate::innertube::ChannelCommunityPost>,
}

#[derive(Clone, Debug)]
pub struct ChannelFetchOutcome {
    pub page: ChannelPage,
    pub from_cache: bool,
}

static YT_DLP_CACHE_DIR: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

pub fn set_yt_dlp_cache_dir(base_cache_dir: Option<PathBuf>) {
    let target = base_cache_dir.map(|dir| crate::settings::yt_dlp_cache_subdir(&dir));
    let lock = YT_DLP_CACHE_DIR.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock().unwrap_or_else(|error| error.into_inner());
    *guard = target;
}

pub fn effective_yt_dlp_cache_dir() -> Option<PathBuf> {
    let lock = YT_DLP_CACHE_DIR.get_or_init(|| Mutex::new(None));
    lock.lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

pub fn clear_in_memory_caches() {
    playlist::clear_playback_caches();
    channel::clear_channel_page_cache();
}

pub(crate) fn yt_dlp_command() -> Command {
    yt_dlp()
}

fn yt_dlp() -> Command {
    let program = crate::updater::managed_yt_dlp_path()
        .map(std::path::PathBuf::into_os_string)
        .unwrap_or_else(|| "yt-dlp".into());
    let mut command = Command::new(program);
    if let Some(cache_dir) = effective_yt_dlp_cache_dir() {
        let _ = std::fs::create_dir_all(&cache_dir);
        command.arg("--cache-dir").arg(cache_dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

#[cfg(test)]
pub(crate) fn bundled_yt_dlp(executable: &std::path::Path) -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    let name = "yt-dlp.exe";
    #[cfg(not(windows))]
    let name = "yt-dlp";
    let candidate = executable.parent()?.join(name);
    candidate.is_file().then_some(candidate)
}

pub(crate) fn ytdl_hook_path_option(path: &std::path::Path) -> String {
    let value = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace(',', "\\,");
    format!("ytdl_hook-ytdl_path={value}")
}

fn format_ytdl_hook_raw_options(cache_dir: Option<&std::path::Path>) -> String {
    let mut opts = String::from("no-playlist=");
    if let Some(dir) = cache_dir
        && let Some(raw) = dir.to_str().map(str::trim).filter(|s| !s.is_empty())
        && !raw.contains([',', '=', '\n', '\r'])
    {
        opts.push_str(",cache-dir=");
        opts.push_str(raw);
    }
    opts
}

pub(crate) fn ytdl_hook_raw_options() -> String {
    let dir = effective_yt_dlp_cache_dir();
    if let Some(path) = &dir {
        let _ = std::fs::create_dir_all(path);
    }
    format_ytdl_hook_raw_options(dir.as_deref())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChannelFeedTab {
    #[default]
    Videos,
    Shorts,
    Streams,
}

fn extract_yt_initial_data(html: &str) -> Option<Value> {
    for marker in [
        "var ytInitialData = ",
        "window[\"ytInitialData\"] = ",
        "ytInitialData = ",
    ] {
        if let Some(pos) = html.find(marker) {
            let rest = &html[pos + marker.len()..];
            let trimmed = rest.trim_start();
            if trimmed.starts_with('{') {
                let mut stream = serde_json::Deserializer::from_str(trimmed).into_iter::<Value>();
                if let Some(Ok(val)) = stream.next() {
                    return Some(val);
                }
            }
        }
    }
    None
}

pub(crate) fn normalize_youtube_asset_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("//") {
        format!("https:{trimmed}")
    } else {
        trimmed.to_owned()
    }
}

pub(crate) fn yt_text(value: Option<&Value>) -> String {
    let Some(val) = value else {
        return String::new();
    };
    if let Some(simple) = val.get("simpleText").and_then(Value::as_str) {
        return simple.trim().to_owned();
    }
    if let Some(content) = val.get("content").and_then(Value::as_str) {
        return content.trim().to_owned();
    }
    if let Some(runs) = val.get("runs").and_then(Value::as_array) {
        let mut out = String::new();
        for run in runs {
            if let Some(text) = run.get("text").and_then(Value::as_str) {
                out.push_str(text);
            }
        }
        return out.trim().to_owned();
    }
    String::new()
}

pub(crate) fn select_yt_thumbnail(thumbnails: Option<&Value>, target_width: u64) -> String {
    let Some(arr) = thumbnails.and_then(Value::as_array) else {
        return String::new();
    };
    arr.iter()
        .filter_map(|item| {
            let url = item.get("url").and_then(Value::as_str)?;
            let norm = normalize_youtube_asset_url(url);
            valid_http_url(&norm)?;
            let width = item.get("width").and_then(Value::as_u64).unwrap_or(0);
            Some((width, norm))
        })
        .min_by_key(|(w, _)| w.abs_diff(target_width))
        .map(|(_, url)| url)
        .unwrap_or_default()
}

pub(crate) fn normalize_youtube_spaces(raw: &str) -> String {
    raw.replace(['\u{00a0}', '\u{202f}'], " ").trim().to_owned()
}

pub(crate) fn format_epoch_iso8601_utc(epoch: i64) -> Option<String> {
    if epoch < 0 {
        return None;
    }
    let days = epoch.div_euclid(86_400);
    let rem = epoch.rem_euclid(86_400);
    let hour = rem / 3_600;
    let min = (rem % 3_600) / 60;
    let sec = rem % 60;

    // Howard Hinnant's civil_from_days
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    if !(1970..=9999).contains(&year) {
        return None;
    }
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z"
    ))
}

pub(crate) fn extract_yt_dlp_upload_date(value: &Value) -> Option<String> {
    let ts_iso = ["timestamp", "release_timestamp"]
        .into_iter()
        .find_map(|key| {
            let v = value.get(key)?;
            v.as_i64()
                .or_else(|| {
                    v.as_f64()
                        .filter(|f| f.is_finite() && *f > 0.0)
                        .map(|f| f as i64)
                })
                .filter(|&epoch| epoch > 0)
                .and_then(format_epoch_iso8601_utc)
        });
    ts_iso.or_else(|| nonempty_str(value, "upload_date").map(str::to_owned))
}

fn is_view_count_phrase(lower: &str) -> bool {
    lower.contains("view")
        || lower.contains("visualiza")
        || lower.contains("watching")
        || lower.contains("assistindo")
}

pub(crate) fn is_published_time_text(raw: &str) -> bool {
    let norm = normalize_youtube_spaces(raw);
    if norm.is_empty() {
        return false;
    }
    let lower = norm.to_lowercase();
    lower.ends_with(" ago")
        || lower.contains(" ago")
        || lower.starts_with("há ")
        || lower.contains(" há ")
        || lower.ends_with(" atrás")
        || lower.ends_with(" atras")
        || lower.starts_with("streamed ")
        || lower.starts_with("transmitido ")
        || lower.starts_with("transmissão ")
        || lower.starts_with("transmissao ")
        || lower.starts_with("premiered ")
        || lower.starts_with("premieres ")
        || lower.starts_with("estreou ")
        || lower.starts_with("estreia em ")
        || lower.starts_with("scheduled ")
        || lower.starts_with("agendado ")
        || lower.starts_with("programado ")
        || matches!(
            lower.as_str(),
            "agora mesmo" | "just now" | "hoje" | "today" | "ontem" | "yesterday"
        )
}

pub(crate) fn clean_published_time_text(raw: &str) -> Option<String> {
    let norm = normalize_youtube_spaces(raw);
    if norm.is_empty() {
        return None;
    }
    let collapsed = norm.split_whitespace().collect::<Vec<_>>().join(" ");
    (!collapsed.is_empty()).then_some(collapsed)
}

pub(crate) fn parse_video_info_views_and_date(
    raw: &str,
) -> (Option<u64>, Option<String>, Option<String>) {
    let norm = normalize_youtube_spaces(raw);
    if norm.is_empty() {
        return (None, None, None);
    }
    let segments: Vec<&str> = norm
        .split(['•', '·'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let mut view_count = None;
    let mut view_count_text = None;
    let mut published_time_text = None;

    for &seg in &segments {
        let lower = seg.to_lowercase();
        if view_count_text.is_none() && is_view_count_phrase(&lower) {
            view_count = parse_pt_br_compact_count(seg);
            view_count_text = clean_pt_br_view_label(seg);
        } else if published_time_text.is_none() && is_published_time_text(seg) {
            published_time_text = clean_published_time_text(seg);
        }
    }

    if published_time_text.is_none()
        && view_count_text.is_some()
        && let Some(&second) = segments.get(1)
        && second.chars().any(|c| c.is_ascii_digit())
    {
        published_time_text = clean_published_time_text(second);
    }

    (view_count, view_count_text, published_time_text)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LockupMetadataFields {
    pub channel: String,
    pub channel_id: Option<String>,
    pub channel_url: Option<String>,
    pub channel_verified: bool,
    pub is_live: bool,
    pub view_count: Option<u64>,
    pub view_count_text: Option<String>,
    pub published_time_text: Option<String>,
}

pub(crate) fn parse_lockup_metadata_rows(l: &Value) -> LockupMetadataFields {
    let mut out = LockupMetadataFields::default();
    let Some(rows) = l
        .pointer("/metadata/lockupMetadataViewModel/metadata/contentMetadataViewModel/metadataRows")
        .and_then(Value::as_array)
    else {
        return out;
    };

    for row in rows {
        let Some(parts) = row.get("metadataParts").and_then(Value::as_array) else {
            continue;
        };
        for part in parts {
            let text_obj = part.get("text").unwrap_or(part);
            let raw_text = text_obj
                .get("content")
                .and_then(Value::as_str)
                .map(normalize_youtube_spaces)
                .unwrap_or_default();
            if raw_text.is_empty() {
                continue;
            }
            let has_verified_attachment = text_obj
                .get("attachmentRuns")
                .and_then(Value::as_array)
                .is_some_and(|runs| !runs.is_empty());

            if let Some(cmd) = text_obj.pointer("/commandRuns/0/onTap/innertubeCommand") {
                let (cid, curl) = channel_url_from_browse_endpoint(Some(cmd));
                if cid.is_some() || curl.is_some() {
                    if out.channel_id.is_none() {
                        out.channel_id = cid;
                    }
                    if out.channel_url.is_none() {
                        out.channel_url = curl;
                    }
                    if out.channel.is_empty() {
                        out.channel = raw_text;
                    }
                    if has_verified_attachment {
                        out.channel_verified = true;
                    }
                    continue;
                }
            }

            let a11y = part
                .get("accessibilityLabel")
                .and_then(Value::as_str)
                .map(normalize_youtube_spaces)
                .filter(|s| !s.is_empty());
            let leading_icon = part
                .pointer("/leadingIcon/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let icon_suggests_views = leading_icon.contains("PLAY")
                || leading_icon.contains("EYE")
                || leading_icon.contains("VIEW");

            let segments: Vec<&str> = raw_text
                .split(['•', '·'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            let single_segment = segments.len() == 1;

            for seg in segments {
                let lower = seg.to_lowercase();
                let a11y_lower = if single_segment {
                    a11y.as_deref().map(str::to_lowercase)
                } else {
                    None
                };
                let is_views = is_view_count_phrase(&lower)
                    || a11y_lower.as_deref().is_some_and(is_view_count_phrase)
                    || (single_segment
                        && icon_suggests_views
                        && parse_pt_br_compact_count(seg).is_some());

                if is_views {
                    if lower.contains("watching")
                        || lower.contains("assistindo")
                        || a11y_lower
                            .as_deref()
                            .is_some_and(|s| s.contains("watching") || s.contains("assistindo"))
                    {
                        out.is_live = true;
                    }
                    if out.view_count.is_none() {
                        out.view_count = parse_pt_br_compact_count(seg).or_else(|| {
                            single_segment
                                .then_some(a11y.as_deref())
                                .flatten()
                                .and_then(parse_pt_br_compact_count)
                        });
                    }
                    if out.view_count_text.is_none() {
                        out.view_count_text = clean_pt_br_view_label(seg).or_else(|| {
                            single_segment
                                .then_some(a11y.as_deref())
                                .flatten()
                                .and_then(clean_pt_br_view_label)
                        });
                    }
                    continue;
                }

                let is_date = is_published_time_text(seg)
                    || (single_segment && a11y.as_deref().is_some_and(is_published_time_text));
                if is_date {
                    if out.published_time_text.is_none() {
                        out.published_time_text = clean_published_time_text(seg).or_else(|| {
                            single_segment
                                .then_some(a11y.as_deref())
                                .flatten()
                                .and_then(clean_published_time_text)
                        });
                    }
                    continue;
                }

                if lower == "ao vivo" || lower == "live" {
                    out.is_live = true;
                    continue;
                }

                if out.channel.is_empty()
                    && out.view_count.is_none()
                    && out.view_count_text.is_none()
                    && out.published_time_text.is_none()
                {
                    out.channel = seg.to_owned();
                    if has_verified_attachment {
                        out.channel_verified = true;
                    }
                }
            }
        }
    }

    out
}

pub(crate) fn parse_pt_br_compact_count(raw: &str) -> Option<u64> {
    let lower = normalize_youtube_spaces(raw).to_lowercase();
    if lower.is_empty() {
        return None;
    }
    let digits_only: String = lower
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .collect();
    if digits_only.is_empty() {
        return None;
    }
    let tokens: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric() && c != '.' && c != ',')
        .filter(|t| !t.is_empty())
        .collect();
    let has_suffix = |word: &str, short: char| {
        tokens.iter().any(|t| {
            *t == word
                || (t.len() > 1
                    && t.ends_with(short)
                    && t[..t.len() - 1]
                        .chars()
                        .all(|c| c.is_ascii_digit() || c == '.' || c == ','))
        })
    };
    let multiplier: f64 = if has_suffix("bi", 'b')
        || tokens
            .iter()
            .any(|t| matches!(*t, "bilhão" | "bilhões" | "bilhao" | "bilhoes"))
    {
        1_000_000_000.0
    } else if has_suffix("mil", 'k') {
        1_000.0
    } else if has_suffix("mi", 'm')
        || tokens
            .iter()
            .any(|t| matches!(*t, "milhão" | "milhões" | "milhao" | "milhoes"))
    {
        1_000_000.0
    } else {
        1.0
    };
    if multiplier > 1.0 {
        let normalized = digits_only.replace(',', ".");
        normalized
            .parse::<f64>()
            .ok()
            .map(|n| (n * multiplier).round() as u64)
    } else {
        let int_str: String = lower.chars().filter(char::is_ascii_digit).collect();
        int_str.parse::<u64>().ok()
    }
}

pub(crate) fn clean_pt_br_view_label(raw: &str) -> Option<String> {
    let trimmed = normalize_youtube_spaces(raw);
    if trimmed.is_empty() {
        return None;
    }
    let stripped = trimmed
        .strip_suffix(" visualizações")
        .or_else(|| trimmed.strip_suffix(" visualização"))
        .or_else(|| trimmed.strip_suffix(" views"))
        .or_else(|| trimmed.strip_suffix(" view"))
        .unwrap_or(&trimmed)
        .trim();
    (!stripped.is_empty()).then(|| stripped.to_owned())
}

pub(crate) fn channel_url_from_browse_endpoint(
    endpoint: Option<&Value>,
) -> (Option<String>, Option<String>) {
    let Some(browse) = endpoint.and_then(|ep| ep.get("browseEndpoint")) else {
        return (None, None);
    };
    let channel_id = browse
        .get("browseId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_channel_id(id))
        .map(str::to_owned);
    let canonical_base = browse
        .get("canonicalBaseUrl")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|base| {
            let full = if base.starts_with('/') {
                format!("https://www.youtube.com{base}")
            } else {
                base.to_owned()
            };
            parse_youtube_channel_url(&full).map(|target| target.canonical_url())
        });
    let channel_url = canonical_base.or_else(|| {
        channel_id
            .as_ref()
            .map(|id| format!("https://www.youtube.com/channel/{id}"))
    });
    (channel_id, channel_url)
}

pub(crate) fn has_verified_owner_badge(owner_badges: Option<&Value>) -> bool {
    let Some(arr) = owner_badges.and_then(Value::as_array) else {
        return false;
    };
    arr.iter().any(|badge| {
        let renderer = badge.get("metadataBadgeRenderer").unwrap_or(badge);
        let style = renderer.get("style").and_then(Value::as_str).unwrap_or("");
        let icon = renderer
            .pointer("/icon/iconType")
            .and_then(Value::as_str)
            .unwrap_or("");
        let tooltip = renderer
            .get("tooltip")
            .and_then(Value::as_str)
            .unwrap_or("");
        style.contains("VERIFIED")
            || icon.contains("CHECK")
            || icon.contains("OFFICIAL_ARTIST")
            || tooltip.to_lowercase().contains("verific")
    })
}

fn nonempty_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

pub(crate) fn valid_http_url(url: &str) -> Option<&str> {
    let trimmed = url.trim();
    if (trimmed.starts_with("https://") || trimmed.starts_with("http://"))
        && !trimmed
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        Some(trimmed)
    } else {
        None
    }
}

pub(crate) fn format_duration_seconds(seconds: u64) -> String {
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else if seconds > 0 {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    } else {
        String::new()
    }
}

fn select_video_thumbnail(value: &Value) -> String {
    value
        .get("thumbnails")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let raw = item.get("url").and_then(Value::as_str)?;
                    let norm = normalize_youtube_asset_url(raw);
                    valid_http_url(&norm)?;
                    let width = item.get("width").and_then(Value::as_u64).unwrap_or(0);
                    Some((width, norm))
                })
                .min_by_key(|(w, _)| w.abs_diff(320))
                .map(|(_, url)| url)
        })
        .or_else(|| {
            nonempty_str(value, "thumbnail").and_then(|raw| {
                let norm = normalize_youtube_asset_url(raw);
                valid_http_url(&norm).is_some().then_some(norm)
            })
        })
        .unwrap_or_default()
}

fn extract_channel_association(
    value: &Value,
    fallback_id: Option<&str>,
    fallback_url: Option<&str>,
) -> (Option<String>, Option<String>) {
    let direct_id = ["channel_id"]
        .into_iter()
        .filter_map(|key| nonempty_str(value, key))
        .find(|candidate| is_valid_channel_id(candidate));

    let target_from_fields = ["channel_url", "uploader_url"]
        .into_iter()
        .filter_map(|key| nonempty_str(value, key))
        .find_map(parse_youtube_channel_url)
        .or_else(|| {
            ["uploader_id"]
                .into_iter()
                .filter_map(|key| nonempty_str(value, key))
                .find(|candidate| is_valid_channel_handle(candidate))
                .map(|handle| ChannelTarget::Handle(handle.to_owned()))
        });

    if direct_id.is_some() || target_from_fields.is_some() {
        let channel_id = direct_id
            .map(str::to_owned)
            .or_else(|| match &target_from_fields {
                Some(ChannelTarget::ChannelId(id)) => Some(id.clone()),
                _ => None,
            });

        let channel_url = if let Some(id) = &channel_id {
            Some(ChannelTarget::ChannelId(id.clone()).canonical_url())
        } else {
            target_from_fields.map(|target| target.canonical_url())
        };

        return (channel_id, channel_url);
    }

    let valid_fallback_id = fallback_id
        .map(str::trim)
        .filter(|candidate| is_valid_channel_id(candidate));
    let fallback_target = fallback_url.and_then(parse_channel_target);

    let channel_id = valid_fallback_id
        .map(str::to_owned)
        .or_else(|| match &fallback_target {
            Some(ChannelTarget::ChannelId(id)) => Some(id.clone()),
            _ => None,
        });

    let channel_url = if let Some(id) = &channel_id {
        Some(ChannelTarget::ChannelId(id.clone()).canonical_url())
    } else {
        fallback_target.map(|target| target.canonical_url())
    };

    (channel_id, channel_url)
}

fn parse_video_entry(
    value: &Value,
    youtube_context: bool,
    fallback_channel_name: Option<&str>,
    fallback_channel_id: Option<&str>,
    fallback_channel_url: Option<&str>,
) -> Option<SearchResult> {
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let nonempty = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
    };
    let url = ["webpage_url", "url"]
        .into_iter()
        .filter_map(nonempty)
        .find(|url| url.starts_with("https://") || url.starts_with("http://"))
        .map(str::to_owned)
        .or_else(|| {
            let is_youtube = youtube_context
                || ["extractor_key", "ie_key", "extractor"]
                    .into_iter()
                    .filter_map(nonempty)
                    .any(|extractor| extractor.eq_ignore_ascii_case("youtube"));
            let id = nonempty("id")?;
            if is_youtube
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            {
                Some(format!("https://www.youtube.com/watch?v={id}"))
            } else {
                None
            }
        })?;
    let seconds = value.get("duration").and_then(Value::as_f64).unwrap_or(0.0) as u64;
    let duration = format_duration_seconds(seconds);
    let channel = nonempty("channel")
        .or_else(|| nonempty("uploader"))
        .or_else(|| {
            fallback_channel_name
                .map(str::trim)
                .filter(|name| !name.is_empty())
        })
        .unwrap_or("")
        .to_owned();
    let thumbnail = select_video_thumbnail(value);
    let (channel_id, channel_url) =
        extract_channel_association(value, fallback_channel_id, fallback_channel_url);

    let live_status = nonempty("live_status");
    let is_live = value.get("is_live").and_then(Value::as_bool) == Some(true)
        || live_status == Some("is_live");
    let concurrent_views = value.get("concurrent_view_count").and_then(Value::as_u64);
    let view_count = value
        .get("view_count")
        .and_then(Value::as_u64)
        .or(concurrent_views);
    let channel_verified = value
        .get("channel_is_verified")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let kind = if url.contains("/shorts/") {
        SearchResultKind::Short
    } else if url.contains("playlist?list=") {
        SearchResultKind::Playlist
    } else {
        SearchResultKind::Video
    };

    Some(SearchResult {
        video: Video {
            title: text("title"),
            channel,
            url,
            thumbnail,
            duration,
        },
        metadata: VideoMetadata {
            view_count,
            upload_date: extract_yt_dlp_upload_date(value),
            description: nonempty("description").map(str::to_owned),
            channel_id,
            channel_url,
            kind,
            is_live,
            channel_verified,
            ..Default::default()
        },
    })
}
