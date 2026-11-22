//! Local-only followed channels persistence and OAuth-free pooled RSS feed polling.

use crate::oauth::parse_youtube_atom_feed;
use crate::process::Cancellation;
use crate::search::{self, ChannelTarget, SearchResult, SearchResultKind, Video, VideoMetadata};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_FOLLOWED_CHANNELS: usize = 500;
const RSS_MAX_BYTES: usize = 512 * 1024;
const RSS_TIMEOUT: Duration = Duration::from_secs(10);
pub const RSS_FETCH_WORKERS: usize = 24;
pub const RSS_MAX_ITEMS_PER_CHANNEL: usize = 20;
const RSS_CACHE_VERSION: u32 = 1;
const RSS_CACHE_MAX_BYTES: u64 = 4 * 1024 * 1024;
const RSS_CACHE_MAX_HEADER_LEN: usize = 256;
const RSS_BASE_URL: &str = "https://www.youtube.com";
const RSS_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FollowedChannel {
    pub channel_id: String,
    pub title: String,
    pub handle: Option<String>,
    pub url: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub followed_at_ms: u64,
    #[serde(default)]
    pub last_seen_video_id: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FollowedChannels {
    pub channels: Vec<FollowedChannel>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FollowedMergeSummary {
    pub added: usize,
    pub conflicts: usize,
    pub skipped_for_limit: usize,
}

impl FollowedChannels {
    pub fn len(&self) -> usize {
        self.channels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }

    pub fn is_following(&self, channel_id_or_handle: &str) -> bool {
        let needle = channel_id_or_handle.trim();
        if needle.is_empty() {
            return false;
        }
        self.channels.iter().any(|ch| {
            ch.channel_id.eq_ignore_ascii_case(needle)
                || ch
                    .handle
                    .as_deref()
                    .is_some_and(|h| h.eq_ignore_ascii_case(needle))
        })
    }

    pub fn is_following_target(
        &self,
        target: Option<&ChannelTarget>,
        id_hint: Option<&str>,
    ) -> bool {
        if let Some(id) = id_hint.map(str::trim).filter(|s| !s.is_empty())
            && self.is_following(id)
        {
            return true;
        }
        match target {
            Some(ChannelTarget::ChannelId(id)) => self.is_following(id),
            Some(ChannelTarget::Handle(handle)) => self.is_following(handle),
            None => false,
        }
    }

    pub fn follow(&mut self, channel: FollowedChannel) -> Result<bool, &'static str> {
        let id = channel.channel_id.trim();
        if id.is_empty() || id.len() > 128 {
            return Err("Identificador de canal inválido.");
        }
        if let Some(existing) = self.channels.iter_mut().find(|ch| {
            ch.channel_id.eq_ignore_ascii_case(id)
                || (channel.handle.is_some() && ch.handle == channel.handle)
        }) {
            if existing.title.trim().is_empty() && !channel.title.trim().is_empty() {
                existing.title = channel.title;
            }
            if existing.avatar_url.is_none() {
                existing.avatar_url = channel.avatar_url;
            }
            if existing.handle.is_none() {
                existing.handle = channel.handle;
            }
            return Ok(false);
        }
        if self.channels.len() >= MAX_FOLLOWED_CHANNELS {
            return Err("Limite de canais seguidos locais atingido (500).");
        }
        self.channels.push(channel);
        Ok(true)
    }

    pub fn unfollow(&mut self, target: Option<&ChannelTarget>, id_hint: Option<&str>) -> bool {
        let before = self.channels.len();
        self.channels.retain(|ch| {
            let matches_id = id_hint
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_some_and(|id| ch.channel_id.eq_ignore_ascii_case(id));
            let matches_target = match target {
                Some(ChannelTarget::ChannelId(id)) => ch.channel_id.eq_ignore_ascii_case(id),
                Some(ChannelTarget::Handle(handle)) => ch
                    .handle
                    .as_deref()
                    .is_some_and(|h| h.eq_ignore_ascii_case(handle)),
                None => false,
            };
            !(matches_id || matches_target)
        });
        self.channels.len() != before
    }

    pub fn mark_seen(&mut self, channel_id: &str, video_id: &str) -> bool {
        let Some(entry) = self
            .channels
            .iter_mut()
            .find(|ch| ch.channel_id.eq_ignore_ascii_case(channel_id))
        else {
            return false;
        };
        if entry.last_seen_video_id.as_deref() == Some(video_id) {
            return false;
        }
        entry.last_seen_video_id = Some(video_id.to_owned());
        true
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.channels.len() > MAX_FOLLOWED_CHANNELS {
            return Err("Too many followed channels");
        }
        for ch in &self.channels {
            if ch.channel_id.trim().is_empty()
                || ch.channel_id.len() > 128
                || ch.title.len() > 512
                || ch.url.len() > 2048
            {
                return Err("Invalid followed channel entry");
            }
        }
        Ok(())
    }

    /// Adds imported channels that are absent locally while preserving local metadata on conflicts.
    pub fn merged(&self, imported: &Self) -> (Self, FollowedMergeSummary) {
        let mut merged = self.clone();
        let mut summary = FollowedMergeSummary::default();
        for ch in &imported.channels {
            let id = ch.channel_id.trim();
            if id.is_empty() || id.len() > 128 {
                continue;
            }
            let existing_pos = merged.channels.iter().position(|cur| {
                cur.channel_id.eq_ignore_ascii_case(id)
                    || ch.handle.as_deref().is_some_and(|h| {
                        cur.handle
                            .as_deref()
                            .is_some_and(|existing_h| existing_h.eq_ignore_ascii_case(h))
                    })
            });
            if let Some(pos) = existing_pos {
                summary.conflicts += 1;
                let existing = &mut merged.channels[pos];
                if existing.title.trim().is_empty() && !ch.title.trim().is_empty() {
                    existing.title = ch.title.clone();
                }
                if existing.avatar_url.is_none() {
                    existing.avatar_url = ch.avatar_url.clone();
                }
                if existing.handle.is_none() {
                    existing.handle = ch.handle.clone();
                }
            } else if merged.channels.len() < MAX_FOLLOWED_CHANNELS {
                merged.channels.push(ch.clone());
                summary.added += 1;
            } else {
                summary.skipped_for_limit += 1;
            }
        }
        (merged, summary)
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

pub fn path() -> io::Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))?;
    let root = PathBuf::from(local);
    if !root.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LOCALAPPDATA must be absolute",
        ));
    }
    Ok(root.join("FastTube").join("followed-channels.json"))
}

pub fn load(path: &Path) -> io::Result<FollowedChannels> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(FollowedChannels::default());
        }
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "canais seguidos inválidos");
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    let root: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if root.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err(invalid());
    }
    let data: FollowedChannels =
        serde_json::from_value(root.get("followed").cloned().ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
    data.validate().map_err(|_| invalid())?;
    Ok(data)
}

