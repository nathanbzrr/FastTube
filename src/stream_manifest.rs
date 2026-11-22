//! Unified single-extraction stream manifest, stream selector, bounded TTL LRU cache,
//! and single-slot speculative next-video prefetcher.

use crate::overlay::{OverlayAudioTrack, OverlayChapter};
use crate::player::DirectStreamOptions;
use crate::process::{self, Cancellation};
use crate::search::{
    StoryboardFragment, StoryboardSpec, SubtitleTrack, effective_yt_dlp_cache_dir,
    is_allowed_subtitle_url,
};
use crate::settings::VideoCodecPreference;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Safety margin subtracted from `expire=` query parameters so stream URLs are refreshed
/// before CDN expiry.
pub const EXPIRE_SAFETY_MARGIN: Duration = Duration::from_secs(120);
/// Upper bound on cached manifest lifetime regardless of a longer `expire=` parameter.
pub const MAX_MANIFEST_TTL: Duration = Duration::from_secs(20 * 60);
/// Fallback manifest lifetime when no `expire=` timestamp is present on stream URLs.
pub const DEFAULT_MANIFEST_TTL: Duration = Duration::from_secs(10 * 60);
/// Maximum number of cached manifests retained in `StreamManifestCache`.
pub const MAX_MANIFEST_CACHE_ENTRIES: usize = 32;

const EXTRACT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_HEADER_VALUE_BYTES: usize = 1024;
const MAX_CAPTIONS: usize = 80;
const HIGH_FPS_THRESHOLD: f32 = 48.0;

