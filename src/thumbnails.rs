use crate::process::Cancellation;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant, SystemTime};

#[cfg(test)]
pub const THUMBNAIL_WIDTH: f32 = 160.0;
#[cfg(test)]
pub const THUMBNAIL_HEIGHT: f32 = 90.0;
#[cfg(test)]
pub const THUMBNAIL_SIZE: Vec2 = egui::vec2(THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT);

pub const MAX_RETAINED_THUMBNAILS: usize = 36;
pub const MAX_CONCURRENT_LOADS: usize = 6;
pub const MAX_NEW_LOADS_PER_PASS: usize = 3;
pub const MAX_GPU_UPLOADS_PER_FRAME: usize = 3;
pub const LOAD_START_INTERVAL: Duration = Duration::from_millis(20);
pub const VIEWPORT_OVERSCAN_PX: f32 = 110.0;
pub const IN_FLIGHT_TIMEOUT: Duration = Duration::from_secs(15);

pub const DEFAULT_DISK_CACHE_MAX_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CACHED_IMAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_THUMBNAIL_DECODE_WIDTH: u32 = 640;
const MAX_THUMBNAIL_DECODE_HEIGHT: u32 = 360;
pub const MAX_BANNER_DECODE_WIDTH: u32 = 2048;
const MAX_BANNER_DECODE_HEIGHT: u32 = 2048;
const MAX_AVATAR_DECODE_SIZE: u32 = 96;
const AVATAR_URL_SIZE_PX: u32 = 88;
const MAX_STORYBOARD_DECODE_SIZE: u32 = 1024;
const MAX_RETAINED_STORYBOARD_SHEETS: usize = 4;
const WORKER_POOL_THREADS: usize = 6;
const BATCH_EVICT_WATERMARK_THRESHOLD_BYTES: u64 = 1024 * 1024;
const THUMBNAIL_FADE_DURATION_SECS: f32 = 0.14;
const IMAGE_FETCH_TIMEOUT: Duration = Duration::from_secs(12);
const MTIME_TOUCH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

static CACHE_TEMP_ID: AtomicU64 = AtomicU64::new(0);
static IMAGE_WORKER_POOL: OnceLock<ImageWorkerPool> = OnceLock::new();

#[derive(Debug)]
pub enum DiskCacheError {
    Io,
    Corrupt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FetchKind {
    Thumbnail { target_width_px: u32 },
    Avatar,
    Banner,
    Storyboard,
}

#[derive(Clone, Debug)]
struct DiskEntryMeta {
    size: u64,
    last_used: SystemTime,
    seq: u64,
    legacy: bool,
}

#[derive(Default)]
struct DiskIndex {
    initialized: bool,
    entries: HashMap<String, DiskEntryMeta>,
    removed_before_init: HashSet<String>,
    total_bytes: u64,
    next_seq: u64,
}

struct DiskIndexState {
    index: Mutex<DiskIndex>,
    init_done: Condvar,
}

pub struct DiskImageCache {
    root: PathBuf,
    max_bytes: u64,
    state: Arc<DiskIndexState>,
}

pub fn default_disk_cache_dir() -> io::Result<PathBuf> {
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
    Ok(path.join("FastTube").join("cache").join("thumbnails"))
}

pub fn uri_cache_key(uri: &str) -> String {
    let digest = Sha256::digest(uri.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn is_valid_cache_stem(stem: &str) -> bool {
    stem.len() == 64
        && stem
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_valid_shard_name(name: &str) -> bool {
    name.len() == 2
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn shard_dir_for_root(root: &Path, key: &str) -> PathBuf {
    let shard = key.get(..2).unwrap_or("00");
    root.join(shard)
}

fn sharded_entry_path_for_root(root: &Path, key: &str) -> PathBuf {
    shard_dir_for_root(root, key).join(format!("{key}.img"))
}

fn legacy_entry_path_for_root(root: &Path, key: &str) -> PathBuf {
    root.join(format!("{key}.img"))
}

fn is_ytimg_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "i.ytimg.com"
            | "i1.ytimg.com"
            | "i2.ytimg.com"
            | "i3.ytimg.com"
            | "i4.ytimg.com"
            | "img.youtube.com"
    )
}

fn is_google_image_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "yt3.ggpht.com"
            | "yt3.googleusercontent.com"
            | "lh3.googleusercontent.com"
            | "lh4.googleusercontent.com"
            | "lh5.googleusercontent.com"
            | "lh6.googleusercontent.com"
    )
}

pub(crate) fn select_youtube_thumbnail_variant(target_width_px: u32) -> &'static str {
    if target_width_px <= 320 {
        "mqdefault"
    } else if target_width_px <= 480 {
        "hqdefault"
    } else {
        "sddefault"
    }
}

fn parse_unsigned_ytimg_vi(parsed: &url::Url) -> Option<(&str, &str, &str, &str)> {
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?;
    if !is_ytimg_host(host) {
        return None;
    }
    // Preserve signed YouTube thumbnail URLs verbatim so query signatures stay valid.
    if parsed
        .query()
        .is_some_and(|query| query.contains("sqp=") || query.contains("rs="))
    {
        return None;
    }
    let mut segments = parsed.path_segments()?;
    let prefix = segments.next()?;
    let video_id = segments.next()?;
    let filename = segments.next()?;
    if (prefix != "vi" && prefix != "vi_webp") || video_id.is_empty() || segments.next().is_some() {
        return None;
    }
    let stem = filename
        .strip_suffix(".jpg")
        .or_else(|| filename.strip_suffix(".jpeg"))
        .or_else(|| {
            (prefix == "vi_webp")
                .then(|| filename.strip_suffix(".webp"))
                .flatten()
        })?;
    Some((host, prefix, video_id, stem))
}

pub(crate) fn is_youtube_letterboxed_4_3_uri(uri: &str) -> bool {
    let Ok(parsed) = url::Url::parse(uri) else {
        return false;
    };
    let Some((_, _, _, stem)) = parse_unsigned_ytimg_vi(&parsed) else {
        return false;
    };
    matches!(stem, "default" | "hqdefault" | "sddefault" | "0")
}

pub(crate) fn rewrite_youtube_thumbnail_hq720_webp_url(uri: &str) -> Option<String> {
    let parsed = url::Url::parse(uri).ok()?;
    let (host, prefix, video_id, stem) = parse_unsigned_ytimg_vi(&parsed)?;
    if prefix != "vi"
        || !matches!(
            stem,
            "default" | "mqdefault" | "hqdefault" | "sddefault" | "hq720" | "maxresdefault"
        )
    {
        return None;
    }
    let mut rewritten = format!(
        "{}://{}/vi_webp/{}/hq720.webp",
        parsed.scheme(),
        host,
        video_id
    );
    if let Some(query) = parsed.query()
        && !query.is_empty()
    {
        rewritten.push('?');
        rewritten.push_str(query);
    }
    Some(rewritten)
}

pub(crate) fn rewrite_youtube_thumbnail_webp_url(
    uri: &str,
    target_width_px: u32,
) -> Option<String> {
    let parsed = url::Url::parse(uri).ok()?;
    let (host, prefix, video_id, stem) = parse_unsigned_ytimg_vi(&parsed)?;
    if prefix != "vi" {
        return None;
    }
    let target_variant = match stem {
        "default" | "mqdefault" | "hqdefault" | "sddefault" | "hq720" | "maxresdefault" => {
            select_youtube_thumbnail_variant(target_width_px)
        }
        "0" | "1" | "2" | "3" => stem,
        _ => return None,
    };
    let mut rewritten = format!(
        "{}://{}/vi_webp/{}/{}.webp",
        parsed.scheme(),
        host,
        video_id,
        target_variant
    );
    if let Some(query) = parsed.query()
        && !query.is_empty()
    {
        rewritten.push('?');
        rewritten.push_str(query);
    }
    Some(rewritten)
}

fn rewrite_google_size_token(params: &str, target_px: u32) -> Option<String> {
    let mut replaced = false;
    let tokens: Vec<String> = params
        .split('-')
        .map(|token| {
            if !replaced
                && let Some(digits) = token.strip_prefix('s')
                && !digits.is_empty()
                && digits.bytes().all(|b| b.is_ascii_digit())
            {
                replaced = true;
                format!("s{target_px}")
            } else {
                token.to_owned()
            }
        })
        .collect();
    replaced.then(|| tokens.join("-"))
}

pub(crate) fn rewrite_google_avatar_url(uri: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(uri) else {
        return uri.to_owned();
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return uri.to_owned();
    }
    let Some(host) = parsed.host_str() else {
        return uri.to_owned();
    };
    if !is_google_image_host(host) {
        return uri.to_owned();
    }
    let path = parsed.path().to_owned();
    let last_slash = path.rfind('/').map(|i| i + 1).unwrap_or(0);
    let Some(eq_rel) = path[last_slash..].find('=') else {
        return uri.to_owned();
    };
    let params_start = last_slash + eq_rel + 1;
    let params = &path[params_start..];
    if params.contains("fcrop64=") {
        return uri.to_owned();
    }
    let Some(rewritten_params) = rewrite_google_size_token(params, AVATAR_URL_SIZE_PX) else {
        return uri.to_owned();
    };
    let mut new_path = String::with_capacity(path.len() + 8);
    new_path.push_str(&path[..params_start]);
    new_path.push_str(&rewritten_params);
    parsed.set_path(&new_path);
    parsed.into()
}

pub(crate) fn is_channel_banner_url(uri: &str) -> bool {
    let Ok(parsed) = url::Url::parse(uri) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    if !is_google_image_host(host) {
        return false;
    }
    let path = parsed.path();
    let last_slash = path.rfind('/').map(|i| i + 1).unwrap_or(0);
    let Some(eq_rel) = path[last_slash..].find('=') else {
        return false;
    };
    let params = &path[last_slash + eq_rel + 1..];
    if params.contains("fcrop64=") {
        return true;
    }
    params.split('-').any(|token| {
        token
            .strip_prefix('w')
            .and_then(|digits| digits.parse::<u32>().ok())
            .is_some_and(|w| w >= 640)
    })
}

pub(crate) fn decode_limits_for_request(
    uri: &str,
    display_size: Vec2,
    pixels_per_point: f32,
    cover: bool,
) -> (u32, u32, FetchKind) {
    let wide_cover_banner =
        cover && display_size.x >= 400.0 && display_size.x / display_size.y.max(1.0) >= 2.8;
    if is_channel_banner_url(uri) || wide_cover_banner {
        return (
            MAX_BANNER_DECODE_WIDTH,
            MAX_BANNER_DECODE_HEIGHT,
            FetchKind::Banner,
        );
    }
    let target_width_px = (display_size.x.max(1.0) * pixels_per_point.max(1.0)).round() as u32;
    (
        MAX_THUMBNAIL_DECODE_WIDTH,
        MAX_THUMBNAIL_DECODE_HEIGHT,
        FetchKind::Thumbnail { target_width_px },
    )
}

pub(crate) fn image_fetch_candidates(uri: &str, fetch_kind: FetchKind) -> Vec<String> {
    match fetch_kind {
        FetchKind::Thumbnail { target_width_px } => {
            let mut candidates = Vec::with_capacity(3);
            if target_width_px > 320
                && let Some(hq720) = rewrite_youtube_thumbnail_hq720_webp_url(uri)
                && hq720 != uri
            {
                candidates.push(hq720);
            }
            if let Some(webp) = rewrite_youtube_thumbnail_webp_url(uri, target_width_px)
                && webp != uri
                && !candidates.contains(&webp)
            {
                candidates.push(webp);
            }
            candidates.push(uri.to_owned());
            candidates
        }
        FetchKind::Avatar => {
            let sized = rewrite_google_avatar_url(uri);
            if sized != uri {
                return vec![sized, uri.to_owned()];
            }
            vec![uri.to_owned()]
        }
        FetchKind::Banner | FetchKind::Storyboard => vec![uri.to_owned()],
    }
}

fn decode_color_image(
    bytes: &[u8],
    max_width: u32,
    max_height: u32,
) -> Result<egui::ColorImage, image::ImageError> {
    let mut img = image::load_from_memory(bytes)?;
    let (limit_w, limit_h) = if img.height() > img.width() && max_width > max_height {
        (max_height, max_width)
    } else {
        (max_width, max_height)
    };
    if img.width() > limit_w || img.height() > limit_h {
        img = img.resize(limit_w, limit_h, image::imageops::FilterType::Triangle);
    }
    let rgba = img.into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        size,
        rgba.as_raw(),
    ))
}

