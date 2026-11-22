use crate::settings::{
    BufferSettings, CaptionStylePrefs, HwdecMode, UltrawideZoomMode, VideoCodecPreference,
};
use eframe::egui::{self, Rect};
use eframe::egui_glow;
use eframe::glow;
use eframe::glow::HasContext;
use libloading::Library;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SW_HIDE, SW_SHOWNOACTIVATE, SWP_HIDEWINDOW, SWP_NOACTIVATE,
    SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowPos, ShowWindow, WS_CHILD, WS_VISIBLE,
};

type Create = unsafe extern "C" fn() -> *mut c_void;
type SetOption = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
type Initialize = unsafe extern "C" fn(*mut c_void) -> c_int;
type Command = unsafe extern "C" fn(*mut c_void, *const *const c_char) -> c_int;
type Terminate = unsafe extern "C" fn(*mut c_void);
type GetProperty = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char;
type GetPropertyTyped =
    unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int;
type SetProperty = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
type Free = unsafe extern "C" fn(*mut c_void);
type WaitEvent = unsafe extern "C" fn(*mut c_void, f64) -> *const MpvEvent;
type RequestEvent = unsafe extern "C" fn(*mut c_void, c_int, c_int) -> c_int;
type ObserveProperty = unsafe extern "C" fn(*mut c_void, u64, *const c_char, c_int) -> c_int;
type SetWakeupCallback =
    unsafe extern "C" fn(*mut c_void, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void);
type ErrorString = unsafe extern "C" fn(c_int) -> *const c_char;
type RenderContextCreate =
    unsafe extern "C" fn(*mut *mut c_void, *mut c_void, *mut MpvRenderParam) -> c_int;
type RenderContextFree = unsafe extern "C" fn(*mut c_void);
type RenderContextRender = unsafe extern "C" fn(*mut c_void, *mut MpvRenderParam) -> c_int;
type RenderContextUpdate = unsafe extern "C" fn(*mut c_void) -> u64;
type RenderContextSetUpdateCallback =
    unsafe extern "C" fn(*mut c_void, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void);
type GetProcAddress = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
type WglGetProcAddressFn = unsafe extern "system" fn(*const c_char) -> *mut c_void;
type EglGetProcAddressFn = unsafe extern "system" fn(*const c_char) -> *mut c_void;

// From libmpv/client.h (`enum mpv_format`).
const MPV_FORMAT_NONE: c_int = 0;
const MPV_FORMAT_STRING: c_int = 1;
const MPV_FORMAT_FLAG: c_int = 3;
const MPV_FORMAT_INT64: c_int = 4;
const MPV_FORMAT_DOUBLE: c_int = 5;
const MPV_FORMAT_NODE: c_int = 6;
const MPV_FORMAT_NODE_ARRAY: c_int = 7;
const MPV_FORMAT_NODE_MAP: c_int = 8;

// From libmpv/client.h (`enum mpv_event_id`).
const MPV_EVENT_NONE: c_int = 0;
const MPV_EVENT_START_FILE: c_int = 6;
const MPV_EVENT_END_FILE: c_int = 7;
const MPV_EVENT_FILE_LOADED: c_int = 8;
const MPV_EVENT_PROPERTY_CHANGE: c_int = 22;

// From libmpv/client.h (`enum mpv_end_file_reason`).
const MPV_END_FILE_REASON_EOF: c_int = 0;
const MPV_END_FILE_REASON_STOP: c_int = 2;
const MPV_END_FILE_REASON_ERROR: c_int = 4;

const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const MPV_RENDER_PARAM_OPENGL_FBO: c_int = 3;
const MPV_RENDER_PARAM_FLIP_Y: c_int = 4;
// From mpv/render.h (`mpv_render_param_type::MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME = 12`).
const MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME: c_int = 12;
const MPV_RENDER_API_TYPE_OPENGL: &[u8] = b"opengl\0";

pub const MAX_VOLUME_PERCENT: f64 = 200.0;

pub fn clamp_volume_percent(percent: f64) -> f64 {
    if percent.is_finite() {
        percent.clamp(0.0, MAX_VOLUME_PERCENT)
    } else {
        100.0
    }
}