#[derive(Clone, Debug, PartialEq)]
pub struct VideoStreamFormat {
    pub format_id: String,
    pub url: String,
    pub height: u32,
    pub width: Option<u32>,
    pub fps: Option<f32>,
    pub vcodec: String,
    pub is_hdr: bool,
    pub tbr_kbps: Option<f64>,
    pub ext: String,
    pub has_audio: bool,
    pub http_headers: HashMap<String, String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioStreamFormat {
    pub format_id: String,
    pub url: String,
    pub acodec: String,
    pub abr_kbps: Option<f64>,
    pub ext: String,
    pub language: Option<String>,
    pub language_label: String,
    pub is_default: bool,
    pub is_dubbed: bool,
    pub format_note: Option<String>,
    pub http_headers: HashMap<String, String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ManifestChapter {
    pub title: String,
    pub start_secs: f64,
    pub end_secs: f64,
}

impl ManifestChapter {
    #[allow(dead_code)]
    pub fn to_overlay_chapter(&self) -> OverlayChapter {
        OverlayChapter {
            title: self.title.clone(),
            time_secs: self.start_secs,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestCaptionTrack {
    pub language_code: String,
    pub label: String,
    pub url: String,
    pub is_auto_generated: bool,
}

impl ManifestCaptionTrack {
    #[allow(dead_code)]
    pub fn to_subtitle_track(&self) -> SubtitleTrack {
        SubtitleTrack {
            language: self.language_code.clone(),
            automatic: self.is_auto_generated,
            url: self.url.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ManifestStoryboardFragment {
    pub url: String,
    pub duration_secs: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ManifestStoryboardSpec {
    pub rows: u32,
    pub columns: u32,
    pub width: u32,
    pub height: u32,
    pub fps: Option<f64>,
    pub fragments: Vec<ManifestStoryboardFragment>,
}

impl ManifestStoryboardSpec {
    #[allow(dead_code)]
    pub fn to_storyboard_spec(&self) -> StoryboardSpec {
        StoryboardSpec {
            rows: self.rows,
            columns: self.columns,
            thumb_width: self.width,
            thumb_height: self.height,
            fragments: self
                .fragments
                .iter()
                .map(|frag| StoryboardFragment {
                    url: frag.url.clone(),
                    duration_secs: frag.duration_secs,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StreamManifest {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub channel_id: Option<String>,
    pub channel_verified: bool,
    pub duration_secs: Option<f64>,
    pub is_live: bool,
    pub upload_date: Option<String>,
    pub video_formats: Vec<VideoStreamFormat>,
    pub audio_formats: Vec<AudioStreamFormat>,
    pub muxed_formats: Vec<VideoStreamFormat>,
    pub chapters: Vec<ManifestChapter>,
    pub captions: Vec<ManifestCaptionTrack>,
    pub storyboard: Option<ManifestStoryboardSpec>,
    pub fetched_at: Instant,
    pub expires_at: Instant,
}

impl StreamManifest {
    #[allow(dead_code)]
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(Instant::now())
    }

    pub fn is_expired_at(&self, now: Instant) -> bool {
        now >= self.expires_at
    }

    #[allow(dead_code)]
    pub fn available_heights(&self) -> Vec<u32> {
        available_quality_options(self)
            .into_iter()
            .map(|opt| opt.height)
            .collect()
    }

    #[allow(dead_code)]
    pub fn to_subtitle_tracks(&self) -> Vec<SubtitleTrack> {
        self.captions
            .iter()
            .map(ManifestCaptionTrack::to_subtitle_track)
            .collect()
    }

    #[allow(dead_code)]
    pub fn to_overlay_chapters(&self) -> Vec<OverlayChapter> {
        self.chapters
            .iter()
            .map(ManifestChapter::to_overlay_chapter)
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectedStreamPair {
    pub video_url: String,
    pub audio_url: Option<String>,
    pub height: u32,
    pub fps: Option<f32>,
    pub is_hdr: bool,
    pub vcodec: String,
    pub audio_language: Option<String>,
    pub audio_label: Option<String>,
    pub http_headers: HashMap<String, String>,
    pub format_summary: String,
}

impl SelectedStreamPair {
    pub fn user_agent(&self) -> Option<&str> {
        lookup_header_ignore_ascii_case(&self.http_headers, "user-agent")
    }

    pub fn referer(&self) -> Option<&str> {
        lookup_header_ignore_ascii_case(&self.http_headers, "referer")
    }

    #[allow(dead_code)]
    pub fn to_direct_stream_options(
        &self,
        start_secs: Option<f64>,
        audio_only: bool,
    ) -> DirectStreamOptions<'_> {
        DirectStreamOptions {
            video_url: &self.video_url,
            audio_url: self.audio_url.as_deref(),
            start_secs,
            audio_only,
            user_agent: self.user_agent(),
            referer: self.referer(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityOption {
    pub height: u32,
    pub max_fps: Option<u32>,
    pub high_fps: bool,
    pub is_hdr: bool,
    pub codecs: Vec<String>,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ManifestAudioOption {
    pub language_code: String,
    pub label: String,
    pub is_default: bool,
    pub is_dubbed: bool,
    pub best_format_id: String,
    pub best_abr_kbps: Option<f64>,
    pub acodec: String,
}

impl ManifestAudioOption {
    #[allow(dead_code)]
    pub fn to_overlay_audio_track(&self, selected: bool) -> OverlayAudioTrack {
        OverlayAudioTrack {
            id: self.language_code.clone(),
            label: self.label.clone(),
            selected,
        }
    }
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn lookup_header_ignore_ascii_case<'a>(
    headers: &'a HashMap<String, String>,
    key: &str,
) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}

fn is_safe_http_stream_url(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
    {
        return false;
    }
    let Ok(parsed) = url::Url::parse(trimmed) else {
        return false;
    };
    matches!(parsed.scheme(), "https" | "http")
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.host_str().is_some()
}

fn sanitize_header_value(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .trim()
        .chars()
        .filter(|&c| c != '\r' && c != '\n' && c != '\0')
        .collect();
    if cleaned.is_empty() || cleaned.len() > MAX_HEADER_VALUE_BYTES {
        return None;
    }
    Some(cleaned)
}

/// Keeps only non-sensitive transport headers; cookies and auth tokens are never stored.
fn extract_safe_http_headers(
    primary: Option<&Value>,
    fallback: Option<&Value>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let collect = |source: Option<&Value>, map: &mut HashMap<String, String>| {
        let Some(obj) = source.and_then(Value::as_object) else {
            return;
        };
        for (raw_key, raw_val) in obj {
            let Some(canonical_key) = (match raw_key.trim().to_ascii_lowercase().as_str() {
                "user-agent" => Some("User-Agent"),
                "referer" => Some("Referer"),
                "accept" => Some("Accept"),
                "accept-language" => Some("Accept-Language"),
                "origin" => Some("Origin"),
                _ => None,
            }) else {
                continue;
            };
            if let Some(val) = raw_val.as_str().and_then(sanitize_header_value) {
                map.entry(canonical_key.to_owned()).or_insert(val);
            }
        }
    };
    collect(primary, &mut out);
    collect(fallback, &mut out);
    out
}

fn extract_expire_epoch(raw_url: &str) -> Option<u64> {
    let parsed = url::Url::parse(raw_url).ok()?;
    for (k, v) in parsed.query_pairs() {
        if k.eq_ignore_ascii_case("expire")
            && let Ok(epoch) = v.trim().parse::<u64>()
            && epoch > 0
        {
            return Some(epoch);
        }
    }
    if let Some(mut segments) = parsed.path_segments() {
        while let Some(seg) = segments.next() {
            if seg.eq_ignore_ascii_case("expire")
                && let Some(next_seg) = segments.next()
                && let Ok(epoch) = next_seg.trim().parse::<u64>()
                && epoch > 0
            {
                return Some(epoch);
            }
        }
    }
    None
}

fn compute_expires_at(
    urls: impl Iterator<Item = String>,
    now: Instant,
    now_unix_secs: u64,
) -> Instant {
    let min_expire = urls.filter_map(|u| extract_expire_epoch(&u)).min();
    let ttl = match min_expire {
        Some(expire_epoch) => {
            let remaining = expire_epoch.saturating_sub(now_unix_secs);
            Duration::from_secs(remaining)
                .saturating_sub(EXPIRE_SAFETY_MARGIN)
                .min(MAX_MANIFEST_TTL)
        }
        None => DEFAULT_MANIFEST_TTL.min(MAX_MANIFEST_TTL),
    };
    now + ttl
}

fn is_codec_present(codec: Option<&str>) -> Option<String> {
    let raw = codec?.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(raw.to_owned())
    }
}

fn detect_hdr(fmt: &Value, vcodec: &str) -> bool {
    if let Some(dr) = fmt
        .get("dynamic_range")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        && !dr.eq_ignore_ascii_case("sdr")
    {
        return true;
    }
    let codec_lower = vcodec.to_ascii_lowercase();
    if codec_lower.starts_with("vp9.2") || codec_lower.starts_with("vp09.02") {
        return true;
    }
    // AV1 10-bit BT.2020 PQ (transfer 16) or HLG (transfer 18) codec strings.
    if codec_lower.starts_with("av01.")
        && codec_lower.contains(".10.")
        && (codec_lower.contains(".09.16") || codec_lower.contains(".09.18"))
    {
        return true;
    }
    if let Some(note) = fmt.get("format_note").and_then(Value::as_str) {
        let note_lower = note.to_ascii_lowercase();
        if note_lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|tok| matches!(tok, "hdr" | "hdr10" | "hlg" | "dv"))
        {
            return true;
        }
    }
    false
}

fn normalize_codec_family(vcodec: &str) -> Option<&'static str> {
    let lower = vcodec.trim().to_ascii_lowercase();
    if lower.starts_with("av01") || lower.starts_with("av1") {
        Some("AV1")
    } else if lower.starts_with("vp9") || lower.starts_with("vp09") {
        Some("VP9")
    } else if lower.starts_with("avc1") || lower.starts_with("h264") || lower.starts_with("avc") {
        Some("H.264")
    } else if lower.starts_with("hev1") || lower.starts_with("hvc1") || lower.starts_with("hevc") {
        Some("HEVC")
    } else {
        None
    }
}

fn matches_codec_pref(vcodec: &str, pref: VideoCodecPreference) -> bool {
    let lower = vcodec.trim().to_ascii_lowercase();
    match pref {
        VideoCodecPreference::Auto => true,
        VideoCodecPreference::Vp9 => lower.starts_with("vp9") || lower.starts_with("vp09"),
        VideoCodecPreference::H264 => {
            lower.starts_with("avc1") || lower.starts_with("h264") || lower.starts_with("avc")
        }
        VideoCodecPreference::Av1 => lower.starts_with("av01") || lower.starts_with("av1"),
    }
}

fn codec_auto_rank(vcodec: &str) -> u8 {
    match normalize_codec_family(vcodec) {
        Some("VP9") => 3,
        Some("AV1") => 2,
        Some("H.264") => 1,
        _ => 0,
    }
}

fn audio_codec_rank(acodec: &str) -> u8 {
    let lower = acodec.trim().to_ascii_lowercase();
    if lower.starts_with("opus") {
        3
    } else if lower.starts_with("mp4a") || lower.starts_with("aac") {
        2
    } else if lower.starts_with("vorbis") || lower.starts_with("ec-3") || lower.starts_with("ac-3")
    {
        1
    } else {
        0
    }
}

fn is_drc_audio(format_id: &str, format_note: Option<&str>) -> bool {
    if format_id.to_ascii_lowercase().ends_with("-drc") {
        return true;
    }
    format_note.is_some_and(|note| {
        note.to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|tok| tok == "drc")
    })
}

fn language_display_name(code: &str) -> String {
    let normalized = code.trim().replace('_', "-");
    let lower = normalized.to_ascii_lowercase();
    let known = match lower.as_str() {
        "pt-br" => Some("Português (Brasil)"),
        "pt-pt" => Some("Português (Portugal)"),
        "pt" => Some("Português"),
        "en-us" => Some("English (US)"),
        "en-gb" => Some("English (UK)"),
        "en" => Some("English"),
        "es-419" => Some("Español (Latinoamérica)"),
        "es-es" => Some("Español (España)"),
        "es" => Some("Español"),
        "fr" | "fr-fr" => Some("Français"),
        "de" | "de-de" => Some("Deutsch"),
        "it" | "it-it" => Some("Italiano"),
        "ja" | "ja-jp" => Some("日本語"),
        "ko" | "ko-kr" => Some("한국어"),
        "zh-cn" | "zh-hans" => Some("中文 (简体)"),
        "zh-tw" | "zh-hant" => Some("中文 (繁體)"),
        "zh" => Some("中文"),
        "ru" | "ru-ru" => Some("Русский"),
        "hi" | "hi-in" => Some("हिन्दी"),
        "ar" => Some("العربية"),
        _ => None,
    };
    if let Some(name) = known {
        name.to_owned()
    } else if !normalized.is_empty() {
        normalized.to_ascii_uppercase()
    } else {
        "Original".to_owned()
    }
}

fn build_audio_language_label(
    language: Option<&str>,
    format_note: Option<&str>,
    is_default: bool,
    is_dubbed: bool,
) -> String {
    let base = if let Some(lang) = language.map(str::trim).filter(|s| !s.is_empty()) {
        language_display_name(lang)
    } else if let Some(note) = format_note.map(str::trim).filter(|s| !s.is_empty()) {
        return note.to_owned();
    } else {
        return "Default".to_owned();
    };
    if is_default {
        format!("{base} (Original)")
    } else if is_dubbed {
        format!("{base} (Dubbed)")
    } else {
        base
    }
}

fn parse_storyboard_spec(formats: &[Value]) -> Option<ManifestStoryboardSpec> {
    let mut best: Option<(u32, ManifestStoryboardSpec)> = None;
    for fmt in formats {
        let is_sb = fmt
            .get("format_note")
            .and_then(Value::as_str)
            .is_some_and(|n| n.eq_ignore_ascii_case("storyboard"))
            || fmt
                .get("protocol")
                .and_then(Value::as_str)
                .is_some_and(|p| p.eq_ignore_ascii_case("mhtml"))
            || fmt
                .get("ext")
                .and_then(Value::as_str)
                .is_some_and(|e| e.eq_ignore_ascii_case("mhtml"));
        if !is_sb {
            continue;
        }
        let rows = fmt.get("rows").and_then(Value::as_u64).unwrap_or(0) as u32;
        let columns = fmt.get("columns").and_then(Value::as_u64).unwrap_or(0) as u32;
        if rows == 0 || columns == 0 {
            continue;
        }
        let width = fmt.get("width").and_then(Value::as_u64).unwrap_or(160) as u32;
        let height = fmt.get("height").and_then(Value::as_u64).unwrap_or(90) as u32;
        let fps = fmt
            .get("fps")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v > 0.0);
        let Some(raw_frags) = fmt.get("fragments").and_then(Value::as_array) else {
            continue;
        };
        let fragments: Vec<ManifestStoryboardFragment> = raw_frags
            .iter()
            .filter_map(|frag| {
                let url = frag.get("url").and_then(Value::as_str)?.trim();
                if !url.starts_with("https://") {
                    return None;
                }
                let duration_secs = frag
                    .get("duration")
                    .and_then(Value::as_f64)
                    .filter(|d| d.is_finite() && *d > 0.0)
                    .unwrap_or(100.0);
                Some(ManifestStoryboardFragment {
                    url: url.to_owned(),
                    duration_secs,
                })
            })
            .collect();
        if fragments.is_empty() {
            continue;
        }
        if best.as_ref().is_none_or(|(best_w, _)| width > *best_w) {
            best = Some((
                width,
                ManifestStoryboardSpec {
                    rows,
                    columns,
                    width,
                    height,
                    fps,
                    fragments,
                },
            ));
        }
    }
    best.map(|(_, spec)| spec)
}

fn parse_chapters(json: &Value, duration_secs: Option<f64>) -> Vec<ManifestChapter> {
    let Some(items) = json.get("chapters").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut chapters: Vec<(String, f64, Option<f64>)> = items
        .iter()
        .filter_map(|item| {
            let title = item.get("title").and_then(Value::as_str)?.trim();
            if title.is_empty() {
                return None;
            }
            let start = item
                .get("start_time")
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && *v >= 0.0)?;
            let end = item
                .get("end_time")
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && *v >= start);
            Some((title.to_owned(), start, end))
        })
        .collect();
    chapters.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut out = Vec::with_capacity(chapters.len());
    for idx in 0..chapters.len() {
        let (title, start_secs, explicit_end) = &chapters[idx];
        let next_start = chapters.get(idx + 1).map(|c| c.1);
        let end_secs = explicit_end
            .or(next_start)
            .or(duration_secs)
            .unwrap_or(*start_secs)
            .max(*start_secs);
        out.push(ManifestChapter {
            title: title.clone(),
            start_secs: *start_secs,
            end_secs,
        });
    }
    out
}

fn valid_lang_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn canonical_caption_language(language: &str, is_auto: bool) -> String {
    if is_auto && language.len() > 5 && language[language.len() - 5..].eq_ignore_ascii_case("-orig")
    {
        language[..language.len() - 5].to_owned()
    } else {
        language.to_owned()
    }
}

fn parse_captions(json: &Value) -> Vec<ManifestCaptionTrack> {
    struct Candidate {
        track: ManifestCaptionTrack,
        translated: bool,
        is_hls: bool,
        from_orig_alias: bool,
    }

    let mut candidates = Vec::new();
    for (field, is_auto) in [("subtitles", false), ("automatic_captions", true)] {
        let Some(langs) = json.get(field).and_then(Value::as_object) else {
            continue;
        };
        for (raw_lang, formats_val) in langs {
            if !valid_lang_token(raw_lang) {
                continue;
            }
            let Some(formats) = formats_val.as_array() else {
                continue;
            };
            let mut chosen: Option<(String, String, bool, bool)> = None;
            for fmt in formats {
                if fmt.get("ext").and_then(Value::as_str) != Some("vtt") {
                    continue;
                }
                let Some(url) = fmt.get("url").and_then(Value::as_str).map(str::trim) else {
                    continue;
                };
                if !is_allowed_subtitle_url(url) {
                    continue;
                }
                let Ok(parsed) = url::Url::parse(url) else {
                    continue;
                };
                let translated = parsed.query_pairs().any(|(k, _)| k == "tlang");
                let is_hls = fmt
                    .get("protocol")
                    .and_then(Value::as_str)
                    .is_some_and(|p| p.starts_with("m3u8"))
                    || parsed.path().ends_with(".m3u8");
                let label = fmt
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| language_display_name(raw_lang));
                let rank = (translated, is_hls);
                match &chosen {
                    Some((_, _, prev_t, prev_hls)) if (*prev_t, *prev_hls) <= rank => {}
                    _ => chosen = Some((url.to_owned(), label, translated, is_hls)),
                }
            }
            let Some((url, label, translated, is_hls)) = chosen else {
                continue;
            };
            let canonical = canonical_caption_language(raw_lang, is_auto);
            let from_orig_alias = is_auto && canonical != *raw_lang;
            candidates.push(Candidate {
                track: ManifestCaptionTrack {
                    language_code: canonical,
                    label,
                    url,
                    is_auto_generated: is_auto,
                },
                translated,
                is_hls,
                from_orig_alias,
            });
        }
    }

    let has_native = candidates.iter().any(|c| !c.translated);
    let mut deduped: HashMap<(bool, String), Candidate> = HashMap::new();
    for candidate in candidates {
        if has_native && candidate.translated {
            continue;
        }
        let key = (
            candidate.track.is_auto_generated,
            candidate.track.language_code.to_ascii_lowercase(),
        );
        let rank = (
            candidate.translated,
            candidate.is_hls,
            candidate.from_orig_alias,
        );
        match deduped.get(&key) {
            Some(existing)
                if (
                    existing.translated,
                    existing.is_hls,
                    existing.from_orig_alias,
                ) <= rank => {}
            _ => {
                deduped.insert(key, candidate);
            }
        }
    }

    let mut tracks: Vec<ManifestCaptionTrack> = deduped.into_values().map(|c| c.track).collect();
    tracks.sort_by(|a, b| {
        (
            a.is_auto_generated,
            a.language_code.to_ascii_lowercase(),
            &a.language_code,
        )
            .cmp(&(
                b.is_auto_generated,
                b.language_code.to_ascii_lowercase(),
                &b.language_code,
            ))
    });
    let mut seen_urls = HashSet::with_capacity(tracks.len());
    tracks.retain(|t| seen_urls.insert(t.url.clone()));
    tracks.truncate(MAX_CAPTIONS);
    tracks
}

/// Parses a `yt-dlp --dump-single-json` response into a unified `StreamManifest`.
pub fn parse_yt_dlp_single_json(
    video_id: &str,
    json: &Value,
    now: Instant,
) -> Result<StreamManifest, String> {
    parse_yt_dlp_single_json_at(video_id, json, now, current_unix_secs())
}

pub(crate) fn parse_yt_dlp_single_json_at(
    video_id: &str,
    json: &Value,
    now: Instant,
    now_unix_secs: u64,
) -> Result<StreamManifest, String> {
    if !json.is_object() {
        return Err("Invalid stream manifest JSON: expected root object.".to_owned());
    }
    let non_empty_str = |key: &str| -> Option<String> {
        json.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let resolved_video_id = {
        let trimmed = video_id.trim();
        if !trimmed.is_empty() {
            trimmed.to_owned()
        } else {
            non_empty_str("id")
                .ok_or_else(|| "Stream manifest JSON missing video identifier.".to_owned())?
        }
    };
    let title = non_empty_str("title").unwrap_or_default();
    let channel = non_empty_str("channel")
        .or_else(|| non_empty_str("uploader"))
        .unwrap_or_default();
    let channel_id = non_empty_str("channel_id");
    let channel_verified = json
        .get("channel_is_verified")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let duration_secs = json
        .get("duration")
        .and_then(Value::as_f64)
        .filter(|d| d.is_finite() && *d > 0.0);
    let is_live = json.get("is_live").and_then(Value::as_bool) == Some(true)
        || json.get("live_status").and_then(Value::as_str) == Some("is_live");
    let upload_date = crate::search::extract_yt_dlp_upload_date(json);
    let root_headers = json.get("http_headers");

    let raw_formats = json
        .get("formats")
        .and_then(Value::as_array)
        .ok_or_else(|| "Stream manifest JSON missing formats list.".to_owned())?;

    let storyboard = parse_storyboard_spec(raw_formats);
    let chapters = parse_chapters(json, duration_secs);
    let captions = parse_captions(json);

    let mut video_formats = Vec::new();
    let mut audio_formats = Vec::new();
    let mut muxed_formats = Vec::new();

    for fmt in raw_formats {
        let protocol = fmt
            .get("protocol")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let format_note_opt = fmt
            .get("format_note")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        if protocol.eq_ignore_ascii_case("mhtml")
            || format_note_opt
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case("storyboard"))
        {
            continue;
        }
        // Skip segmented DASH fragment entries that are not directly playable as single URLs.
        if protocol.eq_ignore_ascii_case("http_dash_segments")
            && fmt
                .get("fragments")
                .and_then(Value::as_array)
                .is_some_and(|frags| !frags.is_empty())
        {
            continue;
        }
        let Some(url) = fmt
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|u| is_safe_http_stream_url(u))
        else {
            continue;
        };
        let format_id = fmt
            .get("format_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown")
            .to_owned();
        let ext = fmt
            .get("ext")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("")
            .to_owned();
        let vcodec = is_codec_present(fmt.get("vcodec").and_then(Value::as_str));
        let acodec = is_codec_present(fmt.get("acodec").and_then(Value::as_str));
        let height = fmt
            .get("height")
            .and_then(Value::as_u64)
            .and_then(|h| u32::try_from(h).ok())
            .filter(|&h| h > 0);
        let width = fmt
            .get("width")
            .and_then(Value::as_u64)
            .and_then(|w| u32::try_from(w).ok())
            .filter(|&w| w > 0);
        let fps = fmt
            .get("fps")
            .and_then(Value::as_f64)
            .filter(|f| f.is_finite() && *f > 0.0)
            .map(|f| f as f32);
        let tbr_kbps = fmt
            .get("vbr")
            .and_then(Value::as_f64)
            .or_else(|| fmt.get("tbr").and_then(Value::as_f64))
            .filter(|b| b.is_finite() && *b > 0.0);
        let abr_kbps = fmt
            .get("abr")
            .and_then(Value::as_f64)
            .or_else(|| fmt.get("tbr").and_then(Value::as_f64))
            .filter(|b| b.is_finite() && *b > 0.0);
        let http_headers = extract_safe_http_headers(fmt.get("http_headers"), root_headers);

        match (vcodec, acodec, height) {
            (Some(vcodec), None, Some(height)) => {
                let is_hdr = detect_hdr(fmt, &vcodec);
                video_formats.push(VideoStreamFormat {
                    format_id,
                    url: url.to_owned(),
                    height,
                    width,
                    fps,
                    vcodec,
                    is_hdr,
                    tbr_kbps,
                    ext,
                    has_audio: false,
                    http_headers,
                });
            }
            (Some(vcodec), Some(_), Some(height)) => {
                let is_hdr = detect_hdr(fmt, &vcodec);
                muxed_formats.push(VideoStreamFormat {
                    format_id,
                    url: url.to_owned(),
                    height,
                    width,
                    fps,
                    vcodec,
                    is_hdr,
                    tbr_kbps,
                    ext,
                    has_audio: true,
                    http_headers,
                });
            }
            (None, Some(acodec), _) => {
                let language = fmt
                    .get("language")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);
                let lang_pref = fmt.get("language_preference").and_then(Value::as_i64);
                let note_lower = format_note_opt
                    .as_deref()
                    .map(str::to_ascii_lowercase)
                    .unwrap_or_default();
                let explicit_original = note_lower.contains("original")
                    || note_lower.contains("default")
                    || lang_pref.is_some_and(|p| p >= 10)
                    || fmt.get("is_default").and_then(Value::as_bool) == Some(true);
                let explicit_dubbed = note_lower.contains("dubbed")
                    || note_lower.contains("dublado")
                    || lang_pref.is_some_and(|p| p < 0);
                let is_dubbed = explicit_dubbed && !explicit_original;
                let is_default = explicit_original;
                let language_label = build_audio_language_label(
                    language.as_deref(),
                    format_note_opt.as_deref(),
                    is_default,
                    is_dubbed,
                );
                audio_formats.push(AudioStreamFormat {
                    format_id,
                    url: url.to_owned(),
                    acodec,
                    abr_kbps,
                    ext,
                    language,
                    language_label,
                    is_default,
                    is_dubbed,
                    format_note: format_note_opt,
                    http_headers,
                });
            }
            _ => {}
        }
    }

    // Ensure at least one audio group is marked default when audio tracks exist.
    if !audio_formats.is_empty() && !audio_formats.iter().any(|a| a.is_default) {
        let default_lang = audio_formats
            .iter()
            .find(|a| !a.is_dubbed)
            .and_then(|a| a.language.as_deref())
            .map(str::to_ascii_lowercase);
        for track in &mut audio_formats {
            let matches_default = match (&default_lang, &track.language) {
                (Some(def), Some(lang)) => !track.is_dubbed && lang.eq_ignore_ascii_case(def),
                (None, _) => !track.is_dubbed,
                _ => false,
            };
            if matches_default {
                track.is_default = true;
                track.language_label = build_audio_language_label(
                    track.language.as_deref(),
                    track.format_note.as_deref(),
                    true,
                    false,
                );
            }
        }
    }

    if video_formats.is_empty() && audio_formats.is_empty() && muxed_formats.is_empty() {
        return Err("No playable stream formats found in manifest.".to_owned());
    }

    let expires_at = compute_expires_at(
        video_formats
            .iter()
            .map(|f| f.url.clone())
            .chain(audio_formats.iter().map(|f| f.url.clone()))
            .chain(muxed_formats.iter().map(|f| f.url.clone())),
        now,
        now_unix_secs,
    );

    Ok(StreamManifest {
        video_id: resolved_video_id,
        title,
        channel,
        channel_id,
        channel_verified,
        duration_secs,
        is_live,
        upload_date,
        video_formats,
        audio_formats,
        muxed_formats,
        chapters,
        captions,
        storyboard,
        fetched_at: now,
        expires_at,
    })
}

fn build_yt_dlp_extract_command(video_url: &str) -> Command {
    let program = crate::updater::managed_yt_dlp_path().unwrap_or_else(|| PathBuf::from("yt-dlp"));
    let mut cmd = Command::new(program);
    if let Some(cache_dir) = effective_yt_dlp_cache_dir() {
        let _ = std::fs::create_dir_all(&cache_dir);
        cmd.arg("--cache-dir").arg(cache_dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.args([
        "--dump-single-json",
        "--no-playlist",
        "--no-warnings",
        "--skip-download",
        "--",
        video_url,
    ]);
    cmd
}

/// Runs a single `yt-dlp --dump-single-json` extraction for `video_url` using the
/// managed binary and `crate::process::output` (inheriting Win32 Job Object & Cancellation).
pub fn extract_stream_manifest(
    video_url: &str,
    cancel: &Cancellation,
) -> Result<StreamManifest, String> {
    if cancel.cancelled() {
        return Err("Stream extraction cancelled.".to_owned());
    }
    let trimmed = video_url.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') || !is_safe_http_stream_url(trimmed) {
        return Err("Invalid video URL for stream extraction.".to_owned());
    }
    let mut cmd = build_yt_dlp_extract_command(trimmed);
    let output =
        process::output(&mut cmd, cancel, EXTRACT_TIMEOUT).map_err(|err| match err.kind() {
            std::io::ErrorKind::Interrupted => "Stream extraction cancelled.".to_owned(),
            std::io::ErrorKind::TimedOut => "Stream extraction timed out.".to_owned(),
            _ => "Failed to execute yt-dlp for stream extraction.".to_owned(),
        })?;
    if !output.status.success() {
        return Err("Stream extraction failed: yt-dlp returned a non-zero exit status.".to_owned());
    }
    let json: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Stream extraction failed: invalid JSON output from yt-dlp.".to_owned())?;
    let video_id = crate::history::youtube_video_id(trimmed).unwrap_or_default();
    parse_yt_dlp_single_json(&video_id, &json, Instant::now())
}

fn lang_base(lang: &str) -> &str {
    lang.split(['-', '_']).next().unwrap_or(lang)
}

fn compare_audio_quality(a: &AudioStreamFormat, b: &AudioStreamFormat) -> std::cmp::Ordering {
    let a_non_drc = !is_drc_audio(&a.format_id, a.format_note.as_deref());
    let b_non_drc = !is_drc_audio(&b.format_id, b.format_note.as_deref());
    let a_abr = a.abr_kbps.unwrap_or(0.0);
    let b_abr = b.abr_kbps.unwrap_or(0.0);
    a_non_drc
        .cmp(&b_non_drc)
        .then_with(|| a_abr.total_cmp(&b_abr))
        .then_with(|| audio_codec_rank(&a.acodec).cmp(&audio_codec_rank(&b.acodec)))
        .then_with(|| b.format_id.cmp(&a.format_id))
}

fn select_audio_format<'a>(
    audio_formats: &'a [AudioStreamFormat],
    preferred_audio_lang: Option<&str>,
) -> Option<&'a AudioStreamFormat> {
    if audio_formats.is_empty() {
        return None;
    }
    if let Some(pref) = preferred_audio_lang
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let pref_lower = pref.to_ascii_lowercase().replace('_', "-");
        let pref_base = lang_base(&pref_lower);
        let exact: Vec<&AudioStreamFormat> = audio_formats
            .iter()
            .filter(|a| {
                a.language
                    .as_deref()
                    .is_some_and(|l| l.trim().replace('_', "-").eq_ignore_ascii_case(&pref_lower))
            })
            .collect();
        if let Some(best) = exact.into_iter().max_by(|a, b| compare_audio_quality(a, b)) {
            return Some(best);
        }
        let base_match: Vec<&AudioStreamFormat> = audio_formats
            .iter()
            .filter(|a| {
                a.language
                    .as_deref()
                    .is_some_and(|l| lang_base(l.trim()).eq_ignore_ascii_case(pref_base))
            })
            .collect();
        if let Some(best) = base_match
            .into_iter()
            .max_by(|a, b| compare_audio_quality(a, b))
        {
            return Some(best);
        }
    }

    let defaults: Vec<&AudioStreamFormat> = audio_formats
        .iter()
        .filter(|a| a.is_default && !a.is_dubbed)
        .collect();
    if let Some(best) = defaults
        .into_iter()
        .max_by(|a, b| compare_audio_quality(a, b))
    {
        return Some(best);
    }

    let non_dubbed: Vec<&AudioStreamFormat> =
        audio_formats.iter().filter(|a| !a.is_dubbed).collect();
    if let Some(best) = non_dubbed
        .into_iter()
        .max_by(|a, b| compare_audio_quality(a, b))
    {
        return Some(best);
    }

    audio_formats
        .iter()
        .max_by(|a, b| compare_audio_quality(a, b))
}

fn compare_video_at_height(
    a: &VideoStreamFormat,
    b: &VideoStreamFormat,
    codec_pref: VideoCodecPreference,
) -> std::cmp::Ordering {
    let pref_a = matches_codec_pref(&a.vcodec, codec_pref);
    let pref_b = matches_codec_pref(&b.vcodec, codec_pref);
    let fps_a = a.fps.unwrap_or(0.0);
    let fps_b = b.fps.unwrap_or(0.0);
    let tbr_a = a.tbr_kbps.unwrap_or(0.0);
    let tbr_b = b.tbr_kbps.unwrap_or(0.0);

    match codec_pref {
        VideoCodecPreference::Auto => fps_a
            .total_cmp(&fps_b)
            .then_with(|| tbr_a.total_cmp(&tbr_b))
            .then_with(|| codec_auto_rank(&a.vcodec).cmp(&codec_auto_rank(&b.vcodec)))
            .then_with(|| b.format_id.cmp(&a.format_id)),
        _ => pref_a
            .cmp(&pref_b)
            .then_with(|| fps_a.total_cmp(&fps_b))
            .then_with(|| tbr_a.total_cmp(&tbr_b))
            .then_with(|| b.format_id.cmp(&a.format_id)),
    }
}

fn pick_best_video_from(
    candidates: &[VideoStreamFormat],
    max_height: Option<u32>,
    exact_height: Option<u32>,
    codec_pref: VideoCodecPreference,
) -> Option<&VideoStreamFormat> {
    if candidates.is_empty() {
        return None;
    }
    let target_height = if let Some(exact) = exact_height {
        if candidates.iter().any(|f| f.height == exact) {
            exact
        } else {
            candidates
                .iter()
                .filter(|f| f.height <= exact)
                .map(|f| f.height)
                .max()
                .or_else(|| candidates.iter().map(|f| f.height).min())?
        }
    } else {
        let cap = max_height.unwrap_or(1080);
        candidates
            .iter()
            .filter(|f| f.height <= cap)
            .map(|f| f.height)
            .max()
            .or_else(|| candidates.iter().map(|f| f.height).min())?
    };

    candidates
        .iter()
        .filter(|f| f.height == target_height)
        .max_by(|a, b| compare_video_at_height(a, b, codec_pref))
}

fn format_quality_label(height: u32, max_fps: Option<f32>, is_hdr: bool) -> String {
    let fps_suffix = max_fps
        .filter(|&fps| fps >= HIGH_FPS_THRESHOLD)
        .map(|fps| fps.round() as u32)
        .map(|fps| fps.to_string())
        .unwrap_or_default();
    let hdr_suffix = if is_hdr { " HDR" } else { "" };
    format!("{height}p{fps_suffix}{hdr_suffix}")
}

fn merge_headers(
    primary: &HashMap<String, String>,
    secondary: Option<&HashMap<String, String>>,
) -> HashMap<String, String> {
    let mut merged = primary.clone();
    if let Some(sec) = secondary {
        for (k, v) in sec {
            merged.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    merged
}

/// Selects the optimal video + audio stream pair (or audio-only / muxed fallback)
/// from a parsed `StreamManifest`.
pub fn select_stream_pair(
    manifest: &StreamManifest,
    max_height: Option<u32>,
    exact_height: Option<u32>,
    codec_pref: VideoCodecPreference,
    preferred_audio_lang: Option<&str>,
    audio_only: bool,
) -> Option<SelectedStreamPair> {
    if audio_only {
        if let Some(audio) = select_audio_format(&manifest.audio_formats, preferred_audio_lang) {
            let abr_note = audio
                .abr_kbps
                .map(|kbps| format!(" {}kbps", kbps.round() as u32))
                .unwrap_or_default();
            let format_summary = format!(
                "Audio only · {}{} · {}",
                audio.acodec, abr_note, audio.language_label
            );
            return Some(SelectedStreamPair {
                video_url: audio.url.clone(),
                audio_url: None,
                height: 0,
                fps: None,
                is_hdr: false,
                vcodec: String::new(),
                audio_language: audio.language.clone(),
                audio_label: Some(audio.language_label.clone()),
                http_headers: audio.http_headers.clone(),
                format_summary,
            });
        }
        // Fallback to lowest-bandwidth muxed stream when no audio-only stream exists.
        let muxed = manifest.muxed_formats.iter().min_by(|a, b| {
            a.height.cmp(&b.height).then_with(|| {
                a.tbr_kbps
                    .unwrap_or(f64::MAX)
                    .total_cmp(&b.tbr_kbps.unwrap_or(f64::MAX))
            })
        })?;
        return Some(SelectedStreamPair {
            video_url: muxed.url.clone(),
            audio_url: None,
            height: 0,
            fps: None,
            is_hdr: false,
            vcodec: String::new(),
            audio_language: None,
            audio_label: None,
            http_headers: muxed.http_headers.clone(),
            format_summary: format!("Audio only (muxed fallback · {})", muxed.ext),
        });
    }

    let selected_audio = select_audio_format(&manifest.audio_formats, preferred_audio_lang);

    // Prefer split adaptive video + audio whenever both are available, unless exact_height
    // exists only in muxed formats.
    let use_adaptive = !manifest.video_formats.is_empty()
        && selected_audio.is_some()
        && exact_height.is_none_or(|exact| {
            manifest.video_formats.iter().any(|f| f.height == exact)
                || !manifest.muxed_formats.iter().any(|f| f.height == exact)
        });

    if use_adaptive
        && let Some(video) = pick_best_video_from(
            &manifest.video_formats,
            max_height,
            exact_height,
            codec_pref,
        )
        && let Some(audio) = selected_audio
    {
        let quality_label = format_quality_label(video.height, video.fps, video.is_hdr);
        let codec_name = normalize_codec_family(&video.vcodec).unwrap_or(&video.vcodec);
        let format_summary = format!(
            "{quality_label} · {codec_name} + {} ({})",
            audio.acodec, audio.language_label
        );
        return Some(SelectedStreamPair {
            video_url: video.url.clone(),
            audio_url: Some(audio.url.clone()),
            height: video.height,
            fps: video.fps,
            is_hdr: video.is_hdr,
            vcodec: video.vcodec.clone(),
            audio_language: audio.language.clone(),
            audio_label: Some(audio.language_label.clone()),
            http_headers: merge_headers(&video.http_headers, Some(&audio.http_headers)),
            format_summary,
        });
    }

    // Graceful fallback to muxed streams when adaptive streams are unavailable.
    if let Some(muxed) = pick_best_video_from(
        &manifest.muxed_formats,
        max_height,
        exact_height,
        codec_pref,
    ) {
        let quality_label = format_quality_label(muxed.height, muxed.fps, muxed.is_hdr);
        let codec_name = normalize_codec_family(&muxed.vcodec).unwrap_or(&muxed.vcodec);
        let format_summary = format!("{quality_label} · {codec_name} (muxed)");
        return Some(SelectedStreamPair {
            video_url: muxed.url.clone(),
            audio_url: None,
            height: muxed.height,
            fps: muxed.fps,
            is_hdr: muxed.is_hdr,
            vcodec: muxed.vcodec.clone(),
            audio_language: None,
            audio_label: None,
            http_headers: muxed.http_headers.clone(),
            format_summary,
        });
    }

    None
}

/// Returns deduplicated quality options sorted from highest to lowest resolution,
/// annotated with high-FPS (`>= 48`), HDR badge, and available codec families.
pub fn available_quality_options(manifest: &StreamManifest) -> Vec<QualityOption> {
    struct Accumulator {
        max_fps: Option<f32>,
        is_hdr: bool,
        codecs: Vec<String>,
    }

    let mut by_height: HashMap<u32, Accumulator> = HashMap::new();
    for fmt in manifest
        .video_formats
        .iter()
        .chain(manifest.muxed_formats.iter())
    {
        if fmt.height == 0 {
            continue;
        }
        let acc = by_height.entry(fmt.height).or_insert_with(|| Accumulator {
            max_fps: None,
            is_hdr: false,
            codecs: Vec::new(),
        });
        if let Some(fps) = fmt.fps {
            acc.max_fps = Some(acc.max_fps.map_or(fps, |prev| prev.max(fps)));
        }
        acc.is_hdr |= fmt.is_hdr;
        let codec = normalize_codec_family(&fmt.vcodec)
            .map(str::to_owned)
            .unwrap_or_else(|| fmt.vcodec.clone());
        if !codec.is_empty() && !acc.codecs.contains(&codec) {
            acc.codecs.push(codec);
        }
    }

    let mut options: Vec<QualityOption> = by_height
        .into_iter()
        .map(|(height, mut acc)| {
            acc.codecs.sort();
            let high_fps = acc.max_fps.is_some_and(|fps| fps >= HIGH_FPS_THRESHOLD);
            let max_fps_rounded = acc.max_fps.map(|fps| fps.round() as u32);
            let label = format_quality_label(height, acc.max_fps, acc.is_hdr);
            QualityOption {
                height,
                max_fps: max_fps_rounded,
                high_fps,
                is_hdr: acc.is_hdr,
                codecs: acc.codecs,
                label,
            }
        })
        .collect();

    options.sort_by_key(|a| std::cmp::Reverse(a.height));
    options
}

/// Returns deduplicated audio tracks (including multi-language dubbing tracks),
/// choosing the highest-quality format per `(language, is_dubbed)` bucket.
pub fn available_audio_tracks(manifest: &StreamManifest) -> Vec<ManifestAudioOption> {
    let mut best_by_key: HashMap<(String, bool), &AudioStreamFormat> = HashMap::new();
    for track in &manifest.audio_formats {
        let lang_key = track
            .language
            .as_deref()
            .map(|l| l.trim().replace('_', "-").to_ascii_lowercase())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| "default".to_owned());
        let key = (lang_key, track.is_dubbed);
        match best_by_key.get(&key) {
            Some(existing)
                if compare_audio_quality(existing, track) != std::cmp::Ordering::Less => {}
            _ => {
                best_by_key.insert(key, track);
            }
        }
    }

    let mut options: Vec<ManifestAudioOption> = best_by_key
        .into_values()
        .map(|track| ManifestAudioOption {
            language_code: track
                .language
                .clone()
                .unwrap_or_else(|| "default".to_owned()),
            label: track.language_label.clone(),
            is_default: track.is_default,
            is_dubbed: track.is_dubbed,
            best_format_id: track.format_id.clone(),
            best_abr_kbps: track.abr_kbps,
            acodec: track.acodec.clone(),
        })
        .collect();

    options.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then_with(|| a.is_dubbed.cmp(&b.is_dubbed))
            .then_with(|| {
                a.label
                    .to_ascii_lowercase()
                    .cmp(&b.label.to_ascii_lowercase())
            })
            .then_with(|| a.language_code.cmp(&b.language_code))
    });
    options
}

struct ManifestCacheEntry {
    video_id: String,
    manifest: Arc<StreamManifest>,
}

struct StreamManifestLru {
    capacity: usize,
    entries: VecDeque<ManifestCacheEntry>,
}

impl StreamManifestLru {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::new(),
        }
    }

    fn purge_expired(&mut self, now: Instant) {
        self.entries
            .retain(|entry| !entry.manifest.is_expired_at(now));
    }

    fn get(&mut self, video_id: &str, now: Instant) -> Option<Arc<StreamManifest>> {
        self.purge_expired(now);
        let pos = self
            .entries
            .iter()
            .position(|entry| entry.video_id == video_id)?;
        let entry = self.entries.remove(pos)?;
        let manifest = Arc::clone(&entry.manifest);
        self.entries.push_back(entry);
        Some(manifest)
    }

    fn contains_fresh(&mut self, video_id: &str, now: Instant) -> bool {
        self.purge_expired(now);
        self.entries.iter().any(|entry| entry.video_id == video_id)
    }

    fn insert(&mut self, manifest: StreamManifest, now: Instant) -> Arc<StreamManifest> {
        self.purge_expired(now);
        let video_id = manifest.video_id.clone();
        let arc = Arc::new(manifest);
        if let Some(pos) = self.entries.iter().position(|e| e.video_id == video_id) {
            self.entries.remove(pos);
        }
        if arc.is_expired_at(now) {
            return arc;
        }
        while self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(ManifestCacheEntry {
            video_id,
            manifest: Arc::clone(&arc),
        });
        arc
    }

    fn invalidate(&mut self, video_id: &str) -> bool {
        if let Some(pos) = self.entries.iter().position(|e| e.video_id == video_id) {
            self.entries.remove(pos);
            true
        } else {
            false
        }
    }
}

/// Thread-safe bounded TTL LRU cache of parsed `StreamManifest` instances.
pub struct StreamManifestCache {
    inner: Mutex<StreamManifestLru>,
}

impl Default for StreamManifestCache {
    fn default() -> Self {
        Self::new(MAX_MANIFEST_CACHE_ENTRIES)
    }
}

impl StreamManifestCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(StreamManifestLru::new(capacity)),
        }
    }