pub fn save(path: &Path, data: &FollowedChannels) -> io::Result<()> {
    data.validate()
        .map_err(|msg| io::Error::new(io::ErrorKind::InvalidInput, msg))?;
    let payload = serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "followed": data,
    }))?;
    if payload.len() as u64 > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "followed channels exceed storage limit",
        ));
    }
    crate::history::write_atomic(path, &payload)
}

pub struct LocalFeedOutput {
    pub items: Vec<SearchResult>,
    pub new_video_urls: HashSet<String>,
    #[allow(dead_code)]
    pub latest_ids_by_channel: HashMap<String, String>,
    pub errors: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RssFetchStats {
    #[allow(dead_code)]
    pub not_modified_hits: usize,
    #[allow(dead_code)]
    pub rss_updates: usize,
    #[allow(dead_code)]
    pub fallback_hits: usize,
    #[allow(dead_code)]
    pub pruned_channels: usize,
}

pub struct LocalFeedWithCacheOutput {
    pub feed: LocalFeedOutput,
    #[allow(dead_code)]
    pub stats: RssFetchStats,
    #[allow(dead_code)]
    pub cache_warning: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CachedRssItem {
    pub title: String,
    pub channel: String,
    pub url: String,
    pub thumbnail: String,
    #[serde(default)]
    pub duration: String,
    #[serde(default)]
    pub upload_date: Option<String>,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub view_count: Option<u64>,
    #[serde(default)]
    pub is_short: bool,
}

fn truncate_utf8(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    input[..end].to_owned()
}

impl From<&SearchResult> for CachedRssItem {
    fn from(item: &SearchResult) -> Self {
        Self {
            title: truncate_utf8(&item.video.title, 1024),
            channel: truncate_utf8(&item.video.channel, 512),
            url: truncate_utf8(&item.video.url, 2048),
            thumbnail: truncate_utf8(&item.video.thumbnail, 2048),
            duration: truncate_utf8(&item.video.duration, 64),
            upload_date: item
                .metadata
                .upload_date
                .as_deref()
                .map(|s| truncate_utf8(s, 64)),
            channel_id: item
                .metadata
                .channel_id
                .as_deref()
                .map(|s| truncate_utf8(s, 128)),
            description: item
                .metadata
                .description
                .as_deref()
                .map(|s| truncate_utf8(s, 4096)),
            view_count: item.metadata.view_count,
            is_short: item.metadata.kind == SearchResultKind::Short,
        }
    }
}

impl From<CachedRssItem> for SearchResult {
    fn from(item: CachedRssItem) -> Self {
        let is_short = item.is_short || item.url.contains("/shorts/");
        Self {
            video: Video {
                title: item.title,
                channel: item.channel,
                url: item.url,
                thumbnail: item.thumbnail,
                duration: item.duration,
            },
            metadata: VideoMetadata {
                upload_date: item.upload_date,
                channel_id: item.channel_id,
                description: item.description,
                view_count: item.view_count,
                kind: if is_short {
                    SearchResultKind::Short
                } else {
                    SearchResultKind::Video
                },
                ..Default::default()
            },
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RssChannelCacheEntry {
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub last_modified: Option<String>,
    #[serde(default)]
    pub last_checked_unix: u64,
    #[serde(default)]
    pub items: Vec<CachedRssItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RssDiskCache {
    pub version: u32,
    #[serde(default)]
    pub channels: HashMap<String, RssChannelCacheEntry>,
}

impl Default for RssDiskCache {
    fn default() -> Self {
        Self {
            version: RSS_CACHE_VERSION,
            channels: HashMap::new(),
        }
    }
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_safe_http_header_value(raw: &str) -> bool {
    !raw.is_empty()
        && raw.len() <= RSS_CACHE_MAX_HEADER_LEN
        && raw.bytes().all(|b| (0x20..=0x7E).contains(&b))
}

fn sanitize_http_cache_header(value: Option<&reqwest::header::HeaderValue>) -> Option<String> {
    value
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| is_safe_http_header_value(v))
        .map(str::to_owned)
}

pub fn rss_cache_path() -> io::Result<PathBuf> {
    RssDiskCache::default_path()
}

impl RssDiskCache {
    pub fn default_path() -> io::Result<PathBuf> {
        let local = std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))?;
        let root = PathBuf::from(local);
        if !root.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "LOCALAPPDATA must be absolute",
            ));
        }
        Ok(root.join("FastTube").join("rss-cache.json"))
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != RSS_CACHE_VERSION {
            return Err("Unsupported RSS cache version");
        }
        if self.channels.len() > MAX_FOLLOWED_CHANNELS {
            return Err("Too many cached RSS channels");
        }
        for (channel_id, entry) in &self.channels {
            if channel_id.trim().is_empty() || channel_id.len() > 128 {
                return Err("Invalid cached RSS channel id");
            }
            if entry.items.len() > RSS_MAX_ITEMS_PER_CHANNEL {
                return Err("Too many cached RSS items for channel");
            }
            if entry
                .etag
                .as_deref()
                .is_some_and(|v| !is_safe_http_header_value(v))
            {
                return Err("Invalid cached ETag header");
            }
            if entry
                .last_modified
                .as_deref()
                .is_some_and(|v| !is_safe_http_header_value(v))
            {
                return Err("Invalid cached Last-Modified header");
            }
            for item in &entry.items {
                if item.title.len() > 1024
                    || item.channel.len() > 512
                    || item.url.len() > 2048
                    || item.thumbnail.len() > 2048
                    || item.duration.len() > 64
                    || item.upload_date.as_deref().is_some_and(|d| d.len() > 64)
                    || item.channel_id.as_deref().is_some_and(|c| c.len() > 128)
                    || item.description.as_deref().is_some_and(|d| d.len() > 4096)
                {
                    return Err("Invalid cached RSS item entry");
                }
            }
        }
        Ok(())
    }

    pub fn load_from_path(path: &Path) -> io::Result<Self> {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(RSS_CACHE_MAX_BYTES + 1).read_to_end(&mut bytes)?;
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid RSS disk cache");
        if bytes.is_empty() || bytes.len() as u64 > RSS_CACHE_MAX_BYTES {
            return Err(invalid());
        }
        let parsed: Self = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        parsed.validate().map_err(|_| invalid())?;
        Ok(parsed)
    }