fn scan_directory_entries(
    dir: &Path,
    expected_shard: Option<&str>,
    current_pid_marker: &str,
    discovered: &mut Vec<(String, u64, SystemTime, bool)>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let file_name = entry.file_name();
        let Some(name_str) = file_name.to_str() else {
            continue;
        };
        if file_type.is_dir() {
            if expected_shard.is_none() && is_valid_shard_name(name_str) {
                scan_directory_entries(
                    &entry.path(),
                    Some(name_str),
                    current_pid_marker,
                    discovered,
                );
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if name_str.starts_with('.') && name_str.ends_with(".tmp") {
            // Avoid deleting in-flight temp files belonging to this live process.
            if !name_str.contains(current_pid_marker) {
                let _ = fs::remove_file(entry.path());
            }
            continue;
        }
        let Some(stem) = name_str.strip_suffix(".img") else {
            continue;
        };
        if !is_valid_cache_stem(stem) {
            continue;
        }
        if let Some(shard) = expected_shard
            && !stem.starts_with(shard)
        {
            continue;
        }
        // `DirEntry::metadata` reuses Windows `FindNextFileW` metadata without extra stat syscalls.
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let size = meta.len();
        let last_used = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        discovered.push((stem.to_owned(), size, last_used, expected_shard.is_none()));
    }
}

fn collect_eviction_batch_locked(
    root: &Path,
    max_bytes: u64,
    index: &mut DiskIndex,
    protected_key: Option<&str>,
) -> Vec<PathBuf> {
    if index.total_bytes <= max_bytes {
        return Vec::new();
    }
    let target_bytes = if max_bytes >= BATCH_EVICT_WATERMARK_THRESHOLD_BYTES {
        max_bytes.saturating_mul(9) / 10
    } else {
        max_bytes
    };
    let mut candidates: Vec<(String, SystemTime, u64, u64, bool)> = index
        .entries
        .iter()
        .filter(|(key, _)| protected_key != Some(key.as_str()))
        .map(|(key, meta)| {
            (
                key.clone(),
                meta.last_used,
                meta.seq,
                meta.size,
                meta.legacy,
            )
        })
        .collect();
    candidates.sort_unstable_by_key(|(_, last_used, seq, _, _)| (*last_used, *seq));

    let mut evicted_paths = Vec::new();
    for (key, _, _, size, legacy) in candidates {
        if index.total_bytes <= target_bytes {
            break;
        }
        if index.entries.remove(&key).is_some() {
            index.total_bytes = index.total_bytes.saturating_sub(size);
            let path = if legacy {
                legacy_entry_path_for_root(root, &key)
            } else {
                sharded_entry_path_for_root(root, &key)
            };
            evicted_paths.push(path);
        }
    }
    evicted_paths
}

fn remove_evicted_paths(paths: Vec<PathBuf>) {
    for path in paths {
        let _ = fs::remove_file(path);
    }
}

fn spawn_async_index_scan(root: PathBuf, max_bytes: u64, state: Arc<DiskIndexState>) {
    let worker_state = Arc::clone(&state);
    let spawn_result = std::thread::Builder::new()
        .name("fasttube-disk-cache-scan".into())
        .spawn(move || {
            let mut discovered = Vec::new();
            if root.is_dir() {
                let current_pid_marker = format!(".{}.", std::process::id());
                scan_directory_entries(&root, None, &current_pid_marker, &mut discovered);
            }
            let evicted = {
                let mut index = worker_state.index.lock().unwrap_or_else(|e| e.into_inner());
                for (key, size, last_used, legacy) in discovered {
                    if index.removed_before_init.contains(&key) {
                        continue;
                    }
                    if index.entries.contains_key(&key) {
                        continue;
                    }
                    index.total_bytes = index.total_bytes.saturating_add(size);
                    index.entries.insert(
                        key,
                        DiskEntryMeta {
                            size,
                            last_used,
                            seq: 0,
                            legacy,
                        },
                    );
                }
                index.removed_before_init.clear();
                index.initialized = true;
                let victims = collect_eviction_batch_locked(&root, max_bytes, &mut index, None);
                worker_state.init_done.notify_all();
                victims
            };
            remove_evicted_paths(evicted);
        });
    if spawn_result.is_err() {
        let mut index = state.index.lock().unwrap_or_else(|e| e.into_inner());
        index.initialized = true;
        state.init_done.notify_all();
    }
}

enum FetchUrlError {
    HttpOrNotFound,
    CancelledOrNetwork,
}

fn fetch_url_bytes(uri: &str, is_cancelled: &impl Fn() -> bool) -> Result<Vec<u8>, FetchUrlError> {
    if is_cancelled() {
        return Err(FetchUrlError::CancelledOrNetwork);
    }
    let response = crate::http_client::shared()
        .get(uri)
        .timeout(IMAGE_FETCH_TIMEOUT)
        .send()
        .map_err(|_| FetchUrlError::CancelledOrNetwork)?;
    if !response.status().is_success() {
        return Err(FetchUrlError::HttpOrNotFound);
    }
    if is_cancelled() {
        return Err(FetchUrlError::CancelledOrNetwork);
    }
    let mut bytes = Vec::new();
    response
        .take((MAX_CACHED_IMAGE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| FetchUrlError::CancelledOrNetwork)?;
    if bytes.is_empty() || bytes.len() > MAX_CACHED_IMAGE_BYTES {
        return Err(FetchUrlError::HttpOrNotFound);
    }
    Ok(bytes)
}

impl DiskImageCache {
    pub fn new(root: PathBuf, max_bytes: u64) -> Self {
        let state = Arc::new(DiskIndexState {
            index: Mutex::new(DiskIndex::default()),
            init_done: Condvar::new(),
        });
        spawn_async_index_scan(root.clone(), max_bytes, Arc::clone(&state));
        Self {
            root,
            max_bytes,
            state,
        }
    }

    pub fn default_production() -> Option<Arc<Self>> {
        default_disk_cache_dir()
            .ok()
            .map(|dir| Arc::new(Self::new(dir, DEFAULT_DISK_CACHE_MAX_BYTES)))
    }

    #[cfg(test)]
    pub fn entry_path_for_uri(&self, uri: &str) -> PathBuf {
        self.entry_path(&uri_cache_key(uri))
    }

    #[cfg(test)]
    pub fn legacy_entry_path_for_uri(&self, uri: &str) -> PathBuf {
        self.legacy_entry_path(&uri_cache_key(uri))
    }

    #[cfg(test)]
    pub fn is_index_initialized(&self) -> bool {
        self.state
            .index
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .initialized
    }

    #[cfg(test)]
    pub fn wait_for_index_init(&self) {
        let mut index = self.state.index.lock().unwrap_or_else(|e| e.into_inner());
        while !index.initialized {
            index = self
                .state
                .init_done
                .wait(index)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    fn shard_dir(&self, key: &str) -> PathBuf {
        shard_dir_for_root(&self.root, key)
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        sharded_entry_path_for_root(&self.root, key)
    }

    fn legacy_entry_path(&self, key: &str) -> PathBuf {
        legacy_entry_path_for_root(&self.root, key)
    }

    pub fn write_bytes(&self, uri: &str, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() || bytes.len() > MAX_CACHED_IMAGE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid cached image payload size",
            ));
        }
        let size = bytes.len() as u64;
        if size > self.max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "image exceeds disk cache capacity",
            ));
        }
        let key = uri_cache_key(uri);
        let shard_dir = self.shard_dir(&key);
        fs::create_dir_all(&shard_dir)?;
        let target = self.entry_path(&key);
        let (temp_path, mut file) = loop {
            let id = CACHE_TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let candidate = shard_dir.join(format!(".{key}.{}.{id}.tmp", std::process::id()));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(file) => break (candidate, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        let write_result = (|| {
            file.write_all(bytes)?;
            // Disposable cache: skip fsync (sync_all) to avoid storage barrier stalls;
            // temp-file + rename keeps readers from observing partial writes.
            drop(file);
            fs::rename(&temp_path, &target)
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
            return write_result;
        }
        let _ = fs::remove_file(self.legacy_entry_path(&key));

        let evicted = {
            let mut index = self.state.index.lock().unwrap_or_else(|e| e.into_inner());
            index.removed_before_init.remove(&key);
            if let Some(previous) = index.entries.remove(&key) {
                index.total_bytes = index.total_bytes.saturating_sub(previous.size);
            }
            index.next_seq = index.next_seq.wrapping_add(1);
            let seq = index.next_seq;
            index.total_bytes = index.total_bytes.saturating_add(size);
            index.entries.insert(
                key.clone(),
                DiskEntryMeta {
                    size,
                    last_used: SystemTime::now(),
                    seq,
                    legacy: false,
                },
            );
            collect_eviction_batch_locked(&self.root, self.max_bytes, &mut index, Some(&key))
        };
        remove_evicted_paths(evicted);
        Ok(())
    }

    pub fn read_bytes(&self, uri: &str) -> io::Result<Option<Vec<u8>>> {
        let key = uri_cache_key(uri);
        let sharded_path = self.entry_path(&key);
        let (file, opened_path, legacy) = match fs::File::open(&sharded_path) {
            Ok(file) => (file, sharded_path, false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let legacy_path = self.legacy_entry_path(&key);
                match fs::File::open(&legacy_path) {
                    Ok(file) => (file, legacy_path, true),
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {
                        let mut index = self.state.index.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(prev) = index.entries.remove(&key) {
                            index.total_bytes = index.total_bytes.saturating_sub(prev.size);
                        }
                        return Ok(None);
                    }
                    Err(err) => return Err(err),
                }
            }
            Err(error) => return Err(error),
        };
        let disk_mtime = file.metadata().ok().and_then(|meta| meta.modified().ok());
        let mut bytes = Vec::new();
        file.take((MAX_CACHED_IMAGE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.is_empty() || bytes.len() > MAX_CACHED_IMAGE_BYTES {
            self.remove(uri);
            return Ok(None);
        }
        let now = SystemTime::now();
        let should_touch_mtime = {
            let mut index = self.state.index.lock().unwrap_or_else(|e| e.into_inner());
            index.next_seq = index.next_seq.wrapping_add(1);
            let seq = index.next_seq;
            if let Some(entry) = index.entries.get_mut(&key) {
                let stale = now
                    .duration_since(entry.last_used)
                    .unwrap_or(Duration::ZERO)
                    >= MTIME_TOUCH_INTERVAL;
                entry.last_used = now;
                entry.seq = seq;
                entry.legacy = legacy;
                stale
            } else {
                let size = bytes.len() as u64;
                let stale = disk_mtime.is_some_and(|mtime| {
                    now.duration_since(mtime).unwrap_or(Duration::ZERO) >= MTIME_TOUCH_INTERVAL
                });
                index.total_bytes = index.total_bytes.saturating_add(size);
                index.entries.insert(
                    key,
                    DiskEntryMeta {
                        size,
                        last_used: now,
                        seq,
                        legacy,
                    },
                );
                stale
            }
        };
        if should_touch_mtime && let Ok(file) = OpenOptions::new().write(true).open(&opened_path) {
            let _ = file.set_modified(now);
        }
        Ok(Some(bytes))
    }

    pub fn remove(&self, uri: &str) {
        let key = uri_cache_key(uri);
        let _ = fs::remove_file(self.entry_path(&key));
        let _ = fs::remove_file(self.legacy_entry_path(&key));
        let mut index = self.state.index.lock().unwrap_or_else(|e| e.into_inner());
        if !index.initialized {
            index.removed_before_init.insert(key.clone());
        }
        if let Some(prev) = index.entries.remove(&key) {
            index.total_bytes = index.total_bytes.saturating_sub(prev.size);
        }
    }

    pub fn decode_cached_image(
        &self,
        uri: &str,
        max_width: u32,
        max_height: u32,
    ) -> Result<Option<egui::ColorImage>, DiskCacheError> {
        let Some(bytes) = self.read_bytes(uri).map_err(|_| DiskCacheError::Io)? else {
            return Ok(None);
        };
        match decode_color_image(&bytes, max_width, max_height) {
            Ok(image) => Ok(Some(image)),
            Err(_) => {
                self.remove(uri);
                Err(DiskCacheError::Corrupt)
            }
        }
    }

    fn fetch_and_cache_with_options(
        &self,
        uri: &str,
        fetch_kind: FetchKind,
        max_width: u32,
        max_height: u32,
        is_cancelled: impl Fn() -> bool,
    ) -> Result<egui::ColorImage, ()> {
        let candidates = image_fetch_candidates(uri, fetch_kind);
        for (idx, candidate_uri) in candidates.iter().enumerate() {
            if is_cancelled() {
                return Err(());
            }
            match fetch_url_bytes(candidate_uri, &is_cancelled) {
                Ok(bytes) => {
                    if is_cancelled() {
                        return Err(());
                    }
                    if let Ok(image) = decode_color_image(&bytes, max_width, max_height) {
                        let _ = self.write_bytes(uri, &bytes);
                        return Ok(image);
                    }
                    if idx + 1 < candidates.len() {
                        continue;
                    }
                    return Err(());
                }
                Err(FetchUrlError::HttpOrNotFound) if idx + 1 < candidates.len() => continue,
                Err(_) => return Err(()),
            }
        }
        Err(())
    }

    #[cfg(test)]
    pub fn total_bytes(&self) -> u64 {
        self.wait_for_index_init();
        self.state
            .index
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .total_bytes
    }

    #[cfg(test)]
    pub fn entry_count(&self) -> usize {
        self.wait_for_index_init();
        self.state
            .index
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries
            .len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum FetchPriority {
    High = 0,
    Normal = 1,
}

struct ImageFetchJob {
    disk_cache: Arc<DiskImageCache>,
    uri: String,
    fetch_kind: FetchKind,
    max_width: u32,
    max_height: u32,
    priority: FetchPriority,
    sort_key: i32,
    seq: u64,
    generation: u64,
    active_generation: Arc<AtomicU64>,
    cancellation: Cancellation,
    tx: mpsc::Sender<LoadedImage>,
    ctx: egui::Context,
}

impl ImageFetchJob {
    fn is_cancelled(&self) -> bool {
        self.cancellation.cancelled()
            || self.active_generation.load(Ordering::Relaxed) != self.generation
    }
}

#[derive(Default)]
struct WorkerQueue {
    jobs: Vec<ImageFetchJob>,
    next_seq: u64,
}

struct ImageWorkerPool {
    queue: Arc<(Mutex<WorkerQueue>, Condvar)>,
}

impl ImageWorkerPool {
    fn global() -> &'static Self {
        IMAGE_WORKER_POOL.get_or_init(|| Self::new(WORKER_POOL_THREADS))
    }

    fn new(thread_count: usize) -> Self {
        let thread_count = thread_count.clamp(1, WORKER_POOL_THREADS);
        let queue = Arc::new((Mutex::new(WorkerQueue::default()), Condvar::new()));
        for idx in 0..thread_count {
            let worker_queue = Arc::clone(&queue);
            let _ = std::thread::Builder::new()
                .name(format!("fasttube-img-worker-{idx}"))
                .spawn(move || worker_loop(worker_queue));
        }
        Self { queue }
    }

    fn enqueue(&self, mut job: ImageFetchJob) {
        let (lock, cvar) = &*self.queue;
        let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
        queue.next_seq = queue.next_seq.wrapping_add(1);
        job.seq = queue.next_seq;
        queue.jobs.push(job);
        cvar.notify_one();
    }
}

fn worker_loop(shared: Arc<(Mutex<WorkerQueue>, Condvar)>) {
    let (lock, cvar) = &*shared;
    loop {
        let (cancelled_jobs, next_job) = {
            let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
            while queue.jobs.is_empty() {
                queue = cvar.wait(queue).unwrap_or_else(|e| e.into_inner());
            }
            let mut cancelled = Vec::new();
            let mut idx = 0;
            while idx < queue.jobs.len() {
                if queue.jobs[idx].is_cancelled() {
                    cancelled.push(queue.jobs.swap_remove(idx));
                } else {
                    idx += 1;
                }
            }
            let best_idx = queue
                .jobs
                .iter()
                .enumerate()
                .min_by_key(|(_, job)| (job.priority, job.sort_key, job.seq))
                .map(|(i, _)| i);
            let next = best_idx.map(|i| queue.jobs.swap_remove(i));
            (cancelled, next)
        };

        for job in cancelled_jobs {
            let _ = job.tx.send(LoadedImage {
                uri: job.uri,
                image: Err(()),
                generation: job.generation,
                cancelled: true,
            });
            job.ctx.request_repaint();
        }

        if let Some(job) = next_job {
            run_image_fetch_job(job);
        }
    }
}

fn run_image_fetch_job(job: ImageFetchJob) {
    if job.is_cancelled() {
        let _ = job.tx.send(LoadedImage {
            uri: job.uri,
            image: Err(()),
            generation: job.generation,
            cancelled: true,
        });
        job.ctx.request_repaint();
        return;
    }

    let image = match job
        .disk_cache
        .decode_cached_image(&job.uri, job.max_width, job.max_height)
    {
        Ok(Some(img)) => Ok(img),
        Ok(None) | Err(_) => {
            if job.is_cancelled() {
                let _ = job.tx.send(LoadedImage {
                    uri: job.uri,
                    image: Err(()),
                    generation: job.generation,
                    cancelled: true,
                });
                job.ctx.request_repaint();
                return;
            }
            job.disk_cache.fetch_and_cache_with_options(
                &job.uri,
                job.fetch_kind,
                job.max_width,
                job.max_height,
                || job.is_cancelled(),
            )
        }
    };

    let cancelled = job.is_cancelled();
    let _ = job.tx.send(LoadedImage {
        uri: job.uri,
        image: if cancelled { Err(()) } else { image },
        generation: job.generation,
        cancelled,
    });
    job.ctx.request_repaint();
}

struct LoadedImage {
    uri: String,
    image: Result<egui::ColorImage, ()>,
    generation: u64,
    cancelled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryState {
    Loading {
        started_at: Instant,
    },
    Ready {
        size: [u32; 2],
        texture_id: Option<egui::TextureId>,
    },
    Failed,
}

#[derive(Clone)]
struct CacheEntry {
    state: EntryState,
    texture: Option<egui::TextureHandle>,
    ready_at: Option<Instant>,
    last_visible_pass: u64,
    last_access_tick: u64,
    cancel: Option<Cancellation>,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub uri: String,
    pub is_visible: bool,
    pub sort_key: i32,
}

#[derive(Clone, Copy, Debug)]
struct RequestSpec {
    fetch_kind: FetchKind,
    max_width: u32,
    max_height: u32,
    is_visible: bool,
    sort_key: i32,
}

#[derive(Clone, Debug)]
struct RetiredInFlight {
    uri: String,
    forget_until: Instant,
}

pub struct ThumbnailCache {
    entries: HashMap<String, CacheEntry>,
    candidates: Vec<Candidate>,
    request_specs: HashMap<String, RequestSpec>,
    retired_in_flight: VecDeque<RetiredInFlight>,
    pending_uploads: VecDeque<LoadedImage>,
    pass_index: u64,
    active_frame_pass: Option<u64>,
    began_frame_pass: Option<u64>,
    ended_frame_pass: Option<u64>,
    started_in_frame_pass: usize,
    upload_budget_pass: Option<u64>,
    uploads_in_pass: usize,
    access_counter: u64,
    last_load_started_at: Option<Instant>,
    max_retained: usize,
    max_concurrent_loads: usize,
    max_new_loads_per_pass: usize,
    load_start_interval: Duration,
    allow_burst_loads: bool,
    disk_cache: Option<Arc<DiskImageCache>>,
    generation: Arc<AtomicU64>,
    result_tx: mpsc::Sender<LoadedImage>,
    result_rx: mpsc::Receiver<LoadedImage>,
    storyboard_sheets: StoryboardSheetCache,
}

impl Default for ThumbnailCache {
    fn default() -> Self {
        let mut cache = Self::with_limits(
            MAX_RETAINED_THUMBNAILS,
            MAX_CONCURRENT_LOADS,
            MAX_NEW_LOADS_PER_PASS,
            LOAD_START_INTERVAL,
        );
        cache.allow_burst_loads = true;
        #[cfg(not(test))]
        {
            cache.set_disk_cache(DiskImageCache::default_production());
        }
        cache
    }
}

impl ThumbnailCache {
    pub fn with_limits(
        max_retained: usize,
        max_concurrent_loads: usize,
        max_new_loads_per_pass: usize,
        load_start_interval: Duration,
    ) -> Self {
        let max_retained = max_retained.max(1);
        let max_concurrent_loads = max_concurrent_loads.clamp(1, max_retained);
        let max_new_loads_per_pass = max_new_loads_per_pass.clamp(1, max_concurrent_loads);
        let (result_tx, result_rx) = mpsc::channel();
        Self {
            entries: HashMap::with_capacity(max_retained),
            candidates: Vec::new(),
            request_specs: HashMap::new(),
            retired_in_flight: VecDeque::new(),
            pending_uploads: VecDeque::new(),
            pass_index: 0,
            active_frame_pass: None,
            began_frame_pass: None,
            ended_frame_pass: None,
            started_in_frame_pass: 0,
            upload_budget_pass: None,
            uploads_in_pass: 0,
            access_counter: 0,
            last_load_started_at: None,
            max_retained,
            max_concurrent_loads,
            max_new_loads_per_pass,
            load_start_interval,
            allow_burst_loads: false,
            disk_cache: None,
            generation: Arc::new(AtomicU64::new(0)),
            result_tx,
            result_rx,
            storyboard_sheets: StoryboardSheetCache::with_limit(MAX_RETAINED_STORYBOARD_SHEETS),
        }
    }

    #[cfg(test)]
    pub fn with_disk_cache(mut self, disk_cache: Option<Arc<DiskImageCache>>) -> Self {
        self.set_disk_cache(disk_cache);
        self
    }

    pub fn set_disk_cache(&mut self, disk_cache: Option<Arc<DiskImageCache>>) {
        self.disk_cache = disk_cache.clone();
        self.storyboard_sheets.set_disk_cache(disk_cache);
    }

    #[cfg(test)]
    pub fn retained_count(&self) -> usize {
        self.entries.len()
    }

    pub fn in_flight_count(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| matches!(entry.state, EntryState::Loading { .. }))
            .count()
    }

    pub fn has_pending_work(&self) -> bool {
        self.in_flight_count() > 0
            || !self.pending_uploads.is_empty()
            || !self.candidates.is_empty()
            || self.storyboard_sheets.has_pending_work()
    }

    pub fn resolve_storyboard_texture(
        &mut self,
        ctx: &egui::Context,
        uri: &str,
    ) -> Option<(egui::TextureId, [u32; 2])> {
        self.storyboard_sheets.resolve_texture(ctx, uri)
    }

    pub fn clear_storyboards(&mut self, ctx: &egui::Context) {
        self.storyboard_sheets.clear(ctx);
    }

    pub fn evict_all_gpu_textures(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let forgotten = self.drain_all_for_reset(now);
        for uri in forgotten {
            ctx.forget_image(&uri);
        }
        self.storyboard_sheets.clear(ctx);
    }

    fn bump_tick(&mut self) -> u64 {
        self.access_counter = self.access_counter.wrapping_add(1);
        self.access_counter
    }

    pub fn reset(&mut self, ctx: &egui::Context, previous_videos: &[crate::search::Video]) {
        let now = Instant::now();
        let forgotten = self.drain_all_for_reset(now);
        for uri in forgotten {
            ctx.forget_image(&uri);
        }
        for video in previous_videos {
            if !video.thumbnail.is_empty() {
                ctx.forget_image(&video.thumbnail);
            }
        }
    }

    pub fn retain_for_videos(
        &mut self,
        ctx: &egui::Context,
        previous_videos: &[crate::search::Video],
        new_videos: &[crate::search::Video],
    ) {
        let retained: std::collections::HashSet<&str> = new_videos
            .iter()
            .filter(|video| !video.thumbnail.is_empty())
            .map(|video| video.thumbnail.as_str())
            .collect();
        let now = Instant::now();
        self.candidates
            .retain(|candidate| retained.contains(candidate.uri.as_str()));
        self.request_specs
            .retain(|uri, _| retained.contains(uri.as_str()));
        self.pending_uploads
            .retain(|loaded| retained.contains(loaded.uri.as_str()));
        let removed: Vec<String> = self
            .entries
            .keys()
            .filter(|uri| !retained.contains(uri.as_str()))
            .cloned()
            .collect();
        for uri in removed {
            if let Some(entry) = self.entries.remove(&uri)
                && matches!(entry.state, EntryState::Loading { .. })
            {
                if let Some(cancel) = entry.cancel {
                    cancel.cancel();
                }
                self.push_retired_in_flight(&uri, now);
            }
            ctx.forget_image(&uri);
        }
        for video in previous_videos {
            if !video.thumbnail.is_empty() && !retained.contains(video.thumbnail.as_str()) {
                ctx.forget_image(&video.thumbnail);
            }
        }
        self.retired_in_flight
            .retain(|item| !retained.contains(item.uri.as_str()));
    }

    fn drain_all_for_reset(&mut self, now: Instant) -> Vec<String> {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.candidates.clear();
        self.request_specs.clear();
        self.pending_uploads.clear();
        let entries = std::mem::take(&mut self.entries);
        let mut forgotten = Vec::with_capacity(entries.len());
        for (uri, entry) in entries {
            if matches!(entry.state, EntryState::Loading { .. }) {
                if let Some(cancel) = entry.cancel {
                    cancel.cancel();
                }
                self.push_retired_in_flight(&uri, now);
            }
            forgotten.push(uri);
        }
        forgotten
    }

    fn push_retired_in_flight(&mut self, uri: &str, now: Instant) {
        if let Some(existing) = self
            .retired_in_flight
            .iter_mut()
            .find(|item| item.uri == uri)
        {
            existing.forget_until = now + IN_FLIGHT_TIMEOUT;
            return;
        }
        self.retired_in_flight.push_back(RetiredInFlight {
            uri: uri.to_owned(),
            forget_until: now + IN_FLIGHT_TIMEOUT,
        });
    }

    fn drain_loaded(&mut self, ctx: &egui::Context) {
        let current_gen = self.generation.load(Ordering::Relaxed);
        while let Ok(loaded) = self.result_rx.try_recv() {
            self.retired_in_flight.retain(|item| item.uri != loaded.uri);
            if loaded.cancelled || loaded.generation != current_gen {
                continue;
            }
            let Some(entry) = self.entries.get_mut(&loaded.uri) else {
                continue;
            };
            if !matches!(entry.state, EntryState::Loading { .. }) {
                continue;
            }
            match loaded.image {
                Ok(_) => self.pending_uploads.push_back(loaded),
                Err(()) => {
                    entry.state = EntryState::Failed;
                    entry.texture = None;
                    entry.ready_at = None;
                    entry.cancel = None;
                }
            }
        }

        let pass_nr = ctx.cumulative_pass_nr();
        if self.upload_budget_pass != Some(pass_nr) {
            self.upload_budget_pass = Some(pass_nr);
            self.uploads_in_pass = 0;
        }

        while self.uploads_in_pass < MAX_GPU_UPLOADS_PER_FRAME {
            let Some(loaded) = self.pending_uploads.pop_front() else {
                break;
            };
            if loaded.cancelled || loaded.generation != current_gen {
                continue;
            }
            let Some(entry) = self.entries.get_mut(&loaded.uri) else {
                continue;
            };
            if !matches!(entry.state, EntryState::Loading { .. }) {
                continue;
            }
            if let Ok(color_image) = loaded.image {
                let size = [color_image.width() as u32, color_image.height() as u32];
                let texture = ctx.load_texture(
                    format!("thumb:{}", loaded.uri),
                    color_image,
                    egui::TextureOptions::LINEAR,
                );
                entry.state = EntryState::Ready {
                    size,
                    texture_id: Some(texture.id()),
                };
                entry.texture = Some(texture);
                entry.ready_at = Some(Instant::now());
                entry.cancel = None;
                self.uploads_in_pass += 1;
            }
        }

        if !self.pending_uploads.is_empty()
            && !ctx.input(|i| i.viewport().minimized.unwrap_or(false))
        {
            ctx.request_repaint();
        }
    }

    pub fn maintain(&mut self, ctx: &egui::Context) {
        self.active_frame_pass = Some(ctx.cumulative_pass_nr());
        self.drain_loaded(ctx);
        self.storyboard_sheets.maintain(ctx);
        if self.retired_in_flight.is_empty()
            && !self
                .entries
                .values()
                .any(|entry| matches!(entry.state, EntryState::Loading { .. }))
        {
            return;
        }
        let now = Instant::now();
        for uri in self.sweep_expired(now) {
            ctx.forget_image(&uri);
        }
    }

    fn sweep_expired(&mut self, now: Instant) -> Vec<String> {
        let mut to_forget = Vec::new();

        let timed_out: Vec<String> = self
            .entries
            .iter()
            .filter_map(|(uri, entry)| match entry.state {
                EntryState::Loading { started_at }
                    if now.saturating_duration_since(started_at) >= IN_FLIGHT_TIMEOUT =>
                {
                    Some(uri.clone())
                }
                _ => None,
            })
            .collect();

        for uri in timed_out {
            if let Some(entry) = self.entries.get_mut(&uri) {
                if let Some(cancel) = entry.cancel.take() {
                    cancel.cancel();
                }
                entry.state = EntryState::Failed;
                entry.texture = None;
                entry.ready_at = None;
            }
            self.push_retired_in_flight(&uri, now);
            to_forget.push(uri);
        }

        let mut remaining = VecDeque::with_capacity(self.retired_in_flight.len());
        while let Some(item) = self.retired_in_flight.pop_front() {
            if !matches!(
                self.entries.get(&item.uri).map(|entry| entry.state),
                Some(EntryState::Loading { .. } | EntryState::Ready { .. })
            ) {
                to_forget.push(item.uri.clone());
            }
            if now < item.forget_until {
                remaining.push_back(item);
            }
        }
        self.retired_in_flight = remaining;

        to_forget
    }

    /// Frame-scoped pass starter for Batch 2 top-of-frame integration.
    #[allow(dead_code)]
    pub fn begin_pass_for_frame(&mut self, ctx: &egui::Context) {
        self.active_frame_pass = Some(ctx.cumulative_pass_nr());
        self.begin_pass();
    }

    pub fn begin_pass(&mut self) {
        if let Some(pass_nr) = self.active_frame_pass
            && self.began_frame_pass == Some(pass_nr)
        {
            return;
        }
        self.pass_index = self.pass_index.wrapping_add(1);
        self.candidates.clear();
        self.request_specs.clear();
        self.started_in_frame_pass = 0;
        self.began_frame_pass = self.active_frame_pass;
        self.ended_frame_pass = None;
    }

    pub fn show_rounded(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        uri: &str,
        size: Vec2,
        corner_radius: f32,
    ) -> egui::Response {
        self.show_rounded_mode(ui, ctx, uri, size, corner_radius, false, true)
    }

    pub fn show_cover_rounded(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        uri: &str,
        size: Vec2,
        corner_radius: f32,
    ) -> egui::Response {
        self.show_rounded_mode(ui, ctx, uri, size, corner_radius, true, true)
    }

    pub fn show_cover_rounded_borderless(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        uri: &str,
        size: Vec2,
        corner_radius: f32,
    ) -> egui::Response {
        self.show_rounded_mode(ui, ctx, uri, size, corner_radius, true, false)
    }

    #[allow(clippy::too_many_arguments)]
    fn show_rounded_mode(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        uri: &str,
        size: Vec2,
        corner_radius: f32,
        cover: bool,
        paint_outline: bool,
    ) -> egui::Response {
        let pass_nr = ctx.cumulative_pass_nr();
        self.active_frame_pass = Some(pass_nr);
        if self.began_frame_pass.is_none() {
            self.began_frame_pass = Some(pass_nr);
        }

        let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
        response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

        let viewport = ui.clip_rect();
        let is_visible = viewport_intersects(viewport, rect, 0.0);
        let is_near_viewport = viewport_intersects(viewport, rect, VIEWPORT_OVERSCAN_PX);

        if is_visible {
            let bg_color = if response.hovered() {
                Color32::from_rgb(32, 32, 36)
            } else {
                Color32::from_rgb(20, 20, 23)
            };
            ui.painter().rect_filled(rect, corner_radius, bg_color);
        }

        if uri.is_empty() {
            if is_visible {
                paint_placeholder(ui, rect, PlaceholderKind::Empty, corner_radius);
                if paint_outline {
                    paint_hover_outline_rounded(ui, rect, response.hovered(), corner_radius);
                }
            }
            return response;
        }

        if is_near_viewport {
            let tick = self.bump_tick();
            let pass = self.pass_index;
            if let Some(entry) = self.entries.get_mut(uri) {
                entry.last_visible_pass = pass;
                entry.last_access_tick = tick;
            }
        }

        let strip_4_3_letterbox = cover && is_youtube_letterboxed_4_3_uri(uri);
        let uses_disk_worker = self.disk_cache.is_some() && uri_uses_disk_worker(uri);
        let (state, ready_at) = match self.entries.get(uri) {
            Some(entry) => (Some(entry.state), entry.ready_at),
            None => (None, None),
        };
        match state {
            Some(EntryState::Ready {
                size,
                texture_id: Some(texture_id),
            }) => {
                if is_visible {
                    paint_ready_thumbnail(
                        ui,
                        rect,
                        texture_id,
                        size,
                        corner_radius,
                        cover,
                        strip_4_3_letterbox,
                        ready_at,
                    );
                }
            }
            Some(EntryState::Ready { .. } | EntryState::Loading { .. }) => {
                if uses_disk_worker && matches!(state, Some(EntryState::Loading { .. })) {
                    if is_visible {
                        paint_placeholder(ui, rect, PlaceholderKind::Loading, corner_radius);
                    }
                } else if is_near_viewport || matches!(state, Some(EntryState::Loading { .. })) {
                    match Self::poll_texture(ctx, uri) {
                        PollOutcome::Ready { texture_id, size } => {
                            let now = Instant::now();
                            if let Some(entry) = self.entries.get_mut(uri) {
                                entry.state = EntryState::Ready {
                                    size,
                                    texture_id: Some(texture_id),
                                };
                                entry.ready_at = Some(now);
                                entry.cancel = None;
                            }
                            if is_visible {
                                paint_ready_thumbnail(
                                    ui,
                                    rect,
                                    texture_id,
                                    size,
                                    corner_radius,
                                    cover,
                                    strip_4_3_letterbox,
                                    Some(now),
                                );
                            }
                        }
                        PollOutcome::Pending => {
                            if is_visible {
                                paint_placeholder(
                                    ui,
                                    rect,
                                    PlaceholderKind::Loading,
                                    corner_radius,
                                );
                            }
                        }
                        PollOutcome::Failed => {
                            if let Some(entry) = self.entries.get_mut(uri) {
                                entry.state = EntryState::Failed;
                                entry.ready_at = None;
                                entry.cancel = None;
                            }
                            ctx.forget_image(uri);
                            if is_visible {
                                paint_placeholder(ui, rect, PlaceholderKind::Failed, corner_radius);
                            }
                        }
                    }
                } else if is_visible {
                    paint_placeholder(ui, rect, PlaceholderKind::Pending, corner_radius);
                }
            }
            Some(EntryState::Failed) => {
                if is_visible {
                    paint_placeholder(ui, rect, PlaceholderKind::Failed, corner_radius);
                }
            }
            None => {
                if is_near_viewport {
                    let (max_width, max_height, fetch_kind) =
                        decode_limits_for_request(uri, size, ctx.pixels_per_point(), cover);
                    let relative_top = rect.top() - viewport.top();
                    self.record_candidate(uri, is_visible, relative_top);
                    let sort_key = relative_top.round() as i32;
                    self.request_specs
                        .entry(uri.to_owned())
                        .and_modify(|spec| {
                            spec.is_visible |= is_visible;
                            spec.sort_key = spec.sort_key.min(sort_key);
                            spec.max_width = spec.max_width.max(max_width);
                            spec.max_height = spec.max_height.max(max_height);
                        })
                        .or_insert(RequestSpec {
                            fetch_kind,
                            max_width,
                            max_height,
                            is_visible,
                            sort_key,
                        });
                }
                if is_visible {
                    paint_placeholder(ui, rect, PlaceholderKind::Pending, corner_radius);
                }
            }
        }

        if is_visible && paint_outline {
            paint_hover_outline_rounded(ui, rect, response.hovered(), corner_radius);
        }
        response
    }

    fn record_candidate(&mut self, uri: &str, is_visible: bool, relative_top: f32) {
        if self.entries.contains_key(uri) {
            return;
        }
        let sort_key = relative_top.round() as i32;
        if let Some(existing) = self.candidates.iter_mut().find(|item| item.uri == uri) {
            existing.is_visible |= is_visible;
            existing.sort_key = existing.sort_key.min(sort_key);
            return;
        }
        self.candidates.push(Candidate {
            uri: uri.to_owned(),
            is_visible,
            sort_key,
        });
    }

    pub fn end_pass(&mut self, ctx: &egui::Context) {
        let pass_nr = ctx.cumulative_pass_nr();
        self.active_frame_pass = Some(pass_nr);
        if self.began_frame_pass != Some(pass_nr) {
            self.began_frame_pass = Some(pass_nr);
            self.started_in_frame_pass = 0;
        }
        if self.ended_frame_pass == Some(pass_nr) && self.candidates.is_empty() {
            return;
        }

        self.drain_loaded(ctx);
        let now = Instant::now();
        let remaining_budget = self
            .max_new_loads_per_pass
            .saturating_sub(self.started_in_frame_pass);
        let (admitted, evicted) = self.schedule_pass_with_budget(now, remaining_budget);
        self.started_in_frame_pass = self.started_in_frame_pass.saturating_add(admitted.len());
        self.ended_frame_pass = Some(pass_nr);

        for uri in evicted {
            ctx.forget_image(&uri);
        }

        let generation = self.generation.load(Ordering::Relaxed);
        for uri in admitted {
            if let Some(disk_cache) = self.disk_cache.clone()
                && uri_uses_disk_worker(&uri)
            {
                let spec = self.request_specs.remove(&uri).unwrap_or(RequestSpec {
                    fetch_kind: FetchKind::Thumbnail {
                        target_width_px: MAX_THUMBNAIL_DECODE_WIDTH,
                    },
                    max_width: MAX_THUMBNAIL_DECODE_WIDTH,
                    max_height: MAX_THUMBNAIL_DECODE_HEIGHT,
                    is_visible: true,
                    sort_key: 0,
                });
                let cancel = Cancellation::default();
                if let Some(entry) = self.entries.get_mut(&uri) {
                    entry.cancel = Some(cancel.clone());
                }
                enqueue_image_load(
                    disk_cache,
                    uri,
                    spec.fetch_kind,
                    spec.max_width,
                    spec.max_height,
                    if spec.is_visible {
                        FetchPriority::High
                    } else {
                        FetchPriority::Normal
                    },
                    spec.sort_key,
                    generation,
                    Arc::clone(&self.generation),
                    cancel,
                    self.result_tx.clone(),
                    ctx.clone(),
                );
                continue;
            }
            match Self::poll_texture(ctx, &uri) {
                PollOutcome::Ready { texture_id, size } => {
                    if let Some(entry) = self.entries.get_mut(&uri) {
                        entry.state = EntryState::Ready {
                            size,
                            texture_id: Some(texture_id),
                        };
                        entry.ready_at = Some(now);
                        entry.cancel = None;
                    }
                }
                PollOutcome::Pending => {}
                PollOutcome::Failed => {
                    if let Some(entry) = self.entries.get_mut(&uri) {
                        entry.state = EntryState::Failed;
                        entry.ready_at = None;
                        entry.cancel = None;
                    }
                    ctx.forget_image(&uri);
                }
            }
        }

        // Worker completions wake the UI directly via `ctx.request_repaint()`; never spin an
        // idle polling timer for in-flight or `retired_in_flight` entries. Only schedule a
        // cooldown wakeup when queued candidates can actually be admitted after `load_start_interval`.
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        if !minimized
            && !self.candidates.is_empty()
            && self.in_flight_count() + self.retired_in_flight.len() < self.max_concurrent_loads
            && (self.entries.len() < self.max_retained
                || self.entries.values().any(|entry| {
                    entry.last_visible_pass < self.pass_index
                        && !matches!(entry.state, EntryState::Loading { .. })
                }))
        {
            let wait = self
                .last_load_started_at
                .map(|started| {
                    self.load_start_interval
                        .saturating_sub(now.saturating_duration_since(started))
                })
                .unwrap_or(Duration::ZERO)
                .max(Duration::from_millis(16));
            ctx.request_repaint_after(wait);
        }
    }

    #[cfg(test)]
    fn schedule_pass(&mut self, now: Instant) -> (Vec<String>, Vec<String>) {
        self.schedule_pass_with_budget(now, self.max_new_loads_per_pass)
    }

    fn schedule_pass_with_budget(
        &mut self,
        now: Instant,
        max_new_loads: usize,
    ) -> (Vec<String>, Vec<String>) {
        let mut evicted = Vec::new();
        let mut admitted = Vec::new();

        while self.entries.len() > self.max_retained {
            if let Some(uri) = self.evict_one() {
                evicted.push(uri);
            } else {
                break;
            }
        }

        self.candidates.sort_by(|first, second| {
            second
                .is_visible
                .cmp(&first.is_visible)
                .then_with(|| first.sort_key.cmp(&second.sort_key))
        });

        let cooldown_ready = self
            .last_load_started_at
            .map(|started| now.saturating_duration_since(started) >= self.load_start_interval)
            .unwrap_or(true);

        if !cooldown_ready || max_new_loads == 0 {
            return (admitted, evicted);
        }

        let mut started_this_pass = 0usize;
        let candidates = std::mem::take(&mut self.candidates);
        let mut remaining_candidates = Vec::new();

        for candidate in candidates {
            if self.entries.contains_key(&candidate.uri) {
                continue;
            }
            if self
                .retired_in_flight
                .iter()
                .any(|item| item.uri == candidate.uri)
            {
                remaining_candidates.push(candidate);
                continue;
            }
            let can_start = started_this_pass < max_new_loads
                && self.in_flight_count() + self.retired_in_flight.len()
                    < self.max_concurrent_loads
                && (started_this_pass == 0
                    || self.allow_burst_loads
                    || self.load_start_interval.is_zero());
            if !can_start {
                remaining_candidates.push(candidate);
                continue;
            }

            if self.entries.len() >= self.max_retained {
                if let Some(victim) = self.evict_one() {
                    evicted.push(victim);
                } else {
                    remaining_candidates.push(candidate);
                    continue;
                }
            }

            let tick = self.bump_tick();
            self.entries.insert(
                candidate.uri.clone(),
                CacheEntry {
                    state: EntryState::Loading { started_at: now },
                    texture: None,
                    ready_at: None,
                    last_visible_pass: self.pass_index,
                    last_access_tick: tick,
                    cancel: None,
                },
            );
            self.last_load_started_at = Some(now);
            started_this_pass += 1;
            admitted.push(candidate.uri);
        }

        self.candidates = remaining_candidates;
        (admitted, evicted)
    }

    fn evict_one(&mut self) -> Option<String> {
        let current_pass = self.pass_index;

        let offscreen_finished = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                entry.last_visible_pass < current_pass
                    && !matches!(entry.state, EntryState::Loading { .. })
            })
            .min_by_key(|(_, entry)| (entry.last_visible_pass, entry.last_access_tick))
            .map(|(uri, _)| uri.clone());

        if let Some(uri) = offscreen_finished {
            self.entries.remove(&uri);
            return Some(uri);
        }

        None
    }

    fn poll_texture(ctx: &egui::Context, uri: &str) -> PollOutcome {
        let size_hint = egui::SizeHint::Size {
            width: MAX_THUMBNAIL_DECODE_WIDTH,
            height: MAX_THUMBNAIL_DECODE_HEIGHT,
            maintain_aspect_ratio: true,
        };
        match ctx.try_load_texture(uri, egui::TextureOptions::LINEAR, size_hint) {
            Ok(egui::load::TexturePoll::Ready { texture }) => {
                let width = texture.size.x.max(1.0).round() as u32;
                let height = texture.size.y.max(1.0).round() as u32;
                PollOutcome::Ready {
                    texture_id: texture.id,
                    size: [width, height],
                }
            }
            Ok(egui::load::TexturePoll::Pending { .. }) => PollOutcome::Pending,
            Err(_) => PollOutcome::Failed,
        }
    }
}

fn uri_uses_disk_worker(uri: &str) -> bool {
    uri.starts_with("http://") || uri.starts_with("https://")
}

#[allow(clippy::too_many_arguments)]
fn enqueue_image_load(
    disk_cache: Arc<DiskImageCache>,
    uri: String,
    fetch_kind: FetchKind,
    max_width: u32,
    max_height: u32,
    priority: FetchPriority,
    sort_key: i32,
    generation: u64,
    active_generation: Arc<AtomicU64>,
    cancellation: Cancellation,
    tx: mpsc::Sender<LoadedImage>,
    ctx: egui::Context,
) {
    ImageWorkerPool::global().enqueue(ImageFetchJob {
        disk_cache,
        uri,
        fetch_kind,
        max_width,
        max_height,
        priority,
        sort_key,
        seq: 0,
        generation,
        active_generation,
        cancellation,
        tx,
        ctx,
    });
}

enum PollOutcome {
    Ready {
        texture_id: egui::TextureId,
        size: [u32; 2],
    },
    Pending,
    Failed,
}

#[derive(Clone, Copy)]
enum PlaceholderKind {
    Empty,
    Pending,
    Loading,
    Failed,
}

pub fn viewport_intersects(viewport: Rect, item_rect: Rect, vertical_overscan: f32) -> bool {
    if !viewport.is_positive() || !item_rect.is_positive() {
        return false;
    }
    let expanded = Rect::from_min_max(
        Pos2::new(viewport.min.x, viewport.min.y - vertical_overscan),
        Pos2::new(viewport.max.x, viewport.max.y + vertical_overscan),
    );
    expanded.intersects(item_rect)
}

pub fn fit_aspect_rect(bounds: Rect, intrinsic_size: [u32; 2]) -> Rect {
    let width = intrinsic_size[0].max(1) as f32;
    let height = intrinsic_size[1].max(1) as f32;
    let scale = (bounds.width() / width).min(bounds.height() / height);
    let target_size = Vec2::new(
        (width * scale).min(bounds.width()),
        (height * scale).min(bounds.height()),
    );
    Rect::from_center_size(bounds.center(), target_size)
}

fn thumbnail_fade_alpha(ready_at: Option<Instant>) -> f32 {
    let Some(ready_at) = ready_at else {
        return 1.0;
    };
    let elapsed = ready_at.elapsed().as_secs_f32();
    if elapsed >= THUMBNAIL_FADE_DURATION_SECS {
        1.0
    } else {
        (elapsed / THUMBNAIL_FADE_DURATION_SECS).clamp(0.15, 1.0)
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_ready_thumbnail(
    ui: &mut egui::Ui,
    rect: Rect,
    texture_id: egui::TextureId,
    size: [u32; 2],
    corner_radius: f32,
    cover: bool,
    strip_4_3_letterbox: bool,
    ready_at: Option<Instant>,
) {
    let fade = thumbnail_fade_alpha(ready_at);
    let tint = if fade < 1.0 {
        paint_placeholder(ui, rect, PlaceholderKind::Loading, corner_radius);
        if !ui.ctx().input(|i| i.viewport().minimized.unwrap_or(false)) {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        let alpha = (fade * 255.0).round().clamp(1.0, 255.0) as u8;
        Color32::from_white_alpha(alpha)
    } else {
        Color32::WHITE
    };
    if cover {
        paint_cover_texture_rounded(
            ui,
            rect,
            texture_id,
            size,
            corner_radius,
            strip_4_3_letterbox,
            tint,
        );
    } else {
        paint_fitted_texture_rounded(ui, rect, texture_id, size, corner_radius, tint);
    }
}

fn paint_fitted_texture_rounded(
    ui: &mut egui::Ui,
    bounds: Rect,
    texture_id: egui::TextureId,
    intrinsic_size: [u32; 2],
    corner_radius: f32,
    tint: Color32,
) {
    let target = fit_aspect_rect(bounds, intrinsic_size);
    if corner_radius > 0.0 {
        egui::Image::from_texture((texture_id, target.size()))
            .corner_radius(corner_radius)
            .tint(tint)
            .paint_at(ui, target);
    } else {
        let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
        ui.painter().image(texture_id, target, uv, tint);
    }
}

pub(crate) fn compute_cover_uv(
    intrinsic_size: [u32; 2],
    bounds_size: Vec2,
    strip_4_3_letterbox: bool,
) -> Rect {
    let tex_w = intrinsic_size[0].max(1) as f32;
    let tex_h = intrinsic_size[1].max(1) as f32;
    let tex_aspect = tex_w / tex_h;
    // Standard 4:3 YouTube thumbnails (`hqdefault`/`sddefault`/`default`/`0`) pad 16:9 frames
    // with 12.5% black bars at top and bottom; trim them so cover-cropped cards render edge-to-edge.
    let (v_min, v_max, eff_aspect) = if strip_4_3_letterbox && (tex_aspect - 4.0 / 3.0).abs() < 0.04
    {
        (0.125_f32, 0.875_f32, tex_w / (tex_h * 0.75))
    } else {
        (0.0_f32, 1.0_f32, tex_aspect)
    };
    let bounds_w = bounds_size.x.max(1.0);
    let bounds_h = bounds_size.y.max(1.0);
    let bounds_aspect = bounds_w / bounds_h;
    if (eff_aspect - bounds_aspect).abs() < 1e-4 {
        Rect::from_min_max(Pos2::new(0.0, v_min), Pos2::new(1.0, v_max))
    } else if eff_aspect > bounds_aspect {
        let visible_fraction = (bounds_aspect / eff_aspect).clamp(0.0, 1.0);
        let pad = (1.0 - visible_fraction) * 0.5;
        Rect::from_min_max(Pos2::new(pad, v_min), Pos2::new(1.0 - pad, v_max))
    } else {
        let visible_fraction = (eff_aspect / bounds_aspect).clamp(0.0, 1.0);
        let v_span = v_max - v_min;
        let pad = v_span * (1.0 - visible_fraction) * 0.5;
        Rect::from_min_max(Pos2::new(0.0, v_min + pad), Pos2::new(1.0, v_max - pad))
    }
}

fn paint_cover_texture_rounded(
    ui: &mut egui::Ui,
    bounds: Rect,
    texture_id: egui::TextureId,
    intrinsic_size: [u32; 2],
    corner_radius: f32,
    strip_4_3_letterbox: bool,
    tint: Color32,
) {
    let uv = compute_cover_uv(intrinsic_size, bounds.size(), strip_4_3_letterbox);
    if corner_radius > 0.0 {
        egui::Image::from_texture((texture_id, bounds.size()))
            .uv(uv)
            .corner_radius(corner_radius)
            .tint(tint)
            .paint_at(ui, bounds);
    } else {
        ui.painter().image(texture_id, bounds, uv, tint);
    }
}

fn paint_avatar_circle_ring(ui: &egui::Ui, rect: Rect) {
    let radius = (rect.width().min(rect.height()) * 0.5 - 0.5).max(0.5);
    ui.painter().circle_stroke(
        rect.center(),
        radius,
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(128, 128, 128, 48)),
    );
}

pub const MAX_RETAINED_AVATARS: usize = 48;
const MAX_CONCURRENT_AVATAR_LOADS: usize = 4;
const MAX_CONCURRENT_STORYBOARD_LOADS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AvatarEntryState {
    Loading {
        started_at: Instant,
    },
    Ready {
        size: [u32; 2],
        texture_id: Option<egui::TextureId>,
    },
    Failed,
}

#[derive(Clone)]
struct AvatarEntry {
    state: AvatarEntryState,
    texture: Option<egui::TextureHandle>,
    last_access_tick: u64,
}

pub struct AvatarCache {
    entries: HashMap<String, AvatarEntry>,
    pending_uploads: VecDeque<LoadedImage>,
    upload_budget_pass: Option<u64>,
    uploads_in_pass: usize,
    access_counter: u64,
    max_retained: usize,
    disk_cache: Option<Arc<DiskImageCache>>,
    generation: Arc<AtomicU64>,
    result_tx: mpsc::Sender<LoadedImage>,
    result_rx: mpsc::Receiver<LoadedImage>,
}

impl Default for AvatarCache {
    fn default() -> Self {
        #[cfg(not(test))]
        {
            let mut cache = Self::with_limit(MAX_RETAINED_AVATARS);
            cache.disk_cache = DiskImageCache::default_production();
            cache
        }
        #[cfg(test)]
        {
            Self::with_limit(MAX_RETAINED_AVATARS)
        }
    }
}

impl AvatarCache {
    pub fn with_limit(max_retained: usize) -> Self {
        let max_retained = max_retained.max(1);
        let (result_tx, result_rx) = mpsc::channel();
        Self {
            entries: HashMap::with_capacity(max_retained),
            pending_uploads: VecDeque::new(),
            upload_budget_pass: None,
            uploads_in_pass: 0,
            access_counter: 0,
            max_retained,
            disk_cache: None,
            generation: Arc::new(AtomicU64::new(0)),
            result_tx,
            result_rx,
        }
    }

    pub fn set_disk_cache(&mut self, disk_cache: Option<Arc<DiskImageCache>>) {
        self.disk_cache = disk_cache;
    }

    pub fn evict_all_gpu_textures(&mut self, ctx: &egui::Context) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.pending_uploads.clear();
        for uri in self.entries.keys() {
            ctx.forget_image(uri);
        }
        self.entries.clear();
    }

    pub fn has_pending_work(&self) -> bool {
        !self.pending_uploads.is_empty()
            || self
                .entries
                .values()
                .any(|entry| matches!(entry.state, AvatarEntryState::Loading { .. }))
    }

    pub fn maintain(&mut self, ctx: &egui::Context) {
        let current_gen = self.generation.load(Ordering::Relaxed);
        while let Ok(loaded) = self.result_rx.try_recv() {
            if loaded.cancelled || loaded.generation != current_gen {
                continue;
            }
            let Some(entry) = self.entries.get_mut(&loaded.uri) else {
                continue;
            };
            if !matches!(entry.state, AvatarEntryState::Loading { .. }) {
                continue;
            }
            match loaded.image {
                Ok(_) => self.pending_uploads.push_back(loaded),
                Err(()) => {
                    entry.state = AvatarEntryState::Failed;
                    entry.texture = None;
                }
            }
        }

        let pass_nr = ctx.cumulative_pass_nr();
        if self.upload_budget_pass != Some(pass_nr) {
            self.upload_budget_pass = Some(pass_nr);
            self.uploads_in_pass = 0;
        }
        while self.uploads_in_pass < MAX_GPU_UPLOADS_PER_FRAME {
            let Some(loaded) = self.pending_uploads.pop_front() else {
                break;
            };
            if loaded.cancelled || loaded.generation != current_gen {
                continue;
            }
            let Some(entry) = self.entries.get_mut(&loaded.uri) else {
                continue;
            };
            if !matches!(entry.state, AvatarEntryState::Loading { .. }) {
                continue;
            }
            if let Ok(color_image) = loaded.image {
                let size = [color_image.width() as u32, color_image.height() as u32];
                let texture = ctx.load_texture(
                    format!("avatar:{}", loaded.uri),
                    color_image,
                    egui::TextureOptions::LINEAR,
                );
                entry.state = AvatarEntryState::Ready {
                    size,
                    texture_id: Some(texture.id()),
                };
                entry.texture = Some(texture);
                self.uploads_in_pass += 1;
            }
        }
        if !self.pending_uploads.is_empty()
            && !ctx.input(|i| i.viewport().minimized.unwrap_or(false))
        {
            ctx.request_repaint();
        }

        let now = Instant::now();
        for entry in self.entries.values_mut() {
            if let AvatarEntryState::Loading { started_at } = entry.state
                && now.saturating_duration_since(started_at) >= IN_FLIGHT_TIMEOUT
            {
                entry.state = AvatarEntryState::Failed;
                entry.texture = None;
            }
        }
    }

    fn bump_tick(&mut self) -> u64 {
        self.access_counter = self.access_counter.wrapping_add(1);
        self.access_counter
    }

    fn touch_or_insert(&mut self, uri: &str, now: Instant) -> Option<String> {
        let tick = self.bump_tick();
        if let Some(entry) = self.entries.get_mut(uri) {
            entry.last_access_tick = tick;
            return None;
        }
        let evicted = if self.entries.len() >= self.max_retained {
            self.entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_access_tick)
                .map(|(key, _)| key.clone())
                .inspect(|victim| {
                    self.entries.remove(victim);
                })
        } else {
            None
        };
        self.entries.insert(
            uri.to_owned(),
            AvatarEntry {
                state: AvatarEntryState::Loading { started_at: now },
                texture: None,
                last_access_tick: tick,
            },
        );
        evicted
    }

    pub fn try_paint_circle(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        rect: Rect,
        uri: &str,
    ) -> bool {
        let trimmed = uri.trim();
        if trimmed.is_empty() {
            return false;
        }
        let viewport = ui.clip_rect();
        let is_near_viewport = viewport_intersects(viewport, rect, VIEWPORT_OVERSCAN_PX);
        let is_visible = viewport_intersects(viewport, rect, 0.0);
        let already_tracked = self.entries.contains_key(trimmed);
        if !is_near_viewport && !already_tracked {
            return false;
        }

        let uses_disk_worker = self.disk_cache.is_some() && uri_uses_disk_worker(trimmed);
        let now = Instant::now();
        if let Some(entry) = self.entries.get_mut(trimmed) {
            match entry.state {
                AvatarEntryState::Failed => return false,
                AvatarEntryState::Ready {
                    size,
                    texture_id: Some(texture_id),
                } => {
                    self.access_counter = self.access_counter.wrapping_add(1);
                    entry.last_access_tick = self.access_counter;
                    if is_visible {
                        paint_fitted_texture_rounded(
                            ui,
                            rect,
                            texture_id,
                            size,
                            rect.width().min(rect.height()) * 0.5,
                            Color32::WHITE,
                        );
                        paint_avatar_circle_ring(ui, rect);
                    }
                    return true;
                }
                AvatarEntryState::Loading { started_at }
                    if now.saturating_duration_since(started_at) >= IN_FLIGHT_TIMEOUT =>
                {
                    entry.state = AvatarEntryState::Failed;
                    entry.texture = None;
                    ctx.forget_image(trimmed);
                    return false;
                }
                AvatarEntryState::Loading { .. } if uses_disk_worker => {
                    self.access_counter = self.access_counter.wrapping_add(1);
                    entry.last_access_tick = self.access_counter;
                    return false;
                }
                _ => {}
            }
        }

        let is_new = !already_tracked;
        if is_new && uses_disk_worker {
            let in_flight = self
                .entries
                .values()
                .filter(|entry| matches!(entry.state, AvatarEntryState::Loading { .. }))
                .count();
            if in_flight >= MAX_CONCURRENT_AVATAR_LOADS {
                // Active worker completions already wake the UI via `ctx.request_repaint()`.
                return false;
            }
        }
        if let Some(evicted) = self.touch_or_insert(trimmed, now) {
            ctx.forget_image(&evicted);
        }

        if uses_disk_worker {
            if is_new && let Some(disk_cache) = self.disk_cache.clone() {
                let generation = self.generation.load(Ordering::Relaxed);
                enqueue_image_load(
                    disk_cache,
                    trimmed.to_owned(),
                    FetchKind::Avatar,
                    MAX_AVATAR_DECODE_SIZE,
                    MAX_AVATAR_DECODE_SIZE,
                    if is_visible {
                        FetchPriority::High
                    } else {
                        FetchPriority::Normal
                    },
                    (rect.top() - viewport.top()).round() as i32,
                    generation,
                    Arc::clone(&self.generation),
                    Cancellation::default(),
                    self.result_tx.clone(),
                    ctx.clone(),
                );
            }
            return false;
        }

        let size_hint = egui::SizeHint::Size {
            width: MAX_AVATAR_DECODE_SIZE,
            height: MAX_AVATAR_DECODE_SIZE,
            maintain_aspect_ratio: true,
        };
        match ctx.try_load_texture(trimmed, egui::TextureOptions::LINEAR, size_hint) {
            Ok(egui::load::TexturePoll::Ready { texture }) => {
                let size = [
                    texture.size.x.max(1.0).round() as u32,
                    texture.size.y.max(1.0).round() as u32,
                ];
                if let Some(entry) = self.entries.get_mut(trimmed) {
                    entry.state = AvatarEntryState::Ready {
                        size,
                        texture_id: Some(texture.id),
                    };
                }
                if is_visible {
                    paint_fitted_texture_rounded(
                        ui,
                        rect,
                        texture.id,
                        size,
                        rect.width().min(rect.height()) * 0.5,
                        Color32::WHITE,
                    );
                    paint_avatar_circle_ring(ui, rect);
                }
                true
            }
            Ok(egui::load::TexturePoll::Pending { .. }) => false,
            Err(_) => {
                if let Some(entry) = self.entries.get_mut(trimmed) {
                    entry.state = AvatarEntryState::Failed;
                }
                ctx.forget_image(trimmed);
                false
            }
        }
    }
}

pub struct StoryboardSheetCache {
    entries: HashMap<String, AvatarEntry>,
    pending_uploads: VecDeque<LoadedImage>,
    upload_budget_pass: Option<u64>,
    uploads_in_pass: usize,
    access_counter: u64,
    max_retained: usize,
    disk_cache: Option<Arc<DiskImageCache>>,
    generation: Arc<AtomicU64>,
    result_tx: mpsc::Sender<LoadedImage>,
    result_rx: mpsc::Receiver<LoadedImage>,
}

impl StoryboardSheetCache {
    pub fn with_limit(max_retained: usize) -> Self {
        let max_retained = max_retained.max(1);
        let (result_tx, result_rx) = mpsc::channel();
        Self {
            entries: HashMap::with_capacity(max_retained),
            pending_uploads: VecDeque::new(),
            upload_budget_pass: None,
            uploads_in_pass: 0,
            access_counter: 0,
            max_retained,
            disk_cache: None,
            generation: Arc::new(AtomicU64::new(0)),
            result_tx,
            result_rx,
        }
    }

    pub fn set_disk_cache(&mut self, disk_cache: Option<Arc<DiskImageCache>>) {
        self.disk_cache = disk_cache;
    }

    pub fn has_pending_work(&self) -> bool {
        !self.pending_uploads.is_empty()
            || self
                .entries
                .values()
                .any(|entry| matches!(entry.state, AvatarEntryState::Loading { .. }))
    }

    pub fn clear(&mut self, ctx: &egui::Context) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.pending_uploads.clear();
        for uri in self.entries.keys() {
            ctx.forget_image(uri);
        }
        self.entries.clear();
    }

    pub fn maintain(&mut self, ctx: &egui::Context) {
        let current_gen = self.generation.load(Ordering::Relaxed);
        while let Ok(loaded) = self.result_rx.try_recv() {
            if loaded.cancelled || loaded.generation != current_gen {
                continue;
            }
            let Some(entry) = self.entries.get_mut(&loaded.uri) else {
                continue;
            };
            if !matches!(entry.state, AvatarEntryState::Loading { .. }) {
                continue;
            }
            match loaded.image {
                Ok(_) => self.pending_uploads.push_back(loaded),
                Err(()) => {
                    entry.state = AvatarEntryState::Failed;
                    entry.texture = None;
                }
            }
        }

        let pass_nr = ctx.cumulative_pass_nr();
        if self.upload_budget_pass != Some(pass_nr) {
            self.upload_budget_pass = Some(pass_nr);
            self.uploads_in_pass = 0;
        }
        while self.uploads_in_pass < MAX_GPU_UPLOADS_PER_FRAME {
            let Some(loaded) = self.pending_uploads.pop_front() else {
                break;
            };
            if loaded.cancelled || loaded.generation != current_gen {
                continue;
            }
            let Some(entry) = self.entries.get_mut(&loaded.uri) else {
                continue;
            };
            if !matches!(entry.state, AvatarEntryState::Loading { .. }) {
                continue;
            }
            if let Ok(color_image) = loaded.image {
                let size = [color_image.width() as u32, color_image.height() as u32];
                let texture = ctx.load_texture(
                    format!("sb:{}", loaded.uri),
                    color_image,
                    egui::TextureOptions::LINEAR,
                );
                entry.state = AvatarEntryState::Ready {
                    size,
                    texture_id: Some(texture.id()),
                };
                entry.texture = Some(texture);
                self.uploads_in_pass += 1;
            }
        }
        if !self.pending_uploads.is_empty()
            && !ctx.input(|i| i.viewport().minimized.unwrap_or(false))
        {
            ctx.request_repaint();
        }

        let now = Instant::now();
        for entry in self.entries.values_mut() {
            if let AvatarEntryState::Loading { started_at } = entry.state
                && now.saturating_duration_since(started_at) >= IN_FLIGHT_TIMEOUT
            {
                entry.state = AvatarEntryState::Failed;
                entry.texture = None;
            }
        }
    }

    pub fn resolve_texture(
        &mut self,
        ctx: &egui::Context,
        uri: &str,
    ) -> Option<(egui::TextureId, [u32; 2])> {
        let trimmed = uri.trim();
        if trimmed.is_empty() {
            return None;
        }
        let uses_disk_worker = self.disk_cache.is_some() && uri_uses_disk_worker(trimmed);
        let now = Instant::now();
        let already_tracked = self.entries.contains_key(trimmed);

        if let Some(entry) = self.entries.get_mut(trimmed) {
            match entry.state {
                AvatarEntryState::Failed => return None,
                AvatarEntryState::Ready {
                    size,
                    texture_id: Some(texture_id),
                } => {
                    self.access_counter = self.access_counter.wrapping_add(1);
                    entry.last_access_tick = self.access_counter;
                    return Some((texture_id, size));
                }
                AvatarEntryState::Loading { started_at }
                    if now.saturating_duration_since(started_at) >= IN_FLIGHT_TIMEOUT =>
                {
                    entry.state = AvatarEntryState::Failed;
                    entry.texture = None;
                    ctx.forget_image(trimmed);
                    return None;
                }
                AvatarEntryState::Loading { .. } if uses_disk_worker => {
                    self.access_counter = self.access_counter.wrapping_add(1);
                    entry.last_access_tick = self.access_counter;
                    return None;
                }
                _ => {}
            }
        }

        let is_new = !already_tracked;
        if is_new && uses_disk_worker {
            let in_flight = self
                .entries
                .values()
                .filter(|entry| matches!(entry.state, AvatarEntryState::Loading { .. }))
                .count();
            if in_flight >= MAX_CONCURRENT_STORYBOARD_LOADS {
                return None;
            }
        }
        self.access_counter = self.access_counter.wrapping_add(1);
        let tick = self.access_counter;
        if is_new {
            if self.entries.len() >= self.max_retained
                && let Some(victim) = self
                    .entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.last_access_tick)
                    .map(|(k, _)| k.clone())
            {
                self.entries.remove(&victim);
                ctx.forget_image(&victim);
            }
            self.entries.insert(
                trimmed.to_owned(),
                AvatarEntry {
                    state: AvatarEntryState::Loading { started_at: now },
                    texture: None,
                    last_access_tick: tick,
                },
            );
        }

        if uses_disk_worker {
            if is_new && let Some(disk_cache) = self.disk_cache.clone() {
                let generation = self.generation.load(Ordering::Relaxed);
                enqueue_image_load(
                    disk_cache,
                    trimmed.to_owned(),
                    FetchKind::Storyboard,
                    MAX_STORYBOARD_DECODE_SIZE,
                    MAX_STORYBOARD_DECODE_SIZE,
                    FetchPriority::Normal,
                    0,
                    generation,
                    Arc::clone(&self.generation),
                    Cancellation::default(),
                    self.result_tx.clone(),
                    ctx.clone(),
                );
            }
            return None;
        }

        let size_hint = egui::SizeHint::Size {
            width: MAX_STORYBOARD_DECODE_SIZE,
            height: MAX_STORYBOARD_DECODE_SIZE,
            maintain_aspect_ratio: true,
        };
        match ctx.try_load_texture(trimmed, egui::TextureOptions::LINEAR, size_hint) {
            Ok(egui::load::TexturePoll::Ready { texture }) => {
                let size = [
                    texture.size.x.max(1.0).round() as u32,
                    texture.size.y.max(1.0).round() as u32,
                ];
                if let Some(entry) = self.entries.get_mut(trimmed) {
                    entry.state = AvatarEntryState::Ready {
                        size,
                        texture_id: Some(texture.id),
                    };
                }
                Some((texture.id, size))
            }
            Ok(egui::load::TexturePoll::Pending { .. }) => None,
            Err(_) => {
                if let Some(entry) = self.entries.get_mut(trimmed) {
                    entry.state = AvatarEntryState::Failed;
                }
                ctx.forget_image(trimmed);
                None
            }
        }
    }
}

