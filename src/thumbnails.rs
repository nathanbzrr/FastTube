use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
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
pub const LOAD_START_INTERVAL: Duration = Duration::from_millis(20);
pub const VIEWPORT_OVERSCAN_PX: f32 = 110.0;
pub const IN_FLIGHT_TIMEOUT: Duration = Duration::from_secs(15);

pub const DEFAULT_DISK_CACHE_MAX_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CACHED_IMAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_THUMBNAIL_DECODE_WIDTH: u32 = 640;
const MAX_THUMBNAIL_DECODE_HEIGHT: u32 = 360;
const MAX_AVATAR_DECODE_SIZE: u32 = 96;
const MAX_STORYBOARD_DECODE_SIZE: u32 = 1024;
const MAX_RETAINED_STORYBOARD_SHEETS: usize = 4;
const THUMBNAIL_FADE_DURATION_SECS: f32 = 0.14;
const IMAGE_FETCH_TIMEOUT: Duration = Duration::from_secs(12);
const MTIME_TOUCH_INTERVAL: Duration = Duration::from_secs(60);

static CACHE_TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum DiskCacheError {
    Io,
    Corrupt,
}

#[derive(Clone, Debug)]
struct DiskEntryMeta {
    size: u64,
    last_used: SystemTime,
    seq: u64,
}

#[derive(Default)]
struct DiskIndex {
    initialized: bool,
    entries: HashMap<String, DiskEntryMeta>,
    total_bytes: u64,
    next_seq: u64,
}

pub struct DiskImageCache {
    root: PathBuf,
    max_bytes: u64,
    index: Mutex<DiskIndex>,
    http_client: OnceLock<reqwest::blocking::Client>,
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

impl DiskImageCache {
    pub fn new(root: PathBuf, max_bytes: u64) -> Self {
        Self {
            root,
            max_bytes,
            index: Mutex::new(DiskIndex::default()),
            http_client: OnceLock::new(),
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

    fn entry_path(&self, key: &str) -> PathBuf {
        self.root.join(format!("{key}.img"))
    }

    fn ensure_initialized(&self, index: &mut DiskIndex) {
        if index.initialized {
            return;
        }
        index.initialized = true;
        if !self.root.is_dir() {
            return;
        }
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() || !meta.is_file() {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if file_name.starts_with('.') && file_name.ends_with(".tmp") {
                let _ = fs::remove_file(&path);
                continue;
            }
            let Some(stem) = file_name.strip_suffix(".img") else {
                continue;
            };
            if !is_valid_cache_stem(stem) {
                continue;
            }
            let size = meta.len();
            let last_used = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            index.total_bytes = index.total_bytes.saturating_add(size);
            index.entries.insert(
                stem.to_owned(),
                DiskEntryMeta {
                    size,
                    last_used,
                    seq: 0,
                },
            );
        }
        self.evict_excess_locked(index, None);
    }

    fn evict_excess_locked(&self, index: &mut DiskIndex, protected_key: Option<&str>) {
        while index.total_bytes > self.max_bytes {
            let victim = index
                .entries
                .iter()
                .filter(|(key, _)| protected_key != Some(key.as_str()))
                .min_by_key(|(_, meta)| (meta.last_used, meta.seq))
                .map(|(key, _)| key.clone());
            let Some(victim_key) = victim else {
                break;
            };
            if let Some(meta) = index.entries.remove(&victim_key) {
                index.total_bytes = index.total_bytes.saturating_sub(meta.size);
            }
            let _ = fs::remove_file(self.entry_path(&victim_key));
        }
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
        fs::create_dir_all(&self.root)?;
        let key = uri_cache_key(uri);
        let target = self.entry_path(&key);
        let (temp_path, mut file) = loop {
            let id = CACHE_TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let candidate = self
                .root
                .join(format!(".{key}.{}.{id}.tmp", std::process::id()));
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
            file.sync_all()?;
            drop(file);
            fs::rename(&temp_path, &target)
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
            return write_result;
        }
        let mut index = self.index.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure_initialized(&mut index);
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
            },
        );
        self.evict_excess_locked(&mut index, Some(&key));
        Ok(())
    }