    pub fn save_to_path(&self, path: &Path) -> io::Result<()> {
        self.validate()
            .map_err(|msg| io::Error::new(io::ErrorKind::InvalidInput, msg))?;
        let payload = serde_json::to_vec(self)?;
        if payload.len() as u64 > RSS_CACHE_MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "RSS disk cache exceeds storage limit",
            ));
        }
        crate::history::write_atomic(path, &payload)
    }

    pub fn get(&self, channel_id: &str) -> Option<&RssChannelCacheEntry> {
        self.channels.get(channel_id.trim())
    }

    fn evict_oldest_if_over_limit(&mut self, max_entries: usize) {
        while self.channels.len() > max_entries {
            let Some(oldest_key) = self
                .channels
                .iter()
                .min_by(|(k_a, a), (k_b, b)| {
                    a.last_checked_unix
                        .cmp(&b.last_checked_unix)
                        .then_with(|| k_a.cmp(k_b))
                })
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.channels.remove(&oldest_key);
        }
    }

    pub fn upsert_channel(&mut self, channel_id: String, mut entry: RssChannelCacheEntry) {
        let key = channel_id.trim();
        if key.is_empty() || key.len() > 128 {
            return;
        }
        entry.etag = entry
            .etag
            .as_deref()
            .map(str::trim)
            .filter(|v| is_safe_http_header_value(v))
            .map(str::to_owned);
        entry.last_modified = entry
            .last_modified
            .as_deref()
            .map(str::trim)
            .filter(|v| is_safe_http_header_value(v))
            .map(str::to_owned);
        entry.items.truncate(RSS_MAX_ITEMS_PER_CHANNEL);
        let key_owned = key.to_owned();
        if !self.channels.contains_key(&key_owned) && self.channels.len() >= MAX_FOLLOWED_CHANNELS {
            self.evict_oldest_if_over_limit(MAX_FOLLOWED_CHANNELS.saturating_sub(1));
        }
        self.channels.insert(key_owned, entry);
    }

    pub fn touch_checked(
        &mut self,
        channel_id: &str,
        checked_unix: u64,
        etag: Option<String>,
        last_modified: Option<String>,
    ) {
        let Some(entry) = self.channels.get_mut(channel_id.trim()) else {
            return;
        };
        entry.last_checked_unix = checked_unix;
        if let Some(etag) = etag
            .as_deref()
            .map(str::trim)
            .filter(|v| is_safe_http_header_value(v))
        {
            entry.etag = Some(etag.to_owned());
        }
        if let Some(last_modified) = last_modified
            .as_deref()
            .map(str::trim)
            .filter(|v| is_safe_http_header_value(v))
        {
            entry.last_modified = Some(last_modified.to_owned());
        }
    }

    /// Removes cached entries for channels no longer present in `followed` and enforces bounds.
    pub fn prune_stale_channels(&mut self, followed: &[FollowedChannel]) -> usize {
        let active_ids: HashSet<&str> = followed
            .iter()
            .map(|ch| ch.channel_id.trim())
            .filter(|id| !id.is_empty())
            .collect();
        let before = self.channels.len();
        self.channels
            .retain(|channel_id, _| active_ids.contains(channel_id.trim()));
        for entry in self.channels.values_mut() {
            entry.items.truncate(RSS_MAX_ITEMS_PER_CHANNEL);
        }
        self.evict_oldest_if_over_limit(MAX_FOLLOWED_CHANNELS);
        before.saturating_sub(self.channels.len())
    }
}

pub fn build_conditional_headers(
    cached: Option<&RssChannelCacheEntry>,
) -> Vec<(&'static str, String)> {
    let mut headers = Vec::new();
    if let Some(entry) = cached {
        if let Some(etag) = entry.etag.as_deref().map(str::trim)
            && is_safe_http_header_value(etag)
        {
            headers.push(("If-None-Match", etag.to_owned()));
        }
        if let Some(last_modified) = entry.last_modified.as_deref().map(str::trim)
            && is_safe_http_header_value(last_modified)
        {
            headers.push(("If-Modified-Since", last_modified.to_owned()));
        }
    }
    headers
}

fn is_valid_uc_channel_id(id: &str) -> bool {
    id.starts_with("UC")
        && id.len() >= 16
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

enum RssFetchOutcome {
    NotModified {
        etag: Option<String>,
        last_modified: Option<String>,
    },
    Updated {
        items: Vec<(String, SearchResult)>,
        etag: Option<String>,
        last_modified: Option<String>,
    },
    FallbackNeeded,
}

fn fetch_channel_rss_at(
    client: &reqwest::blocking::Client,
    rss_base_url: &str,
    channel: &FollowedChannel,
    cached: Option<&RssChannelCacheEntry>,
) -> RssFetchOutcome {
    if !is_valid_uc_channel_id(&channel.channel_id) {
        return RssFetchOutcome::FallbackNeeded;
    }
    let rss_url = format!(
        "{}/feeds/videos.xml?channel_id={}",
        rss_base_url.trim_end_matches('/'),
        channel.channel_id
    );
    let mut request = client
        .get(&rss_url)
        .timeout(RSS_TIMEOUT)
        .header(reqwest::header::USER_AGENT, RSS_USER_AGENT);
    for (name, value) in build_conditional_headers(cached) {
        request = request.header(name, value);
    }
    let Ok(resp) = request.send() else {
        return RssFetchOutcome::FallbackNeeded;
    };
    let status = resp.status();
    let etag = sanitize_http_cache_header(resp.headers().get(reqwest::header::ETAG));
    let last_modified =
        sanitize_http_cache_header(resp.headers().get(reqwest::header::LAST_MODIFIED));
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return if cached.is_some() {
            RssFetchOutcome::NotModified {
                etag,
                last_modified,
            }
        } else {
            RssFetchOutcome::FallbackNeeded
        };
    }
    if !status.is_success() {
        return RssFetchOutcome::FallbackNeeded;
    }
    let mut body = Vec::new();
    if resp
        .take((RSS_MAX_BYTES + 1) as u64)
        .read_to_end(&mut body)
        .is_err()
        || body.len() > RSS_MAX_BYTES
    {
        return RssFetchOutcome::FallbackNeeded;
    }
    let Ok(xml) = std::str::from_utf8(&body) else {
        return RssFetchOutcome::FallbackNeeded;
    };
    let Some(mut items) = parse_youtube_atom_feed(xml, &channel.title, &channel.channel_id) else {
        return RssFetchOutcome::FallbackNeeded;
    };
    items.truncate(RSS_MAX_ITEMS_PER_CHANNEL);
    RssFetchOutcome::Updated {
        items,
        etag,
        last_modified,
    }
}

struct RssPoolConfig<'a> {
    rss_base_url: &'a str,
    allow_page_fallback: bool,
    max_workers: usize,
}

enum ChannelWorkerOutcome {
    NotModified {
        channel_idx: usize,
        etag: Option<String>,
        last_modified: Option<String>,
        checked_unix: u64,
    },
    RssUpdated {
        channel_idx: usize,
        items: Vec<(String, SearchResult)>,
        etag: Option<String>,
        last_modified: Option<String>,
        checked_unix: u64,
    },
    FallbackFetched {
        channel_idx: usize,
        items: Vec<(String, SearchResult)>,
    },
    Failed {
        channel_idx: usize,
    },
}