fn paint_placeholder(ui: &egui::Ui, rect: Rect, kind: PlaceholderKind, corner_radius: f32) {
    let dark_theme = ui.style().visuals.dark_mode;
    if matches!(kind, PlaceholderKind::Loading | PlaceholderKind::Pending) {
        crate::skeleton::paint_skeleton_rect(ui, rect, corner_radius, dark_theme);
        return;
    }

    let painter = ui.painter();
    let base_fill = if dark_theme {
        Color32::from_rgb(30, 30, 34)
    } else {
        Color32::from_rgb(232, 232, 236)
    };
    painter.rect_filled(rect, corner_radius, base_fill);

    let center = rect.center();
    let accent = match kind {
        PlaceholderKind::Empty => {
            if dark_theme {
                Color32::from_rgb(76, 76, 84)
            } else {
                Color32::from_rgb(165, 165, 175)
            }
        }
        PlaceholderKind::Failed => Color32::from_rgb(140, 74, 74),
        PlaceholderKind::Pending | PlaceholderKind::Loading => unreachable!(),
    };
    painter.circle_stroke(center, 13.0, Stroke::new(1.2, accent));

    let p0 = Pos2::new(center.x - 4.0, center.y - 5.5);
    let p1 = Pos2::new(center.x - 4.0, center.y + 5.5);
    let p2 = Pos2::new(center.x + 5.5, center.y);
    let stroke = Stroke::new(1.4, accent);
    painter.line_segment([p0, p1], stroke);
    painter.line_segment([p1, p2], stroke);
    painter.line_segment([p2, p0], stroke);
}