    pub fn read_bytes(&self, uri: &str) -> io::Result<Option<Vec<u8>>> {
        let key = uri_cache_key(uri);
        let path = self.entry_path(&key);
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut index = self.index.lock().unwrap_or_else(|e| e.into_inner());
                if index.initialized
                    && let Some(prev) = index.entries.remove(&key)
                {
                    index.total_bytes = index.total_bytes.saturating_sub(prev.size);
                }
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take((MAX_CACHED_IMAGE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.is_empty() || bytes.len() > MAX_CACHED_IMAGE_BYTES {
            self.remove(uri);
            return Ok(None);
        }
        let now = SystemTime::now();
        let should_touch_mtime = {
            let mut index = self.index.lock().unwrap_or_else(|e| e.into_inner());
            self.ensure_initialized(&mut index);
            index.next_seq = index.next_seq.wrapping_add(1);
            let seq = index.next_seq;
            if let Some(entry) = index.entries.get_mut(&key) {
                let stale = now
                    .duration_since(entry.last_used)
                    .unwrap_or(Duration::ZERO)
                    >= MTIME_TOUCH_INTERVAL;
                entry.last_used = now;
                entry.seq = seq;
                stale
            } else {
                let size = bytes.len() as u64;
                index.total_bytes = index.total_bytes.saturating_add(size);
                index.entries.insert(
                    key,
                    DiskEntryMeta {
                        size,
                        last_used: now,
                        seq,
                    },
                );
                false
            }
        };
        if should_touch_mtime && let Ok(file) = OpenOptions::new().write(true).open(&path) {
            let _ = file.set_modified(now);
        }
        Ok(Some(bytes))
    }

    pub fn remove(&self, uri: &str) {
        let key = uri_cache_key(uri);
        let _ = fs::remove_file(self.entry_path(&key));
        let mut index = self.index.lock().unwrap_or_else(|e| e.into_inner());
        if index.initialized
            && let Some(prev) = index.entries.remove(&key)
        {
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

    fn fetch_and_cache(
        &self,
        uri: &str,
        max_width: u32,
        max_height: u32,
    ) -> Result<egui::ColorImage, ()> {
        let client = self.http_client.get_or_init(|| {
            reqwest::blocking::Client::builder()
                .timeout(IMAGE_FETCH_TIMEOUT)
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new())
        });
        let response = client.get(uri).send().map_err(|_| ())?;
        if !response.status().is_success() {
            return Err(());
        }
        let mut bytes = Vec::new();
        response
            .take((MAX_CACHED_IMAGE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| ())?;
        if bytes.is_empty() || bytes.len() > MAX_CACHED_IMAGE_BYTES {
            return Err(());
        }
        let image = decode_color_image(&bytes, max_width, max_height).map_err(|_| ())?;
        let _ = self.write_bytes(uri, &bytes);
        Ok(image)
    }

    #[cfg(test)]
    pub fn total_bytes(&self) -> u64 {
        let mut index = self.index.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure_initialized(&mut index);
        index.total_bytes
    }

    #[cfg(test)]
    pub fn entry_count(&self) -> usize {
        let mut index = self.index.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure_initialized(&mut index);
        index.entries.len()
    }
}

struct LoadedImage {
    uri: String,
    image: Result<egui::ColorImage, ()>,
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
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub uri: String,
    pub is_visible: bool,
    pub sort_key: i32,
}

#[derive(Clone, Debug)]
struct RetiredInFlight {
    uri: String,
    forget_until: Instant,
}

pub struct ThumbnailCache {
    entries: HashMap<String, CacheEntry>,
    candidates: Vec<Candidate>,
    retired_in_flight: VecDeque<RetiredInFlight>,
    pass_index: u64,
    access_counter: u64,
    last_load_started_at: Option<Instant>,
    max_retained: usize,
    max_concurrent_loads: usize,
    max_new_loads_per_pass: usize,
    load_start_interval: Duration,
    allow_burst_loads: bool,
    disk_cache: Option<Arc<DiskImageCache>>,
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
            retired_in_flight: VecDeque::new(),
            pass_index: 0,
            access_counter: 0,
            last_load_started_at: None,
            max_retained,
            max_concurrent_loads,
            max_new_loads_per_pass,
            load_start_interval,
            allow_burst_loads: false,
            disk_cache: None,
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
            || !self.retired_in_flight.is_empty()
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
        self.candidates.clear();
        let entries = std::mem::take(&mut self.entries);
        let mut forgotten = Vec::with_capacity(entries.len());
        for (uri, entry) in entries {
            if matches!(entry.state, EntryState::Loading { .. }) {
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
        while let Ok(loaded) = self.result_rx.try_recv() {
            self.retired_in_flight.retain(|item| item.uri != loaded.uri);
            let Some(entry) = self.entries.get_mut(&loaded.uri) else {
                continue;
            };
            if !matches!(entry.state, EntryState::Loading { .. }) {
                continue;
            }
            match loaded.image {
                Ok(color_image) => {
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
                }
                Err(()) => {
                    entry.state = EntryState::Failed;
                    entry.texture = None;
                    entry.ready_at = None;
                }
            }
        }
    }

    pub fn maintain(&mut self, ctx: &egui::Context) {
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

    pub fn begin_pass(&mut self) {
        self.pass_index = self.pass_index.wrapping_add(1);
        self.candidates.clear();
    }

    pub fn show_rounded(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        uri: &str,
        size: Vec2,
        corner_radius: f32,
    ) -> egui::Response {
        self.show_rounded_mode(ui, ctx, uri, size, corner_radius, false)
    }

    pub fn show_cover_rounded(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        uri: &str,
        size: Vec2,
        corner_radius: f32,
    ) -> egui::Response {
        self.show_rounded_mode(ui, ctx, uri, size, corner_radius, true)
    }

    fn show_rounded_mode(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        uri: &str,
        size: Vec2,
        corner_radius: f32,
        cover: bool,
    ) -> egui::Response {
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
                paint_hover_outline_rounded(ui, rect, response.hovered(), corner_radius);
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
                            }
                            if is_visible {
                                paint_ready_thumbnail(
                                    ui,
                                    rect,
                                    texture_id,
                                    size,
                                    corner_radius,
                                    cover,
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
                    self.record_candidate(uri, is_visible, rect.top() - viewport.top());
                }
                if is_visible {
                    paint_placeholder(ui, rect, PlaceholderKind::Pending, corner_radius);
                }
            }
        }

        if is_visible {
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
        self.drain_loaded(ctx);
        let now = Instant::now();
        let (admitted, evicted) = self.schedule_pass(now);

        for uri in evicted {
            ctx.forget_image(&uri);
        }

        for uri in admitted {
            if let Some(disk_cache) = self.disk_cache.clone()
                && uri_uses_disk_worker(&uri)
            {
                spawn_image_load(
                    disk_cache,
                    uri,
                    MAX_THUMBNAIL_DECODE_WIDTH,
                    MAX_THUMBNAIL_DECODE_HEIGHT,
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
                    }
                }
                PollOutcome::Pending => {}
                PollOutcome::Failed => {
                    if let Some(entry) = self.entries.get_mut(&uri) {
                        entry.state = EntryState::Failed;
                        entry.ready_at = None;
                    }
                    ctx.forget_image(&uri);
                }
            }
        }

        if self.in_flight_count() > 0 || !self.retired_in_flight.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(80));
        } else if !self.candidates.is_empty()
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

    fn schedule_pass(&mut self, now: Instant) -> (Vec<String>, Vec<String>) {
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

        if !cooldown_ready {
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
            let can_start = started_this_pass < self.max_new_loads_per_pass
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

fn spawn_image_load(
    disk_cache: Arc<DiskImageCache>,
    uri: String,
    max_width: u32,
    max_height: u32,
    tx: mpsc::Sender<LoadedImage>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let image = match disk_cache.decode_cached_image(&uri, max_width, max_height) {
            Ok(Some(img)) => Ok(img),
            Ok(None) | Err(_) => disk_cache.fetch_and_cache(&uri, max_width, max_height),
        };
        let _ = tx.send(LoadedImage { uri, image });
        ctx.request_repaint();
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

fn paint_ready_thumbnail(
    ui: &mut egui::Ui,
    rect: Rect,
    texture_id: egui::TextureId,
    size: [u32; 2],
    corner_radius: f32,
    cover: bool,
    ready_at: Option<Instant>,
) {
    let fade = thumbnail_fade_alpha(ready_at);
    let tint = if fade < 1.0 {
        paint_placeholder(ui, rect, PlaceholderKind::Loading, corner_radius);
        ui.ctx().request_repaint_after(Duration::from_millis(16));
        let alpha = (fade * 255.0).round().clamp(1.0, 255.0) as u8;
        Color32::from_white_alpha(alpha)
    } else {
        Color32::WHITE
    };
    if cover {
        paint_cover_texture_rounded(ui, rect, texture_id, size, corner_radius, tint);
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

fn paint_cover_texture_rounded(
    ui: &mut egui::Ui,
    bounds: Rect,
    texture_id: egui::TextureId,
    intrinsic_size: [u32; 2],
    corner_radius: f32,
    tint: Color32,
) {
    let tex_w = intrinsic_size[0].max(1) as f32;
    let tex_h = intrinsic_size[1].max(1) as f32;
    let bounds_w = bounds.width().max(1.0);
    let bounds_h = bounds.height().max(1.0);
    let tex_aspect = tex_w / tex_h;
    let bounds_aspect = bounds_w / bounds_h;
    let uv = if (tex_aspect - bounds_aspect).abs() < 1e-4 {
        Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0))
    } else if tex_aspect > bounds_aspect {
        let visible_fraction = (bounds_aspect / tex_aspect).clamp(0.0, 1.0);
        let pad = (1.0 - visible_fraction) * 0.5;
        Rect::from_min_max(Pos2::new(pad, 0.0), Pos2::new(1.0 - pad, 1.0))
    } else {
        let visible_fraction = (tex_aspect / bounds_aspect).clamp(0.0, 1.0);
        let pad = (1.0 - visible_fraction) * 0.5;
        Rect::from_min_max(Pos2::new(0.0, pad), Pos2::new(1.0, 1.0 - pad))
    };
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
    access_counter: u64,
    max_retained: usize,
    disk_cache: Option<Arc<DiskImageCache>>,
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
            access_counter: 0,
            max_retained,
            disk_cache: None,
            result_tx,
            result_rx,
        }
    }

    pub fn set_disk_cache(&mut self, disk_cache: Option<Arc<DiskImageCache>>) {
        self.disk_cache = disk_cache;
    }

    pub fn evict_all_gpu_textures(&mut self, ctx: &egui::Context) {
        for uri in self.entries.keys() {
            ctx.forget_image(uri);
        }
        self.entries.clear();
    }

    pub fn has_pending_work(&self) -> bool {
        self.entries
            .values()
            .any(|entry| matches!(entry.state, AvatarEntryState::Loading { .. }))
    }

    pub fn maintain(&mut self, ctx: &egui::Context) {
        while let Ok(loaded) = self.result_rx.try_recv() {
            let Some(entry) = self.entries.get_mut(&loaded.uri) else {
                continue;
            };
            if !matches!(entry.state, AvatarEntryState::Loading { .. }) {
                continue;
            }
            match loaded.image {
                Ok(color_image) => {
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
                }
                Err(()) => {
                    entry.state = AvatarEntryState::Failed;
                    entry.texture = None;
                }
            }
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
                ctx.request_repaint_after(Duration::from_millis(60));
                return false;
            }
        }
        if let Some(evicted) = self.touch_or_insert(trimmed, now) {
            ctx.forget_image(&evicted);
        }

        if uses_disk_worker {
            if is_new && let Some(disk_cache) = self.disk_cache.clone() {
                spawn_image_load(
                    disk_cache,
                    trimmed.to_owned(),
                    MAX_AVATAR_DECODE_SIZE,
                    MAX_AVATAR_DECODE_SIZE,
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
            Ok(egui::load::TexturePoll::Pending { .. }) => {
                ctx.request_repaint_after(Duration::from_millis(80));
                false
            }
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
    access_counter: u64,
    max_retained: usize,
    disk_cache: Option<Arc<DiskImageCache>>,
    result_tx: mpsc::Sender<LoadedImage>,
    result_rx: mpsc::Receiver<LoadedImage>,
}

impl StoryboardSheetCache {
    pub fn with_limit(max_retained: usize) -> Self {
        let max_retained = max_retained.max(1);
        let (result_tx, result_rx) = mpsc::channel();
        Self {
            entries: HashMap::with_capacity(max_retained),
            access_counter: 0,
            max_retained,
            disk_cache: None,
            result_tx,
            result_rx,
        }
    }

    pub fn set_disk_cache(&mut self, disk_cache: Option<Arc<DiskImageCache>>) {
        self.disk_cache = disk_cache;
    }

    pub fn has_pending_work(&self) -> bool {
        self.entries
            .values()
            .any(|entry| matches!(entry.state, AvatarEntryState::Loading { .. }))
    }

    pub fn clear(&mut self, ctx: &egui::Context) {
        for uri in self.entries.keys() {
            ctx.forget_image(uri);
        }
        self.entries.clear();
    }

    pub fn maintain(&mut self, ctx: &egui::Context) {
        while let Ok(loaded) = self.result_rx.try_recv() {
            let Some(entry) = self.entries.get_mut(&loaded.uri) else {
                continue;
            };
            if !matches!(entry.state, AvatarEntryState::Loading { .. }) {
                continue;
            }
            match loaded.image {
                Ok(color_image) => {
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
                }
                Err(()) => {
                    entry.state = AvatarEntryState::Failed;
                    entry.texture = None;
                }
            }
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
                ctx.request_repaint_after(Duration::from_millis(60));
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
                spawn_image_load(
                    disk_cache,
                    trimmed.to_owned(),
                    MAX_STORYBOARD_DECODE_SIZE,
                    MAX_STORYBOARD_DECODE_SIZE,
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
            Ok(egui::load::TexturePoll::Pending { .. }) => {
                ctx.request_repaint_after(Duration::from_millis(80));
                None
            }
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
}