pub(crate) fn ultrawide_zoom_mpv_properties(
    mode: UltrawideZoomMode,
    custom_zoom: f32,
) -> [(&'static str, String); 2] {
    let clamped_zoom = if custom_zoom.is_finite() {
        custom_zoom.clamp(0.0, 0.5)
    } else {
        0.0
    };
    match mode {
        UltrawideZoomMode::Fit => [
            ("panscan", "0.000".to_owned()),
            ("video-zoom", "0.000".to_owned()),
        ],
        UltrawideZoomMode::Crop21x9 => [
            ("panscan", "1.000".to_owned()),
            ("video-zoom", "0.000".to_owned()),
        ],
        UltrawideZoomMode::Custom => [
            ("panscan", "0.000".to_owned()),
            ("video-zoom", format!("{clamped_zoom:.3}")),
        ],
    }
}

pub(crate) fn caption_style_mpv_properties(
    style: &CaptionStylePrefs,
) -> [(&'static str, String); 3] {
    let normalized = style.clone().normalized();
    [
        ("sub-scale", format!("{:.2}", normalized.font_scale)),
        ("sub-color", normalized.text_color_hex),
        (
            "sub-back-color",
            format!("0.0/0.0/0.0/{:.2}", normalized.bg_opacity),
        ),
    ]
}

/// Appends, updates, or removes the `tlang=` query parameter on a YouTube timedtext URL.
#[allow(dead_code)]
pub fn subtitle_url_with_tlang(base_url: &str, tlang: Option<&str>) -> String {
    let trimmed_url = base_url.trim();
    if trimmed_url.is_empty() {
        return String::new();
    }
    let clean_tlang = tlang.and_then(|raw| {
        let filtered: String = raw
            .trim()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(16)
            .collect();
        (!filtered.is_empty()).then_some(filtered)
    });

    let (without_frag, fragment) = match trimmed_url.find('#') {
        Some(idx) => (&trimmed_url[..idx], Some(&trimmed_url[idx..])),
        None => (trimmed_url, None),
    };
    let (base, query_opt) = match without_frag.find('?') {
        Some(idx) => (&without_frag[..idx], Some(&without_frag[idx + 1..])),
        None => (without_frag, None),
    };

    let mut params: Vec<String> = query_opt
        .map(|q| {
            q.split('&')
                .filter(|pair| !pair.is_empty())
                .filter(|pair| {
                    let key = pair.split('=').next().unwrap_or("");
                    !key.eq_ignore_ascii_case("tlang")
                })
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    if let Some(code) = clean_tlang {
        params.push(format!("tlang={code}"));
    }

    let mut out = String::with_capacity(trimmed_url.len() + 16);
    out.push_str(base);
    if !params.is_empty() {
        out.push('?');
        out.push_str(&params.join("&"));
    }
    if let Some(frag) = fragment {
        out.push_str(frag);
    }
    out
}

/// Builds the per-file `loadfile` option string for offline local file playback (`ytdl=no`).
pub fn build_local_file_loadfile_options(start_secs: Option<f64>) -> String {
    let mut parts = Vec::with_capacity(2);
    parts.push("ytdl=no".to_owned());
    if let Some(start_secs) = start_secs
        && start_secs.is_finite()
        && start_secs > 0.5
    {
        parts.push(format!("start={start_secs:.3}"));
    }
    parts.join(",")
}

#[allow(dead_code)]
pub(crate) const WARM_PLAYER_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub(crate) const WGL_HWDEC_CHAIN: &str = "nvdec,d3d11va-copy,dxva2-copy";

pub(crate) const DISABLED_BUILTIN_SCRIPT_OPTIONS: &[(&str, &str)] = &[
    ("load-stats-overlay", "no"),
    ("load-console", "no"),
    ("load-auto-profiles", "no"),
    ("osc", "no"),
    ("osd-bar", "no"),
    ("input-default-bindings", "no"),
    ("input-vo-keyboard", "no"),
];

pub(crate) fn wgl_hwdec_chain(enabled: bool) -> &'static str {
    if enabled { WGL_HWDEC_CHAIN } else { "no" }
}

pub(crate) fn effective_hwdec_option(hwdec: HwdecMode, render_api: bool) -> &'static str {
    if render_api {
        match hwdec {
            HwdecMode::AutoSafe | HwdecMode::Auto => wgl_hwdec_chain(true),
            HwdecMode::Software => wgl_hwdec_chain(false),
        }
    } else {
        hwdec.as_mpv_value()
    }
}

pub(crate) fn ytdl_format_for_codec_pref(
    max_height: Option<u32>,
    codec: VideoCodecPreference,
) -> String {
    let height = max_height.unwrap_or(1080);
    match codec {
        VideoCodecPreference::Auto => {
            format!("bestvideo[height<={height}]+bestaudio/best[height<={height}]/best")
        }
        VideoCodecPreference::Vp9 => format!(
            "bestvideo[height<={height}][vcodec^=vp9]+bestaudio/\
             bestvideo[height<={height}][vcodec^=vp09]+bestaudio/\
             bestvideo[height<={height}]+bestaudio/best[height<={height}]/best"
        ),
        VideoCodecPreference::H264 => format!(
            "bestvideo[height<={height}][vcodec^=avc1]+bestaudio/\
             bestvideo[height<={height}]+bestaudio/best[height<={height}]/best"
        ),
        VideoCodecPreference::Av1 => format!(
            "bestvideo[height<={height}][vcodec^=av01]+bestaudio/\
             bestvideo[height<={height}]+bestaudio/best[height<={height}]/best"
        ),
    }
}

#[allow(dead_code)]
pub(crate) fn ytdl_format_for_exact_height_and_codec(
    height: u32,
    codec: VideoCodecPreference,
) -> String {
    match codec {
        VideoCodecPreference::Auto => format!(
            "bestvideo[height={height}]+bestaudio/best[height={height}]/bestvideo[height={height}]"
        ),
        VideoCodecPreference::Vp9 => format!(
            "bestvideo[height={height}][vcodec^=vp9]+bestaudio/\
             bestvideo[height={height}][vcodec^=vp09]+bestaudio/\
             bestvideo[height={height}]+bestaudio/best[height={height}]/bestvideo[height={height}]"
        ),
        VideoCodecPreference::H264 => format!(
            "bestvideo[height={height}][vcodec^=avc1]+bestaudio/\
             bestvideo[height={height}]+bestaudio/best[height={height}]/bestvideo[height={height}]"
        ),
        VideoCodecPreference::Av1 => format!(
            "bestvideo[height={height}][vcodec^=av01]+bestaudio/\
             bestvideo[height={height}]+bestaudio/best[height={height}]/bestvideo[height={height}]"
        ),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DirectStreamOptions<'a> {
    pub video_url: &'a str,
    pub audio_url: Option<&'a str>,
    pub start_secs: Option<f64>,
    pub audio_only: bool,
    pub user_agent: Option<&'a str>,
    pub referer: Option<&'a str>,
}

fn sanitize_mpv_option_value(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|&c| c != '\r' && c != '\n' && c != '\0')
        .collect()
}

/// Escapes an arbitrary value for libmpv's comma-separated `loadfile` options string.
/// Uses libmpv's fixed-length `%<byte_len>%<raw>` syntax whenever special delimiters
/// are present so commas, equals signs, quotes, or whitespace cannot split options.
fn escape_mpv_loadfile_option_value(value: &str) -> String {
    let needs_escape = value.starts_with('%')
        || value
            .bytes()
            .any(|b| matches!(b, b',' | b'=' | b'"' | b'\'' | b'\\' | b';' | b' ' | b'\t'));
    if needs_escape {
        format!("%{}%{value}", value.len())
    } else {
        value.to_owned()
    }
}

/// Builds the per-file `loadfile` option string for direct-URL (`ytdl=no`) playback.
pub fn build_direct_stream_loadfile_options(opts: &DirectStreamOptions<'_>) -> String {
    let mut parts = Vec::with_capacity(6);
    parts.push("ytdl=no".to_owned());
    if !opts.audio_only
        && let Some(audio_url) = opts.audio_url
    {
        let clean = sanitize_mpv_option_value(audio_url);
        if !clean.is_empty() {
            parts.push(format!(
                "audio-file={}",
                escape_mpv_loadfile_option_value(&clean)
            ));
        }
    }
    if let Some(start_secs) = opts.start_secs
        && start_secs.is_finite()
        && start_secs > 0.5
    {
        parts.push(format!("start={start_secs:.3}"));
    }
    parts.push("cache-pause-wait=0.25".to_owned());
    if opts.audio_only {
        parts.push("vid=no".to_owned());
    }
    if let Some(user_agent) = opts.user_agent {
        let clean = sanitize_mpv_option_value(user_agent);
        if !clean.is_empty() {
            parts.push(format!(
                "user-agent={}",
                escape_mpv_loadfile_option_value(&clean)
            ));
        }
    }
    if let Some(referer) = opts.referer {
        let clean = sanitize_mpv_option_value(referer);
        if !clean.is_empty() {
            parts.push(format!(
                "referrer={}",
                escape_mpv_loadfile_option_value(&clean)
            ));
        }
    }
    parts.join(",")
}

#[repr(C)]
struct MpvEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *const c_void,
}

#[repr(C)]
struct MpvEventProperty {
    name: *const c_char,
    format: c_int,
    data: *const c_void,
}

#[repr(C)]
union MpvNodeValue {
    string: *mut c_char,
    flag: c_int,
    int64: i64,
    double_: f64,
    list: *mut MpvNodeList,
}

// Field order (`u` before `format`) must match `struct mpv_node` in libmpv/client.h.
#[repr(C)]
struct MpvNode {
    u: MpvNodeValue,
    format: c_int,
}

#[repr(C)]
struct MpvNodeList {
    num: c_int,
    values: *mut MpvNode,
    keys: *mut *mut c_char,
}

#[repr(C)]
struct StartFile {
    playlist_entry_id: u64,
}

#[repr(C)]
struct EndFile {
    reason: c_int,
    error: c_int,
    playlist_entry_id: u64,
    _playlist_insert_id: u64,
}

#[repr(C)]
struct MpvRenderParam {
    type_: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct MpvOpenglInitParams {
    get_proc_address: Option<GetProcAddress>,
    get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
struct MpvOpenglFbo {
    fbo: c_int,
    width: c_int,
    height: c_int,
    internal_format: c_int,
}

#[repr(u64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObservedProperty {
    TimePos = 1,
    Duration = 2,
    Pause = 3,
    EofReached = 4,
    CoreIdle = 5,
    PausedForCache = 6,
    CacheBufferingState = 7,
    DemuxerCacheDuration = 8,
    Volume = 9,
    Mute = 10,
    Speed = 11,
    VideoWidth = 12,
    VideoHeight = 13,
    HwdecCurrent = 14,
    TrackList = 15,
    Aid = 16,
    Sid = 17,
    Vid = 18,
    MediaTitle = 19,
    Path = 20,
    Seekable = 21,
    SubVisibility = 22,
}

impl ObservedProperty {
    fn from_userdata(value: u64) -> Option<Self> {
        match value {
            1 => Some(Self::TimePos),
            2 => Some(Self::Duration),
            3 => Some(Self::Pause),
            4 => Some(Self::EofReached),
            5 => Some(Self::CoreIdle),
            6 => Some(Self::PausedForCache),
            7 => Some(Self::CacheBufferingState),
            8 => Some(Self::DemuxerCacheDuration),
            9 => Some(Self::Volume),
            10 => Some(Self::Mute),
            11 => Some(Self::Speed),
            12 => Some(Self::VideoWidth),
            13 => Some(Self::VideoHeight),
            14 => Some(Self::HwdecCurrent),
            15 => Some(Self::TrackList),
            16 => Some(Self::Aid),
            17 => Some(Self::Sid),
            18 => Some(Self::Vid),
            19 => Some(Self::MediaTitle),
            20 => Some(Self::Path),
            21 => Some(Self::Seekable),
            22 => Some(Self::SubVisibility),
            _ => None,
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "time-pos" => Some(Self::TimePos),
            "duration" => Some(Self::Duration),
            "pause" => Some(Self::Pause),
            "eof-reached" => Some(Self::EofReached),
            "core-idle" => Some(Self::CoreIdle),
            "paused-for-cache" => Some(Self::PausedForCache),
            "cache-buffering-state" => Some(Self::CacheBufferingState),
            "demuxer-cache-duration" => Some(Self::DemuxerCacheDuration),
            "volume" => Some(Self::Volume),
            "mute" => Some(Self::Mute),
            "speed" => Some(Self::Speed),
            "video-params/w" => Some(Self::VideoWidth),
            "video-params/h" => Some(Self::VideoHeight),
            "hwdec-current" => Some(Self::HwdecCurrent),
            "track-list" => Some(Self::TrackList),
            "aid" => Some(Self::Aid),
            "sid" => Some(Self::Sid),
            "vid" => Some(Self::Vid),
            "media-title" => Some(Self::MediaTitle),
            "path" => Some(Self::Path),
            "seekable" => Some(Self::Seekable),
            "sub-visibility" => Some(Self::SubVisibility),
            _ => None,
        }
    }
}

const OBSERVED_PROPERTIES: &[(ObservedProperty, &CStr, c_int)] = &[
    (ObservedProperty::TimePos, c"time-pos", MPV_FORMAT_DOUBLE),
    (ObservedProperty::Duration, c"duration", MPV_FORMAT_DOUBLE),
    (ObservedProperty::Pause, c"pause", MPV_FORMAT_FLAG),
    (
        ObservedProperty::EofReached,
        c"eof-reached",
        MPV_FORMAT_FLAG,
    ),
    (ObservedProperty::CoreIdle, c"core-idle", MPV_FORMAT_FLAG),
    (
        ObservedProperty::PausedForCache,
        c"paused-for-cache",
        MPV_FORMAT_FLAG,
    ),
    (
        ObservedProperty::CacheBufferingState,
        c"cache-buffering-state",
        MPV_FORMAT_INT64,
    ),
    (
        ObservedProperty::DemuxerCacheDuration,
        c"demuxer-cache-duration",
        MPV_FORMAT_DOUBLE,
    ),
    (ObservedProperty::Volume, c"volume", MPV_FORMAT_DOUBLE),
    (ObservedProperty::Mute, c"mute", MPV_FORMAT_FLAG),
    (ObservedProperty::Speed, c"speed", MPV_FORMAT_DOUBLE),
    (
        ObservedProperty::VideoWidth,
        c"video-params/w",
        MPV_FORMAT_INT64,
    ),
    (
        ObservedProperty::VideoHeight,
        c"video-params/h",
        MPV_FORMAT_INT64,
    ),
    (
        ObservedProperty::HwdecCurrent,
        c"hwdec-current",
        MPV_FORMAT_STRING,
    ),
    (ObservedProperty::TrackList, c"track-list", MPV_FORMAT_NODE),
    (ObservedProperty::Aid, c"aid", MPV_FORMAT_STRING),
    (ObservedProperty::Sid, c"sid", MPV_FORMAT_STRING),
    (ObservedProperty::Vid, c"vid", MPV_FORMAT_STRING),
    (
        ObservedProperty::MediaTitle,
        c"media-title",
        MPV_FORMAT_STRING,
    ),
    (ObservedProperty::Path, c"path", MPV_FORMAT_STRING),
    (ObservedProperty::Seekable, c"seekable", MPV_FORMAT_FLAG),
    (
        ObservedProperty::SubVisibility,
        c"sub-visibility",
        MPV_FORMAT_FLAG,
    ),
];

fn format_audio_track_label(id: &str, title: &str, lang: &str, codec: &str) -> String {
    let mut label_parts = Vec::new();
    if !title.trim().is_empty() {
        label_parts.push(title.trim().to_owned());
    }
    if !lang.trim().is_empty() {
        label_parts.push(lang.trim().to_ascii_uppercase());
    }
    if !codec.trim().is_empty() {
        label_parts.push(codec.trim().to_owned());
    }
    if label_parts.is_empty() {
        format!("Faixa #{id}")
    } else {
        label_parts.join(" · ")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioTrackInfo {
    pub id: String,
    pub title: String,
    pub lang: String,
    pub codec: String,
    pub selected: bool,
}

impl AudioTrackInfo {
    pub fn to_option(&self) -> AudioTrackOption {
        AudioTrackOption {
            id: self.id.clone(),
            label: format_audio_track_label(&self.id, &self.title, &self.lang, &self.codec),
            selected: self.selected,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubtitleTrackInfo {
    pub id: String,
    pub title: String,
    pub lang: String,
    pub codec: String,
    pub external: bool,
    pub external_filename: Option<String>,
    pub selected: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerSnapshot {
    pub observed: bool,
    pub time_pos: Option<f64>,
    pub duration: Option<f64>,
    pub pause: Option<bool>,
    pub eof_reached: Option<bool>,
    pub core_idle: Option<bool>,
    pub paused_for_cache: Option<bool>,
    pub cache_buffering_state: Option<i64>,
    pub demuxer_cache_duration: Option<f64>,
    pub volume: Option<f64>,
    pub mute: Option<bool>,
    pub speed: Option<f64>,
    pub video_width: Option<i64>,
    pub video_height: Option<i64>,
    pub hwdec_current: Option<String>,
    pub audio_tracks: Vec<AudioTrackInfo>,
    pub subtitle_tracks: Vec<SubtitleTrackInfo>,
    pub track_count: usize,
    pub track_list_observed: bool,
    pub aid: Option<String>,
    pub sid: Option<String>,
    pub vid: Option<String>,
    pub media_title: Option<String>,
    pub path: Option<String>,
    pub seekable: Option<bool>,
    pub sub_visibility: Option<bool>,
    pub file_loaded: bool,
    pub active_playlist_entry_id: Option<u64>,
}

impl PlayerSnapshot {
    #[allow(dead_code)]
    pub fn is_buffering(&self) -> bool {
        self.paused_for_cache == Some(true)
            || self
                .cache_buffering_state
                .is_some_and(|pct| (0..100).contains(&pct))
    }

    pub(crate) fn reset_playback_fields(&mut self) {
        self.time_pos = None;
        self.duration = None;
        self.pause = Some(true);
        self.eof_reached = Some(false);
        self.core_idle = Some(true);
        self.paused_for_cache = Some(false);
        self.cache_buffering_state = None;
        self.demuxer_cache_duration = None;
        self.video_width = None;
        self.video_height = None;
        self.hwdec_current = None;
        self.audio_tracks.clear();
        self.subtitle_tracks.clear();
        self.track_count = 0;
        self.aid = None;
        self.sid = None;
        self.media_title = None;
        self.path = None;
        self.seekable = None;
        self.file_loaded = false;
        self.active_playlist_entry_id = None;
    }

    fn on_start_file(&mut self, playlist_entry_id: u64) {
        self.file_loaded = false;
        self.active_playlist_entry_id = Some(playlist_entry_id);
        self.eof_reached = Some(false);
        self.time_pos = None;
        self.duration = None;
        self.demuxer_cache_duration = None;
        self.paused_for_cache = Some(false);
        self.cache_buffering_state = None;
    }

    fn on_file_loaded(&mut self) {
        self.file_loaded = true;
    }

    fn on_end_file(&mut self, reason: c_int) {
        self.file_loaded = false;
        self.active_playlist_entry_id = None;
        self.paused_for_cache = Some(false);
        self.cache_buffering_state = None;
        self.demuxer_cache_duration = None;
        if reason == MPV_END_FILE_REASON_EOF {
            self.eof_reached = Some(true);
        } else if reason == MPV_END_FILE_REASON_STOP {
            self.eof_reached = Some(false);
            self.time_pos = None;
            self.duration = None;
            self.audio_tracks.clear();
            self.subtitle_tracks.clear();
            self.track_count = 0;
        }
    }

    fn sync_audio_selection(&mut self, aid: Option<&str>) {
        for track in &mut self.audio_tracks {
            track.selected = aid.is_some_and(|active| active == track.id);
        }
    }

    fn sync_subtitle_selection(&mut self, sid: Option<&str>) {
        for track in &mut self.subtitle_tracks {
            track.selected = sid.is_some_and(|active| active == track.id);
        }
    }

    fn lookup_string_cstr(&self, name: &CStr) -> Option<Option<String>> {
        let bytes = name.to_bytes();
        match bytes {
            b"hwdec-current" => Some(self.hwdec_current.clone()),
            b"aid" => Some(self.aid.clone()),
            b"sid" => Some(self.sid.clone()),
            b"vid" => Some(self.vid.clone()),
            b"media-title" => Some(self.media_title.clone()),
            b"path" => Some(self.path.clone()),
            b"pause" => Some(self.pause.map(|v| if v { "yes" } else { "no" }.to_owned())),
            b"eof-reached" => Some(
                self.eof_reached
                    .map(|v| if v { "yes" } else { "no" }.to_owned()),
            ),
            b"mute" => Some(self.mute.map(|v| if v { "yes" } else { "no" }.to_owned())),
            b"core-idle" => Some(
                self.core_idle
                    .map(|v| if v { "yes" } else { "no" }.to_owned()),
            ),
            b"paused-for-cache" => Some(
                self.paused_for_cache
                    .map(|v| if v { "yes" } else { "no" }.to_owned()),
            ),
            b"seekable" => Some(
                self.seekable
                    .map(|v| if v { "yes" } else { "no" }.to_owned()),
            ),
            b"sub-visibility" => Some(
                self.sub_visibility
                    .map(|v| if v { "yes" } else { "no" }.to_owned()),
            ),
            _ => None,
        }
    }

    fn lookup_flag_cstr(&self, name: &CStr) -> Option<Option<bool>> {
        match name.to_bytes() {
            b"pause" => Some(self.pause),
            b"eof-reached" => Some(self.eof_reached),
            b"mute" => Some(self.mute),
            b"core-idle" => Some(self.core_idle),
            b"paused-for-cache" => Some(self.paused_for_cache),
            b"seekable" => Some(self.seekable),
            b"sub-visibility" => Some(self.sub_visibility),
            _ => None,
        }
    }

    fn lookup_int_cstr(&self, name: &CStr) -> Option<Option<i64>> {
        match name.to_bytes() {
            b"cache-buffering-state" => Some(self.cache_buffering_state),
            b"video-params/w" => Some(self.video_width),
            b"video-params/h" => Some(self.video_height),
            b"track-list/count" if self.track_list_observed => Some(Some(self.track_count as i64)),
            _ => None,
        }
    }

    fn lookup_double_cstr(&self, name: &CStr) -> Option<Option<f64>> {
        match name.to_bytes() {
            b"time-pos" => Some(self.time_pos),
            b"duration" => Some(self.duration),
            b"demuxer-cache-duration" => Some(self.demuxer_cache_duration),
            b"volume" => Some(self.volume),
            b"speed" => Some(self.speed),
            _ => None,
        }
    }

    fn apply_optimistic_set(&mut self, name: &str, value: &str) {
        match name {
            "pause" => {
                if value == "yes" {
                    self.pause = Some(true);
                } else if value == "no" {
                    self.pause = Some(false);
                }
            }
            "mute" => {
                if value == "yes" {
                    self.mute = Some(true);
                } else if value == "no" {
                    self.mute = Some(false);
                }
            }
            "volume" => {
                if let Ok(parsed) = value.parse::<f64>()
                    && parsed.is_finite()
                {
                    self.volume = Some(parsed);
                }
            }
            "speed" => {
                if let Ok(parsed) = value.parse::<f64>()
                    && parsed.is_finite()
                {
                    self.speed = Some(parsed);
                }
            }
            "aid" => {
                self.aid = Some(value.to_owned());
                self.sync_audio_selection(Some(value));
            }
            "sid" => {
                self.sid = Some(value.to_owned());
                self.sync_subtitle_selection(Some(value));
            }
            "vid" => {
                self.vid = Some(value.to_owned());
            }
            "sub-visibility" => {
                if value == "yes" {
                    self.sub_visibility = Some(true);
                } else if value == "no" {
                    self.sub_visibility = Some(false);
                }
            }
            _ => {}
        }
    }

    unsafe fn apply_property_change(&mut self, reply_userdata: u64, prop: &MpvEventProperty) {
        let property = ObservedProperty::from_userdata(reply_userdata).or_else(|| {
            if prop.name.is_null() {
                None
            } else {
                let name = unsafe { CStr::from_ptr(prop.name) }.to_str().ok()?;
                ObservedProperty::from_name(name)
            }
        });
        let Some(property) = property else {
            return;
        };
        let unavailable = prop.format == MPV_FORMAT_NONE || prop.data.is_null();
        match property {
            ObservedProperty::TimePos => {
                self.time_pos = if unavailable {
                    None
                } else {
                    unsafe { read_prop_double(prop) }
                };
            }
            ObservedProperty::Duration => {
                self.duration = if unavailable {
                    None
                } else {
                    unsafe { read_prop_double(prop) }
                };
            }
            ObservedProperty::Pause => {
                self.pause = if unavailable {
                    None
                } else {
                    unsafe { read_prop_flag(prop) }
                };
            }
            ObservedProperty::EofReached => {
                self.eof_reached = if unavailable {
                    None
                } else {
                    unsafe { read_prop_flag(prop) }
                };
            }
            ObservedProperty::CoreIdle => {
                self.core_idle = if unavailable {
                    None
                } else {
                    unsafe { read_prop_flag(prop) }
                };
            }
            ObservedProperty::PausedForCache => {
                self.paused_for_cache = if unavailable {
                    None
                } else {
                    unsafe { read_prop_flag(prop) }
                };
            }
            ObservedProperty::CacheBufferingState => {
                self.cache_buffering_state = if unavailable {
                    None
                } else {
                    unsafe { read_prop_int64(prop) }
                };
            }
            ObservedProperty::DemuxerCacheDuration => {
                self.demuxer_cache_duration = if unavailable {
                    None
                } else {
                    unsafe { read_prop_double(prop) }
                };
            }
            ObservedProperty::Volume => {
                self.volume = if unavailable {
                    None
                } else {
                    unsafe { read_prop_double(prop) }
                };
            }
            ObservedProperty::Mute => {
                self.mute = if unavailable {
                    None
                } else {
                    unsafe { read_prop_flag(prop) }
                };
            }
            ObservedProperty::Speed => {
                self.speed = if unavailable {
                    None
                } else {
                    unsafe { read_prop_double(prop) }
                };
            }
            ObservedProperty::VideoWidth => {
                self.video_width = if unavailable {
                    None
                } else {
                    unsafe { read_prop_int64(prop) }
                };
            }
            ObservedProperty::VideoHeight => {
                self.video_height = if unavailable {
                    None
                } else {
                    unsafe { read_prop_int64(prop) }
                };
            }
            ObservedProperty::HwdecCurrent => {
                self.hwdec_current = if unavailable {
                    None
                } else {
                    unsafe { read_prop_string(prop) }
                };
            }
            ObservedProperty::TrackList => {
                if unavailable {
                    self.audio_tracks.clear();
                    self.subtitle_tracks.clear();
                    self.track_count = 0;
                    self.track_list_observed = true;
                } else if prop.format == MPV_FORMAT_NODE {
                    let node = unsafe { &*(prop.data as *const MpvNode) };
                    let (audio, subs, count) = unsafe { parse_track_list_node(node) };
                    self.audio_tracks = audio;
                    self.subtitle_tracks = subs;
                    self.track_count = count;
                    self.track_list_observed = true;
                }
            }
            ObservedProperty::Aid => {
                self.aid = if unavailable {
                    None
                } else {
                    unsafe { read_prop_string(prop) }
                };
                let aid_clone = self.aid.clone();
                self.sync_audio_selection(aid_clone.as_deref());
            }
            ObservedProperty::Sid => {
                self.sid = if unavailable {
                    None
                } else {
                    unsafe { read_prop_string(prop) }
                };
                let sid_clone = self.sid.clone();
                self.sync_subtitle_selection(sid_clone.as_deref());
            }
            ObservedProperty::Vid => {
                self.vid = if unavailable {
                    None
                } else {
                    unsafe { read_prop_string(prop) }
                };
            }
            ObservedProperty::MediaTitle => {
                self.media_title = if unavailable {
                    None
                } else {
                    unsafe { read_prop_string(prop) }
                };
            }
            ObservedProperty::Path => {
                self.path = if unavailable {
                    None
                } else {
                    unsafe { read_prop_string(prop) }
                };
            }
            ObservedProperty::Seekable => {
                self.seekable = if unavailable {
                    None
                } else {
                    unsafe { read_prop_flag(prop) }
                };
            }
            ObservedProperty::SubVisibility => {
                self.sub_visibility = if unavailable {
                    None
                } else {
                    unsafe { read_prop_flag(prop) }
                };
            }
        }
    }
}

unsafe fn read_prop_double(prop: &MpvEventProperty) -> Option<f64> {
    if prop.format != MPV_FORMAT_DOUBLE || prop.data.is_null() {
        return None;
    }
    let value = unsafe { *(prop.data as *const f64) };
    value.is_finite().then_some(value)
}

unsafe fn read_prop_flag(prop: &MpvEventProperty) -> Option<bool> {
    if prop.format != MPV_FORMAT_FLAG || prop.data.is_null() {
        return None;
    }
    Some(unsafe { *(prop.data as *const c_int) } != 0)
}

unsafe fn read_prop_int64(prop: &MpvEventProperty) -> Option<i64> {
    if prop.format != MPV_FORMAT_INT64 || prop.data.is_null() {
        return None;
    }
    Some(unsafe { *(prop.data as *const i64) })
}

unsafe fn read_prop_string(prop: &MpvEventProperty) -> Option<String> {
    if prop.format != MPV_FORMAT_STRING || prop.data.is_null() {
        return None;
    }
    // `MPV_FORMAT_STRING` property events pass `char **` in `data`.
    let ptr = unsafe { *(prop.data as *const *const c_char) };
    if ptr.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned(),
    )
}

unsafe fn node_as_string(node: &MpvNode) -> Option<String> {
    if node.format != MPV_FORMAT_STRING {
        return None;
    }
    let ptr = unsafe { node.u.string };
    if ptr.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned(),
    )
}

unsafe fn node_as_id_string(node: &MpvNode) -> Option<String> {
    match node.format {
        MPV_FORMAT_INT64 => Some(unsafe { node.u.int64 }.to_string()),
        MPV_FORMAT_STRING => unsafe { node_as_string(node) },
        _ => None,
    }
}

unsafe fn node_as_flag(node: &MpvNode) -> Option<bool> {
    if node.format != MPV_FORMAT_FLAG {
        return None;
    }
    Some(unsafe { node.u.flag } != 0)
}

unsafe fn parse_track_list_node(
    node: &MpvNode,
) -> (Vec<AudioTrackInfo>, Vec<SubtitleTrackInfo>, usize) {
    if node.format != MPV_FORMAT_NODE_ARRAY {
        return (Vec::new(), Vec::new(), 0);
    }
    let list_ptr = unsafe { node.u.list };
    if list_ptr.is_null() {
        return (Vec::new(), Vec::new(), 0);
    }
    let list = unsafe { &*list_ptr };
    if list.num <= 0 || list.values.is_null() {
        return (Vec::new(), Vec::new(), 0);
    }
    let count = list.num as usize;
    let entries = unsafe { std::slice::from_raw_parts(list.values, count) };
    let mut audio_tracks = Vec::new();
    let mut subtitle_tracks = Vec::new();

    for entry in entries {
        if entry.format != MPV_FORMAT_NODE_MAP {
            continue;
        }
        let map_ptr = unsafe { entry.u.list };
        if map_ptr.is_null() {
            continue;
        }
        let map = unsafe { &*map_ptr };
        if map.num <= 0 || map.values.is_null() || map.keys.is_null() {
            continue;
        }
        let mut track_type: Option<String> = None;
        let mut id: Option<String> = None;
        let mut title = String::new();
        let mut lang = String::new();
        let mut codec = String::new();
        let mut selected = false;
        let mut external = false;
        let mut external_filename: Option<String> = None;

        for idx in 0..(map.num as usize) {
            let key_ptr = unsafe { *map.keys.add(idx) };
            if key_ptr.is_null() {
                continue;
            }
            let Ok(key) = unsafe { CStr::from_ptr(key_ptr) }.to_str() else {
                continue;
            };
            let val = unsafe { &*map.values.add(idx) };
            match key {
                "type" => track_type = unsafe { node_as_string(val) },
                "id" => id = unsafe { node_as_id_string(val) },
                "title" => {
                    if let Some(value) = unsafe { node_as_string(val) } {
                        title = value;
                    }
                }
                "lang" => {
                    if let Some(value) = unsafe { node_as_string(val) } {
                        lang = value;
                    }
                }
                "codec" => {
                    if let Some(value) = unsafe { node_as_string(val) } {
                        codec = value;
                    }
                }
                "selected" => {
                    if let Some(value) = unsafe { node_as_flag(val) } {
                        selected = value;
                    }
                }
                "external" => {
                    if let Some(value) = unsafe { node_as_flag(val) } {
                        external = value;
                    }
                }
                "external-filename" => {
                    external_filename = unsafe { node_as_string(val) };
                }
                _ => {}
            }
        }

        let Some(id) = id else {
            continue;
        };
        match track_type.as_deref() {
            Some("audio") => audio_tracks.push(AudioTrackInfo {
                id,
                title,
                lang,
                codec,
                selected,
            }),
            Some("sub") => subtitle_tracks.push(SubtitleTrackInfo {
                id,
                title,
                lang,
                codec,
                external,
                external_filename,
                selected,
            }),
            _ => {}
        }
    }

    (audio_tracks, subtitle_tracks, count)
}

#[derive(Clone, Debug, Default)]
struct WarmParkState {
    parked_since: Option<Instant>,
}

impl WarmParkState {
    fn park_at(&mut self, now: Instant) -> bool {
        if self.parked_since.is_some() {
            return false;
        }
        self.parked_since = Some(now);
        true
    }

    fn wake(&mut self) -> bool {
        self.parked_since.take().is_some()
    }

    #[cfg(test)]
    fn is_parked(&self) -> bool {
        self.parked_since.is_some()
    }

    fn parked_duration_at(&self, now: Instant) -> Option<Duration> {
        self.parked_since
            .map(|since| now.saturating_duration_since(since))
    }

    fn is_idle_expired_at(&self, now: Instant, timeout: Duration) -> bool {
        self.parked_duration_at(now)
            .is_some_and(|elapsed| elapsed >= timeout)
    }
}

pub(crate) fn scaled_window_bounds(rect: Rect, scale: f32) -> (i32, i32, i32, i32) {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    (
        (rect.min.x * scale).round() as i32,
        (rect.min.y * scale).round() as i32,
        (rect.width() * scale).round().max(1.0) as i32,
        (rect.height() * scale).round().max(1.0) as i32,
    )
}

pub use crate::overlay::{AudioEqPreset, OverlayAudioTrack as AudioTrackOption};

pub(crate) struct VideoWindow {
    hwnd: HWND,
    visible: bool,
    last_rect: Option<(Rect, f32)>,
}

impl VideoWindow {
    pub(crate) fn new(parent: HWND) -> Result<Self, String> {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                ptr::null(),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                1,
                1,
                parent,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err("Não foi possível criar a área de vídeo.".into());
        }
        Ok(Self {
            hwnd,
            visible: true,
            last_rect: None,
        })
    }

    pub(crate) fn hwnd(&self) -> HWND {
        self.hwnd
    }

    #[cfg(test)]
    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    pub(crate) fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        unsafe {
            ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
        if visible && let Some((rect, scale)) = self.last_rect {
            self.apply_bounds(rect, scale);
        }
    }

    pub(crate) fn resize(&mut self, rect: Rect, scale: f32) {
        if !rect.is_positive() {
            return;
        }
        if self.last_rect == Some((rect, scale)) {
            return;
        }
        self.last_rect = Some((rect, scale));
        if self.visible {
            self.apply_bounds(rect, scale);
        }
    }

    fn apply_bounds(&self, rect: Rect, scale: f32) {
        let (x, y, width, height) = scaled_window_bounds(rect, scale);
        let visibility_flag = if self.visible {
            SWP_SHOWWINDOW
        } else {
            SWP_HIDEWINDOW
        };
        unsafe {
            SetWindowPos(
                self.hwnd,
                ptr::null_mut(),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER | visibility_flag,
            );
        }
    }
}

impl Drop for VideoWindow {
    fn drop(&mut self) {
        unsafe {
            ShowWindow(self.hwnd, SW_HIDE);
            DestroyWindow(self.hwnd);
        }
    }
}

struct MpvApi {
    create: Create,
    set_option: SetOption,
    initialize: Initialize,
    command: Command,
    terminate: Terminate,
    get_property: GetProperty,
    get_property_typed: Option<GetPropertyTyped>,
    set_property: SetProperty,
    free: Free,
    wait_event: WaitEvent,
    request_event: RequestEvent,
    observe_property: Option<ObserveProperty>,
    set_wakeup_callback: Option<SetWakeupCallback>,
    error_string: ErrorString,
    render_create: RenderContextCreate,
    render_free: RenderContextFree,
    render_render: RenderContextRender,
    render_update: RenderContextUpdate,
    render_set_update_callback: RenderContextSetUpdateCallback,
}

impl MpvApi {
    unsafe fn load(library: &Library) -> Result<Self, String> {
        unsafe {
            Ok(Self {
                create: *library
                    .get(b"mpv_create\0")
                    .map_err(|error| error.to_string())?,
                set_option: *library
                    .get(b"mpv_set_option_string\0")
                    .map_err(|error| error.to_string())?,
                initialize: *library
                    .get(b"mpv_initialize\0")
                    .map_err(|error| error.to_string())?,
                command: *library
                    .get(b"mpv_command\0")
                    .map_err(|error| error.to_string())?,
                terminate: *library
                    .get(b"mpv_terminate_destroy\0")
                    .map_err(|error| error.to_string())?,
                get_property: *library
                    .get(b"mpv_get_property_string\0")
                    .map_err(|error| error.to_string())?,
                get_property_typed: library
                    .get::<GetPropertyTyped>(b"mpv_get_property\0")
                    .ok()
                    .map(|symbol| *symbol),
                set_property: *library
                    .get(b"mpv_set_property_string\0")
                    .map_err(|error| error.to_string())?,
                free: *library
                    .get(b"mpv_free\0")
                    .map_err(|error| error.to_string())?,
                wait_event: *library
                    .get(b"mpv_wait_event\0")
                    .map_err(|error| error.to_string())?,
                request_event: *library
                    .get(b"mpv_request_event\0")
                    .map_err(|error| error.to_string())?,
                observe_property: library
                    .get::<ObserveProperty>(b"mpv_observe_property\0")
                    .ok()
                    .map(|symbol| *symbol),
                set_wakeup_callback: library
                    .get::<SetWakeupCallback>(b"mpv_set_wakeup_callback\0")
                    .ok()
                    .map(|symbol| *symbol),
                error_string: *library
                    .get(b"mpv_error_string\0")
                    .map_err(|error| error.to_string())?,
                render_create: *library
                    .get(b"mpv_render_context_create\0")
                    .map_err(|error| error.to_string())?,
                render_free: *library
                    .get(b"mpv_render_context_free\0")
                    .map_err(|error| error.to_string())?,
                render_render: *library
                    .get(b"mpv_render_context_render\0")
                    .map_err(|error| error.to_string())?,
                render_update: *library
                    .get(b"mpv_render_context_update\0")
                    .map_err(|error| error.to_string())?,
                render_set_update_callback: *library
                    .get(b"mpv_render_context_set_update_callback\0")
                    .map_err(|error| error.to_string())?,
            })
        }
    }

    fn describe_error(&self, code: c_int) -> String {
        let text = unsafe { (self.error_string)(code) };
        if text.is_null() {
            return format!("Erro do player ({code})");
        }
        format!(
            "Erro do player: {}",
            unsafe { CStr::from_ptr(text) }.to_string_lossy()
        )
    }
}

static OPENGL32: OnceLock<Option<Library>> = OnceLock::new();
static EGL: OnceLock<Option<Library>> = OnceLock::new();
static WGL_GET_PROC_ADDRESS: OnceLock<Option<WglGetProcAddressFn>> = OnceLock::new();
static EGL_GET_PROC_ADDRESS: OnceLock<Option<EglGetProcAddressFn>> = OnceLock::new();
static GL_PROC_CACHE: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();

fn opengl32() -> Option<&'static Library> {
    OPENGL32
        .get_or_init(|| unsafe { Library::new("opengl32.dll").ok() })
        .as_ref()
}

fn egl() -> Option<&'static Library> {
    EGL.get_or_init(|| unsafe {
        Library::new("libEGL.dll")
            .or_else(|_| Library::new("EGL.dll"))
            .ok()
    })
    .as_ref()
}

fn wgl_get_proc_address_fn() -> Option<WglGetProcAddressFn> {
    *WGL_GET_PROC_ADDRESS.get_or_init(|| {
        let library = opengl32()?;
        unsafe {
            library
                .get::<WglGetProcAddressFn>(b"wglGetProcAddress\0")
                .ok()
        }
        .map(|symbol| *symbol)
    })
}

fn egl_get_proc_address_fn() -> Option<EglGetProcAddressFn> {
    *EGL_GET_PROC_ADDRESS.get_or_init(|| {
        let library = egl()?;
        unsafe {
            library
                .get::<EglGetProcAddressFn>(b"eglGetProcAddress\0")
                .ok()
        }
        .map(|symbol| *symbol)
    })
}

fn gl_proc_cache() -> &'static Mutex<HashMap<String, usize>> {
    GL_PROC_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn is_valid_wgl_proc_address(address: *mut c_void) -> bool {
    let special = address as isize;
    !address.is_null() && !(1..=3).contains(&special) && special != -1
}

fn lookup_or_cache_gl_proc<F>(
    cache: &Mutex<HashMap<String, usize>>,
    key: &str,
    resolve: F,
) -> *mut c_void
where
    F: FnOnce() -> *mut c_void,
{
    if let Ok(guard) = cache.lock()
        && let Some(&cached) = guard.get(key)
    {
        return cached as *mut c_void;
    }
    let address = resolve();
    if !address.is_null()
        && let Ok(mut guard) = cache.lock()
    {
        guard.insert(key.to_owned(), address as usize);
    }
    address
}

unsafe extern "C" fn mpv_get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    if name.is_null() {
        return ptr::null_mut();
    }
    let cstr = unsafe { CStr::from_ptr(name) };
    let bytes = cstr.to_bytes_with_nul();
    let resolve = || {
        if let Some(wgl_get_proc_address) = wgl_get_proc_address_fn() {
            let address = unsafe { wgl_get_proc_address(name) };
            if is_valid_wgl_proc_address(address) {
                return address;
            }
        }
        if let Some(library) = opengl32()
            && let Ok(symbol) = unsafe { library.get::<unsafe extern "system" fn()>(bytes) }
        {
            return *symbol as usize as *mut c_void;
        }
        if let Some(egl_get_proc_address) = egl_get_proc_address_fn() {
            let address = unsafe { egl_get_proc_address(name) };
            if !address.is_null() {
                return address;
            }
        }
        ptr::null_mut()
    };

    if let Ok(key) = cstr.to_str() {
        lookup_or_cache_gl_proc(gl_proc_cache(), key, resolve)
    } else {
        resolve()
    }
}

struct WakeupCallbackData {
    ctx: egui::Context,
    window_hidden: Arc<AtomicBool>,
    parked: Arc<AtomicBool>,
    pending: Arc<AtomicBool>,
}

fn should_request_repaint_for_wakeup(
    window_hidden: &AtomicBool,
    parked: &AtomicBool,
    pending: &AtomicBool,
) -> bool {
    if window_hidden.load(Ordering::Relaxed) || parked.load(Ordering::Relaxed) {
        return false;
    }
    !pending.swap(true, Ordering::AcqRel)
}

unsafe extern "C" fn mpv_wakeup_callback(data: *mut c_void) {
    if data.is_null() {
        return;
    }
    let state = unsafe { &*(data as *const WakeupCallbackData) };
    if should_request_repaint_for_wakeup(&state.window_hidden, &state.parked, &state.pending) {
        state.ctx.request_repaint();
    }
}

struct RenderUpdateCallbackData {
    ctx: egui::Context,
    window_hidden: Arc<AtomicBool>,
}

fn should_request_repaint_for_render_update(window_hidden: &AtomicBool) -> bool {
    !window_hidden.load(Ordering::Relaxed)
}

unsafe extern "C" fn mpv_render_update_callback(data: *mut c_void) {
    if data.is_null() {
        return;
    }
    let state = unsafe { &*(data as *const RenderUpdateCallbackData) };
    if should_request_repaint_for_render_update(&state.window_hidden) {
        state.ctx.request_repaint();
    }
}

fn build_render_params(
    fbo: &mut MpvOpenglFbo,
    flip_y: &mut c_int,
    block_for_target_time: &mut c_int,
) -> [MpvRenderParam; 4] {
    [
        MpvRenderParam {
            type_: MPV_RENDER_PARAM_OPENGL_FBO,
            data: (fbo as *mut MpvOpenglFbo).cast(),
        },
        MpvRenderParam {
            type_: MPV_RENDER_PARAM_FLIP_Y,
            data: (flip_y as *mut c_int).cast(),
        },
        MpvRenderParam {
            type_: MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
            data: (block_for_target_time as *mut c_int).cast(),
        },
        MpvRenderParam {
            type_: 0,
            data: ptr::null_mut(),
        },
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VideoSuspendTransition {
    vid_property: Option<&'static str>,
    release_gl_surface: bool,
    mark_needs_render: bool,
}

fn compute_window_hidden_transition(
    was_hidden: bool,
    hidden: bool,
    audio_only: bool,
    was_video_suspended: bool,
) -> Option<(bool, VideoSuspendTransition)> {
    if was_hidden == hidden {
        return None;
    }
    let next_suspended = hidden || audio_only;
    let vid_property = if next_suspended != was_video_suspended {
        Some(if next_suspended { "no" } else { "auto" })
    } else {
        None
    };
    Some((
        next_suspended,
        VideoSuspendTransition {
            vid_property,
            release_gl_surface: hidden,
            mark_needs_render: !hidden,
        },
    ))
}

struct RenderContext {
    handle: *mut c_void,
    render: RenderContextRender,
    update: RenderContextUpdate,
    set_update_callback: RenderContextSetUpdateCallback,
    free: RenderContextFree,
    update_callback_data: Box<RenderUpdateCallbackData>,
}

unsafe impl Send for RenderContext {}
unsafe impl Sync for RenderContext {}

impl Drop for RenderContext {
    fn drop(&mut self) {
        unsafe {
            (self.set_update_callback)(self.handle, None, ptr::null_mut());
            (self.free)(self.handle);
        }
        let _ = &self.update_callback_data;
    }
}

struct GlSurface {
    texture: glow::Texture,
    framebuffer: glow::Framebuffer,
    width: i32,
    height: i32,
}

struct RenderShared {
    gl: Arc<glow::Context>,
    context: RenderContext,
    surface: Mutex<Option<GlSurface>>,
    needs_render: AtomicBool,
    window_hidden: Arc<AtomicBool>,
    parked: Arc<AtomicBool>,
    failed: AtomicBool,
}

unsafe impl Send for RenderShared {}
unsafe impl Sync for RenderShared {}

impl RenderShared {
    fn release_surface(&self) {
        if let Ok(mut surface) = self.surface.lock()
            && let Some(surface) = surface.take()
        {
            destroy_surface(self.gl.as_ref(), surface);
        }
        self.needs_render.store(true, Ordering::Relaxed);
    }

    fn render(&self, info: egui::PaintCallbackInfo, painter: &egui_glow::Painter) {
        if self.failed.load(Ordering::Relaxed)
            || self.window_hidden.load(Ordering::Relaxed)
            || self.parked.load(Ordering::Acquire)
        {
            return;
        }
        let update_flags = unsafe { (self.context.update)(self.context.handle) };
        if (update_flags & 1) != 0 {
            self.needs_render.store(true, Ordering::Relaxed);
        }
        let clip = info.clip_rect_in_pixels();
        if clip.width_px <= 0 || clip.height_px <= 0 {
            return;
        }
        let ppp = info.pixels_per_point.max(0.5);
        let screen_h = info.screen_size_px[1] as i32;
        let dst_left = (info.viewport.min.x * ppp).round() as i32;
        let dst_top = (info.viewport.min.y * ppp).round() as i32;
        let width = (info.viewport.width() * ppp).round() as i32;
        let height = (info.viewport.height() * ppp).round() as i32;
        if width <= 0 || height <= 0 {
            return;
        }
        let dst_bottom = screen_h - (dst_top + height);
        let vis_x0 = dst_left.max(clip.left_px);
        let vis_y0 = dst_bottom.max(clip.from_bottom_px);
        let vis_x1 = (dst_left + width).min(clip.left_px + clip.width_px);
        let vis_y1 = (dst_bottom + height).min(clip.from_bottom_px + clip.height_px);
        if vis_x1 <= vis_x0 || vis_y1 <= vis_y0 {
            return;
        }
        let gl = self.gl.as_ref();
        let mut surface = match self.surface.lock() {
            Ok(surface) => surface,
            Err(_) => return,
        };
        let recreate = !matches!(
            surface.as_ref(),
            Some(existing) if existing.width == width && existing.height == height
        );
        if recreate {
            let Some(created) = create_surface(gl, width, height) else {
                self.failed.store(true, Ordering::Relaxed);
                return;
            };
            if let Some(previous) = surface.replace(created) {
                destroy_surface(gl, previous);
            }
        }
        let Some(surface) = surface.as_ref() else {
            return;
        };
        let should_render_mpv = recreate || self.needs_render.swap(false, Ordering::Relaxed);
        unsafe {
            if should_render_mpv {
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(surface.framebuffer));
                gl.viewport(0, 0, width, height);
                gl.disable(glow::SCISSOR_TEST);
                let mut fbo = MpvOpenglFbo {
                    fbo: surface.framebuffer.0.get() as c_int,
                    width,
                    height,
                    internal_format: 0,
                };
                let mut flip_y: c_int = 1;
                let mut block_for_target_time: c_int = 0;
                let mut params =
                    build_render_params(&mut fbo, &mut flip_y, &mut block_for_target_time);
                let result = (self.context.render)(self.context.handle, params.as_mut_ptr());
                if result < 0 {
                    self.failed.store(true, Ordering::Relaxed);
                    gl.bind_framebuffer(glow::FRAMEBUFFER, painter.intermediate_fbo());
                    return;
                }
            }
            let src_x0 = vis_x0 - dst_left;
            let src_y0 = vis_y0 - dst_bottom;
            let src_x1 = vis_x1 - dst_left;
            let src_y1 = vis_y1 - dst_bottom;
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(surface.framebuffer));
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, painter.intermediate_fbo());
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(
                clip.left_px,
                clip.from_bottom_px,
                clip.width_px,
                clip.height_px,
            );
            gl.blit_framebuffer(
                src_x0,
                src_y0,
                src_x1,
                src_y1,
                vis_x0,
                vis_y0,
                vis_x1,
                vis_y1,
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            );
            gl.disable(glow::SCISSOR_TEST);
            gl.bind_framebuffer(glow::FRAMEBUFFER, painter.intermediate_fbo());
        }
    }
}

impl Drop for RenderShared {
    fn drop(&mut self) {
        if let Ok(mut surface) = self.surface.lock()
            && let Some(surface) = surface.take()
        {
            destroy_surface(self.gl.as_ref(), surface);
        }
    }
}

fn create_surface(gl: &glow::Context, width: i32, height: i32) -> Option<GlSurface> {
    unsafe {
        let texture = gl.create_texture().ok()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            width,
            height,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );
        gl.bind_texture(glow::TEXTURE_2D, None);
        let framebuffer = match gl.create_framebuffer() {
            Ok(framebuffer) => framebuffer,
            Err(_) => {
                gl.delete_texture(texture);
                return None;
            }
        };
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        if status != glow::FRAMEBUFFER_COMPLETE {
            gl.delete_framebuffer(framebuffer);
            gl.delete_texture(texture);
            return None;
        }
        Some(GlSurface {
            texture,
            framebuffer,
            width,
            height,
        })
    }
}