fn paint_hover_outline_rounded(ui: &egui::Ui, rect: Rect, hovered: bool, corner_radius: f32) {
    let dark_theme = ui.style().visuals.dark_mode;
    let color = if hovered {
        if dark_theme {
            Color32::from_rgb(95, 95, 105)
        } else {
            Color32::from_rgb(175, 175, 185)
        }
    } else if dark_theme {
        Color32::from_rgb(32, 32, 36)
    } else {
        Color32::from_rgb(228, 228, 232)
    };
    ui.painter().rect_stroke(
        rect,
        corner_radius,
        Stroke::new(1.0, color),
        StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video_with_thumbnail(uri: &str) -> crate::search::Video {
        crate::search::Video {
            title: "Video".into(),
            channel: "Channel".into(),
            duration: "1:00".into(),
            url: "https://www.youtube.com/watch?v=video".into(),
            thumbnail: uri.into(),
        }
    }

    fn mark_ready(cache: &mut ThumbnailCache, uri: &str) {
        if let Some(entry) = cache.entries.get_mut(uri) {
            entry.state = EntryState::Ready {
                size: [320, 180],
                texture_id: None,
            };
            entry.ready_at = Some(Instant::now());
        }
    }

    #[test]
    fn progressive_video_retention_keeps_loaded_and_in_flight_thumbnails() {
        let ctx = egui::Context::default();
        let mut cache = ThumbnailCache::with_limits(4, 2, 1, Duration::ZERO);
        let now = Instant::now();
        cache.entries.insert(
            "https://img/shared.jpg".into(),
            CacheEntry {
                state: EntryState::Loading { started_at: now },
                texture: None,
                ready_at: None,
                last_visible_pass: 0,
                last_access_tick: 1,
                cancel: None,
            },
        );
        cache.entries.insert(
            "https://img/old.jpg".into(),
            CacheEntry {
                state: EntryState::Ready {
                    size: [320, 180],
                    texture_id: None,
                },
                texture: None,
                ready_at: None,
                last_visible_pass: 0,
                last_access_tick: 2,
                cancel: None,
            },
        );
        let previous = vec![
            video_with_thumbnail("https://img/shared.jpg"),
            video_with_thumbnail("https://img/old.jpg"),
        ];
        let next = vec![video_with_thumbnail("https://img/shared.jpg")];

        cache.retain_for_videos(&ctx, &previous, &next);

        assert!(matches!(
            cache
                .entries
                .get("https://img/shared.jpg")
                .map(|entry| entry.state),
            Some(EntryState::Loading { .. })
        ));
        assert!(!cache.entries.contains_key("https://img/old.jpg"));
        assert!(
            cache
                .retired_in_flight
                .iter()
                .all(|item| item.uri != "https://img/shared.jpg")
        );
        assert_eq!(cache.in_flight_count(), 1);
    }

    #[test]
    fn gradual_loading_respects_per_pass_and_concurrency_caps() {
        let mut cache = ThumbnailCache::with_limits(6, 2, 1, Duration::from_millis(50));
        let t0 = Instant::now();

        cache.begin_pass();
        cache.record_candidate("https://img/1.jpg", true, 0.0);
        cache.record_candidate("https://img/2.jpg", true, 98.0);
        cache.record_candidate("https://img/3.jpg", true, 196.0);

        let (admitted_1, evicted_1) = cache.schedule_pass(t0);
        assert_eq!(admitted_1, vec!["https://img/1.jpg"]);
        assert!(evicted_1.is_empty());
        assert_eq!(cache.in_flight_count(), 1);
        assert_eq!(cache.retained_count(), 1);

        let (admitted_early, _) = cache.schedule_pass(t0 + Duration::from_millis(10));
        assert!(admitted_early.is_empty());

        let t1 = t0 + Duration::from_millis(60);
        let (admitted_2, _) = cache.schedule_pass(t1);
        assert_eq!(admitted_2, vec!["https://img/2.jpg"]);
        assert_eq!(cache.in_flight_count(), 2);

        let t2 = t1 + Duration::from_millis(60);
        let (admitted_blocked, _) = cache.schedule_pass(t2);
        assert!(admitted_blocked.is_empty());

        mark_ready(&mut cache, "https://img/1.jpg");
        let (admitted_3, _) = cache.schedule_pass(t2);
        assert_eq!(admitted_3, vec!["https://img/3.jpg"]);
        assert_eq!(cache.retained_count(), 3);
    }

    #[test]
    fn retention_limit_evicts_least_recently_visible_offscreen_entries() {
        let mut cache = ThumbnailCache::with_limits(3, 2, 2, Duration::ZERO);
        let now = Instant::now();

        cache.begin_pass();
        cache.record_candidate("https://img/1.jpg", true, 0.0);
        cache.record_candidate("https://img/2.jpg", true, 100.0);
        let (admitted, _) = cache.schedule_pass(now);
        assert_eq!(admitted.len(), 2);
        mark_ready(&mut cache, "https://img/1.jpg");
        mark_ready(&mut cache, "https://img/2.jpg");

        cache.record_candidate("https://img/3.jpg", true, 200.0);
        let (admitted, evicted) = cache.schedule_pass(now);
        assert_eq!(admitted, vec!["https://img/3.jpg"]);
        assert!(evicted.is_empty());
        mark_ready(&mut cache, "https://img/3.jpg");
        assert_eq!(cache.retained_count(), 3);

        cache.begin_pass();
        for uri in ["https://img/2.jpg", "https://img/3.jpg"] {
            let tick = cache.bump_tick();
            let pass = cache.pass_index;
            if let Some(entry) = cache.entries.get_mut(uri) {
                entry.last_visible_pass = pass;
                entry.last_access_tick = tick;
            }
        }
        cache.record_candidate("https://img/4.jpg", true, 300.0);

        let (admitted, evicted) = cache.schedule_pass(now);
        assert_eq!(admitted, vec!["https://img/4.jpg"]);
        assert_eq!(evicted, vec!["https://img/1.jpg"]);
        assert_eq!(cache.retained_count(), 3);
        assert!(!cache.entries.contains_key("https://img/1.jpg"));
        assert!(cache.entries.contains_key("https://img/2.jpg"));
        assert!(cache.entries.contains_key("https://img/3.jpg"));
        assert!(cache.entries.contains_key("https://img/4.jpg"));
    }

    #[test]
    fn reset_and_timeout_safely_track_retired_in_flight_requests() {
        let mut cache = ThumbnailCache::with_limits(4, 2, 2, Duration::ZERO);
        let t0 = Instant::now();

        cache.begin_pass();
        cache.record_candidate("https://img/inflight.jpg", true, 0.0);
        let (admitted, _) = cache.schedule_pass(t0);
        assert_eq!(admitted, vec!["https://img/inflight.jpg"]);

        let forgotten = cache.drain_all_for_reset(t0);
        assert_eq!(forgotten, vec!["https://img/inflight.jpg"]);
        assert_eq!(cache.retained_count(), 0);

        let swept = cache.sweep_expired(t0 + Duration::from_secs(1));
        assert_eq!(swept, vec!["https://img/inflight.jpg"]);
    }

    #[test]
    fn repeated_resets_keep_retired_slots_and_block_same_uri_until_expiry() {
        let mut cache = ThumbnailCache::with_limits(4, 2, 2, Duration::ZERO);
        let now = Instant::now();
        cache.begin_pass();
        cache.record_candidate("a", true, 0.0);
        cache.record_candidate("b", true, 1.0);
        assert_eq!(cache.schedule_pass(now).0.len(), 2);
        cache.drain_all_for_reset(now);
        for second in 1..5 {
            cache.begin_pass();
            cache.record_candidate("a", true, 0.0);
            cache.record_candidate("c", true, 1.0);
            assert!(
                cache
                    .schedule_pass(now + Duration::from_secs(second))
                    .0
                    .is_empty()
            );
            cache.drain_all_for_reset(now + Duration::from_secs(second));
        }
        assert_eq!(cache.retired_in_flight.len(), 2);
        cache.sweep_expired(now + IN_FLIGHT_TIMEOUT);
        cache.begin_pass();
        cache.record_candidate("a", true, 0.0);
        assert_eq!(cache.schedule_pass(now + IN_FLIGHT_TIMEOUT).0, vec!["a"]);
    }

    #[test]
    fn timed_out_failed_entry_keeps_retired_slot_after_reset() {
        let mut cache = ThumbnailCache::with_limits(1, 1, 1, Duration::ZERO);
        let now = Instant::now();
        cache.begin_pass();
        cache.record_candidate("a", true, 0.0);
        cache.schedule_pass(now);
        cache.sweep_expired(now + IN_FLIGHT_TIMEOUT);
        assert_eq!(cache.entries["a"].state, EntryState::Failed);
        cache.begin_pass();
        cache.record_candidate("b", true, 0.0);
        assert!(cache.schedule_pass(now + IN_FLIGHT_TIMEOUT).0.is_empty());
        cache.drain_all_for_reset(now + IN_FLIGHT_TIMEOUT);
        cache.record_candidate("a", true, 0.0);
        assert!(cache.schedule_pass(now + IN_FLIGHT_TIMEOUT).0.is_empty());
        cache.sweep_expired(now + IN_FLIGHT_TIMEOUT * 2);
        assert_eq!(
            cache.schedule_pass(now + IN_FLIGHT_TIMEOUT * 2).0,
            vec!["a"]
        );
    }

    #[test]
    fn loading_entries_are_not_evicted_and_interval_applies_within_pass() {
        let mut cache = ThumbnailCache::with_limits(2, 2, 2, Duration::from_millis(50));
        let now = Instant::now();
        cache.begin_pass();
        cache.record_candidate("a", true, 0.0);
        cache.record_candidate("b", true, 1.0);
        assert_eq!(cache.schedule_pass(now).0, vec!["a"]);
        assert_eq!(
            cache.schedule_pass(now + Duration::from_millis(50)).0,
            vec!["b"]
        );
        cache.begin_pass();
        assert!(cache.evict_one().is_none());
        assert_eq!(cache.in_flight_count(), 2);
    }

    #[test]
    fn full_visible_cache_does_not_cycle_finished_images() {
        let mut cache = ThumbnailCache::with_limits(1, 1, 1, Duration::ZERO);
        let now = Instant::now();
        cache.begin_pass();
        cache.record_candidate("a", true, 0.0);
        cache.schedule_pass(now);
        mark_ready(&mut cache, "a");
        cache.record_candidate("b", true, 1.0);
        assert!(cache.schedule_pass(now).0.is_empty());
        assert!(cache.entries.contains_key("a"));
        cache.begin_pass();
        cache.record_candidate("b", true, 1.0);
        assert_eq!(cache.schedule_pass(now).0, vec!["b"]);
    }

    #[test]
    fn geometry_helpers_preserve_fixed_slot_and_aspect_ratio() {
        let viewport = Rect::from_min_max(Pos2::new(0.0, 100.0), Pos2::new(800.0, 600.0));
        let visible_card = Rect::from_min_size(Pos2::new(10.0, 150.0), THUMBNAIL_SIZE);
        let overscan_card = Rect::from_min_size(Pos2::new(10.0, 680.0), THUMBNAIL_SIZE);
        let far_card = Rect::from_min_size(Pos2::new(10.0, 850.0), THUMBNAIL_SIZE);

        assert!(viewport_intersects(viewport, visible_card, 0.0));
        assert!(!viewport_intersects(viewport, overscan_card, 0.0));
        assert!(viewport_intersects(
            viewport,
            overscan_card,
            VIEWPORT_OVERSCAN_PX
        ));
        assert!(!viewport_intersects(
            viewport,
            far_card,
            VIEWPORT_OVERSCAN_PX
        ));

        let fitted = fit_aspect_rect(visible_card, [320, 180]);
        assert!((fitted.width() - THUMBNAIL_WIDTH).abs() < 0.01);
        assert!((fitted.height() - THUMBNAIL_HEIGHT).abs() < 0.01);
    }

    #[test]
    fn avatar_cache_evicts_least_recently_used_entries() {
        let mut cache = AvatarCache::with_limit(2);
        let now = Instant::now();
        assert!(cache.touch_or_insert("https://a/1.jpg", now).is_none());
        assert!(cache.touch_or_insert("https://a/2.jpg", now).is_none());
        // Touch 1.jpg so 2.jpg becomes least recently used.
        assert!(cache.touch_or_insert("https://a/1.jpg", now).is_none());
        assert_eq!(
            cache.touch_or_insert("https://a/3.jpg", now).as_deref(),
            Some("https://a/2.jpg")
        );
        assert!(cache.entries.contains_key("https://a/1.jpg"));
        assert!(!cache.entries.contains_key("https://a/2.jpg"));
        assert!(cache.entries.contains_key("https://a/3.jpg"));
    }

    struct TempCacheDir(PathBuf);

    impl TempCacheDir {
        fn new(label: &str) -> Self {
            static SEQ: AtomicU64 = AtomicU64::new(0);
            let id = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "fastube_thumb_test_{label}_{}_{id}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("create temp test cache dir");
            Self(dir)
        }
    }

    impl Drop for TempCacheDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn encode_test_png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba([220, 48, 48, 255]));
        let mut cursor = io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut cursor, image::ImageFormat::Png)
            .expect("encode test png");
        cursor.into_inner()
    }

    #[test]
    fn disk_cache_write_and_read_hit() {
        let dir = TempCacheDir::new("hit");
        let cache = DiskImageCache::new(dir.0.clone(), 1024 * 1024);
        let uri = "https://i.ytimg.com/vi/abc123/hqdefault.jpg";
        let payload = encode_test_png(16, 9);

        cache
            .write_bytes(uri, &payload)
            .expect("write cached image bytes");

        assert!(cache.entry_path_for_uri(uri).is_file());
        assert_eq!(cache.entry_count(), 1);
        assert_eq!(cache.total_bytes(), payload.len() as u64);

        let read_back = cache.read_bytes(uri).expect("read cached image bytes");
        assert_eq!(read_back.as_deref(), Some(payload.as_slice()));

        // Re-opening from disk reconstructs the index accurately without HTTP calls.
        let reopened = DiskImageCache::new(dir.0.clone(), 1024 * 1024);
        assert_eq!(reopened.entry_count(), 1);
        assert_eq!(reopened.total_bytes(), payload.len() as u64);
        let decoded = reopened
            .decode_cached_image(uri, 1280, 720)
            .expect("decode cached image")
            .expect("cached image present");
        assert_eq!(decoded.size, [16, 9]);
    }

    #[test]
    fn disk_cache_corrupt_file_safety() {
        let dir = TempCacheDir::new("corrupt");
        let cache = DiskImageCache::new(dir.0.clone(), 1024 * 1024);
        let uri = "https://i.ytimg.com/vi/corrupt/hqdefault.jpg";

        cache
            .write_bytes(uri, b"not-a-valid-image-header")
            .expect("write corrupt payload");
        let path = cache.entry_path_for_uri(uri);
        assert!(path.is_file());

        let result = cache.decode_cached_image(uri, 1280, 720);
        assert!(matches!(result, Err(DiskCacheError::Corrupt)));
        assert!(!path.exists());
        assert_eq!(cache.entry_count(), 0);
        assert_eq!(cache.total_bytes(), 0);
    }

    #[test]
    fn disk_cache_evicts_oldest_entries_when_exceeding_capacity() {
        let dir = TempCacheDir::new("evict");
        let cache = DiskImageCache::new(dir.0.clone(), 20);
        let uri_a = "https://i.ytimg.com/vi/a/hqdefault.jpg";
        let uri_b = "https://i.ytimg.com/vi/b/hqdefault.jpg";
        let uri_c = "https://i.ytimg.com/vi/c/hqdefault.jpg";

        cache.write_bytes(uri_a, &[1u8; 8]).expect("write a");
        cache.write_bytes(uri_b, &[2u8; 8]).expect("write b");
        // Touch a so b becomes the least recently used entry.
        assert_eq!(
            cache.read_bytes(uri_a).expect("read a").as_deref(),
            Some(&[1u8; 8][..])
        );

        // Writing c (8 bytes) pushes total to 24 > 20, evicting b first.
        cache.write_bytes(uri_c, &[3u8; 8]).expect("write c");

        assert!(cache.total_bytes() <= 20);
        assert_eq!(cache.entry_count(), 2);
        assert!(cache.entry_path_for_uri(uri_a).is_file());
        assert!(!cache.entry_path_for_uri(uri_b).exists());
        assert!(cache.entry_path_for_uri(uri_c).is_file());
    }

    #[test]
    fn thumbnail_cache_loads_from_injected_disk_cache() {
        let dir = TempCacheDir::new("thumb_load");
        let disk_cache = Arc::new(DiskImageCache::new(dir.0.clone(), 1024 * 1024));
        let uri = "https://i.ytimg.com/vi/disk_hit/hqdefault.jpg";
        let png = encode_test_png(32, 18);
        disk_cache.write_bytes(uri, &png).expect("seed disk cache");

        let ctx = egui::Context::default();
        let mut cache =
            ThumbnailCache::with_limits(4, 2, 2, Duration::ZERO).with_disk_cache(Some(disk_cache));

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            cache.begin_pass();
            let _ = cache.show_rounded(ui, &ctx, uri, Vec2::new(160.0, 90.0), 8.0);
            cache.end_pass(&ctx);
        });
        output.textures_delta.clear();

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut ready = false;
        while Instant::now() < deadline {
            cache.maintain(&ctx);
            if matches!(
                cache.entries.get(uri).map(|e| e.state),
                Some(EntryState::Ready {
                    size: [32, 18],
                    texture_id: Some(_),
                })
            ) {
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let mut final_output = ctx.run_ui(egui::RawInput::default(), |ui| {
            cache.begin_pass();
            let _ = cache.show_rounded(ui, &ctx, uri, Vec2::new(160.0, 90.0), 8.0);
            cache.end_pass(&ctx);
        });
        final_output.textures_delta.clear();

        assert!(ready, "expected disk-cached thumbnail to become Ready");
        assert!(
            cache
                .entries
                .get(uri)
                .and_then(|e| e.texture.as_ref())
                .is_some()
        );
    }

    #[test]
    fn evict_all_gpu_textures_drops_retained_textures_but_keeps_disk_cache() {
        let dir = TempCacheDir::new("evict_gpu");
        let disk_cache = Arc::new(DiskImageCache::new(dir.0.clone(), 1024 * 1024));
        let thumb_uri = "https://i.ytimg.com/vi/gpu_evict/hqdefault.jpg";
        let avatar_uri = "https://yt3.ggpht.com/avatar_evict.jpg";
        let png = encode_test_png(32, 18);
        disk_cache.write_bytes(thumb_uri, &png).expect("seed thumb");
        disk_cache
            .write_bytes(avatar_uri, &png)
            .expect("seed avatar");

        let ctx = egui::Context::default();
        let mut thumbs = ThumbnailCache::with_limits(4, 2, 2, Duration::ZERO)
            .with_disk_cache(Some(disk_cache.clone()));
        let color_image = decode_color_image(&png, 32, 18).expect("decode test png");
        let texture = ctx.load_texture("thumb:test", color_image, egui::TextureOptions::LINEAR);
        thumbs.entries.insert(
            thumb_uri.to_owned(),
            CacheEntry {
                state: EntryState::Ready {
                    size: [32, 18],
                    texture_id: Some(texture.id()),
                },
                texture: Some(texture.clone()),
                ready_at: Some(Instant::now()),
                last_visible_pass: 0,
                last_access_tick: 1,
                cancel: None,
            },
        );
        thumbs.storyboard_sheets.entries.insert(
            "https://i.ytimg.com/sb/1/M0.jpg".to_owned(),
            AvatarEntry {
                state: AvatarEntryState::Ready {
                    size: [32, 18],
                    texture_id: Some(texture.id()),
                },
                texture: Some(texture.clone()),
                last_access_tick: 1,
            },
        );

        let mut avatars = AvatarCache::with_limit(4);
        avatars.set_disk_cache(Some(disk_cache.clone()));
        avatars.entries.insert(
            avatar_uri.to_owned(),
            AvatarEntry {
                state: AvatarEntryState::Ready {
                    size: [32, 18],
                    texture_id: Some(texture.id()),
                },
                texture: Some(texture),
                last_access_tick: 1,
            },
        );

        thumbs.evict_all_gpu_textures(&ctx);
        avatars.evict_all_gpu_textures(&ctx);

        assert_eq!(thumbs.retained_count(), 0);
        assert!(thumbs.storyboard_sheets.entries.is_empty());
        assert!(avatars.entries.is_empty());
        assert!(disk_cache.entry_path_for_uri(thumb_uri).is_file());
        assert!(disk_cache.entry_path_for_uri(avatar_uri).is_file());
    }

    #[test]
    fn thumbnail_fade_alpha_progresses_cleanly() {
        assert!((thumbnail_fade_alpha(None) - 1.0).abs() < f32::EPSILON);
        let immediate = thumbnail_fade_alpha(Some(Instant::now()));
        assert!((0.15..1.0).contains(&immediate));
        if let Some(past) = Instant::now().checked_sub(Duration::from_millis(200)) {
            assert!((thumbnail_fade_alpha(Some(past)) - 1.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn decode_color_image_preserves_portrait_shorts_detail() {
        let png = encode_test_png(405, 720);
        let decoded = decode_color_image(
            &png,
            MAX_THUMBNAIL_DECODE_WIDTH,
            MAX_THUMBNAIL_DECODE_HEIGHT,
        )
        .expect("decode portrait png");
        assert_eq!(decoded.size, [360, 640]);

        let landscape_png = encode_test_png(1280, 720);
        let decoded_landscape = decode_color_image(
            &landscape_png,
            MAX_THUMBNAIL_DECODE_WIDTH,
            MAX_THUMBNAIL_DECODE_HEIGHT,
        )
        .expect("decode landscape png");
        assert_eq!(decoded_landscape.size, [640, 360]);
    }

    #[test]
    fn mtime_touch_interval_avoids_frequent_metadata_writes() {
        assert_eq!(MTIME_TOUCH_INTERVAL, Duration::from_secs(24 * 60 * 60));

        let dir = TempCacheDir::new("mtime_interval");
        let cache = DiskImageCache::new(dir.0.clone(), 1024 * 1024);
        let uri = "https://i.ytimg.com/vi/mtime_check/hqdefault.jpg";
        let payload = encode_test_png(16, 9);
        cache.write_bytes(uri, &payload).expect("write image");
        let path = cache.entry_path_for_uri(uri);

        let now = SystemTime::now();
        let recent_mtime = now
            .checked_sub(Duration::from_secs(60 * 60))
            .expect("recent mtime");
        OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open cached file for recent mtime")
            .set_modified(recent_mtime)
            .expect("set recent mtime");
        let recent_before = fs::metadata(&path)
            .expect("metadata before recent read")
            .modified()
            .expect("modified before recent read");

        let reopened_recent = DiskImageCache::new(dir.0.clone(), 1024 * 1024);
        assert_eq!(
            reopened_recent
                .read_bytes(uri)
                .expect("read recent entry")
                .as_deref(),
            Some(payload.as_slice())
        );
        let recent_after = fs::metadata(&path)
            .expect("metadata after recent read")
            .modified()
            .expect("modified after recent read");
        assert_eq!(recent_after, recent_before);

        let stale_mtime = now
            .checked_sub(MTIME_TOUCH_INTERVAL + Duration::from_secs(60 * 60))
            .expect("stale mtime");
        OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open cached file for stale mtime")
            .set_modified(stale_mtime)
            .expect("set stale mtime");
        let stale_before = fs::metadata(&path)
            .expect("metadata before stale read")
            .modified()
            .expect("modified before stale read");

        let reopened_stale = DiskImageCache::new(dir.0.clone(), 1024 * 1024);
        assert_eq!(
            reopened_stale
                .read_bytes(uri)
                .expect("read stale entry")
                .as_deref(),
            Some(payload.as_slice())
        );
        let stale_after = fs::metadata(&path)
            .expect("metadata after stale read")
            .modified()
            .expect("modified after stale read");
        assert!(stale_after > stale_before);
    }

    #[test]
    fn disk_cache_write_bytes_replaces_existing_entry_atomically_without_tmp_leftovers() {
        let dir = TempCacheDir::new("atomic_replace");
        let cache = DiskImageCache::new(dir.0.clone(), 1024 * 1024);
        let uri = "https://i.ytimg.com/vi/replace_me/hqdefault.jpg";
        let first = encode_test_png(16, 9);
        let second = encode_test_png(32, 18);

        cache.write_bytes(uri, &first).expect("write first payload");
        cache
            .write_bytes(uri, &second)
            .expect("overwrite with second payload");

        assert_eq!(cache.entry_count(), 1);
        assert_eq!(cache.total_bytes(), second.len() as u64);
        assert_eq!(
            cache.read_bytes(uri).expect("read replaced").as_deref(),
            Some(second.as_slice())
        );

        let leftovers: Vec<String> = fs::read_dir(&dir.0)
            .expect("read cache dir")
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "unexpected temp files left behind: {leftovers:?}"
        );
    }

    #[test]
    fn disk_cache_uses_256_dir_sharding_and_falls_back_to_legacy_flat_path() {
        let dir = TempCacheDir::new("sharding_fallback");
        let sharded_uri = "https://i.ytimg.com/vi/sharded01/hqdefault.jpg";
        let legacy_uri = "https://i.ytimg.com/vi/legacy01/hqdefault.jpg";
        let sharded_key = uri_cache_key(sharded_uri);
        let legacy_key = uri_cache_key(legacy_uri);
        let sharded_payload = encode_test_png(16, 9);
        let legacy_payload = encode_test_png(24, 14);

        // Seed a legacy flat file directly under root before opening cache.
        let legacy_flat_path = dir.0.join(format!("{legacy_key}.img"));
        fs::write(&legacy_flat_path, &legacy_payload).expect("seed legacy flat cache file");

        let cache = DiskImageCache::new(dir.0.clone(), 1024 * 1024);
        cache
            .write_bytes(sharded_uri, &sharded_payload)
            .expect("write sharded entry");

        let expected_sharded = dir
            .0
            .join(&sharded_key[..2])
            .join(format!("{sharded_key}.img"));
        assert_eq!(cache.entry_path_for_uri(sharded_uri), expected_sharded);
        assert!(expected_sharded.is_file());
        assert!(!cache.legacy_entry_path_for_uri(sharded_uri).exists());

        // Legacy flat entry remains readable without re-downloading.
        assert_eq!(
            cache
                .read_bytes(legacy_uri)
                .expect("read legacy flat entry")
                .as_deref(),
            Some(legacy_payload.as_slice())
        );
        assert_eq!(cache.entry_count(), 2);

        // Overwriting the legacy entry migrates it into its shard directory and removes the flat file.
        let migrated_payload = encode_test_png(32, 18);
        cache
            .write_bytes(legacy_uri, &migrated_payload)
            .expect("migrate legacy entry to shard");
        assert!(cache.entry_path_for_uri(legacy_uri).is_file());
        assert!(!legacy_flat_path.exists());
        assert_eq!(
            cache
                .read_bytes(legacy_uri)
                .expect("read migrated entry")
                .as_deref(),
            Some(migrated_payload.as_slice())
        );
    }

    #[test]
    fn disk_cache_initializes_index_asynchronously_while_serving_reads_immediately() {
        let dir = TempCacheDir::new("async_init");
        let payload = encode_test_png(16, 9);
        let uris: Vec<String> = (0..12)
            .map(|i| format!("https://i.ytimg.com/vi/async_vid_{i}/hqdefault.jpg"))
            .collect();
        for uri in &uris {
            let key = uri_cache_key(uri);
            let shard_dir = dir.0.join(&key[..2]);
            fs::create_dir_all(&shard_dir).expect("create shard dir");
            fs::write(shard_dir.join(format!("{key}.img")), &payload).expect("seed sharded file");
        }

        let reopened = DiskImageCache::new(dir.0.clone(), 1024 * 1024);
        // Immediate read must succeed even if background scan is still running.
        assert_eq!(
            reopened
                .read_bytes(&uris[0])
                .expect("immediate read")
                .as_deref(),
            Some(payload.as_slice())
        );
        reopened.wait_for_index_init();
        assert!(reopened.is_index_initialized());
        assert_eq!(reopened.entry_count(), uris.len());
        assert_eq!(reopened.total_bytes(), (uris.len() * payload.len()) as u64);
    }

    #[test]
    fn disk_cache_unlocked_batch_eviction_removes_files_outside_mutex() {
        let dir = TempCacheDir::new("unlocked_evict");
        let cache = DiskImageCache::new(dir.0.clone(), 30);
        cache.wait_for_index_init();

        let uris = [
            "https://i.ytimg.com/vi/ev1/hqdefault.jpg",
            "https://i.ytimg.com/vi/ev2/hqdefault.jpg",
            "https://i.ytimg.com/vi/ev3/hqdefault.jpg",
            "https://i.ytimg.com/vi/ev4/hqdefault.jpg",
        ];
        cache.write_bytes(uris[0], &[1u8; 10]).expect("write 0");
        cache.write_bytes(uris[1], &[2u8; 10]).expect("write 1");
        cache.write_bytes(uris[2], &[3u8; 10]).expect("write 2");

        // Writing a 25-byte entry exceeds capacity (55 > 30) and evicts a batch of 3 entries.
        let batch_paths = {
            let mut index = cache.state.index.lock().unwrap();
            let key3 = uri_cache_key(uris[3]);
            index.total_bytes += 25;
            index.entries.insert(
                key3.clone(),
                DiskEntryMeta {
                    size: 25,
                    last_used: SystemTime::now(),
                    seq: 99,
                    legacy: false,
                },
            );
            collect_eviction_batch_locked(&cache.root, cache.max_bytes, &mut index, Some(&key3))
        };
        assert_eq!(batch_paths.len(), 3);
        // Files still exist on disk until `remove_evicted_paths` runs outside the mutex.
        assert!(batch_paths.iter().all(|path| path.is_file()));
        assert!(
            cache.state.index.try_lock().is_ok(),
            "index mutex must be unlocked before disk deletion"
        );
        remove_evicted_paths(batch_paths.clone());
        assert!(batch_paths.iter().all(|path| !path.exists()));
    }

    #[test]
    fn thumbnail_cache_caps_gpu_texture_uploads_per_frame_and_drains_across_frames() {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        for _ in 0..3 {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |_| {},
            );
            out.textures_delta.clear();
        }

        let mut cache = ThumbnailCache::with_limits(8, 6, 6, Duration::ZERO);
        let now = Instant::now();
        let generation = cache.generation.load(Ordering::Relaxed);
        let png = encode_test_png(16, 9);
        let uris: Vec<String> = (0..6)
            .map(|i| format!("https://i.ytimg.com/vi/cap_{i}/hqdefault.jpg"))
            .collect();
        for (i, uri) in uris.iter().enumerate() {
            cache.entries.insert(
                uri.clone(),
                CacheEntry {
                    state: EntryState::Loading { started_at: now },
                    texture: None,
                    ready_at: None,
                    last_visible_pass: 1,
                    last_access_tick: i as u64 + 1,
                    cancel: None,
                },
            );
            let color_image = decode_color_image(&png, 16, 9).expect("decode test png");
            cache
                .result_tx
                .send(LoadedImage {
                    uri: uri.clone(),
                    image: Ok(color_image),
                    generation,
                    cancelled: false,
                })
                .expect("send loaded image");
        }

        let mut ready_after_frame_1 = 0usize;
        let mut out1 = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |_| {
                cache.maintain(&ctx);
                // Calling drain_loaded a second time in the same frame must still obey the cap.
                cache.drain_loaded(&ctx);
                ready_after_frame_1 = cache
                    .entries
                    .values()
                    .filter(|e| matches!(e.state, EntryState::Ready { .. }))
                    .count();
            },
        );
        out1.textures_delta.clear();
        assert_eq!(ready_after_frame_1, MAX_GPU_UPLOADS_PER_FRAME);
        assert_eq!(cache.pending_uploads.len(), 6 - MAX_GPU_UPLOADS_PER_FRAME);
        assert_eq!(
            out1.viewport_output
                .get(&egui::ViewportId::ROOT)
                .map(|v| v.repaint_delay),
            Some(Duration::ZERO),
            "remaining pending uploads must request next-frame repaint"
        );

        let mut ready_after_frame_2 = 0usize;
        let mut out2 = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |_| {
                cache.maintain(&ctx);
                ready_after_frame_2 = cache
                    .entries
                    .values()
                    .filter(|e| matches!(e.state, EntryState::Ready { .. }))
                    .count();
            },
        );
        out2.textures_delta.clear();
        assert_eq!(ready_after_frame_2, 6);
        assert!(cache.pending_uploads.is_empty());
    }

    #[test]
    fn single_begin_pass_and_end_pass_per_frame_are_idempotent_and_protect_visible_entries() {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let mut cache = ThumbnailCache::with_limits(2, 2, 2, Duration::ZERO);
        let png = encode_test_png(16, 9);
        let tex = ctx.load_texture(
            "thumb:visible_panel_1",
            decode_color_image(&png, 16, 9).expect("decode"),
            egui::TextureOptions::LINEAR,
        );
        cache.entries.insert(
            "https://img/panel1_a.jpg".into(),
            CacheEntry {
                state: EntryState::Ready {
                    size: [16, 9],
                    texture_id: Some(tex.id()),
                },
                texture: Some(tex.clone()),
                ready_at: None,
                last_visible_pass: 0,
                last_access_tick: 1,
                cancel: None,
            },
        );
        cache.entries.insert(
            "https://img/panel1_b.jpg".into(),
            CacheEntry {
                state: EntryState::Ready {
                    size: [16, 9],
                    texture_id: Some(tex.id()),
                },
                texture: Some(tex),
                ready_at: None,
                last_visible_pass: 0,
                last_access_tick: 2,
                cancel: None,
            },
        );

        let mut pass_after_first = 0u64;
        let mut pass_after_second = 0u64;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                ui.set_clip_rect(screen);
                cache.maintain(&ctx);
                // Panel 1 renders both entries.
                cache.begin_pass();
                pass_after_first = cache.pass_index;
                let _ = cache.show_rounded(
                    ui,
                    &ctx,
                    "https://img/panel1_a.jpg",
                    Vec2::new(160.0, 90.0),
                    8.0,
                );
                let _ = cache.show_rounded(
                    ui,
                    &ctx,
                    "https://img/panel1_b.jpg",
                    Vec2::new(160.0, 90.0),
                    8.0,
                );
                cache.end_pass(&ctx);

                // Panel 2 calls begin_pass + end_pass again in the SAME frame.
                cache.begin_pass();
                pass_after_second = cache.pass_index;
                cache.end_pass(&ctx);
            },
        );
        out.textures_delta.clear();

        assert_eq!(
            pass_after_first, pass_after_second,
            "second begin_pass in the same frame must be idempotent"
        );
        assert!(cache.evict_one().is_none());
        assert!(cache.entries.contains_key("https://img/panel1_a.jpg"));
        assert!(cache.entries.contains_key("https://img/panel1_b.jpg"));
    }

    #[test]
    fn retired_in_flight_does_not_schedule_idle_repaint_polling() {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        for _ in 0..3 {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |_| {},
            );
            out.textures_delta.clear();
        }
        let repaint_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&repaint_calls);
        ctx.set_request_repaint_callback(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        let mut cache = ThumbnailCache::with_limits(4, 2, 2, Duration::ZERO);
        let now = Instant::now();
        cache.push_retired_in_flight("https://i.ytimg.com/vi/retired/hqdefault.jpg", now);
        assert!(
            !cache.has_pending_work(),
            "retired_in_flight alone must not keep has_pending_work active"
        );

        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |_| {
                cache.maintain(&ctx);
                cache.begin_pass();
                cache.end_pass(&ctx);
            },
        );
        output.textures_delta.clear();
        assert_eq!(
            repaint_calls.load(Ordering::SeqCst),
            0,
            "retired_in_flight must not trigger 80ms idle repaint polling"
        );
        assert_eq!(
            output
                .viewport_output
                .get(&egui::ViewportId::ROOT)
                .map(|v| v.repaint_delay),
            Some(Duration::MAX)
        );
    }

    #[test]
    fn worker_pool_skips_cancelled_jobs_and_notifies_slot_release() {
        let dir = TempCacheDir::new("worker_cancel");
        let disk_cache = Arc::new(DiskImageCache::new(dir.0.clone(), 1024 * 1024));
        let uri = "https://i.ytimg.com/vi/cancel_me/hqdefault.jpg";
        disk_cache
            .write_bytes(uri, &encode_test_png(16, 9))
            .expect("seed cache");

        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(1));
        let cancel = Cancellation::default();
        cancel.cancel();

        enqueue_image_load(
            disk_cache,
            uri.to_owned(),
            FetchKind::Thumbnail {
                target_width_px: 320,
            },
            MAX_THUMBNAIL_DECODE_WIDTH,
            MAX_THUMBNAIL_DECODE_HEIGHT,
            FetchPriority::High,
            0,
            1,
            generation,
            cancel,
            tx,
            ctx,
        );

        let result = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("cancelled job should notify channel to free retired slot");
        assert_eq!(result.uri, uri);
        assert!(result.cancelled);
        assert!(result.image.is_err());
    }

    #[test]
    fn youtube_webp_dpi_variant_avatar_s88_and_banner_2048px_sizing_helpers() {
        assert_eq!(select_youtube_thumbnail_variant(160), "mqdefault");
        assert_eq!(select_youtube_thumbnail_variant(320), "mqdefault");
        assert_eq!(select_youtube_thumbnail_variant(400), "hqdefault");
        assert_eq!(select_youtube_thumbnail_variant(640), "sddefault");

        let src_thumb = "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg";
        assert_eq!(
            rewrite_youtube_thumbnail_webp_url(src_thumb, 280).as_deref(),
            Some("https://i.ytimg.com/vi_webp/dQw4w9WgXcQ/mqdefault.webp")
        );
        assert_eq!(
            rewrite_youtube_thumbnail_webp_url(src_thumb, 420).as_deref(),
            Some("https://i.ytimg.com/vi_webp/dQw4w9WgXcQ/hqdefault.webp")
        );
        assert_eq!(
            rewrite_youtube_thumbnail_webp_url(src_thumb, 640).as_deref(),
            Some("https://i.ytimg.com/vi_webp/dQw4w9WgXcQ/sddefault.webp")
        );
        let candidates = image_fetch_candidates(
            src_thumb,
            FetchKind::Thumbnail {
                target_width_px: 280,
            },
        );
        assert_eq!(
            candidates,
            vec![
                "https://i.ytimg.com/vi_webp/dQw4w9WgXcQ/mqdefault.webp".to_owned(),
                src_thumb.to_owned(),
            ]
        );

        // Signed YouTube URLs stay untouched to avoid invalidating `sqp` signatures.
        let signed = "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg?sqp=-oaymwEc&rs=ABC";
        assert!(rewrite_youtube_thumbnail_webp_url(signed, 320).is_none());

        // Google avatar URLs rewrite huge `=s900` / `=s176` sizes down to `=s88`.
        assert_eq!(
            rewrite_google_avatar_url(
                "https://yt3.ggpht.com/ytc/AIdro_abc=s900-c-k-c0x00ffffff-no-rj"
            ),
            "https://yt3.ggpht.com/ytc/AIdro_abc=s88-c-k-c0x00ffffff-no-rj"
        );
        assert_eq!(
            rewrite_google_avatar_url(
                "https://yt3.googleusercontent.com/xyz=s176-c-k-c0x00ffffff-no-rj-mo"
            ),
            "https://yt3.googleusercontent.com/xyz=s88-c-k-c0x00ffffff-no-rj-mo"
        );

        // Channel banners are recognized, never rewritten to `=s88`, and capped at 2048px width.
        let banner_url = "https://yt3.googleusercontent.com/banner_id=w2560-fcrop64=1,00005a57ffffa5a8-k-c0xffffffff-no-nd-rj";
        assert!(is_channel_banner_url(banner_url));
        assert_eq!(rewrite_google_avatar_url(banner_url), banner_url);
        let (max_w, max_h, kind) =
            decode_limits_for_request(banner_url, Vec2::new(1070.0, 172.0), 2.0, true);
        assert_eq!(kind, FetchKind::Banner);
        assert_eq!(max_w, MAX_BANNER_DECODE_WIDTH);
        assert_eq!(max_h, MAX_BANNER_DECODE_HEIGHT);

        let wide_banner = encode_test_png(2560, 424);
        let decoded_banner =
            decode_color_image(&wide_banner, max_w, max_h).expect("decode wide banner");
        assert_eq!(decoded_banner.size[0], 2048);
        assert!(decoded_banner.size[1] <= 424);
    }

    #[test]
    fn cover_uv_strips_4_3_letterbox_bars_and_large_cards_prefer_hq720_webp() {
        assert!(is_youtube_letterboxed_4_3_uri(
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg"
        ));
        assert!(is_youtube_letterboxed_4_3_uri(
            "https://i.ytimg.com/vi_webp/dQw4w9WgXcQ/sddefault.webp"
        ));
        assert!(!is_youtube_letterboxed_4_3_uri(
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/hq720.jpg"
        ));
        assert!(!is_youtube_letterboxed_4_3_uri(
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg?sqp=-oaymwEc&rs=ABC"
        ));

        // 480x360 (4:3) letterboxed thumbnail rendered into 16:9 and 9:16 cards strips 12.5% bars.
        let uv_16_9 = compute_cover_uv([480, 360], Vec2::new(320.0, 180.0), true);
        assert!((uv_16_9.min.x - 0.0).abs() < 1e-4);
        assert!((uv_16_9.max.x - 1.0).abs() < 1e-4);
        assert!((uv_16_9.min.y - 0.125).abs() < 1e-4);
        assert!((uv_16_9.max.y - 0.875).abs() < 1e-4);

        let uv_9_16 = compute_cover_uv([480, 360], Vec2::new(180.0, 320.0), true);
        assert!((uv_9_16.min.y - 0.125).abs() < 1e-4);
        assert!((uv_9_16.max.y - 0.875).abs() < 1e-4);
        assert!(uv_9_16.min.x > 0.25 && uv_9_16.max.x < 0.75);

        // Native vertical 9:16 portrait thumbnail is never vertically cropped.
        let uv_portrait = compute_cover_uv([405, 720], Vec2::new(180.0, 320.0), false);
        assert!((uv_portrait.min.x - 0.0).abs() < 1e-4);
        assert!((uv_portrait.max.x - 1.0).abs() < 1e-4);
        assert!((uv_portrait.min.y - 0.0).abs() < 1e-4);
        assert!((uv_portrait.max.y - 1.0).abs() < 1e-4);

        // Large cards (>320px) try hq720.webp first and keep hqdefault.webp + original as fallbacks.
        let large = image_fetch_candidates(
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg",
            FetchKind::Thumbnail {
                target_width_px: 420,
            },
        );
        assert_eq!(
            large,
            vec![
                "https://i.ytimg.com/vi_webp/dQw4w9WgXcQ/hq720.webp".to_owned(),
                "https://i.ytimg.com/vi_webp/dQw4w9WgXcQ/hqdefault.webp".to_owned(),
                "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_owned(),
            ]
        );
    }
}