impl ChannelWorkerOutcome {
    fn channel_idx(&self) -> usize {
        match self {
            Self::NotModified { channel_idx, .. }
            | Self::RssUpdated { channel_idx, .. }
            | Self::FallbackFetched { channel_idx, .. }
            | Self::Failed { channel_idx } => *channel_idx,
        }
    }
}

fn fetch_single_channel(
    client: &reqwest::blocking::Client,
    config: &RssPoolConfig<'_>,
    channel_idx: usize,
    ch: &FollowedChannel,
    cached_entry: Option<&RssChannelCacheEntry>,
    cancel: &Cancellation,
) -> ChannelWorkerOutcome {
    if cancel.cancelled() {
        return ChannelWorkerOutcome::Failed { channel_idx };
    }
    if is_valid_uc_channel_id(&ch.channel_id) {
        let outcome = fetch_channel_rss_at(client, config.rss_base_url, ch, cached_entry);
        if cancel.cancelled() {
            return ChannelWorkerOutcome::Failed { channel_idx };
        }
        match outcome {
            RssFetchOutcome::NotModified {
                etag,
                last_modified,
            } => {
                return ChannelWorkerOutcome::NotModified {
                    channel_idx,
                    etag,
                    last_modified,
                    checked_unix: now_unix_secs(),
                };
            }
            RssFetchOutcome::Updated {
                items,
                etag,
                last_modified,
            } => {
                return ChannelWorkerOutcome::RssUpdated {
                    channel_idx,
                    items,
                    etag,
                    last_modified,
                    checked_unix: now_unix_secs(),
                };
            }
            RssFetchOutcome::FallbackNeeded => {}
        }
    }

    if !config.allow_page_fallback || cancel.cancelled() {
        return ChannelWorkerOutcome::Failed { channel_idx };
    }

    let target = if is_valid_uc_channel_id(&ch.channel_id) {
        Some(ChannelTarget::ChannelId(ch.channel_id.clone()))
    } else {
        ch.handle.as_ref().map(|h| ChannelTarget::Handle(h.clone()))
    };
    if let Some(target) = target
        && let Ok(outcome) =
            search::fetch_channel_page_with_cancel(&target.canonical_url(), 10, false, cancel)
    {
        if cancel.cancelled() {
            return ChannelWorkerOutcome::Failed { channel_idx };
        }
        let fallback_items = outcome
            .page
            .videos
            .into_iter()
            .map(|r| {
                let ts = r.metadata.upload_date.clone().unwrap_or_default();
                (ts, r)
            })
            .collect();
        return ChannelWorkerOutcome::FallbackFetched {
            channel_idx,
            items: fallback_items,
        };
    }

    ChannelWorkerOutcome::Failed { channel_idx }
}