fn destroy_surface(gl: &glow::Context, surface: GlSurface) {
    unsafe {
        gl.delete_framebuffer(surface.framebuffer);
        gl.delete_texture(surface.texture);
    }
}

pub struct Player {
    handle: *mut c_void,
    window: Option<VideoWindow>,
    render: Option<Arc<RenderShared>>,
    audio_only: AtomicBool,
    window_hidden: Arc<AtomicBool>,
    video_suspended: AtomicBool,
    parked: Arc<AtomicBool>,
    park_state: Mutex<WarmParkState>,
    active_occurrence_id: AtomicU64,
    wakeup_pending: Arc<AtomicBool>,
    wakeup_callback_data: Option<Box<WakeupCallbackData>>,
    snapshot: Mutex<PlayerSnapshot>,
    properties_observed: bool,
    command: Command,
    terminate: Terminate,
    get_property: GetProperty,
    get_property_typed: Option<GetPropertyTyped>,
    set_property: SetProperty,
    set_wakeup_callback: Option<SetWakeupCallback>,
    free: Free,
    wait_event: WaitEvent,
    error_string: ErrorString,
    _library: Library,
}

impl Player {
    #[allow(dead_code)]
    pub fn new(
        frame: &eframe::Frame,
        buffer: BufferSettings,
        hwdec: HwdecMode,
        ctx: &egui::Context,
    ) -> Result<Self, String> {
        Self::new_with_codec_pref(frame, buffer, hwdec, VideoCodecPreference::Auto, ctx)
    }