    pub fn get(&self, video_id: &str) -> Option<Arc<StreamManifest>> {
        self.get_at(video_id, Instant::now())
    }

    pub fn get_at(&self, video_id: &str, now: Instant) -> Option<Arc<StreamManifest>> {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(video_id, now)
    }

    pub fn contains_fresh(&self, video_id: &str) -> bool {
        self.contains_fresh_at(video_id, Instant::now())
    }

    pub fn contains_fresh_at(&self, video_id: &str, now: Instant) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .contains_fresh(video_id, now)
    }

    pub fn insert(&self, manifest: StreamManifest) -> Arc<StreamManifest> {
        self.insert_at(manifest, Instant::now())
    }

    pub fn insert_at(&self, manifest: StreamManifest, now: Instant) -> Arc<StreamManifest> {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(manifest, now)
    }

    /// Invalidates a cached manifest (e.g. on HTTP 403 or libmpv playback error).
    pub fn invalidate(&self, video_id: &str) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .invalidate(video_id)
    }

    #[allow(dead_code)]
    pub fn clear(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .entries
            .clear();
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .entries
            .len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

struct ActivePrefetch {
    video_id: String,
    generation: u64,
    cancel: Cancellation,
}

/// Single-slot background speculative prefetcher for the upcoming Queue or Playlist video.
/// Deduplicates requests that are already cached or in-flight and cancels superseded jobs.
pub struct StreamPrefetcher {
    cache: Arc<StreamManifestCache>,
    slot: Arc<Mutex<Option<ActivePrefetch>>>,
    next_generation: AtomicU64,
}

impl StreamPrefetcher {
    pub fn new(cache: Arc<StreamManifestCache>) -> Self {
        Self {
            cache,
            slot: Arc::new(Mutex::new(None)),
            next_generation: AtomicU64::new(1),
        }
    }

    #[allow(dead_code)]
    pub fn cache(&self) -> &Arc<StreamManifestCache> {
        &self.cache
    }

    #[allow(dead_code)]
    pub fn in_flight_video_id(&self) -> Option<String> {
        self.slot
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_ref()
            .map(|active| active.video_id.clone())
    }

    pub fn cancel(&self) {
        let previous = self
            .slot
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .take();
        if let Some(active) = previous {
            active.cancel.cancel();
        }
    }

    /// Starts a speculative background extraction for `video_id` / `video_url` if it is
    /// not already cached and not already in-flight. Returns `true` if a new job started.
    #[allow(dead_code)]
    pub fn prefetch(&self, video_id: &str, video_url: &str) -> bool {
        self.prefetch_with(video_id, video_url, |url, cancel| {
            extract_stream_manifest(&url, &cancel)
        })
    }

    pub fn prefetch_with<F>(&self, video_id: &str, video_url: &str, extractor: F) -> bool
    where
        F: FnOnce(String, Cancellation) -> Result<StreamManifest, String> + Send + 'static,
    {
        let id = video_id.trim();
        let url = video_url.trim();
        if id.is_empty() || url.is_empty() {
            return false;
        }
        if self.cache.contains_fresh(id) {
            return false;
        }
        let (generation, cancel) = {
            let mut slot = self.slot.lock().unwrap_or_else(|err| err.into_inner());
            if slot.as_ref().is_some_and(|active| active.video_id == id) {
                return false;
            }
            if let Some(previous) = slot.take() {
                previous.cancel.cancel();
            }
            let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
            let cancel = Cancellation::default();
            *slot = Some(ActivePrefetch {
                video_id: id.to_owned(),
                generation,
                cancel: cancel.clone(),
            });
            (generation, cancel)
        };

        let cache = Arc::clone(&self.cache);
        let slot = Arc::clone(&self.slot);
        let url_owned = url.to_owned();
        let worker = move || {
            if !cancel.cancelled()
                && let Ok(manifest) = extractor(url_owned, cancel.clone())
                && !cancel.cancelled()
            {
                cache.insert(manifest);
            }
            let mut guard = slot.lock().unwrap_or_else(|err| err.into_inner());
            if guard
                .as_ref()
                .is_some_and(|active| active.generation == generation)
            {
                *guard = None;
            }
        };

        if std::thread::Builder::new()
            .name("fasttube-prefetch".to_owned())
            .spawn(worker)
            .is_err()
        {
            let mut guard = self.slot.lock().unwrap_or_else(|err| err.into_inner());
            if guard
                .as_ref()
                .is_some_and(|active| active.generation == generation)
            {
                *guard = None;
            }
            return false;
        }
        true
    }
}

impl Drop for StreamPrefetcher {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::mpsc;

    fn sample_single_json(expire_epoch: u64) -> Value {
        let exp = expire_epoch;
        json!({
            "id": "dQw4w9WgXcQ",
            "title": "Sample 4K60 HDR Video",
            "channel": "FastTube Studio",
            "channel_id": "UC1234567890123456789012",
            "channel_is_verified": true,
            "duration": 612.5,
            "is_live": false,
            "http_headers": {
                "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
                "Referer": "https://www.youtube.com/",
                "Cookie": "SAPISID=secret_should_never_be_kept",
                "Authorization": "Bearer secret_token"
            },
            "chapters": [
                { "title": "Intro", "start_time": 0.0, "end_time": 45.0 },
                { "title": "Benchmarks", "start_time": 45.0, "end_time": 300.0 },
                { "title": "Wrap-up", "start_time": 300.0 }
            ],
            "subtitles": {
                "pt-BR": [
                    {
                        "ext": "vtt",
                        "name": "Português (Brasil)",
                        "url": "https://www.youtube.com/api/timedtext?v=dQw4w9WgXcQ&lang=pt-BR&fmt=vtt"
                    }
                ]
            },
            "automatic_captions": {
                "en": [
                    {
                        "ext": "vtt",
                        "name": "English (auto)",
                        "url": "https://www.youtube.com/api/timedtext?v=dQw4w9WgXcQ&lang=en&kind=asr&fmt=vtt"
                    }
                ],
                "fr": [
                    {
                        "ext": "vtt",
                        "name": "French (translated)",
                        "url": "https://www.youtube.com/api/timedtext?v=dQw4w9WgXcQ&lang=en&tlang=fr&fmt=vtt"
                    }
                ]
            },
            "formats": [
                {
                    "format_id": "sb1",
                    "ext": "mhtml",
                    "protocol": "mhtml",
                    "format_note": "storyboard",
                    "rows": 5,
                    "columns": 5,
                    "width": 160,
                    "height": 90,
                    "fps": 0.25,
                    "fragments": [
                        { "url": "https://i.ytimg.com/sb/dQw4w9WgXcQ/storyboard3_L2/M0.jpg", "duration": 100.0 },
                        { "url": "https://i.ytimg.com/sb/dQw4w9WgXcQ/storyboard3_L2/M1.jpg", "duration": 100.0 }
                    ]
                },
                {
                    "format_id": "18",
                    "ext": "mp4",
                    "protocol": "https",
                    "url": format!("https://rr1.googlevideo.com/videoplayback?itag=18&expire={exp}"),
                    "height": 360,
                    "width": 640,
                    "fps": 30.0,
                    "vcodec": "avc1.42001E",
                    "acodec": "mp4a.40.2",
                    "tbr": 520.0
                },
                {
                    "format_id": "137",
                    "ext": "mp4",
                    "protocol": "https",
                    "url": format!("https://rr1.googlevideo.com/videoplayback?itag=137&expire={exp}"),
                    "height": 1080,
                    "width": 1920,
                    "fps": 30.0,
                    "vcodec": "avc1.640028",
                    "acodec": "none",
                    "tbr": 3200.0
                },
                {
                    "format_id": "299",
                    "ext": "mp4",
                    "protocol": "https",
                    "url": format!("https://rr1.googlevideo.com/videoplayback?itag=299&expire={exp}"),
                    "height": 1080,
                    "width": 1920,
                    "fps": 60.0,
                    "vcodec": "avc1.64002a",
                    "acodec": "none",
                    "tbr": 4800.0
                },
                {
                    "format_id": "303",
                    "ext": "webm",
                    "protocol": "https",
                    "url": format!("https://rr1.googlevideo.com/videoplayback?itag=303&expire={exp}"),
                    "height": 1080,
                    "width": 1920,
                    "fps": 60.0,
                    "vcodec": "vp9",
                    "acodec": "none",
                    "tbr": 4200.0
                },
                {
                    "format_id": "399",
                    "ext": "mp4",
                    "protocol": "https",
                    "url": format!("https://rr1.googlevideo.com/videoplayback?itag=399&expire={exp}"),
                    "height": 1080,
                    "width": 1920,
                    "fps": 60.0,
                    "vcodec": "av01.0.09M.08",
                    "acodec": "none",
                    "tbr": 3600.0
                },
                {
                    "format_id": "337",
                    "ext": "webm",
                    "protocol": "https",
                    "url": format!("https://rr1.googlevideo.com/videoplayback?itag=337&expire={exp}"),
                    "height": 2160,
                    "width": 3840,
                    "fps": 60.0,
                    "vcodec": "vp9.2",
                    "dynamic_range": "HDR10",
                    "acodec": "none",
                    "tbr": 22000.0
                },
                {
                    "format_id": "251-en",
                    "ext": "webm",
                    "protocol": "https",
                    "url": format!("https://rr1.googlevideo.com/videoplayback?itag=251&lang=en&expire={exp}"),
                    "vcodec": "none",
                    "acodec": "opus",
                    "abr": 140.0,
                    "language": "en",
                    "language_preference": 10,
                    "format_note": "English original, medium"
                },
                {
                    "format_id": "251-pt",
                    "ext": "webm",
                    "protocol": "https",
                    "url": format!("https://rr1.googlevideo.com/videoplayback?itag=251&lang=pt&expire={exp}"),
                    "vcodec": "none",
                    "acodec": "opus",
                    "abr": 132.0,
                    "language": "pt-BR",
                    "language_preference": -10,
                    "format_note": "Portuguese (Brazil) dubbed, medium"
                },
                {
                    "format_id": "140-es",
                    "ext": "m4a",
                    "protocol": "https",
                    "url": format!("https://rr1.googlevideo.com/videoplayback?itag=140&lang=es&expire={exp}"),
                    "vcodec": "none",
                    "acodec": "mp4a.40.2",
                    "abr": 128.0,
                    "language": "es",
                    "language_preference": -10,
                    "format_note": "Spanish dubbed, medium"
                }
            ]
        })
    }

    #[test]
    fn parse_single_json_extracts_formats_hdr_dubbing_chapters_captions_storyboard_and_ttl() {
        let now = Instant::now();
        let now_unix = 1_700_000_000_u64;
        // Expire 600s from now -> minus 120s safety margin = 480s TTL.
        let json = sample_single_json(now_unix + 600);
        let manifest =
            parse_yt_dlp_single_json_at("dQw4w9WgXcQ", &json, now, now_unix).expect("valid parse");

        assert_eq!(manifest.video_id, "dQw4w9WgXcQ");
        assert_eq!(manifest.title, "Sample 4K60 HDR Video");
        assert_eq!(manifest.channel, "FastTube Studio");
        assert_eq!(
            manifest.channel_id.as_deref(),
            Some("UC1234567890123456789012")
        );
        assert!(manifest.channel_verified);
        assert_eq!(manifest.duration_secs, Some(612.5));
        assert!(!manifest.is_live);

        // Video, audio, muxed separation.
        assert_eq!(manifest.video_formats.len(), 5);
        assert_eq!(manifest.audio_formats.len(), 3);
        assert_eq!(manifest.muxed_formats.len(), 1);
        assert!(manifest.muxed_formats[0].has_audio);
        assert!(manifest.video_formats.iter().all(|f| !f.has_audio));

        // HDR detection on 2160p60 vp9.2 HDR10.
        let hdr = manifest
            .video_formats
            .iter()
            .find(|f| f.format_id == "337")
            .unwrap();
        assert!(hdr.is_hdr);
        assert_eq!(hdr.height, 2160);
        assert_eq!(hdr.fps, Some(60.0));
        // Sensitive headers must never be kept.
        assert!(hdr.http_headers.contains_key("User-Agent"));
        assert!(hdr.http_headers.contains_key("Referer"));
        assert!(!hdr.http_headers.contains_key("Cookie"));
        assert!(!hdr.http_headers.contains_key("Authorization"));

        // Multi-language audio dubbing tracks.
        let audio_opts = available_audio_tracks(&manifest);
        assert_eq!(audio_opts.len(), 3);
        assert_eq!(audio_opts[0].language_code, "en");
        assert!(audio_opts[0].is_default);
        assert!(!audio_opts[0].is_dubbed);
        assert!(
            audio_opts
                .iter()
                .any(|a| a.language_code == "pt-BR" && a.is_dubbed && !a.is_default)
        );
        assert!(
            audio_opts
                .iter()
                .any(|a| a.language_code == "es" && a.is_dubbed && !a.is_default)
        );

        // Quality options with FPS and HDR badges.
        let qualities = available_quality_options(&manifest);
        assert_eq!(qualities.len(), 3);
        assert_eq!(qualities[0].height, 2160);
        assert!(qualities[0].high_fps);
        assert!(qualities[0].is_hdr);
        assert_eq!(qualities[0].label, "2160p60 HDR");
        assert_eq!(qualities[1].height, 1080);
        assert!(qualities[1].high_fps);
        assert!(!qualities[1].is_hdr);
        assert_eq!(qualities[1].label, "1080p60");
        assert_eq!(qualities[1].codecs, vec!["AV1", "H.264", "VP9"]);
        assert_eq!(qualities[2].height, 360);
        assert!(!qualities[2].high_fps);
        assert_eq!(qualities[2].label, "360p");

        // Chapters (including last chapter end_secs filled from duration).
        assert_eq!(manifest.chapters.len(), 3);
        assert_eq!(manifest.chapters[0].title, "Intro");
        assert_eq!(manifest.chapters[2].start_secs, 300.0);
        assert_eq!(manifest.chapters[2].end_secs, 612.5);

        // Captions (manual before auto, tlang filtered out when native tracks exist).
        assert_eq!(manifest.captions.len(), 2);
        assert_eq!(manifest.captions[0].language_code, "pt-BR");
        assert!(!manifest.captions[0].is_auto_generated);
        assert_eq!(manifest.captions[1].language_code, "en");
        assert!(manifest.captions[1].is_auto_generated);

        // Storyboard spec.
        let sb = manifest.storyboard.as_ref().expect("storyboard parsed");
        assert_eq!(sb.rows, 5);
        assert_eq!(sb.columns, 5);
        assert_eq!(sb.width, 160);
        assert_eq!(sb.height, 90);
        assert_eq!(sb.fps, Some(0.25));
        assert_eq!(sb.fragments.len(), 2);

        // TTL computation: 600s - 120s margin = 480s.
        assert_eq!(
            manifest.expires_at.duration_since(manifest.fetched_at),
            Duration::from_secs(480)
        );
        // Very long expire= is capped at MAX_MANIFEST_TTL (20 minutes).
        let long_json = sample_single_json(now_unix + 6 * 3600);
        let long_manifest =
            parse_yt_dlp_single_json_at("dQw4w9WgXcQ", &long_json, now, now_unix).unwrap();
        assert_eq!(
            long_manifest
                .expires_at
                .duration_since(long_manifest.fetched_at),
            MAX_MANIFEST_TTL
        );
    }

    #[test]
    fn select_stream_pair_respects_codec_pref_heights_dubbing_and_audio_only() {
        let now = Instant::now();
        let now_unix = 1_700_000_000_u64;
        let json = sample_single_json(now_unix + 1200);
        let manifest = parse_yt_dlp_single_json_at("dQw4w9WgXcQ", &json, now, now_unix).unwrap();

        // Auto at max_height=1080 picks highest fps/tbr (1080p60) + default original audio (en).
        let auto_1080 = select_stream_pair(
            &manifest,
            Some(1080),
            None,
            VideoCodecPreference::Auto,
            None,
            false,
        )
        .unwrap();
        assert_eq!(auto_1080.height, 1080);
        assert_eq!(auto_1080.fps, Some(60.0));
        assert_eq!(auto_1080.audio_language.as_deref(), Some("en"));
        assert!(auto_1080.audio_url.is_some());

        // Codec preference: VP9, H264, AV1 at exact_height=1080.
        let vp9 = select_stream_pair(
            &manifest,
            None,
            Some(1080),
            VideoCodecPreference::Vp9,
            None,
            false,
        )
        .unwrap();
        assert!(vp9.vcodec.starts_with("vp9"));
        assert_eq!(vp9.fps, Some(60.0));

        let h264 = select_stream_pair(
            &manifest,
            None,
            Some(1080),
            VideoCodecPreference::H264,
            None,
            false,
        )
        .unwrap();
        assert!(h264.vcodec.starts_with("avc1"));
        assert_eq!(h264.fps, Some(60.0));

        let av1 = select_stream_pair(
            &manifest,
            None,
            Some(1080),
            VideoCodecPreference::Av1,
            None,
            false,
        )
        .unwrap();
        assert!(av1.vcodec.starts_with("av01"));

        // Max height 2160 picks 4K60 HDR.
        let uhd = select_stream_pair(
            &manifest,
            Some(2160),
            None,
            VideoCodecPreference::Auto,
            None,
            false,
        )
        .unwrap();
        assert_eq!(uhd.height, 2160);
        assert!(uhd.is_hdr);

        // Preferred audio dubbing language (base match "pt" -> "pt-BR").
        let dubbed_pt = select_stream_pair(
            &manifest,
            Some(1080),
            None,
            VideoCodecPreference::Vp9,
            Some("pt"),
            false,
        )
        .unwrap();
        assert_eq!(dubbed_pt.audio_language.as_deref(), Some("pt-BR"));
        assert!(dubbed_pt.audio_url.as_deref().unwrap().contains("lang=pt"));

        // Unknown audio language falls back gracefully to original default ("en").
        let fallback_lang = select_stream_pair(
            &manifest,
            Some(1080),
            None,
            VideoCodecPreference::Auto,
            Some("de"),
            false,
        )
        .unwrap();
        assert_eq!(fallback_lang.audio_language.as_deref(), Some("en"));

        // Exact height only present in muxed formats falls back to muxed stream.
        let muxed_360 = select_stream_pair(
            &manifest,
            None,
            Some(360),
            VideoCodecPreference::Auto,
            None,
            false,
        )
        .unwrap();
        assert_eq!(muxed_360.height, 360);
        assert!(muxed_360.audio_url.is_none());

        // Audio-only mode returns direct audio stream URL with audio_url = None.
        let audio_only = select_stream_pair(
            &manifest,
            Some(1080),
            None,
            VideoCodecPreference::Auto,
            Some("es"),
            true,
        )
        .unwrap();
        assert_eq!(audio_only.height, 0);
        assert!(audio_only.audio_url.is_none());
        assert_eq!(audio_only.audio_language.as_deref(), Some("es"));
        assert!(audio_only.video_url.contains("itag=140"));
    }

    #[test]
    fn stream_manifest_cache_enforces_lru_eviction_ttl_and_invalidation() {
        let t0 = Instant::now();
        let cache = StreamManifestCache::new(2);
        let make = |id: &str, ttl_secs: u64| {
            let mut m = parse_yt_dlp_single_json_at(
                id,
                &sample_single_json(1_700_000_000 + 600),
                t0,
                1_700_000_000,
            )
            .unwrap();
            m.expires_at = t0 + Duration::from_secs(ttl_secs);
            m
        };

        cache.insert_at(make("vidA", 300), t0);
        cache.insert_at(make("vidB", 300), t0);
        assert_eq!(cache.len(), 2);

        // Touch vidA so vidB becomes least recently used.
        assert!(cache.get_at("vidA", t0 + Duration::from_secs(10)).is_some());
        cache.insert_at(make("vidC", 60), t0 + Duration::from_secs(10));
        assert_eq!(cache.len(), 2);
        assert!(cache.get_at("vidB", t0 + Duration::from_secs(11)).is_none());
        assert!(cache.get_at("vidA", t0 + Duration::from_secs(11)).is_some());
        assert!(cache.get_at("vidC", t0 + Duration::from_secs(11)).is_some());

        // TTL expiration evicts vidC after 60s while vidA remains fresh.
        assert!(cache.get_at("vidC", t0 + Duration::from_secs(61)).is_none());
        assert!(cache.get_at("vidA", t0 + Duration::from_secs(61)).is_some());

        // Explicit invalidation (e.g. HTTP 403 / playback error) removes immediately.
        assert!(cache.invalidate("vidA"));
        assert!(!cache.invalidate("vidA"));
        assert!(cache.get_at("vidA", t0 + Duration::from_secs(62)).is_none());
        assert!(cache.is_empty());
    }

    #[test]
    fn stream_prefetcher_deduplicates_cached_and_in_flight_and_cancels_superseded() {
        let cache = Arc::new(StreamManifestCache::new(8));
        let prefetcher = StreamPrefetcher::new(Arc::clone(&cache));

        // 1. Pre-cached video is skipped without starting a worker.
        let cached_manifest = parse_yt_dlp_single_json_at(
            "cached11111",
            &sample_single_json(1_700_000_000 + 600),
            Instant::now(),
            1_700_000_000,
        )
        .unwrap();
        cache.insert(cached_manifest);
        assert!(!prefetcher.prefetch_with(
            "cached11111",
            "https://www.youtube.com/watch?v=cached11111",
            |_, _| panic!("cached video must not invoke extractor"),
        ));

        // 2. In-flight deduplication and cancellation when superseded by a newer video.
        let (started_tx, started_rx) = mpsc::channel();
        let (cancelled_tx, cancelled_rx) = mpsc::channel();
        assert!(prefetcher.prefetch_with(
            "first111111",
            "https://www.youtube.com/watch?v=first111111",
            move |_, cancel| {
                let _ = started_tx.send(());
                let deadline = Instant::now() + Duration::from_secs(3);
                while !cancel.cancelled() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                let _ = cancelled_tx.send(cancel.cancelled());
                Err("cancelled".to_owned())
            },
        ));
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("first prefetch started");
        assert_eq!(
            prefetcher.in_flight_video_id().as_deref(),
            Some("first111111")
        );

        // Duplicate request for the same in-flight video is deduplicated.
        assert!(!prefetcher.prefetch_with(
            "first111111",
            "https://www.youtube.com/watch?v=first111111",
            |_, _| panic!("in-flight video must be deduplicated"),
        ));

        // Starting a prefetch for a different video cancels the previous one and populates cache.
        let (done_tx, done_rx) = mpsc::channel();
        assert!(prefetcher.prefetch_with(
            "second22222",
            "https://www.youtube.com/watch?v=second22222",
            move |_, _| {
                let m = parse_yt_dlp_single_json_at(
                    "second22222",
                    &sample_single_json(1_700_000_000 + 600),
                    Instant::now(),
                    1_700_000_000,
                );
                let _ = done_tx.send(());
                m
            },
        ));

        assert!(
            cancelled_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("first job observed cancellation")
        );
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("second job completed");

        // Wait briefly for the worker to insert into cache and clear the active slot.
        let deadline = Instant::now() + Duration::from_secs(2);
        while cache.get("second22222").is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(cache.get("second22222").is_some());
        assert!(cache.get("first111111").is_none());
    }

    #[test]
    fn parse_yt_dlp_single_json_extracts_upload_date_from_timestamp_or_upload_date() {
        let mut with_ts = sample_single_json(4_102_444_800);
        with_ts["timestamp"] = serde_json::json!(1_794_000_000);
        with_ts["upload_date"] = serde_json::json!("20261106");
        let manifest_ts =
            parse_yt_dlp_single_json_at("video000001", &with_ts, Instant::now(), 1_700_000_000)
                .expect("manifest with timestamp");
        assert_eq!(
            manifest_ts.upload_date.as_deref(),
            Some("2026-11-06T21:20:00Z")
        );

        let mut date_only = sample_single_json(4_102_444_800);
        date_only["upload_date"] = serde_json::json!("20261106");
        let manifest_date =
            parse_yt_dlp_single_json_at("video000002", &date_only, Instant::now(), 1_700_000_000)
                .expect("manifest with upload_date");
        assert_eq!(manifest_date.upload_date.as_deref(), Some("20261106"));
    }
}