fn fetch_local_followed_feed_internal(
    channels: &[FollowedChannel],
    cache_path: Option<&Path>,
    config: &RssPoolConfig<'_>,
    cancel: &Cancellation,
) -> Result<LocalFeedWithCacheOutput, String> {
    if cancel.cancelled() {
        return Err("Atualização cancelada.".into());
    }

    let mut cache_warning: Option<String> = None;
    let mut cache_corrupt = false;
    let mut cache = if let Some(path) = cache_path {
        match RssDiskCache::load_from_path(path) {
            Ok(loaded) => loaded,
            Err(_) => {
                // Corrupt disk cache must remain untouched on disk; continue with a fresh
                // in-memory cache and report a non-fatal warning.
                cache_corrupt = true;
                cache_warning =
                    Some("RSS disk cache is invalid and was preserved untouched on disk.".into());
                RssDiskCache::default()
            }
        }
    } else {
        RssDiskCache::default()
    };

    if channels.is_empty() {
        let pruned_channels = cache.prune_stale_channels(channels);
        if let Some(path) = cache_path
            && !cache_corrupt
            && pruned_channels > 0
            && cache.save_to_path(path).is_err()
        {
            cache_warning = Some("Failed to persist pruned RSS disk cache.".into());
        }
        return Ok(LocalFeedWithCacheOutput {
            feed: LocalFeedOutput {
                items: Vec::new(),
                new_video_urls: HashSet::new(),
                latest_ids_by_channel: HashMap::new(),
                errors: 0,
            },
            stats: RssFetchStats {
                pruned_channels,
                ..Default::default()
            },
            cache_warning,
        });
    }

    let client = crate::http_client::shared();
    let worker_count = config.max_workers.max(1).min(channels.len());
    let work_queue: Mutex<VecDeque<usize>> = Mutex::new((0..channels.len()).collect());
    let (tx, rx) = mpsc::channel::<ChannelWorkerOutcome>();
    let mut outcomes = Vec::with_capacity(channels.len());

    std::thread::scope(|scope| {
        let cache_snapshot: &RssDiskCache = &cache;
        for _ in 0..worker_count {
            let tx = tx.clone();
            let work_queue = &work_queue;
            scope.spawn(move || {
                loop {
                    if cancel.cancelled() {
                        break;
                    }
                    let next_idx = {
                        let mut queue = work_queue
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        queue.pop_front()
                    };
                    let Some(idx) = next_idx else {
                        break;
                    };
                    if cancel.cancelled() {
                        break;
                    }
                    let ch = &channels[idx];
                    let cached_entry = cache_snapshot.get(&ch.channel_id);
                    let outcome =
                        fetch_single_channel(client, config, idx, ch, cached_entry, cancel);
                    if cancel.cancelled() {
                        break;
                    }
                    if tx.send(outcome).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        while let Ok(outcome) = rx.recv() {
            outcomes.push(outcome);
        }
    });

    if cancel.cancelled() {
        return Err("Atualização cancelada.".into());
    }

    // Sort by original channel index so merging, tie-breaking, and deduplication are deterministic
    // regardless of worker completion order.
    outcomes.sort_by_key(ChannelWorkerOutcome::channel_idx);

    let mut dated: Vec<(String, SearchResult)> = Vec::new();
    let mut new_video_urls = HashSet::new();
    let mut latest_ids_by_channel = HashMap::new();
    let mut errors = 0usize;
    let mut stats = RssFetchStats::default();

    for outcome in outcomes {
        let ch = &channels[outcome.channel_idx()];
        let entries: Option<Vec<(String, SearchResult)>> = match outcome {
            ChannelWorkerOutcome::NotModified {
                etag,
                last_modified,
                checked_unix,
                ..
            } => {
                stats.not_modified_hits += 1;
                cache.touch_checked(&ch.channel_id, checked_unix, etag, last_modified);
                cache.get(&ch.channel_id).map(|entry| {
                    entry
                        .items
                        .iter()
                        .cloned()
                        .map(|cached_item| {
                            let sr = SearchResult::from(cached_item);
                            let ts = sr.metadata.upload_date.clone().unwrap_or_default();
                            (ts, sr)
                        })
                        .collect()
                })
            }
            ChannelWorkerOutcome::RssUpdated {
                items,
                etag,
                last_modified,
                checked_unix,
                ..
            } => {
                stats.rss_updates += 1;
                let cached_items: Vec<CachedRssItem> = items
                    .iter()
                    .take(RSS_MAX_ITEMS_PER_CHANNEL)
                    .map(|(_, item)| CachedRssItem::from(item))
                    .collect();
                cache.upsert_channel(
                    ch.channel_id.clone(),
                    RssChannelCacheEntry {
                        etag,
                        last_modified,
                        last_checked_unix: checked_unix,
                        items: cached_items,
                    },
                );
                Some(items)
            }
            ChannelWorkerOutcome::FallbackFetched { items, .. } => {
                stats.fallback_hits += 1;
                Some(items)
            }
            ChannelWorkerOutcome::Failed { .. } => None,
        };

        let Some(entries) = entries else {
            errors += 1;
            continue;
        };

        if let Some((_, first)) = entries.first()
            && let Some(first_id) = crate::history::youtube_video_id(&first.video.url)
        {
            latest_ids_by_channel.insert(ch.channel_id.clone(), first_id);
            if let Some(seen) = ch.last_seen_video_id.as_deref() {
                for (_, item) in &entries {
                    if let Some(vid) = crate::history::youtube_video_id(&item.video.url) {
                        if vid == seen {
                            break;
                        }
                        new_video_urls.insert(item.video.url.clone());
                    }
                }
            } else {
                new_video_urls.insert(first.video.url.clone());
            }
        }
        dated.extend(entries);
    }

    stats.pruned_channels = cache.prune_stale_channels(channels);
    if let Some(path) = cache_path
        && !cache_corrupt
        && cache.save_to_path(path).is_err()
    {
        cache_warning = Some("Failed to save RSS disk cache.".into());
    }

    dated.sort_by(|a, b| b.0.cmp(&a.0));
    let mut seen_urls = HashSet::with_capacity(dated.len());
    let mut items = Vec::with_capacity(dated.len());
    for (_, item) in dated {
        if seen_urls.insert(item.video.url.clone()) {
            items.push(item);
        }
    }

    Ok(LocalFeedWithCacheOutput {
        feed: LocalFeedOutput {
            items,
            new_video_urls,
            latest_ids_by_channel,
            errors,
        },
        stats,
        cache_warning,
    })
}

pub fn fetch_local_followed_feed_with_cache(
    channels: &[FollowedChannel],
    cache_path: Option<&Path>,
    cancel: &Cancellation,
) -> Result<LocalFeedWithCacheOutput, String> {
    let config = RssPoolConfig {
        rss_base_url: RSS_BASE_URL,
        allow_page_fallback: true,
        max_workers: RSS_FETCH_WORKERS,
    };
    fetch_local_followed_feed_internal(channels, cache_path, &config, cancel)
}

pub fn fetch_local_followed_feed(
    channels: &[FollowedChannel],
    cancel: &Cancellation,
) -> Result<LocalFeedOutput, String> {
    #[cfg(not(test))]
    let default_cache_path = rss_cache_path().ok();
    #[cfg(test)]
    let default_cache_path: Option<PathBuf> = None;

    fetch_local_followed_feed_with_cache(channels, default_cache_path.as_deref(), cancel)
        .map(|output| output.feed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn sample_channel(id: &str, title: &str) -> FollowedChannel {
        FollowedChannel {
            channel_id: id.to_owned(),
            title: title.to_owned(),
            handle: Some(format!("@{title}")),
            url: format!("https://www.youtube.com/channel/{id}"),
            avatar_url: None,
            followed_at_ms: 1_700_000_000_000,
            last_seen_video_id: None,
        }
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let seq = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fasttube-followed-{label}-{}-{seq}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_atom_xml(
        channel_id: &str,
        channel_name: &str,
        entries: &[(&str, &str, &str)],
    ) -> String {
        let mut xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <feed xmlns:yt=\"http://www.youtube.com/xml/schemas/2015\" \
             xmlns:media=\"http://search.yahoo.com/mrss/\" xmlns=\"http://www.w3.org/2005/Atom\">\
             <author><name>{channel_name}</name></author>"
        );
        for (video_id, title, published) in entries {
            xml.push_str(&format!(
                "<entry>\
                 <yt:videoId>{video_id}</yt:videoId>\
                 <yt:channelId>{channel_id}</yt:channelId>\
                 <title>{title}</title>\
                 <link rel=\"alternate\" href=\"https://www.youtube.com/watch?v={video_id}\"/>\
                 <published>{published}</published>\
                 </entry>"
            ));
        }
        xml.push_str("</feed>");
        xml
    }

    struct ParsedHttpRequest {
        path: String,
        headers: HashMap<String, String>,
    }

    fn read_http_request(stream: &mut TcpStream) -> Option<ParsedHttpRequest> {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() >= 16 * 1024 {
                        break;
                    }
                }
                Err(_) => return None,
            }
        }
        let text = String::from_utf8_lossy(&buf);
        let mut lines = text.split("\r\n");
        let request_line = lines.next()?;
        let path = request_line.split_whitespace().nth(1)?.to_owned();
        let mut headers = HashMap::new();
        for line in lines {
            if line.is_empty() {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
            }
        }
        Some(ParsedHttpRequest { path, headers })
    }

    fn write_http_response(
        stream: &mut TcpStream,
        status_line: &str,
        extra_headers: &[(&str, &str)],
        body: &[u8],
    ) {
        let mut raw = format!("HTTP/1.1 {status_line}\r\nConnection: close\r\n");
        if !status_line.starts_with("304 ") {
            raw.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        for (k, v) in extra_headers {
            raw.push_str(&format!("{k}: {v}\r\n"));
        }
        raw.push_str("\r\n");
        let _ = stream.write_all(raw.as_bytes());
        if !body.is_empty() {
            let _ = stream.write_all(body);
        }
        let _ = stream.flush();
    }

    fn channel_id_from_path(path: &str) -> String {
        path.split_once("channel_id=")
            .map(|(_, id)| id.split('&').next().unwrap_or(id).to_owned())
            .unwrap_or_default()
    }

    #[test]
    fn follow_unfollow_and_atomic_round_trip() {
        let dir = unique_temp_dir("roundtrip");
        let file = dir.join("followed-channels.json");

        let mut followed = FollowedChannels::default();
        assert!(
            followed
                .follow(sample_channel("UC0000000000000001", "RustLang"))
                .unwrap()
        );
        assert!(
            !followed
                .follow(sample_channel("UC0000000000000001", "RustLang"))
                .unwrap()
        );
        assert!(followed.is_following("UC0000000000000001"));
        assert!(followed.is_following("@RustLang"));
        assert!(followed.mark_seen("UC0000000000000001", "dQw4w9WgXcQ"));

        save(&file, &followed).unwrap();
        let loaded = load(&file).unwrap();
        assert_eq!(loaded, followed);

        let mut updated = loaded;
        assert!(updated.unfollow(
            Some(&ChannelTarget::ChannelId("UC0000000000000001".to_owned())),
            None
        ));
        assert!(updated.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_rejected_without_modification() {
        let dir = unique_temp_dir("corrupt");
        let file = dir.join("followed-channels.json");
        fs::write(&file, b"{corrupt").unwrap();
        assert!(load(&file).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"{corrupt");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_keeps_local_channels_and_adds_missing_within_limit() {
        let mut local = FollowedChannels::default();
        let mut ch1 = sample_channel("UC0000000000000001", "Alpha");
        ch1.avatar_url = None;
        local.follow(ch1).unwrap();

        let mut imported = FollowedChannels::default();
        let mut imp1 = sample_channel("UC0000000000000001", "Alpha Remote");
        imp1.avatar_url = Some("https://example.test/avatar.png".into());
        let imp2 = sample_channel("UC0000000000000002", "Beta");
        imported.follow(imp1).unwrap();
        imported.follow(imp2).unwrap();

        let (merged, summary) = local.merged(&imported);
        assert_eq!(summary.added, 1);
        assert_eq!(summary.conflicts, 1);
        assert_eq!(summary.skipped_for_limit, 0);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged.channels[0].title, "Alpha");
        assert_eq!(
            merged.channels[0].avatar_url.as_deref(),
            Some("https://example.test/avatar.png")
        );
        assert_eq!(merged.channels[1].channel_id, "UC0000000000000002");
    }

    #[test]
    fn rss_disk_cache_atomic_round_trip_and_channel_pruning() {
        let dir = unique_temp_dir("rss-cache-roundtrip");
        let cache_file = dir.join("rss-cache.json");

        // Missing file loads as empty default cache.
        let empty = RssDiskCache::load_from_path(&cache_file).unwrap();
        assert!(empty.channels.is_empty());
        assert_eq!(empty.version, RSS_CACHE_VERSION);

        let mut cache = RssDiskCache::default();
        let over_limit_items: Vec<CachedRssItem> = (0..25)
            .map(|i| CachedRssItem {
                title: format!("Video {i}"),
                channel: "Alpha".into(),
                url: format!("https://www.youtube.com/watch?v=vid000000{i:02}"),
                thumbnail: format!("https://i.ytimg.com/vi/vid000000{i:02}/hqdefault.jpg"),
                duration: String::new(),
                upload_date: Some(format!("2026-11-{:02}T10:00:00+00:00", (i % 28) + 1)),
                channel_id: Some("UC0000000000000001".into()),
                description: Some("Sample description".into()),
                view_count: Some(100 + i as u64),
                is_short: false,
            })
            .collect();

        cache.upsert_channel(
            "UC0000000000000001".into(),
            RssChannelCacheEntry {
                etag: Some("\"etag-alpha\"".into()),
                last_modified: Some("Wed, 21 Oct 2026 07:28:00 GMT".into()),
                last_checked_unix: 1_700_000_100,
                items: over_limit_items,
            },
        );
        cache.upsert_channel(
            "UC0000000000000002".into(),
            RssChannelCacheEntry {
                etag: Some("\"etag-stale\"".into()),
                last_modified: None,
                last_checked_unix: 1_700_000_050,
                items: Vec::new(),
            },
        );

        // Per-channel item count is strictly bounded.
        assert_eq!(
            cache.get("UC0000000000000001").unwrap().items.len(),
            RSS_MAX_ITEMS_PER_CHANNEL
        );

        cache.save_to_path(&cache_file).unwrap();
        let mut loaded = RssDiskCache::load_from_path(&cache_file).unwrap();
        assert_eq!(loaded, cache);

        // Prune channels no longer followed.
        let active = vec![sample_channel("UC0000000000000001", "Alpha")];
        let pruned = loaded.prune_stale_channels(&active);
        assert_eq!(pruned, 1);
        assert!(loaded.get("UC0000000000000001").is_some());
        assert!(loaded.get("UC0000000000000002").is_none());

        loaded.save_to_path(&cache_file).unwrap();
        let reloaded = RssDiskCache::load_from_path(&cache_file).unwrap();
        assert_eq!(reloaded, loaded);

        if let Ok(default_path) = rss_cache_path() {
            assert_eq!(
                default_path.file_name().and_then(|n| n.to_str()),
                Some("rss-cache.json")
            );
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rss_disk_cache_preserves_corrupt_file_untouched_and_reports_warning() {
        let dir = unique_temp_dir("rss-cache-corrupt");
        let cache_file = dir.join("rss-cache.json");
        let corrupt_bytes = b"{not-valid-json";
        fs::write(&cache_file, corrupt_bytes).unwrap();

        assert!(RssDiskCache::load_from_path(&cache_file).is_err());
        assert_eq!(fs::read(&cache_file).unwrap(), corrupt_bytes);

        let wrong_version_file = dir.join("rss-cache-v99.json");
        let wrong_version_bytes = br#"{"version":99,"channels":{}}"#;
        fs::write(&wrong_version_file, wrong_version_bytes).unwrap();
        assert!(RssDiskCache::load_from_path(&wrong_version_file).is_err());
        assert_eq!(fs::read(&wrong_version_file).unwrap(), wrong_version_bytes);

        // Fetch against an offline stub while rss-cache.json is corrupt: fetch still succeeds
        // with a non-fatal cache_warning and the corrupt file on disk is NOT overwritten.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let xml = sample_atom_xml(
            "UC0000000000000010",
            "CorruptSafe",
            &[(
                "vidCorrupt1",
                "Recovered Upload",
                "2026-11-20T12:00:00+00:00",
            )],
        );
        let server = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = read_http_request(&mut stream);
                write_http_response(
                    &mut stream,
                    "200 OK",
                    &[("Content-Type", "application/atom+xml"), ("ETag", "\"e1\"")],
                    xml.as_bytes(),
                );
            }
        });

        let channels = vec![sample_channel("UC0000000000000010", "CorruptSafe")];
        let base_url = format!("http://{addr}");
        let config = RssPoolConfig {
            rss_base_url: &base_url,
            allow_page_fallback: false,
            max_workers: RSS_FETCH_WORKERS,
        };
        let cancel = Cancellation::default();
        let output =
            fetch_local_followed_feed_internal(&channels, Some(&cache_file), &config, &cancel)
                .unwrap();
        server.join().unwrap();

        assert_eq!(output.feed.items.len(), 1);
        assert_eq!(output.feed.items[0].video.title, "Recovered Upload");
        assert!(output.cache_warning.is_some());
        assert_eq!(fs::read(&cache_file).unwrap(), corrupt_bytes);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn conditional_header_builder_emits_and_sanitizes_etag_and_last_modified() {
        assert!(build_conditional_headers(None).is_empty());

        let full = RssChannelCacheEntry {
            etag: Some(" \"abc123\" ".into()),
            last_modified: Some("Wed, 21 Oct 2026 07:28:00 GMT".into()),
            last_checked_unix: 10,
            items: Vec::new(),
        };
        assert_eq!(
            build_conditional_headers(Some(&full)),
            vec![
                ("If-None-Match", "\"abc123\"".to_owned()),
                (
                    "If-Modified-Since",
                    "Wed, 21 Oct 2026 07:28:00 GMT".to_owned()
                ),
            ]
        );

        let unsafe_entry = RssChannelCacheEntry {
            etag: Some("bad\r\nX-Injected: 1".into()),
            last_modified: Some("   ".into()),
            last_checked_unix: 10,
            items: Vec::new(),
        };
        assert!(build_conditional_headers(Some(&unsafe_entry)).is_empty());
    }

    #[test]
    fn rss_conditional_http_304_reuses_cache_and_200_updates_disk_cache() {
        let dir = unique_temp_dir("rss-304");
        let cache_file = dir.join("rss-cache.json");
        let channel_id = "UC0000000000000304";
        let channels = vec![sample_channel(channel_id, "CondChannel")];

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let xml_v1 = sample_atom_xml(
            channel_id,
            "CondChannel",
            &[("vidCond0001", "Initial Upload", "2026-11-18T08:00:00+00:00")],
        );
        let xml_v2 = sample_atom_xml(
            channel_id,
            "CondChannel",
            &[("vidCond0002", "Updated Upload", "2026-11-21T09:00:00+00:00")],
        );

        let seen_if_none_match = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
        let seen_if_modified_since = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
        let seen_inm_clone = Arc::clone(&seen_if_none_match);
        let seen_ims_clone = Arc::clone(&seen_if_modified_since);

        let server = std::thread::spawn(move || {
            for step in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let req = read_http_request(&mut stream).unwrap();
                seen_inm_clone
                    .lock()
                    .unwrap()
                    .push(req.headers.get("if-none-match").cloned());
                seen_ims_clone
                    .lock()
                    .unwrap()
                    .push(req.headers.get("if-modified-since").cloned());
                match step {
                    0 => {
                        write_http_response(
                            &mut stream,
                            "200 OK",
                            &[
                                ("Content-Type", "application/atom+xml"),
                                ("ETag", "\"etag-v1\""),
                                ("Last-Modified", "Wed, 18 Nov 2026 08:00:00 GMT"),
                            ],
                            xml_v1.as_bytes(),
                        );
                    }
                    1 => {
                        // Return 304 with empty body (zero XML to parse).
                        write_http_response(
                            &mut stream,
                            "304 Not Modified",
                            &[("ETag", "\"etag-v1\"")],
                            b"",
                        );
                    }
                    _ => {
                        write_http_response(
                            &mut stream,
                            "200 OK",
                            &[
                                ("Content-Type", "application/atom+xml"),
                                ("ETag", "\"etag-v2\""),
                                ("Last-Modified", "Sat, 21 Nov 2026 09:00:00 GMT"),
                            ],
                            xml_v2.as_bytes(),
                        );
                    }
                }
            }
        });

        let base_url = format!("http://{addr}");
        let config = RssPoolConfig {
            rss_base_url: &base_url,
            allow_page_fallback: false,
            max_workers: RSS_FETCH_WORKERS,
        };
        let cancel = Cancellation::default();

        // Step 1: Cold fetch -> 200 OK populates disk cache.
        let pass1 =
            fetch_local_followed_feed_internal(&channels, Some(&cache_file), &config, &cancel)
                .unwrap();
        assert_eq!(pass1.stats.rss_updates, 1);
        assert_eq!(pass1.stats.not_modified_hits, 0);
        assert_eq!(pass1.feed.items.len(), 1);
        assert_eq!(pass1.feed.items[0].video.title, "Initial Upload");

        let mut cache_after_pass1 = RssDiskCache::load_from_path(&cache_file).unwrap();
        let entry1 = cache_after_pass1.channels.get_mut(channel_id).unwrap();
        assert_eq!(entry1.etag.as_deref(), Some("\"etag-v1\""));
        assert_eq!(
            entry1.last_modified.as_deref(),
            Some("Wed, 18 Nov 2026 08:00:00 GMT")
        );
        // Backdate last_checked_unix so we can deterministically verify 304 refreshes it.
        entry1.last_checked_unix = 100;
        cache_after_pass1.save_to_path(&cache_file).unwrap();

        // Step 2: Warm fetch -> 304 Not Modified reuses cached items & updates last_checked_unix.
        let pass2 =
            fetch_local_followed_feed_internal(&channels, Some(&cache_file), &config, &cancel)
                .unwrap();
        assert_eq!(pass2.stats.not_modified_hits, 1);
        assert_eq!(pass2.stats.rss_updates, 0);
        assert_eq!(pass2.feed.errors, 0);
        assert_eq!(pass2.feed.items.len(), 1);
        assert_eq!(pass2.feed.items[0].video.title, "Initial Upload");

        let cache_after_pass2 = RssDiskCache::load_from_path(&cache_file).unwrap();
        let entry2 = cache_after_pass2.get(channel_id).unwrap();
        assert!(entry2.last_checked_unix > 100);
        assert_eq!(entry2.etag.as_deref(), Some("\"etag-v1\""));

        // Step 3: Server returns 200 OK with fresh XML -> updates items and ETag on disk.
        let pass3 =
            fetch_local_followed_feed_internal(&channels, Some(&cache_file), &config, &cancel)
                .unwrap();
        assert_eq!(pass3.stats.rss_updates, 1);
        assert_eq!(pass3.stats.not_modified_hits, 0);
        assert_eq!(pass3.feed.items.len(), 1);
        assert_eq!(pass3.feed.items[0].video.title, "Updated Upload");

        let cache_after_pass3 = RssDiskCache::load_from_path(&cache_file).unwrap();
        let entry3 = cache_after_pass3.get(channel_id).unwrap();
        assert_eq!(entry3.etag.as_deref(), Some("\"etag-v2\""));
        assert_eq!(entry3.items[0].title, "Updated Upload");

        server.join().unwrap();

        let inm = seen_if_none_match.lock().unwrap();
        let ims = seen_if_modified_since.lock().unwrap();
        assert_eq!(inm[0], None);
        assert_eq!(ims[0], None);
        assert_eq!(inm[1].as_deref(), Some("\"etag-v1\""));
        assert_eq!(ims[1].as_deref(), Some("Wed, 18 Nov 2026 08:00:00 GMT"));
        assert_eq!(inm[2].as_deref(), Some("\"etag-v1\""));
        assert_eq!(ims[2].as_deref(), Some("Wed, 18 Nov 2026 08:00:00 GMT"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rss_worker_pool_caps_concurrency_and_orders_and_deduplicates_items() {
        const TOTAL_CHANNELS: usize = 36;
        let dir = unique_temp_dir("rss-pool-cap");
        let cache_file = dir.join("rss-cache.json");

        let channels: Vec<FollowedChannel> = (0..TOTAL_CHANNELS)
            .map(|i| sample_channel(&format!("UC000000000000{i:04}"), &format!("Channel{i}")))
            .collect();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let active_requests = Arc::new(AtomicUsize::new(0));
        let peak_concurrency = Arc::new(AtomicUsize::new(0));
        let active_clone = Arc::clone(&active_requests);
        let peak_clone = Arc::clone(&peak_concurrency);

        let server = std::thread::spawn(move || {
            let mut handlers = Vec::with_capacity(TOTAL_CHANNELS);
            for _ in 0..TOTAL_CHANNELS {
                let (mut stream, _) = listener.accept().unwrap();
                let active = Arc::clone(&active_clone);
                let peak = Arc::clone(&peak_clone);
                handlers.push(std::thread::spawn(move || {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(current, Ordering::SeqCst);
                    let req = read_http_request(&mut stream).unwrap();
                    let cid = channel_id_from_path(&req.path);
                    let idx: usize = cid
                        .strip_prefix("UC000000000000")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    // Hold briefly so workers overlap in flight and exercise the pool cap.
                    std::thread::sleep(Duration::from_millis(35));

                    // Channels 0 and 1 emit a shared duplicate video URL to test deduplication.
                    let vid = format!("vidPool00{idx:02}");
                    let day = (idx % 20) + 1;
                    let hour = idx % 24;
                    let published = format!("2026-10-{day:02}T{hour:02}:00:00+00:00");
                    let xml = if idx == 0 || idx == 1 {
                        sample_atom_xml(
                            &cid,
                            &format!("Channel{idx}"),
                            &[
                                (&vid, &format!("Unique {idx}"), &published),
                                (
                                    "vidShared01",
                                    "Shared Collab Video",
                                    "2026-11-21T23:59:59+00:00",
                                ),
                            ],
                        )
                    } else {
                        sample_atom_xml(
                            &cid,
                            &format!("Channel{idx}"),
                            &[(&vid, &format!("Unique {idx}"), &published)],
                        )
                    };
                    write_http_response(
                        &mut stream,
                        "200 OK",
                        &[("Content-Type", "application/atom+xml")],
                        xml.as_bytes(),
                    );
                    active.fetch_sub(1, Ordering::SeqCst);
                }));
            }
            for h in handlers {
                h.join().unwrap();
            }
        });

        let base_url = format!("http://{addr}");
        let config = RssPoolConfig {
            rss_base_url: &base_url,
            allow_page_fallback: false,
            max_workers: RSS_FETCH_WORKERS,
        };
        let cancel = Cancellation::default();
        let output =
            fetch_local_followed_feed_internal(&channels, Some(&cache_file), &config, &cancel)
                .unwrap();
        server.join().unwrap();

        let observed_peak = peak_concurrency.load(Ordering::SeqCst);
        assert!(
            observed_peak <= RSS_FETCH_WORKERS,
            "peak concurrency {observed_peak} exceeded RSS_FETCH_WORKERS ({RSS_FETCH_WORKERS})"
        );
        assert!(
            observed_peak >= 2,
            "expected pooled concurrent fetches, observed peak {observed_peak}"
        );
        assert_eq!(output.feed.errors, 0);
        assert_eq!(output.stats.rss_updates, TOTAL_CHANNELS);
        // 36 unique channel videos + 1 deduplicated shared video = 37 total items.
        assert_eq!(output.feed.items.len(), TOTAL_CHANNELS + 1);
        assert_eq!(output.feed.items[0].video.title, "Shared Collab Video");

        // Verify newest-first ordering and uniqueness of URLs.
        let mut urls = HashSet::new();
        for pair in output.feed.items.windows(2) {
            let left = pair[0].metadata.upload_date.as_deref().unwrap_or("");
            let right = pair[1].metadata.upload_date.as_deref().unwrap_or("");
            assert!(
                left >= right,
                "items not sorted descending: {left} < {right}"
            );
        }
        for item in &output.feed.items {
            assert!(urls.insert(item.video.url.clone()), "duplicate URL found");
        }
        assert_eq!(output.feed.latest_ids_by_channel.len(), TOTAL_CHANNELS);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rss_worker_pool_exits_early_on_cancellation() {
        let channels: Vec<FollowedChannel> = (0..60)
            .map(|i| sample_channel(&format!("UC000000000099{i:04}"), &format!("CancelCh{i}")))
            .collect();

        // Pre-cancelled token returns immediately without any network calls.
        let pre_cancel = Cancellation::default();
        pre_cancel.cancel();
        let config_unused = RssPoolConfig {
            rss_base_url: "http://127.0.0.1:1",
            allow_page_fallback: false,
            max_workers: RSS_FETCH_WORKERS,
        };
        assert!(
            fetch_local_followed_feed_internal(&channels, None, &config_unused, &pre_cancel)
                .is_err()
        );

        // Mid-flight cancellation stops workers from draining remaining queued channels.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stop_server = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_server);
        let served = Arc::new(AtomicUsize::new(0));
        let served_clone = Arc::clone(&served);
        let cancel = Cancellation::default();
        let cancel_clone = cancel.clone();

        let server = std::thread::spawn(move || {
            let mut handlers = Vec::new();
            while !stop_clone.load(Ordering::Acquire) {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if stop_clone.load(Ordering::Acquire) {
                    break;
                }
                // Cancel before sending any response so no worker can pop a second channel.
                cancel_clone.cancel();
                served_clone.fetch_add(1, Ordering::SeqCst);
                handlers.push(std::thread::spawn(move || {
                    let _ = read_http_request(&mut stream);
                    let xml = sample_atom_xml(
                        "UC0000000000990000",
                        "CancelCh0",
                        &[("vidCancel01", "Cancelled Item", "2026-11-21T00:00:00+00:00")],
                    );
                    write_http_response(
                        &mut stream,
                        "200 OK",
                        &[("Content-Type", "application/atom+xml")],
                        xml.as_bytes(),
                    );
                }));
            }
            for h in handlers {
                let _ = h.join();
            }
        });

        let base_url = format!("http://{addr}");
        let config = RssPoolConfig {
            rss_base_url: &base_url,
            allow_page_fallback: false,
            max_workers: RSS_FETCH_WORKERS,
        };
        let result = fetch_local_followed_feed_internal(&channels, None, &config, &cancel);
        stop_server.store(true, Ordering::Release);
        let _ = TcpStream::connect(addr);
        server.join().unwrap();

        assert!(result.is_err());
        let total_served = served.load(Ordering::SeqCst);
        assert!(
            total_served <= RSS_FETCH_WORKERS,
            "expected early exit with <= {RSS_FETCH_WORKERS} requests, got {total_served}"
        );
    }
}