    #[allow(dead_code)]
    pub(crate) fn new_with_codec_pref(
        frame: &eframe::Frame,
        buffer: BufferSettings,
        hwdec: HwdecMode,
        codec_pref: VideoCodecPreference,
        ctx: &egui::Context,
    ) -> Result<Self, String> {
        let parent = match frame
            .window_handle()
            .map_err(|error| error.to_string())?
            .as_raw()
        {
            RawWindowHandle::Win32(handle) => handle.hwnd.get() as HWND,
            _ => return Err("A janela do Windows não está disponível.".into()),
        };
        let mut render_error = None;
        if let Some(gl) = frame.gl() {
            match Self::new_render_api(buffer, hwdec, codec_pref, ctx, Arc::clone(gl)) {
                Ok(player) => return Ok(player),
                Err(error) => render_error = Some(error),
            }
        }
        Self::new_embedded(parent, buffer, hwdec, codec_pref, ctx).map_err(|error| {
            match render_error {
                Some(render_error) => {
                    format!(
                        "{error} (a renderização OpenGL do libmpv também falhou: {render_error})"
                    )
                }
                None => error,
            }
        })
    }

    fn load_library() -> Result<Library, String> {
        for name in ["libmpv-2.dll", "mpv-2.dll"] {
            let mut candidates = Vec::new();
            if let Ok(executable) = std::env::current_exe() {
                let mut current = executable.parent();
                for _ in 0..4 {
                    let Some(directory) = current else { break };
                    candidates.push(directory.join(name));
                    current = directory.parent();
                }
            }
            if let Ok(directory) = std::env::current_dir() {
                candidates.push(directory.join(name));
            }
            candidates.push(name.into());
            for candidate in candidates {
                if let Ok(found) = unsafe { Library::new(&candidate) } {
                    return Ok(found);
                }
            }
        }
        Err("Não foi possível carregar libmpv-2.dll ou mpv-2.dll. Verifique se a DLL e suas dependências estão na pasta do aplicativo ou no PATH.".into())
    }

    fn register_observers_and_wakeup(&mut self, api: &MpvApi, ctx: &egui::Context) {
        if let Some(set_wakeup) = api.set_wakeup_callback {
            let data = Box::new(WakeupCallbackData {
                ctx: ctx.clone(),
                window_hidden: Arc::clone(&self.window_hidden),
                parked: Arc::clone(&self.parked),
                pending: Arc::clone(&self.wakeup_pending),
            });
            let data_ptr = (&*data as *const WakeupCallbackData).cast_mut().cast();
            self.wakeup_callback_data = Some(data);
            unsafe {
                set_wakeup(self.handle, Some(mpv_wakeup_callback), data_ptr);
            }
        }
        if let Some(observe) = api.observe_property {
            let mut all_ok = true;
            for &(property, name, format) in OBSERVED_PROPERTIES {
                let rc = unsafe { observe(self.handle, property as u64, name.as_ptr(), format) };
                if rc < 0 {
                    all_ok = false;
                }
            }
            self.properties_observed = all_ok;
        }
    }

    fn new_render_api(
        buffer: BufferSettings,
        hwdec: HwdecMode,
        codec_pref: VideoCodecPreference,
        ctx: &egui::Context,
        gl: Arc<glow::Context>,
    ) -> Result<Self, String> {
        let library = Self::load_library()?;
        unsafe {
            let api = MpvApi::load(&library)?;
            let handle = (api.create)();
            if handle.is_null() {
                return Err("Não foi possível iniciar o libmpv.".into());
            }
            let window_hidden = Arc::new(AtomicBool::new(false));
            let parked = Arc::new(AtomicBool::new(false));
            let mut player = Self {
                handle,
                window: None,
                render: None,
                audio_only: AtomicBool::new(false),
                window_hidden: Arc::clone(&window_hidden),
                video_suspended: AtomicBool::new(false),
                parked: Arc::clone(&parked),
                park_state: Mutex::new(WarmParkState::default()),
                active_occurrence_id: AtomicU64::new(0),
                wakeup_pending: Arc::new(AtomicBool::new(false)),
                wakeup_callback_data: None,
                snapshot: Mutex::new(PlayerSnapshot::default()),
                properties_observed: false,
                command: api.command,
                terminate: api.terminate,
                get_property: api.get_property,
                get_property_typed: api.get_property_typed,
                set_property: api.set_property,
                set_wakeup_callback: api.set_wakeup_callback,
                free: api.free,
                wait_event: api.wait_event,
                error_string: api.error_string,
                _library: library,
            };
            let options = Self::options_with_codec_pref(buffer, hwdec, codec_pref, None, true);
            for (name, value) in options {
                let option_name = CString::new(name).unwrap();
                let value = CString::new(value).unwrap();
                let result = (api.set_option)(handle, option_name.as_ptr(), value.as_ptr());
                if result < 0 {
                    let detail = player.describe_error(result);
                    return Err(format!("Não foi possível configurar {name}: {detail}"));
                }
            }
            for (name, value) in Self::tuning_options() {
                if let (Ok(option_name), Ok(value)) = (CString::new(name), CString::new(value)) {
                    let _ = (api.set_option)(handle, option_name.as_ptr(), value.as_ptr());
                }
            }
            let result = (api.initialize)(handle);
            if result < 0 {
                let detail = player.describe_error(result);
                return Err(format!("Não foi possível inicializar o libmpv: {detail}"));
            }
            for event_id in [
                MPV_EVENT_START_FILE,
                MPV_EVENT_END_FILE,
                MPV_EVENT_FILE_LOADED,
                MPV_EVENT_PROPERTY_CHANGE,
            ] {
                let event_result = (api.request_event)(handle, event_id, 1);
                if event_result < 0 {
                    let detail = player.describe_error(event_result);
                    return Err(format!(
                        "Não foi possível habilitar a correlação de eventos do libmpv: {detail}"
                    ));
                }
            }
            player.register_observers_and_wakeup(&api, ctx);
            let render_context =
                create_render_context(&api, handle, ctx, Arc::clone(&window_hidden))?;
            player.render = Some(Arc::new(RenderShared {
                gl,
                context: render_context,
                surface: Mutex::new(None),
                needs_render: AtomicBool::new(true),
                window_hidden,
                parked,
                failed: AtomicBool::new(false),
            }));
            Ok(player)
        }
    }

    fn new_embedded(
        parent: HWND,
        buffer: BufferSettings,
        hwdec: HwdecMode,
        codec_pref: VideoCodecPreference,
        ctx: &egui::Context,
    ) -> Result<Self, String> {
        let library = Self::load_library()?;
        unsafe {
            let api = MpvApi::load(&library)?;
            let window = VideoWindow::new(parent)?;
            let wid = window.hwnd() as isize;
            let handle = (api.create)();
            if handle.is_null() {
                return Err("Não foi possível iniciar o libmpv.".into());
            }
            let mut player = Self {
                handle,
                window: Some(window),
                render: None,
                audio_only: AtomicBool::new(false),
                window_hidden: Arc::new(AtomicBool::new(false)),
                video_suspended: AtomicBool::new(false),
                parked: Arc::new(AtomicBool::new(false)),
                park_state: Mutex::new(WarmParkState::default()),
                active_occurrence_id: AtomicU64::new(0),
                wakeup_pending: Arc::new(AtomicBool::new(false)),
                wakeup_callback_data: None,
                snapshot: Mutex::new(PlayerSnapshot::default()),
                properties_observed: false,
                command: api.command,
                terminate: api.terminate,
                get_property: api.get_property,
                get_property_typed: api.get_property_typed,
                set_property: api.set_property,
                set_wakeup_callback: api.set_wakeup_callback,
                free: api.free,
                wait_event: api.wait_event,
                error_string: api.error_string,
                _library: library,
            };
            let options =
                Self::options_with_codec_pref(buffer, hwdec, codec_pref, Some(wid), false);
            for (name, value) in options {
                let option_name = CString::new(name).unwrap();
                let value = CString::new(value).unwrap();
                let result = (api.set_option)(handle, option_name.as_ptr(), value.as_ptr());
                if result < 0 {
                    let detail = player.describe_error(result);
                    return Err(format!("Não foi possível configurar {name}: {detail}"));
                }
            }
            for (name, value) in Self::tuning_options() {
                if let (Ok(option_name), Ok(value)) = (CString::new(name), CString::new(value)) {
                    let _ = (api.set_option)(handle, option_name.as_ptr(), value.as_ptr());
                }
            }
            let result = (api.initialize)(handle);
            if result < 0 {
                let detail = player.describe_error(result);
                return Err(format!("Não foi possível inicializar o libmpv: {detail}"));
            }
            for event_id in [
                MPV_EVENT_START_FILE,
                MPV_EVENT_END_FILE,
                MPV_EVENT_FILE_LOADED,
                MPV_EVENT_PROPERTY_CHANGE,
            ] {
                let event_result = (api.request_event)(handle, event_id, 1);
                if event_result < 0 {
                    let detail = player.describe_error(event_result);
                    return Err(format!(
                        "Não foi possível habilitar a correlação de eventos do libmpv: {detail}"
                    ));
                }
            }
            player.register_observers_and_wakeup(&api, ctx);
            player.resize(Rect::NOTHING, 1.0);
            Ok(player)
        }
    }

    #[cfg(test)]
    fn options(
        buffer: BufferSettings,
        hwdec: HwdecMode,
        wid: Option<isize>,
        render_api: bool,
    ) -> Vec<(&'static str, String)> {
        Self::options_with_codec_pref(buffer, hwdec, VideoCodecPreference::Auto, wid, render_api)
    }

    fn options_with_codec_pref(
        buffer: BufferSettings,
        hwdec: HwdecMode,
        codec_pref: VideoCodecPreference,
        wid: Option<isize>,
        render_api: bool,
    ) -> Vec<(&'static str, String)> {
        let mut options = Vec::new();
        if let Some(wid) = wid {
            options.push(("wid", format!("{wid}")));
        }
        if render_api {
            options.push(("vo", "libmpv".to_owned()));
        }
        for &(name, value) in DISABLED_BUILTIN_SCRIPT_OPTIONS {
            options.push((name, value.to_owned()));
        }
        options.extend([
            ("terminal", "no".to_owned()),
            (
                "hwdec",
                effective_hwdec_option(hwdec, render_api).to_owned(),
            ),
            ("demuxer-max-bytes", format!("{}MiB", buffer.max_mib)),
            ("demuxer-max-back-bytes", format!("{}MiB", buffer.back_mib)),
            ("cache-secs", format!("{}", buffer.cache_seconds)),
            ("volume-max", "200".to_owned()),
            ("screenshot-format", "png".to_owned()),
            ("ytdl-format", ytdl_format_for_codec_pref(None, codec_pref)),
        ]);
        let yt_dlp_override = crate::updater::managed_yt_dlp_path();
        if let Some(yt_dlp) = yt_dlp_override {
            options.push(("script-opts", crate::search::ytdl_hook_path_option(&yt_dlp)));
        }
        options
    }

    fn tuning_options() -> Vec<(&'static str, String)> {
        vec![
            ("hwdec-codecs", "all".to_owned()),
            ("cache-pause-initial", "yes".to_owned()),
            ("cache-pause-wait", "1".to_owned()),
            ("network-timeout", "15".to_owned()),
            ("ytdl-raw-options", crate::search::ytdl_hook_raw_options()),
        ]
    }

    pub fn play(&mut self, url: &str) -> Result<(), String> {
        self.wake_from_park();
        self.active_occurrence_id.store(0, Ordering::Release);
        self.run(&["loadfile", url, "replace"])
    }

    /// Plays a local offline media file directly via libmpv (`ytdl=no`) with optional start offset.
    #[allow(dead_code)]
    pub(crate) fn play_local_file(
        &self,
        path: &std::path::Path,
        start_secs: Option<f64>,
    ) -> Result<(), String> {
        let path_str = path
            .to_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "Caminho de arquivo local inválido.".to_owned())?;
        self.wake_from_park();
        let loadfile_opts = build_local_file_loadfile_options(start_secs);
        self.active_occurrence_id.store(0, Ordering::Release);
        self.run(&["loadfile", path_str, "replace", "-1", &loadfile_opts])
    }

    /// Starts direct-URL (`ytdl=no`) playback using pre-extracted video/audio URLs,
    /// waking from warm park and preserving `QueuePlaybackCorrelation` occurrence tagging.
    #[allow(dead_code)]
    pub(crate) fn play_direct_stream(
        &self,
        opts: &DirectStreamOptions<'_>,
        occurrence_id: Option<u64>,
    ) -> Result<(), String> {
        let video_url = opts.video_url.trim();
        if video_url.is_empty() {
            return Err("URL de stream direta vazia.".into());
        }
        self.wake_from_park();
        let loadfile_opts = build_direct_stream_loadfile_options(opts);
        self.run(&["loadfile", video_url, "replace", "-1", &loadfile_opts])?;
        self.active_occurrence_id
            .store(occurrence_id.unwrap_or(0), Ordering::Release);
        Ok(())
    }

    /// Switches video quality or audio dubbing track via direct URLs (`ytdl=no`) without
    /// re-running `yt-dlp`. Preserves the current playback position when `opts.start_secs` is `None`.
    #[allow(dead_code)]
    pub(crate) fn switch_direct_stream(
        &self,
        opts: &DirectStreamOptions<'_>,
    ) -> Result<(), String> {
        let video_url = opts.video_url.trim();
        if video_url.is_empty() {
            return Err("URL de stream direta vazia.".into());
        }
        self.wake_from_park();
        let preserved_pos = opts.start_secs.or_else(|| self.position());
        let effective = DirectStreamOptions {
            video_url,
            audio_url: opts.audio_url,
            start_secs: preserved_pos,
            audio_only: opts.audio_only,
            user_agent: opts.user_agent,
            referer: opts.referer,
        };
        let loadfile_opts = build_direct_stream_loadfile_options(&effective);
        self.run(&["loadfile", video_url, "replace", "-1", &loadfile_opts])
    }

    #[allow(dead_code)]
    pub(crate) fn active_occurrence_id(&self) -> Option<u64> {
        let raw = self.active_occurrence_id.load(Ordering::Acquire);
        (raw != 0).then_some(raw)
    }

    /// Parks the player in a warm idle state across video closes, stopping playback
    /// and releasing GL FBO/texture VRAM without tearing down the libmpv core.
    #[allow(dead_code)]
    pub(crate) fn park_warm(&self) {
        let _ = self.run(&["stop"]);
        let _ = self.set("pause", "yes");
        self.release_gl_surface();
        self.active_occurrence_id.store(0, Ordering::Release);
        self.parked.store(true, Ordering::Release);
        if let Ok(mut state) = self.park_state.lock() {
            state.park_at(Instant::now());
        }
        if let Ok(mut snap) = self.snapshot.lock() {
            snap.reset_playback_fields();
        }
    }

    #[allow(dead_code)]
    pub(crate) fn wake_from_park(&self) {
        let was_parked = self.parked.swap(false, Ordering::AcqRel);
        if let Ok(mut state) = self.park_state.lock() {
            state.wake();
        }
        if was_parked && let Some(render) = &self.render {
            render.needs_render.store(true, Ordering::Relaxed);
        }
    }

    #[allow(dead_code)]
    pub(crate) fn is_parked(&self) -> bool {
        self.parked.load(Ordering::Acquire)
    }

    #[allow(dead_code)]
    pub(crate) fn parked_duration(&self) -> Option<Duration> {
        self.park_state
            .lock()
            .ok()
            .and_then(|state| state.parked_duration_at(Instant::now()))
    }

    #[allow(dead_code)]
    pub(crate) fn should_teardown_idle(&self, timeout: Duration) -> bool {
        self.park_state
            .lock()
            .ok()
            .is_some_and(|state| state.is_idle_expired_at(Instant::now(), timeout))
    }

    #[allow(dead_code)]
    pub(crate) fn snapshot(&self) -> PlayerSnapshot {
        self.snapshot
            .lock()
            .map(|snap| snap.clone())
            .unwrap_or_default()
    }

    pub fn set_audio_only(&self, audio_only: bool) -> Result<(), String> {
        self.audio_only.store(audio_only, Ordering::Relaxed);
        let next_suspended = audio_only || self.window_hidden.load(Ordering::Relaxed);
        let result = self.set("vid", if next_suspended { "no" } else { "auto" });
        if result.is_ok() {
            self.video_suspended
                .store(next_suspended, Ordering::Relaxed);
        }
        result
    }

    /// Suspends or resumes video decoding and GL surface allocation when the host
    /// window is minimized or hidden to the system tray. No-op when unchanged.
    pub(crate) fn set_window_hidden(&self, hidden: bool) {
        let was_hidden = self.window_hidden.swap(hidden, Ordering::Relaxed);
        let audio_only = self.audio_only.load(Ordering::Relaxed);
        let was_suspended = self.video_suspended.load(Ordering::Relaxed);
        let Some((next_suspended, transition)) =
            compute_window_hidden_transition(was_hidden, hidden, audio_only, was_suspended)
        else {
            return;
        };
        if let Some(vid_value) = transition.vid_property
            && self.set("vid", vid_value).is_ok()
        {
            self.video_suspended
                .store(next_suspended, Ordering::Relaxed);
        }
        if let Some(render) = &self.render {
            if transition.release_gl_surface {
                render.release_surface();
            } else if transition.mark_needs_render {
                render.needs_render.store(true, Ordering::Relaxed);
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn is_window_hidden(&self) -> bool {
        self.window_hidden.load(Ordering::Relaxed)
    }

    #[allow(dead_code)]
    pub(crate) fn release_gl_surface(&self) {
        if let Some(render) = &self.render {
            render.release_surface();
        }
    }

    #[allow(dead_code)]
    pub fn position(&self) -> Option<f64> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.time_pos.filter(|&pos| pos.is_finite() && pos >= 0.0);
        }
        self.double_cstr_ffi(c"time-pos")
            .filter(|&pos| pos.is_finite() && pos >= 0.0)
    }

    #[allow(dead_code)]
    pub fn duration(&self) -> Option<f64> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.duration.filter(|&dur| dur.is_finite() && dur > 0.0);
        }
        self.double_cstr_ffi(c"duration")
            .filter(|&dur| dur.is_finite() && dur > 0.0)
    }

    #[allow(dead_code)]
    pub fn is_paused(&self) -> bool {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.pause.unwrap_or(false);
        }
        self.flag_cstr_ffi(c"pause").unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn is_eof(&self) -> bool {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.eof_reached.unwrap_or(false);
        }
        self.flag_cstr_ffi(c"eof-reached").unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn is_core_idle(&self) -> bool {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.core_idle.unwrap_or(false);
        }
        self.flag_cstr_ffi(c"core-idle").unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn is_buffering(&self) -> bool {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.is_buffering();
        }
        self.flag_cstr_ffi(c"paused-for-cache").unwrap_or(false)
            || self
                .int_cstr_ffi(c"cache-buffering-state")
                .is_some_and(|pct| (0..100).contains(&pct))
    }

    #[allow(dead_code)]
    pub fn cache_buffering_state(&self) -> Option<i64> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.cache_buffering_state;
        }
        self.int_cstr_ffi(c"cache-buffering-state")
    }

    pub fn buffered_duration_secs(&self) -> Option<f64> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap
                .demuxer_cache_duration
                .filter(|&duration| duration.is_finite() && duration >= 0.0);
        }
        self.double_cstr_ffi(c"demuxer-cache-duration")
            .filter(|&duration| duration.is_finite() && duration >= 0.0)
    }

    #[allow(dead_code)]
    pub fn volume(&self) -> Option<f64> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.volume;
        }
        self.double_cstr_ffi(c"volume")
    }

    #[allow(dead_code)]
    pub fn is_muted(&self) -> bool {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.mute.unwrap_or(false);
        }
        self.flag_cstr_ffi(c"mute").unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn speed(&self) -> f64 {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap
                .speed
                .filter(|&s| s.is_finite() && s > 0.0)
                .unwrap_or(1.0);
        }
        self.double_cstr_ffi(c"speed")
            .filter(|&s| s.is_finite() && s > 0.0)
            .unwrap_or(1.0)
    }

    #[allow(dead_code)]
    pub fn video_dimensions(&self) -> Option<(i64, i64)> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return match (snap.video_width, snap.video_height) {
                (Some(w), Some(h)) if w > 0 && h > 0 => Some((w, h)),
                _ => None,
            };
        }
        let w = self.int_cstr_ffi(c"video-params/w")?;
        let h = self.int_cstr_ffi(c"video-params/h")?;
        (w > 0 && h > 0).then_some((w, h))
    }

    #[allow(dead_code)]
    pub fn hwdec_current(&self) -> Option<String> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.hwdec_current.clone();
        }
        self.property_cstr_ffi(c"hwdec-current")
    }

    #[allow(dead_code)]
    pub fn aid(&self) -> Option<String> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.aid.clone();
        }
        self.property_cstr_ffi(c"aid")
    }

    #[allow(dead_code)]
    pub fn sid(&self) -> Option<String> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.sid.clone();
        }
        self.property_cstr_ffi(c"sid")
    }

    #[allow(dead_code)]
    pub fn vid(&self) -> Option<String> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.vid.clone();
        }
        self.property_cstr_ffi(c"vid")
    }

    #[allow(dead_code)]
    pub fn media_title(&self) -> Option<String> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.media_title.clone();
        }
        self.property_cstr_ffi(c"media-title")
    }

    #[allow(dead_code)]
    pub fn path(&self) -> Option<String> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.path.clone();
        }
        self.property_cstr_ffi(c"path")
    }

    #[allow(dead_code)]
    pub fn is_seekable(&self) -> Option<bool> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.seekable;
        }
        self.flag_cstr_ffi(c"seekable")
    }

    #[allow(dead_code)]
    pub fn sub_visibility(&self) -> Option<bool> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
        {
            return snap.sub_visibility;
        }
        self.flag_cstr_ffi(c"sub-visibility")
    }

    pub fn screenshot_to_file(&self, path: &std::path::Path) -> Result<(), String> {
        let path_str = path
            .to_str()
            .ok_or_else(|| "Caminho de captura inválido.".to_owned())?;
        let _ = self.set("screenshot-format", "png");
        self.run(&["screenshot-to-file", path_str])
    }

    pub fn frame_step(&self) -> Result<(), String> {
        self.run(&["frame-step"])
    }

    pub fn frame_back_step(&self) -> Result<(), String> {
        self.run(&["frame-back-step"])
    }

    pub fn add_subtitle(
        &self,
        path_or_url: &str,
        title: &str,
        language: &str,
    ) -> Result<(), String> {
        if let Some(existing_id) = self.find_subtitle_track_by_path(path_or_url) {
            self.set("sid", &existing_id)?;
        } else {
            self.run(&["sub-add", path_or_url, "select", title, language])?;
        }
        self.set("sub-visibility", "yes")
    }

    pub fn disable_subtitles(&self) -> Result<(), String> {
        self.set("sid", "no")
    }

    pub fn audio_tracks(&self) -> Vec<AudioTrackOption> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
            && snap.track_list_observed
        {
            return snap
                .audio_tracks
                .iter()
                .map(AudioTrackInfo::to_option)
                .collect();
        }
        self.audio_tracks_ffi()
    }

    #[allow(dead_code)]
    pub fn audio_track_infos(&self) -> Vec<AudioTrackInfo> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
            && snap.track_list_observed
        {
            return snap.audio_tracks.clone();
        }
        let count = self.number("track-list/count").unwrap_or(0.0).max(0.0) as usize;
        let mut tracks = Vec::new();
        for index in 0..count {
            if self
                .property(&format!("track-list/{index}/type"))
                .as_deref()
                != Some("audio")
            {
                continue;
            }
            let Some(id) = self.property(&format!("track-list/{index}/id")) else {
                continue;
            };
            tracks.push(AudioTrackInfo {
                id,
                title: self
                    .property(&format!("track-list/{index}/title"))
                    .unwrap_or_default(),
                lang: self
                    .property(&format!("track-list/{index}/lang"))
                    .unwrap_or_default(),
                codec: self
                    .property(&format!("track-list/{index}/codec"))
                    .unwrap_or_default(),
                selected: self
                    .property(&format!("track-list/{index}/selected"))
                    .as_deref()
                    == Some("yes"),
            });
        }
        tracks
    }

    fn audio_tracks_ffi(&self) -> Vec<AudioTrackOption> {
        let count = self.number("track-list/count").unwrap_or(0.0).max(0.0) as usize;
        let mut tracks = Vec::new();
        for index in 0..count {
            if self
                .property(&format!("track-list/{index}/type"))
                .as_deref()
                != Some("audio")
            {
                continue;
            }
            let Some(id) = self.property(&format!("track-list/{index}/id")) else {
                continue;
            };
            let title = self
                .property(&format!("track-list/{index}/title"))
                .unwrap_or_default();
            let lang = self
                .property(&format!("track-list/{index}/lang"))
                .unwrap_or_default();
            let codec = self
                .property(&format!("track-list/{index}/codec"))
                .unwrap_or_default();
            let selected = self
                .property(&format!("track-list/{index}/selected"))
                .as_deref()
                == Some("yes");
            let label = format_audio_track_label(&id, &title, &lang, &codec);
            tracks.push(AudioTrackOption {
                id,
                label,
                selected,
            });
        }
        tracks
    }

    #[allow(dead_code)]
    pub fn subtitle_tracks(&self) -> Vec<SubtitleTrackInfo> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
            && snap.track_list_observed
        {
            return snap.subtitle_tracks.clone();
        }
        let count = self.number("track-list/count").unwrap_or(0.0).max(0.0) as usize;
        let mut tracks = Vec::new();
        for index in 0..count {
            if self
                .property(&format!("track-list/{index}/type"))
                .as_deref()
                != Some("sub")
            {
                continue;
            }
            let Some(id) = self.property(&format!("track-list/{index}/id")) else {
                continue;
            };
            tracks.push(SubtitleTrackInfo {
                id,
                title: self
                    .property(&format!("track-list/{index}/title"))
                    .unwrap_or_default(),
                lang: self
                    .property(&format!("track-list/{index}/lang"))
                    .unwrap_or_default(),
                codec: self
                    .property(&format!("track-list/{index}/codec"))
                    .unwrap_or_default(),
                external: self
                    .property(&format!("track-list/{index}/external"))
                    .as_deref()
                    == Some("yes"),
                external_filename: self.property(&format!("track-list/{index}/external-filename")),
                selected: self
                    .property(&format!("track-list/{index}/selected"))
                    .as_deref()
                    == Some("yes"),
            });
        }
        tracks
    }

    pub fn set_audio_track(&self, id: &str) -> Result<(), String> {
        self.set("aid", id)
    }

    #[allow(dead_code)]
    pub(crate) fn set_volume(&self, percent: f64) -> Result<(), String> {
        let clamped = clamp_volume_percent(percent);
        let formatted = if (clamped - clamped.round()).abs() < 1e-4 {
            format!("{}", clamped.round() as i32)
        } else {
            format!("{clamped:.1}")
        };
        self.set("volume", &formatted)
    }

    #[allow(dead_code)]
    pub(crate) fn set_ultrawide_zoom(
        &self,
        mode: UltrawideZoomMode,
        custom_zoom: f32,
    ) -> Result<(), String> {
        for (prop, val) in ultrawide_zoom_mpv_properties(mode, custom_zoom) {
            self.set(prop, &val)?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn apply_caption_style(&self, style: &CaptionStylePrefs) -> Result<(), String> {
        for (prop, val) in caption_style_mpv_properties(style) {
            self.set(prop, &val)?;
        }
        Ok(())
    }

    pub fn set_video_eq(
        &self,
        brightness: i32,
        contrast: i32,
        saturation: i32,
    ) -> Result<(), String> {
        self.set("brightness", &brightness.clamp(-50, 50).to_string())?;
        self.set("contrast", &contrast.clamp(-50, 50).to_string())?;
        self.set("saturation", &saturation.clamp(-50, 50).to_string())
    }

    pub fn apply_audio_filters(&self, normalize: bool, eq: AudioEqPreset) -> Result<(), String> {
        let mut filters: Vec<&str> = Vec::new();
        if normalize {
            filters.push("lavfi=[loudnorm=I=-16:TP=-1.5:LRA=11]");
        }
        match eq {
            AudioEqPreset::Flat => {}
            AudioEqPreset::Voice => {
                filters.push("lavfi=[equalizer=f=120:width_type=h:width=100:g=-4,equalizer=f=2200:width_type=h:width=1200:g=4]");
            }
            AudioEqPreset::BassBoost => {
                filters.push("lavfi=[equalizer=f=90:width_type=h:width=80:g=5]");
            }
            AudioEqPreset::Night => {
                filters.push("lavfi=[acompressor=threshold=-20dB:ratio=4:attack=10:release=150]");
            }
        }
        self.set("af", &filters.join(","))
    }

    fn find_subtitle_track_by_path(&self, path_or_url: &str) -> Option<String> {
        let target = std::path::Path::new(path_or_url);
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
            && snap.track_list_observed
        {
            return snap.subtitle_tracks.iter().find_map(|track| {
                let external = track.external_filename.as_deref()?;
                (external == path_or_url || std::path::Path::new(external) == target)
                    .then(|| track.id.clone())
            });
        }
        let count = self.number("track-list/count")? as usize;
        for index in 0..count {
            if self
                .property(&format!("track-list/{index}/type"))
                .as_deref()
                != Some("sub")
            {
                continue;
            }
            let Some(external) = self.property(&format!("track-list/{index}/external-filename"))
            else {
                continue;
            };
            if external == path_or_url || std::path::Path::new(&external) == target {
                return self.property(&format!("track-list/{index}/id"));
            }
        }
        None
    }

    fn run(&self, args: &[&str]) -> Result<(), String> {
        let strings: Vec<CString> = args
            .iter()
            .map(|arg| CString::new(*arg))
            .collect::<Result<_, _>>()
            .map_err(|error| error.to_string())?;
        let mut pointers: Vec<*const c_char> = strings.iter().map(|arg| arg.as_ptr()).collect();
        pointers.push(ptr::null());
        let result = unsafe { (self.command)(self.handle, pointers.as_ptr()) };
        if result < 0 {
            Err(self.describe_error(result))
        } else {
            Ok(())
        }
    }

    fn describe_error(&self, code: c_int) -> String {
        let text = unsafe { (self.error_string)(code) };
        if text.is_null() {
            return format!("Erro do player ({code})");
        }
        format!(
            "Erro do player: {}",
            unsafe { CStr::from_ptr(text) }.to_string_lossy()
        )
    }

    fn property_cstr_ffi(&self, name: &CStr) -> Option<String> {
        let value = unsafe { (self.get_property)(self.handle, name.as_ptr()) };
        if value.is_null() {
            return None;
        }
        let text = unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned();
        unsafe {
            (self.free)(value.cast());
        }
        Some(text)
    }

    pub fn property_cstr(&self, name: &CStr) -> Option<String> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
            && let Some(cached) = snap.lookup_string_cstr(name)
        {
            return cached;
        }
        self.property_cstr_ffi(name)
    }

    pub fn property(&self, name: &str) -> Option<String> {
        let name = CString::new(name).ok()?;
        self.property_cstr(&name)
    }

    fn flag_cstr_ffi(&self, name: &CStr) -> Option<bool> {
        if let Some(get_typed) = self.get_property_typed {
            let mut value: c_int = 0;
            let result = unsafe {
                get_typed(
                    self.handle,
                    name.as_ptr(),
                    MPV_FORMAT_FLAG,
                    (&raw mut value).cast(),
                )
            };
            if result >= 0 {
                return Some(value != 0);
            }
            return None;
        }
        self.property_cstr_ffi(name).map(|text| text == "yes")
    }

    pub fn flag_cstr(&self, name: &CStr) -> Option<bool> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
            && let Some(cached) = snap.lookup_flag_cstr(name)
        {
            return cached;
        }
        self.flag_cstr_ffi(name)
    }

    fn int_cstr_ffi(&self, name: &CStr) -> Option<i64> {
        if let Some(get_typed) = self.get_property_typed {
            let mut value: i64 = 0;
            let result = unsafe {
                get_typed(
                    self.handle,
                    name.as_ptr(),
                    MPV_FORMAT_INT64,
                    (&raw mut value).cast(),
                )
            };
            if result >= 0 {
                return Some(value);
            }
            return None;
        }
        self.property_cstr_ffi(name)?.parse::<i64>().ok()
    }

    pub fn int_cstr(&self, name: &CStr) -> Option<i64> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
            && let Some(cached) = snap.lookup_int_cstr(name)
        {
            return cached;
        }
        self.int_cstr_ffi(name)
    }

    fn double_cstr_ffi(&self, name: &CStr) -> Option<f64> {
        if let Some(get_typed) = self.get_property_typed {
            let mut value: f64 = 0.0;
            let result = unsafe {
                get_typed(
                    self.handle,
                    name.as_ptr(),
                    MPV_FORMAT_DOUBLE,
                    (&raw mut value).cast(),
                )
            };
            if result >= 0 && value.is_finite() {
                return Some(value);
            }
            return None;
        }
        self.property_cstr_ffi(name)?
            .parse()
            .ok()
            .filter(|value: &f64| value.is_finite())
    }

    pub fn double_cstr(&self, name: &CStr) -> Option<f64> {
        if let Ok(snap) = self.snapshot.lock()
            && snap.observed
            && let Some(cached) = snap.lookup_double_cstr(name)
        {
            return cached;
        }
        self.double_cstr_ffi(name)
    }

    pub fn number(&self, name: &str) -> Option<f64> {
        let name = CString::new(name).ok()?;
        self.double_cstr(&name)
    }

    pub fn set(&self, name: &str, value: &str) -> Result<(), String> {
        let cname = CString::new(name).map_err(|error| error.to_string())?;
        let cvalue = CString::new(value).map_err(|error| error.to_string())?;
        let result = unsafe { (self.set_property)(self.handle, cname.as_ptr(), cvalue.as_ptr()) };
        if result < 0 {
            Err(self.describe_error(result))
        } else {
            if let Ok(mut snap) = self.snapshot.lock() {
                snap.apply_optimistic_set(name, value);
            }
            Ok(())
        }
    }

    pub fn seek(&self, seconds: f64, mode: &str) -> Result<(), String> {
        if !seconds.is_finite() {
            return Err("A posição de reprodução deve ser um número finito.".into());
        }
        self.run(&["seek", &format!("{seconds:.3}"), mode])
    }

    pub fn quality(
        &self,
        url: &str,
        height: Option<u32>,
        default_format: &str,
        preserve_position: bool,
    ) -> Result<(), String> {
        self.wake_from_park();
        let position = if preserve_position {
            self.position()
        } else {
            None
        };
        let format = match height {
            Some(height) => {
                ytdl_format_for_exact_height_and_codec(height, VideoCodecPreference::Auto)
            }
            None => default_format.to_owned(),
        };
        let previous = self.property("options/ytdl-format");
        self.set("options/ytdl-format", &format)?;
        let result = if let Some(position) = position {
            self.run(&[
                "loadfile",
                url,
                "replace",
                "-1",
                &format!("start={position:.3}"),
            ])
        } else {
            self.run(&["loadfile", url, "replace"])
        };
        if result.is_err()
            && let Some(previous) = previous
        {
            let _ = self.set("options/ytdl-format", &previous);
        }
        result
    }

    pub fn poll_events(&self) -> (Vec<crate::queue::PlaybackEvent>, Option<String>) {
        // Clear pending flag before draining so events arriving during/after drain schedule a wakeup.
        self.wakeup_pending.store(false, Ordering::Release);
        let mut events = Vec::new();
        let mut error = None;
        while let Some(event) = unsafe { (self.wait_event)(self.handle, 0.0).as_ref() } {
            match event.event_id {
                MPV_EVENT_NONE => break,
                MPV_EVENT_START_FILE if !event.data.is_null() => {
                    let start = unsafe { &*(event.data as *const StartFile) };
                    if let Ok(mut snap) = self.snapshot.lock() {
                        snap.on_start_file(start.playlist_entry_id);
                    }
                    events.push(crate::queue::PlaybackEvent::StartFile {
                        playlist_entry_id: start.playlist_entry_id,
                    });
                }
                MPV_EVENT_END_FILE if !event.data.is_null() => {
                    let end = unsafe { &*(event.data as *const EndFile) };
                    if let Ok(mut snap) = self.snapshot.lock() {
                        snap.on_end_file(end.reason);
                    }
                    let failed = end.reason == MPV_END_FILE_REASON_ERROR;
                    if failed && error.is_none() {
                        error = Some(self.describe_error(end.error));
                    }
                    events.push(crate::queue::PlaybackEvent::EndFile {
                        playlist_entry_id: end.playlist_entry_id,
                        failed,
                    });
                }
                MPV_EVENT_FILE_LOADED => {
                    if let Ok(mut snap) = self.snapshot.lock() {
                        snap.on_file_loaded();
                    }
                    events.push(crate::queue::PlaybackEvent::FileLoaded);
                }
                MPV_EVENT_PROPERTY_CHANGE if !event.data.is_null() => {
                    let prop = unsafe { &*(event.data as *const MpvEventProperty) };
                    if let Ok(mut snap) = self.snapshot.lock() {
                        unsafe {
                            snap.apply_property_change(event.reply_userdata, prop);
                        }
                    }
                }
                _ => {}
            }
        }
        if self.properties_observed
            && let Ok(mut snap) = self.snapshot.lock()
        {
            snap.observed = true;
        }
        (events, error)
    }

    pub fn set_visible(&mut self, visible: bool) {
        if let Some(window) = &mut self.window {
            window.set_visible(visible);
        }
    }

    pub fn resize(&mut self, rect: Rect, scale: f32) {
        if let Some(window) = &mut self.window {
            window.resize(rect, scale);
        }
    }

    pub fn is_render_api(&self) -> bool {
        self.render
            .as_ref()
            .is_some_and(|shared| !shared.failed.load(Ordering::Relaxed))
    }

    /// Returns `false` in native `HWND` fallback mode where `egui` cannot draw over video.
    pub fn paint_into(&self, painter: &egui::Painter, rect: Rect) -> bool {
        let Some(shared) = &self.render else {
            return false;
        };
        if shared.failed.load(Ordering::Relaxed) {
            return false;
        }
        let shared = Arc::downgrade(shared);
        let callback = egui_glow::CallbackFn::new(move |info, painter| {
            if let Some(shared) = shared.upgrade() {
                shared.render(info, painter);
            }
        });
        painter.add(egui::PaintCallback {
            rect,
            callback: Arc::new(callback),
        });
        true
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Some(window) = &mut self.window {
            window.set_visible(false);
        }
        if let Some(set_wakeup) = self.set_wakeup_callback {
            unsafe {
                set_wakeup(self.handle, None, ptr::null_mut());
            }
        }
        self.render = None;
        unsafe {
            (self.terminate)(self.handle);
        }
        let _ = &self.wakeup_callback_data;
    }
}

unsafe fn create_render_context(
    api: &MpvApi,
    handle: *mut c_void,
    ctx: &egui::Context,
    window_hidden: Arc<AtomicBool>,
) -> Result<RenderContext, String> {
    unsafe {
        let mut init = MpvOpenglInitParams {
            get_proc_address: Some(mpv_get_proc_address),
            get_proc_address_ctx: ptr::null_mut(),
        };
        let mut params = [
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_API_TYPE,
                data: MPV_RENDER_API_TYPE_OPENGL.as_ptr().cast_mut().cast(),
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: (&mut init as *mut MpvOpenglInitParams).cast(),
            },
            MpvRenderParam {
                type_: 0,
                data: ptr::null_mut(),
            },
        ];
        let mut render_handle: *mut c_void = ptr::null_mut();
        let result = (api.render_create)(&mut render_handle, handle, params.as_mut_ptr());
        if result < 0 || render_handle.is_null() {
            return Err(api.describe_error(result));
        }
        let update_callback_data = Box::new(RenderUpdateCallbackData {
            ctx: ctx.clone(),
            window_hidden,
        });
        (api.render_set_update_callback)(
            render_handle,
            Some(mpv_render_update_callback),
            (&*update_callback_data as *const RenderUpdateCallbackData)
                .cast_mut()
                .cast(),
        );
        Ok(RenderContext {
            handle: render_handle,
            render: api.render_render,
            update: api.render_update,
            set_update_callback: api.render_set_update_callback,
            free: api.render_free,
            update_callback_data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BufferProfile;
    use eframe::egui::Pos2;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GWL_STYLE, GetClientRect, GetWindowLongW};

    #[test]
    fn video_window_hides_and_restores_hwnd_preserving_latest_bounds() {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let parent = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                ptr::null(),
                0,
                0,
                0,
                800,
                600,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        assert!(!parent.is_null());

        let mut video_window = VideoWindow::new(parent).expect("child window should be created");
        assert!(video_window.is_visible());
        let style_initial = unsafe { GetWindowLongW(video_window.hwnd(), GWL_STYLE) as u32 };
        assert_ne!(style_initial & WS_VISIBLE, 0);

        let initial_rect = Rect::from_min_max(Pos2::new(16.0, 48.0), Pos2::new(656.0, 408.0));
        video_window.resize(initial_rect, 1.0);

        video_window.set_visible(false);
        assert!(!video_window.is_visible());
        let style_hidden = unsafe { GetWindowLongW(video_window.hwnd(), GWL_STYLE) as u32 };
        assert_eq!(style_hidden & WS_VISIBLE, 0);

        // Bounds changed while hidden must still apply once the HWND is restored.
        let fullscreen_rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1280.0, 720.0));
        video_window.resize(fullscreen_rect, 1.5);
        let style_still_hidden = unsafe { GetWindowLongW(video_window.hwnd(), GWL_STYLE) as u32 };
        assert_eq!(style_still_hidden & WS_VISIBLE, 0);

        video_window.set_visible(true);
        assert!(video_window.is_visible());
        let style_restored = unsafe { GetWindowLongW(video_window.hwnd(), GWL_STYLE) as u32 };
        assert_ne!(style_restored & WS_VISIBLE, 0);

        let mut client = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let ok = unsafe { GetClientRect(video_window.hwnd(), &mut client) };
        assert_ne!(ok, 0);
        assert_eq!(client.right - client.left, 1920);
        assert_eq!(client.bottom - client.top, 1080);

        drop(video_window);
        unsafe {
            DestroyWindow(parent);
        }
    }

    #[test]
    fn render_api_options_use_libmpv_vo_and_embedded_options_use_wid() {
        let buffer = BufferProfile::Default.values();
        let render = Player::options(buffer, HwdecMode::AutoSafe, None, true);
        assert!(
            render
                .iter()
                .any(|(name, value)| *name == "vo" && value == "libmpv")
        );
        assert!(
            render
                .iter()
                .any(|(name, value)| *name == "hwdec" && value == WGL_HWDEC_CHAIN)
        );
        assert!(
            render
                .iter()
                .any(|(name, value)| *name == "screenshot-format" && value == "png")
        );
        assert!(!render.iter().any(|(name, _)| *name == "wid"));

        let embedded_auto = Player::options(buffer, HwdecMode::AutoSafe, Some(42), false);
        assert!(
            embedded_auto
                .iter()
                .any(|(name, value)| *name == "hwdec" && value == "auto-safe")
        );

        let embedded = Player::options(buffer, HwdecMode::Software, Some(42), false);
        assert!(
            embedded
                .iter()
                .any(|(name, value)| *name == "wid" && value == "42")
        );
        assert!(
            embedded
                .iter()
                .any(|(name, value)| *name == "hwdec" && value == "no")
        );
        assert!(
            embedded
                .iter()
                .any(|(name, value)| *name == "screenshot-format" && value == "png")
        );
        assert!(!embedded.iter().any(|(name, _)| *name == "vo"));
    }

    #[test]
    fn tuning_options_enable_fast_start_and_no_playlist_ytdl_hook() {
        let tuning = Player::tuning_options();
        assert!(
            tuning
                .iter()
                .any(|(name, value)| *name == "hwdec-codecs" && value == "all")
        );
        assert!(
            tuning
                .iter()
                .any(|(name, value)| *name == "cache-pause-initial" && value == "yes")
        );
        assert!(
            tuning
                .iter()
                .any(|(name, value)| *name == "cache-pause-wait" && value == "1")
        );
        assert!(
            tuning
                .iter()
                .any(|(name, value)| *name == "network-timeout" && value == "15")
        );
        assert!(
            tuning
                .iter()
                .any(|(name, value)| *name == "ytdl-raw-options"
                    && value.starts_with("no-playlist="))
        );
    }

    #[test]
    fn render_params_disable_block_for_target_time_and_terminate_with_invalid() {
        assert_eq!(MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME, 12);
        let mut fbo = MpvOpenglFbo {
            fbo: 7,
            width: 1280,
            height: 720,
            internal_format: 0,
        };
        let mut flip_y: c_int = 1;
        let mut block_for_target_time: c_int = 0;
        let params = build_render_params(&mut fbo, &mut flip_y, &mut block_for_target_time);

        assert_eq!(params[0].type_, MPV_RENDER_PARAM_OPENGL_FBO);
        assert!(!params[0].data.is_null());
        assert_eq!(params[1].type_, MPV_RENDER_PARAM_FLIP_Y);
        assert_eq!(unsafe { *(params[1].data as *const c_int) }, 1);
        assert_eq!(params[2].type_, MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME);
        assert_eq!(unsafe { *(params[2].data as *const c_int) }, 0);
        assert_eq!(params[3].type_, 0);
        assert!(params[3].data.is_null());
    }

    #[test]
    fn render_update_callback_gate_skips_repaint_when_window_hidden() {
        let hidden = Arc::new(AtomicBool::new(false));
        assert!(should_request_repaint_for_render_update(&hidden));

        hidden.store(true, Ordering::Relaxed);
        assert!(!should_request_repaint_for_render_update(&hidden));

        let mut callback_data = RenderUpdateCallbackData {
            ctx: egui::Context::default(),
            window_hidden: Arc::clone(&hidden),
        };
        unsafe {
            mpv_render_update_callback(ptr::null_mut());
            mpv_render_update_callback(
                (&mut callback_data as *mut RenderUpdateCallbackData).cast(),
            );
        }

        hidden.store(false, Ordering::Relaxed);
        assert!(should_request_repaint_for_render_update(&hidden));
        unsafe {
            mpv_render_update_callback(
                (&mut callback_data as *mut RenderUpdateCallbackData).cast(),
            );
        }
    }

    #[test]
    fn window_hidden_transition_deduplicates_and_respects_audio_only() {
        assert_eq!(
            compute_window_hidden_transition(false, false, false, false),
            None
        );
        assert_eq!(
            compute_window_hidden_transition(true, true, false, true),
            None
        );

        assert_eq!(
            compute_window_hidden_transition(false, true, false, false),
            Some((
                true,
                VideoSuspendTransition {
                    vid_property: Some("no"),
                    release_gl_surface: true,
                    mark_needs_render: false,
                },
            ))
        );

        assert_eq!(
            compute_window_hidden_transition(true, false, false, true),
            Some((
                false,
                VideoSuspendTransition {
                    vid_property: Some("auto"),
                    release_gl_surface: false,
                    mark_needs_render: true,
                },
            ))
        );

        // When audio-only is active, hiding releases the GL surface without redundant vid=no
        // and restoring keeps vid=no instead of turning video back on.
        assert_eq!(
            compute_window_hidden_transition(false, true, true, true),
            Some((
                true,
                VideoSuspendTransition {
                    vid_property: None,
                    release_gl_surface: true,
                    mark_needs_render: false,
                },
            ))
        );
        assert_eq!(
            compute_window_hidden_transition(true, false, true, true),
            Some((
                true,
                VideoSuspendTransition {
                    vid_property: None,
                    release_gl_surface: false,
                    mark_needs_render: true,
                },
            ))
        );
    }

    #[test]
    fn mpv_abi_struct_layouts_match_client_h() {
        assert_eq!(std::mem::size_of::<MpvEvent>(), 24);
        assert_eq!(std::mem::offset_of!(MpvEvent, event_id), 0);
        assert_eq!(std::mem::offset_of!(MpvEvent, error), 4);
        assert_eq!(std::mem::offset_of!(MpvEvent, reply_userdata), 8);
        assert_eq!(std::mem::offset_of!(MpvEvent, data), 16);

        assert_eq!(std::mem::size_of::<MpvEventProperty>(), 24);
        assert_eq!(std::mem::offset_of!(MpvEventProperty, name), 0);
        assert_eq!(std::mem::offset_of!(MpvEventProperty, format), 8);
        assert_eq!(std::mem::offset_of!(MpvEventProperty, data), 16);

        assert_eq!(std::mem::size_of::<MpvNode>(), 16);
        assert_eq!(std::mem::offset_of!(MpvNode, u), 0);
        assert_eq!(std::mem::offset_of!(MpvNode, format), 8);

        assert_eq!(std::mem::size_of::<MpvNodeList>(), 24);
        assert_eq!(std::mem::offset_of!(MpvNodeList, num), 0);
        assert_eq!(std::mem::offset_of!(MpvNodeList, values), 8);
        assert_eq!(std::mem::offset_of!(MpvNodeList, keys), 16);
    }

    #[test]
    fn player_snapshot_updates_from_synthetic_property_change_events() {
        let mut snap = PlayerSnapshot {
            observed: true,
            ..Default::default()
        };

        let time_pos = 42.5_f64;
        let duration = 300.0_f64;
        let pause_flag: c_int = 1;
        let eof_flag: c_int = 0;
        let core_idle_flag: c_int = 0;
        let paused_for_cache_flag: c_int = 1;
        let buffering_pct: i64 = 65;
        let cache_secs = 14.25_f64;
        let volume = 85.0_f64;
        let mute_flag: c_int = 1;
        let speed = 1.5_f64;
        let width: i64 = 1920;
        let height: i64 = 1080;
        let seekable_flag: c_int = 1;
        let sub_vis_flag: c_int = 1;

        let hwdec_c = CString::new("d3d11va-copy").unwrap();
        let hwdec_ptr = hwdec_c.as_ptr();
        let aid_c = CString::new("1").unwrap();
        let aid_ptr = aid_c.as_ptr();
        let sid_c = CString::new("2").unwrap();
        let sid_ptr = sid_c.as_ptr();
        let vid_c = CString::new("1").unwrap();
        let vid_ptr = vid_c.as_ptr();
        let title_c = CString::new("FastTube Demo").unwrap();
        let title_ptr = title_c.as_ptr();
        let path_c = CString::new("https://www.youtube.com/watch?v=dQw4w9WgXcQ").unwrap();
        let path_ptr = path_c.as_ptr();

        let events: &[(u64, &CStr, c_int, *const c_void)] = &[
            (
                ObservedProperty::TimePos as u64,
                c"time-pos",
                MPV_FORMAT_DOUBLE,
                (&raw const time_pos).cast(),
            ),
            (
                ObservedProperty::Duration as u64,
                c"duration",
                MPV_FORMAT_DOUBLE,
                (&raw const duration).cast(),
            ),
            (
                ObservedProperty::Pause as u64,
                c"pause",
                MPV_FORMAT_FLAG,
                (&raw const pause_flag).cast(),
            ),
            (
                ObservedProperty::EofReached as u64,
                c"eof-reached",
                MPV_FORMAT_FLAG,
                (&raw const eof_flag).cast(),
            ),
            (
                ObservedProperty::CoreIdle as u64,
                c"core-idle",
                MPV_FORMAT_FLAG,
                (&raw const core_idle_flag).cast(),
            ),
            (
                ObservedProperty::PausedForCache as u64,
                c"paused-for-cache",
                MPV_FORMAT_FLAG,
                (&raw const paused_for_cache_flag).cast(),
            ),
            (
                ObservedProperty::CacheBufferingState as u64,
                c"cache-buffering-state",
                MPV_FORMAT_INT64,
                (&raw const buffering_pct).cast(),
            ),
            (
                ObservedProperty::DemuxerCacheDuration as u64,
                c"demuxer-cache-duration",
                MPV_FORMAT_DOUBLE,
                (&raw const cache_secs).cast(),
            ),
            (
                ObservedProperty::Volume as u64,
                c"volume",
                MPV_FORMAT_DOUBLE,
                (&raw const volume).cast(),
            ),
            (
                ObservedProperty::Mute as u64,
                c"mute",
                MPV_FORMAT_FLAG,
                (&raw const mute_flag).cast(),
            ),
            (
                // Exercise name-based fallback when reply_userdata is 0.
                0,
                c"speed",
                MPV_FORMAT_DOUBLE,
                (&raw const speed).cast(),
            ),
            (
                ObservedProperty::VideoWidth as u64,
                c"video-params/w",
                MPV_FORMAT_INT64,
                (&raw const width).cast(),
            ),
            (
                ObservedProperty::VideoHeight as u64,
                c"video-params/h",
                MPV_FORMAT_INT64,
                (&raw const height).cast(),
            ),
            (
                ObservedProperty::HwdecCurrent as u64,
                c"hwdec-current",
                MPV_FORMAT_STRING,
                (&raw const hwdec_ptr).cast(),
            ),
            (
                ObservedProperty::Aid as u64,
                c"aid",
                MPV_FORMAT_STRING,
                (&raw const aid_ptr).cast(),
            ),
            (
                ObservedProperty::Sid as u64,
                c"sid",
                MPV_FORMAT_STRING,
                (&raw const sid_ptr).cast(),
            ),
            (
                ObservedProperty::Vid as u64,
                c"vid",
                MPV_FORMAT_STRING,
                (&raw const vid_ptr).cast(),
            ),
            (
                ObservedProperty::MediaTitle as u64,
                c"media-title",
                MPV_FORMAT_STRING,
                (&raw const title_ptr).cast(),
            ),
            (
                ObservedProperty::Path as u64,
                c"path",
                MPV_FORMAT_STRING,
                (&raw const path_ptr).cast(),
            ),
            (
                ObservedProperty::Seekable as u64,
                c"seekable",
                MPV_FORMAT_FLAG,
                (&raw const seekable_flag).cast(),
            ),
            (
                ObservedProperty::SubVisibility as u64,
                c"sub-visibility",
                MPV_FORMAT_FLAG,
                (&raw const sub_vis_flag).cast(),
            ),
        ];

        for &(userdata, name, format, data) in events {
            let prop = MpvEventProperty {
                name: name.as_ptr(),
                format,
                data,
            };
            unsafe {
                snap.apply_property_change(userdata, &prop);
            }
        }

        assert_eq!(snap.time_pos, Some(42.5));
        assert_eq!(snap.duration, Some(300.0));
        assert_eq!(snap.pause, Some(true));
        assert_eq!(snap.eof_reached, Some(false));
        assert_eq!(snap.core_idle, Some(false));
        assert_eq!(snap.paused_for_cache, Some(true));
        assert_eq!(snap.cache_buffering_state, Some(65));
        assert!(snap.is_buffering());
        assert_eq!(snap.demuxer_cache_duration, Some(14.25));
        assert_eq!(snap.volume, Some(85.0));
        assert_eq!(snap.mute, Some(true));
        assert_eq!(snap.speed, Some(1.5));
        assert_eq!(snap.video_width, Some(1920));
        assert_eq!(snap.video_height, Some(1080));
        assert_eq!(snap.hwdec_current.as_deref(), Some("d3d11va-copy"));
        assert_eq!(snap.aid.as_deref(), Some("1"));
        assert_eq!(snap.sid.as_deref(), Some("2"));
        assert_eq!(snap.vid.as_deref(), Some("1"));
        assert_eq!(snap.media_title.as_deref(), Some("FastTube Demo"));
        assert_eq!(
            snap.path.as_deref(),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
        );
        assert_eq!(snap.seekable, Some(true));
        assert_eq!(snap.sub_visibility, Some(true));

        // Verify cached CStr lookups avoid FFI and preserve exact mpv string semantics.
        assert_eq!(
            snap.lookup_string_cstr(c"pause"),
            Some(Some("yes".to_owned()))
        );
        assert_eq!(
            snap.lookup_string_cstr(c"hwdec-current"),
            Some(Some("d3d11va-copy".to_owned()))
        );
        assert_eq!(snap.lookup_string_cstr(c"unobserved-prop"), None);
        assert_eq!(snap.lookup_flag_cstr(c"mute"), Some(Some(true)));
        assert_eq!(snap.lookup_int_cstr(c"video-params/w"), Some(Some(1920)));
        assert_eq!(snap.lookup_double_cstr(c"time-pos"), Some(Some(42.5)));

        // Clearing via MPV_FORMAT_NONE resets the field to None.
        let clear_hwdec = MpvEventProperty {
            name: c"hwdec-current".as_ptr(),
            format: MPV_FORMAT_NONE,
            data: ptr::null(),
        };
        unsafe {
            snap.apply_property_change(ObservedProperty::HwdecCurrent as u64, &clear_hwdec);
        }
        assert_eq!(snap.hwdec_current, None);

        // Lifecycle transitions & optimistic set updates.
        snap.on_start_file(77);
        assert_eq!(snap.active_playlist_entry_id, Some(77));
        assert!(!snap.file_loaded);
        assert_eq!(snap.time_pos, None);
        assert!(!snap.is_buffering());

        snap.on_file_loaded();
        assert!(snap.file_loaded);

        snap.apply_optimistic_set("pause", "no");
        snap.apply_optimistic_set("volume", "72");
        assert_eq!(snap.pause, Some(false));
        assert_eq!(snap.volume, Some(72.0));

        snap.on_end_file(MPV_END_FILE_REASON_EOF);
        assert!(!snap.file_loaded);
        assert_eq!(snap.eof_reached, Some(true));
        assert_eq!(snap.active_playlist_entry_id, None);
    }

    #[test]
    fn player_snapshot_parses_synthetic_track_list_node_and_syncs_selection() {
        fn make_str_node(value: &CString) -> MpvNode {
            MpvNode {
                u: MpvNodeValue {
                    string: value.as_ptr().cast_mut(),
                },
                format: MPV_FORMAT_STRING,
            }
        }
        fn make_int_node(value: i64) -> MpvNode {
            MpvNode {
                u: MpvNodeValue { int64: value },
                format: MPV_FORMAT_INT64,
            }
        }
        fn make_flag_node(value: bool) -> MpvNode {
            MpvNode {
                u: MpvNodeValue {
                    flag: i32::from(value),
                },
                format: MPV_FORMAT_FLAG,
            }
        }

        let k_type = CString::new("type").unwrap();
        let k_id = CString::new("id").unwrap();
        let k_title = CString::new("title").unwrap();
        let k_lang = CString::new("lang").unwrap();
        let k_codec = CString::new("codec").unwrap();
        let k_selected = CString::new("selected").unwrap();
        let k_external = CString::new("external").unwrap();
        let k_ext_file = CString::new("external-filename").unwrap();

        let v_video = CString::new("video").unwrap();
        let v_audio = CString::new("audio").unwrap();
        let v_sub = CString::new("sub").unwrap();
        let v_en_title = CString::new("Original").unwrap();
        let v_en = CString::new("en").unwrap();
        let v_pt = CString::new("pt").unwrap();
        let v_opus = CString::new("opus").unwrap();
        let v_aac = CString::new("aac").unwrap();
        let v_webvtt = CString::new("webvtt").unwrap();
        let v_sub_path = CString::new("C:\\temp\\captions_pt.vtt").unwrap();

        // Track 0: video (ignored for audio/sub vectors, still counted in track_count)
        let mut t0_keys = [k_type.as_ptr().cast_mut(), k_id.as_ptr().cast_mut()];
        let mut t0_vals = [make_str_node(&v_video), make_int_node(1)];
        let mut t0_map = MpvNodeList {
            num: t0_vals.len() as c_int,
            values: t0_vals.as_mut_ptr(),
            keys: t0_keys.as_mut_ptr(),
        };

        // Track 1: audio (selected)
        let mut t1_keys = [
            k_type.as_ptr().cast_mut(),
            k_id.as_ptr().cast_mut(),
            k_title.as_ptr().cast_mut(),
            k_lang.as_ptr().cast_mut(),
            k_codec.as_ptr().cast_mut(),
            k_selected.as_ptr().cast_mut(),
        ];
        let mut t1_vals = [
            make_str_node(&v_audio),
            make_int_node(1),
            make_str_node(&v_en_title),
            make_str_node(&v_en),
            make_str_node(&v_opus),
            make_flag_node(true),
        ];
        let mut t1_map = MpvNodeList {
            num: t1_vals.len() as c_int,
            values: t1_vals.as_mut_ptr(),
            keys: t1_keys.as_mut_ptr(),
        };

        // Track 2: audio (unselected, no title)
        let mut t2_keys = [
            k_type.as_ptr().cast_mut(),
            k_id.as_ptr().cast_mut(),
            k_lang.as_ptr().cast_mut(),
            k_codec.as_ptr().cast_mut(),
            k_selected.as_ptr().cast_mut(),
        ];
        let mut t2_vals = [
            make_str_node(&v_audio),
            make_int_node(2),
            make_str_node(&v_pt),
            make_str_node(&v_aac),
            make_flag_node(false),
        ];
        let mut t2_map = MpvNodeList {
            num: t2_vals.len() as c_int,
            values: t2_vals.as_mut_ptr(),
            keys: t2_keys.as_mut_ptr(),
        };

        // Track 3: external subtitle (selected)
        let mut t3_keys = [
            k_type.as_ptr().cast_mut(),
            k_id.as_ptr().cast_mut(),
            k_lang.as_ptr().cast_mut(),
            k_codec.as_ptr().cast_mut(),
            k_external.as_ptr().cast_mut(),
            k_ext_file.as_ptr().cast_mut(),
            k_selected.as_ptr().cast_mut(),
        ];
        let mut t3_vals = [
            make_str_node(&v_sub),
            make_int_node(1),
            make_str_node(&v_pt),
            make_str_node(&v_webvtt),
            make_flag_node(true),
            make_str_node(&v_sub_path),
            make_flag_node(true),
        ];
        let mut t3_map = MpvNodeList {
            num: t3_vals.len() as c_int,
            values: t3_vals.as_mut_ptr(),
            keys: t3_keys.as_mut_ptr(),
        };

        let mut array_items = [
            MpvNode {
                u: MpvNodeValue {
                    list: &raw mut t0_map,
                },
                format: MPV_FORMAT_NODE_MAP,
            },
            MpvNode {
                u: MpvNodeValue {
                    list: &raw mut t1_map,
                },
                format: MPV_FORMAT_NODE_MAP,
            },
            MpvNode {
                u: MpvNodeValue {
                    list: &raw mut t2_map,
                },
                format: MPV_FORMAT_NODE_MAP,
            },
            MpvNode {
                u: MpvNodeValue {
                    list: &raw mut t3_map,
                },
                format: MPV_FORMAT_NODE_MAP,
            },
        ];
        let mut array_list = MpvNodeList {
            num: array_items.len() as c_int,
            values: array_items.as_mut_ptr(),
            keys: ptr::null_mut(),
        };
        let root_node = MpvNode {
            u: MpvNodeValue {
                list: &raw mut array_list,
            },
            format: MPV_FORMAT_NODE_ARRAY,
        };

        let prop = MpvEventProperty {
            name: c"track-list".as_ptr(),
            format: MPV_FORMAT_NODE,
            data: (&raw const root_node).cast(),
        };
        let mut snap = PlayerSnapshot::default();
        unsafe {
            snap.apply_property_change(ObservedProperty::TrackList as u64, &prop);
        }

        assert!(snap.track_list_observed);
        assert_eq!(snap.track_count, 4);
        assert_eq!(snap.lookup_int_cstr(c"track-list/count"), Some(Some(4)));
        assert_eq!(snap.audio_tracks.len(), 2);
        assert_eq!(
            snap.audio_tracks[0].to_option(),
            AudioTrackOption {
                id: "1".to_owned(),
                label: "Original · EN · opus".to_owned(),
                selected: true,
            }
        );
        assert_eq!(
            snap.audio_tracks[1].to_option(),
            AudioTrackOption {
                id: "2".to_owned(),
                label: "PT · aac".to_owned(),
                selected: false,
            }
        );
        assert_eq!(snap.subtitle_tracks.len(), 1);
        assert_eq!(snap.subtitle_tracks[0].id, "1");
        assert!(snap.subtitle_tracks[0].external);
        assert_eq!(
            snap.subtitle_tracks[0].external_filename.as_deref(),
            Some("C:\\temp\\captions_pt.vtt")
        );
        assert!(snap.subtitle_tracks[0].selected);

        // Changing `aid` updates cached audio selection immediately.
        let new_aid = CString::new("2").unwrap();
        let new_aid_ptr = new_aid.as_ptr();
        let aid_prop = MpvEventProperty {
            name: c"aid".as_ptr(),
            format: MPV_FORMAT_STRING,
            data: (&raw const new_aid_ptr).cast(),
        };
        unsafe {
            snap.apply_property_change(ObservedProperty::Aid as u64, &aid_prop);
        }
        assert!(!snap.audio_tracks[0].selected);
        assert!(snap.audio_tracks[1].selected);

        // Disabling subtitles via `sid=no` clears subtitle selection.
        let no_sid = CString::new("no").unwrap();
        let no_sid_ptr = no_sid.as_ptr();
        let sid_prop = MpvEventProperty {
            name: c"sid".as_ptr(),
            format: MPV_FORMAT_STRING,
            data: (&raw const no_sid_ptr).cast(),
        };
        unsafe {
            snap.apply_property_change(ObservedProperty::Sid as u64, &sid_prop);
        }
        assert!(!snap.subtitle_tracks[0].selected);

        // Clearing `track-list` with MPV_FORMAT_NONE empties both lists cleanly.
        let empty_prop = MpvEventProperty {
            name: c"track-list".as_ptr(),
            format: MPV_FORMAT_NONE,
            data: ptr::null(),
        };
        unsafe {
            snap.apply_property_change(ObservedProperty::TrackList as u64, &empty_prop);
        }
        assert!(snap.audio_tracks.is_empty());
        assert!(snap.subtitle_tracks.is_empty());
        assert_eq!(snap.track_count, 0);
    }

    #[test]
    fn wakeup_callback_coalesces_and_respects_hidden_and_parked_gates() {
        let hidden = Arc::new(AtomicBool::new(false));
        let parked = Arc::new(AtomicBool::new(false));
        let pending = Arc::new(AtomicBool::new(false));

        // First wakeup fires and sets pending; second coalesces until cleared.
        assert!(should_request_repaint_for_wakeup(
            &hidden, &parked, &pending
        ));
        assert!(!should_request_repaint_for_wakeup(
            &hidden, &parked, &pending
        ));

        pending.store(false, Ordering::Release);
        hidden.store(true, Ordering::Relaxed);
        assert!(!should_request_repaint_for_wakeup(
            &hidden, &parked, &pending
        ));
        assert!(!pending.load(Ordering::Acquire));

        hidden.store(false, Ordering::Relaxed);
        parked.store(true, Ordering::Relaxed);
        assert!(!should_request_repaint_for_wakeup(
            &hidden, &parked, &pending
        ));
        assert!(!pending.load(Ordering::Acquire));

        parked.store(false, Ordering::Relaxed);
        let mut callback_data = WakeupCallbackData {
            ctx: egui::Context::default(),
            window_hidden: Arc::clone(&hidden),
            parked: Arc::clone(&parked),
            pending: Arc::clone(&pending),
        };
        unsafe {
            mpv_wakeup_callback(ptr::null_mut());
            mpv_wakeup_callback((&raw mut callback_data).cast());
        }
        assert!(pending.load(Ordering::Acquire));
    }

    #[test]
    fn warm_park_state_machine_tracks_park_wake_duration_and_snapshot_reset() {
        let t0 = Instant::now();
        let mut state = WarmParkState::default();
        assert!(!state.is_parked());
        assert_eq!(state.parked_duration_at(t0), None);
        assert!(!state.is_idle_expired_at(t0, WARM_PLAYER_IDLE_TIMEOUT));

        assert!(state.park_at(t0));
        assert!(state.is_parked());
        // Parking again while already parked is idempotent and keeps original timestamp.
        assert!(!state.park_at(t0 + Duration::from_secs(30)));
        assert_eq!(
            state.parked_duration_at(t0 + Duration::from_secs(45)),
            Some(Duration::from_secs(45))
        );
        assert!(!state.is_idle_expired_at(t0 + Duration::from_secs(299), WARM_PLAYER_IDLE_TIMEOUT));
        assert!(state.is_idle_expired_at(t0 + Duration::from_secs(300), WARM_PLAYER_IDLE_TIMEOUT));

        assert!(state.wake());
        assert!(!state.wake());
        assert!(!state.is_parked());
        assert_eq!(
            state.parked_duration_at(t0 + Duration::from_secs(400)),
            None
        );

        // Snapshot reset clears per-file playback fields while retaining player-level audio prefs.
        let mut snap = PlayerSnapshot {
            observed: true,
            time_pos: Some(123.0),
            duration: Some(600.0),
            pause: Some(false),
            eof_reached: Some(true),
            core_idle: Some(false),
            paused_for_cache: Some(true),
            cache_buffering_state: Some(40),
            demuxer_cache_duration: Some(12.0),
            volume: Some(65.0),
            mute: Some(true),
            speed: Some(1.25),
            video_width: Some(1920),
            video_height: Some(1080),
            hwdec_current: Some("nvdec".to_owned()),
            audio_tracks: vec![AudioTrackInfo {
                id: "1".to_owned(),
                ..Default::default()
            }],
            subtitle_tracks: vec![SubtitleTrackInfo {
                id: "1".to_owned(),
                ..Default::default()
            }],
            track_count: 2,
            track_list_observed: true,
            aid: Some("1".to_owned()),
            sid: Some("1".to_owned()),
            vid: Some("auto".to_owned()),
            media_title: Some("Video".to_owned()),
            path: Some("https://example.com".to_owned()),
            seekable: Some(true),
            sub_visibility: Some(true),
            file_loaded: true,
            active_playlist_entry_id: Some(9),
        };
        snap.reset_playback_fields();
        assert!(snap.observed);
        assert_eq!(snap.time_pos, None);
        assert_eq!(snap.duration, None);
        assert_eq!(snap.pause, Some(true));
        assert_eq!(snap.eof_reached, Some(false));
        assert_eq!(snap.core_idle, Some(true));
        assert_eq!(snap.paused_for_cache, Some(false));
        assert_eq!(snap.cache_buffering_state, None);
        assert_eq!(snap.demuxer_cache_duration, None);
        assert_eq!(snap.video_width, None);
        assert_eq!(snap.video_height, None);
        assert_eq!(snap.hwdec_current, None);
        assert!(snap.audio_tracks.is_empty());
        assert!(snap.subtitle_tracks.is_empty());
        assert_eq!(snap.track_count, 0);
        assert_eq!(snap.path, None);
        assert!(!snap.file_loaded);
        assert_eq!(snap.active_playlist_entry_id, None);
        // Preserved across park:
        assert_eq!(snap.volume, Some(65.0));
        assert_eq!(snap.mute, Some(true));
        assert_eq!(snap.speed, Some(1.25));
    }

    #[test]
    fn gl_proc_cache_caches_nonzero_pointers_and_rejects_wgl_sentinels() {
        assert!(!is_valid_wgl_proc_address(ptr::null_mut()));
        assert!(!is_valid_wgl_proc_address(ptr::dangling_mut::<c_void>()));
        assert!(!is_valid_wgl_proc_address(2usize as *mut c_void));
        assert!(!is_valid_wgl_proc_address(3usize as *mut c_void));
        assert!(!is_valid_wgl_proc_address((-1isize) as *mut c_void));
        assert!(is_valid_wgl_proc_address(0x1234_usize as *mut c_void));

        let cache = Mutex::new(HashMap::new());
        let mut calls = 0usize;
        let first = lookup_or_cache_gl_proc(&cache, "glGenFramebuffers", || {
            calls += 1;
            0x4000_usize as *mut c_void
        });
        let second = lookup_or_cache_gl_proc(&cache, "glGenFramebuffers", || {
            calls += 1;
            0x8000_usize as *mut c_void
        });
        assert_eq!(first as usize, 0x4000);
        assert_eq!(second as usize, 0x4000);
        assert_eq!(calls, 1);

        // Null pointers must not be cached so later valid contexts can still resolve them.
        let mut null_calls = 0usize;
        let miss = lookup_or_cache_gl_proc(&cache, "glMissingExt", || {
            null_calls += 1;
            ptr::null_mut()
        });
        assert!(miss.is_null());
        let retry = lookup_or_cache_gl_proc(&cache, "glMissingExt", || {
            null_calls += 1;
            0x9000_usize as *mut c_void
        });
        assert_eq!(retry as usize, 0x9000);
        assert_eq!(null_calls, 2);
    }

    #[test]
    fn wgl_hwdec_chain_and_effective_hwdec_option_values() {
        assert_eq!(wgl_hwdec_chain(true), "nvdec,d3d11va-copy,dxva2-copy");
        assert_eq!(wgl_hwdec_chain(false), "no");

        assert_eq!(
            effective_hwdec_option(HwdecMode::AutoSafe, true),
            "nvdec,d3d11va-copy,dxva2-copy"
        );
        assert_eq!(
            effective_hwdec_option(HwdecMode::Auto, true),
            "nvdec,d3d11va-copy,dxva2-copy"
        );
        assert_eq!(effective_hwdec_option(HwdecMode::Software, true), "no");

        assert_eq!(
            effective_hwdec_option(HwdecMode::AutoSafe, false),
            "auto-safe"
        );
        assert_eq!(effective_hwdec_option(HwdecMode::Auto, false), "auto");
        assert_eq!(effective_hwdec_option(HwdecMode::Software, false), "no");
    }

    #[test]
    fn ytdl_format_for_codec_pref_builds_expected_selectors() {
        assert_eq!(
            ytdl_format_for_codec_pref(None, VideoCodecPreference::Auto),
            "bestvideo[height<=1080]+bestaudio/best[height<=1080]/best"
        );
        assert_eq!(
            ytdl_format_for_codec_pref(Some(2160), VideoCodecPreference::Vp9),
            "bestvideo[height<=2160][vcodec^=vp9]+bestaudio/bestvideo[height<=2160][vcodec^=vp09]+bestaudio/bestvideo[height<=2160]+bestaudio/best[height<=2160]/best"
        );
        assert_eq!(
            ytdl_format_for_codec_pref(Some(720), VideoCodecPreference::H264),
            "bestvideo[height<=720][vcodec^=avc1]+bestaudio/bestvideo[height<=720]+bestaudio/best[height<=720]/best"
        );
        assert_eq!(
            ytdl_format_for_codec_pref(Some(1440), VideoCodecPreference::Av1),
            "bestvideo[height<=1440][vcodec^=av01]+bestaudio/bestvideo[height<=1440]+bestaudio/best[height<=1440]/best"
        );

        assert!(
            ytdl_format_for_exact_height_and_codec(1080, VideoCodecPreference::H264)
                .starts_with("bestvideo[height=1080][vcodec^=avc1]+bestaudio/")
        );

        let buffer = BufferProfile::Default.values();
        let opts = Player::options_with_codec_pref(
            buffer,
            HwdecMode::AutoSafe,
            VideoCodecPreference::Vp9,
            None,
            true,
        );
        assert!(opts.iter().any(|(name, value)| *name == "ytdl-format"
            && value == &ytdl_format_for_codec_pref(None, VideoCodecPreference::Vp9)));
    }

    #[test]
    fn disabled_builtin_script_options_are_applied_to_player_options() {
        let expected = [
            ("load-stats-overlay", "no"),
            ("load-console", "no"),
            ("load-auto-profiles", "no"),
            ("osc", "no"),
            ("osd-bar", "no"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
        ];
        for pair in expected {
            assert!(
                DISABLED_BUILTIN_SCRIPT_OPTIONS.contains(&pair),
                "missing disabled built-in script option {pair:?}"
            );
        }

        let buffer = BufferProfile::Default.values();
        for render_api in [true, false] {
            let wid = (!render_api).then_some(99);
            let opts = Player::options(buffer, HwdecMode::AutoSafe, wid, render_api);
            for (name, value) in expected {
                assert!(
                    opts.iter().any(|(k, v)| *k == name && v == value),
                    "expected ({name}, {value}) in Player::options(render_api={render_api})"
                );
            }
        }
    }

    #[test]
    fn build_direct_stream_loadfile_options_formats_split_video_audio_and_start() {
        let opts = DirectStreamOptions {
            video_url: "https://cdn.example/video.webm",
            audio_url: Some("https://cdn.example/audio.webm"),
            start_secs: Some(42.25),
            audio_only: false,
            user_agent: None,
            referer: None,
        };
        assert_eq!(
            build_direct_stream_loadfile_options(&opts),
            "ytdl=no,audio-file=https://cdn.example/audio.webm,start=42.250,cache-pause-wait=0.25"
        );

        // Start positions <= 0.5s are omitted to avoid redundant initial seek.
        let near_zero = DirectStreamOptions {
            start_secs: Some(0.4),
            ..opts
        };
        assert_eq!(
            build_direct_stream_loadfile_options(&near_zero),
            "ytdl=no,audio-file=https://cdn.example/audio.webm,cache-pause-wait=0.25"
        );
    }

    #[test]
    fn build_direct_stream_loadfile_options_handles_audio_only_mode() {
        let opts = DirectStreamOptions {
            video_url: "https://cdn.example/audio_only.webm",
            audio_url: Some("https://cdn.example/should_be_omitted.webm"),
            start_secs: Some(15.0),
            audio_only: true,
            user_agent: None,
            referer: None,
        };
        assert_eq!(
            build_direct_stream_loadfile_options(&opts),
            "ytdl=no,start=15.000,cache-pause-wait=0.25,vid=no"
        );
    }

    #[test]
    fn build_direct_stream_loadfile_options_escapes_special_chars_and_headers() {
        let audio = "https://rr1.googlevideo.com/videoplayback?itag=251&range=0-100,200-300&expire=1700000000";
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36, Chrome/131.0";
        let referer = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
        let opts = DirectStreamOptions {
            video_url: "https://rr1.googlevideo.com/videoplayback?itag=303",
            audio_url: Some(audio),
            start_secs: None,
            audio_only: false,
            user_agent: Some(ua),
            referer: Some(referer),
        };
        let rendered = build_direct_stream_loadfile_options(&opts);
        assert!(rendered.starts_with("ytdl=no,"));
        assert!(rendered.contains(&format!("audio-file=%{}%{audio},", audio.len())));
        assert!(rendered.contains("cache-pause-wait=0.25"));
        assert!(rendered.contains(&format!("user-agent=%{}%{ua},", ua.len())));
        assert!(rendered.ends_with(&format!("referrer=%{}%{referer}", referer.len())));
    }

    #[test]
    fn volume_boost_option_and_clamp_support_200_percent() {
        assert_eq!(MAX_VOLUME_PERCENT, 200.0);
        assert_eq!(clamp_volume_percent(-10.0), 0.0);
        assert_eq!(clamp_volume_percent(100.0), 100.0);
        assert_eq!(clamp_volume_percent(165.0), 165.0);
        assert_eq!(clamp_volume_percent(250.0), 200.0);
        assert_eq!(clamp_volume_percent(f64::NAN), 100.0);

        let buffer = BufferProfile::Default.values();
        for render_api in [true, false] {
            let opts = Player::options(
                buffer,
                HwdecMode::AutoSafe,
                (!render_api).then_some(7),
                render_api,
            );
            assert!(
                opts.iter()
                    .any(|(name, value)| *name == "volume-max" && value == "200"),
                "expected volume-max=200 in Player::options(render_api={render_api})"
            );
        }
    }

    #[test]
    fn ultrawide_zoom_and_caption_style_mpv_properties_format_correctly() {
        assert_eq!(
            ultrawide_zoom_mpv_properties(UltrawideZoomMode::Fit, 0.3),
            [
                ("panscan", "0.000".to_owned()),
                ("video-zoom", "0.000".to_owned()),
            ]
        );
        assert_eq!(
            ultrawide_zoom_mpv_properties(UltrawideZoomMode::Crop21x9, 0.3),
            [
                ("panscan", "1.000".to_owned()),
                ("video-zoom", "0.000".to_owned()),
            ]
        );
        assert_eq!(
            ultrawide_zoom_mpv_properties(UltrawideZoomMode::Custom, 0.8),
            [
                ("panscan", "0.000".to_owned()),
                ("video-zoom", "0.500".to_owned()),
            ]
        );
        assert_eq!(
            ultrawide_zoom_mpv_properties(UltrawideZoomMode::Custom, -0.2),
            [
                ("panscan", "0.000".to_owned()),
                ("video-zoom", "0.000".to_owned()),
            ]
        );

        let style = CaptionStylePrefs {
            font_scale: 1.35,
            bg_opacity: 0.65,
            text_color_hex: "#fff176".to_owned(),
            translate_lang: Some("pt-BR".to_owned()),
        };
        assert_eq!(
            caption_style_mpv_properties(&style),
            [
                ("sub-scale", "1.35".to_owned()),
                ("sub-color", "#FFF176".to_owned()),
                ("sub-back-color", "0.0/0.0/0.0/0.65".to_owned()),
            ]
        );
    }

    #[test]
    fn subtitle_url_with_tlang_adds_replaces_and_strips_cleanly() {
        let base = "https://www.youtube.com/api/timedtext?v=abc123&lang=en";
        assert_eq!(
            subtitle_url_with_tlang(base, Some("pt-BR")),
            "https://www.youtube.com/api/timedtext?v=abc123&lang=en&tlang=pt-BR"
        );
        assert_eq!(
            subtitle_url_with_tlang(
                "https://www.youtube.com/api/timedtext?v=abc123&tlang=es&lang=en",
                Some("fr")
            ),
            "https://www.youtube.com/api/timedtext?v=abc123&lang=en&tlang=fr"
        );
        assert_eq!(
            subtitle_url_with_tlang(
                "https://www.youtube.com/api/timedtext?v=abc123&lang=en&tlang=de",
                None
            ),
            "https://www.youtube.com/api/timedtext?v=abc123&lang=en"
        );
        assert_eq!(
            subtitle_url_with_tlang("https://www.youtube.com/api/timedtext?tlang=ja", None),
            "https://www.youtube.com/api/timedtext"
        );
        assert_eq!(
            subtitle_url_with_tlang("https://www.youtube.com/api/timedtext#frag", Some("ko")),
            "https://www.youtube.com/api/timedtext?tlang=ko#frag"
        );
    }

    #[test]
    fn build_local_file_loadfile_options_sets_ytdl_no_and_optional_start() {
        assert_eq!(build_local_file_loadfile_options(None), "ytdl=no");
        assert_eq!(build_local_file_loadfile_options(Some(0.3)), "ytdl=no");
        assert_eq!(
            build_local_file_loadfile_options(Some(92.5)),
            "ytdl=no,start=92.500"
        );
        assert_eq!(build_local_file_loadfile_options(Some(f64::NAN)), "ytdl=no");
    }
}
