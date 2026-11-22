use super::channel::{
    MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT, is_valid_channel_id, parse_channel_target,
};
use super::query::{is_playlist_url, parse_results};
use super::{
    PlaybackExtraInfo, QUERY_TIMEOUT, SUBTITLE_USER_AGENT, SearchResult, StoryboardFragment,
    StoryboardSpec, SubtitleTrack, VideoPlaybackMetadata, nonempty_str, parse_video_entry, yt_dlp,
};
use crate::process::{self, Cancellation};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub(super) const RESOLUTION_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

pub(super) const RESOLUTION_CACHE_LIMIT: usize = 32;

pub(super) const RESOLUTION_CACHE_MAX_URL_BYTES: usize = 4096;

pub(super) const RESOLUTION_CACHE_MAX_HEIGHTS: usize = 256;

pub(super) struct CachedResolutions {
    pub(super) url: String,
    heights: Vec<u32>,
    pub(super) inserted: Instant,
}

#[derive(Default)]
pub(super) struct ResolutionCache {
    pub(super) entries: Vec<CachedResolutions>,
    pub(super) in_flight: HashMap<String, Arc<InFlight>>,
}

#[derive(Default)]
pub(super) struct InFlight {
    result: Mutex<Option<Result<Vec<u32>, String>>>,
    ready: Condvar,
}

static RESOLUTION_CACHE: OnceLock<Mutex<ResolutionCache>> = OnceLock::new();

struct CachedSubtitleTracks {
    key: String,
    tracks: Vec<SubtitleTrack>,
    inserted: Instant,
}

#[derive(Default)]
struct SubtitleTrackCache {
    entries: Vec<CachedSubtitleTracks>,
}

static SUBTITLE_TRACK_CACHE: OnceLock<Mutex<SubtitleTrackCache>> = OnceLock::new();

fn subtitle_cache_key(url: &str) -> String {
    crate::history::youtube_video_id(url).unwrap_or_else(|| url.to_owned())
}

pub(super) fn get_cached_subtitle_tracks(url: &str) -> Option<Vec<SubtitleTrack>> {
    let key = subtitle_cache_key(url);
    if key.len() > RESOLUTION_CACHE_MAX_URL_BYTES {
        return None;
    }
    let cache_mutex =
        SUBTITLE_TRACK_CACHE.get_or_init(|| Mutex::new(SubtitleTrackCache::default()));
    let mut cache = cache_mutex
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let now = Instant::now();
    cache
        .entries
        .retain(|entry| now.duration_since(entry.inserted) < RESOLUTION_CACHE_TTL);
    cache
        .entries
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| entry.tracks.clone())
}

pub(super) fn store_cached_subtitle_tracks(url: &str, tracks: &[SubtitleTrack]) {
    let key = subtitle_cache_key(url);
    if key.len() > RESOLUTION_CACHE_MAX_URL_BYTES {
        return;
    }
    let cache_mutex =
        SUBTITLE_TRACK_CACHE.get_or_init(|| Mutex::new(SubtitleTrackCache::default()));
    let mut cache = cache_mutex
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let now = Instant::now();
    cache.entries.retain(|entry| {
        entry.key != key && now.duration_since(entry.inserted) < RESOLUTION_CACHE_TTL
    });
    if cache.entries.len() >= RESOLUTION_CACHE_LIMIT {
        cache.entries.remove(0);
    }
    cache.entries.push(CachedSubtitleTracks {
        key,
        tracks: tracks.to_vec(),
        inserted: now,
    });
}

struct CachedPlaybackExtra {
    key: String,
    info: PlaybackExtraInfo,
    inserted: Instant,
}

#[derive(Default)]
struct PlaybackExtraCache {
    entries: Vec<CachedPlaybackExtra>,
}

static PLAYBACK_EXTRA_CACHE: OnceLock<Mutex<PlaybackExtraCache>> = OnceLock::new();

fn get_cached_playback_extra(url: &str) -> Option<PlaybackExtraInfo> {
    let key = subtitle_cache_key(url);
    if key.len() > RESOLUTION_CACHE_MAX_URL_BYTES {
        return None;
    }
    let cache_mutex =
        PLAYBACK_EXTRA_CACHE.get_or_init(|| Mutex::new(PlaybackExtraCache::default()));
    let mut cache = cache_mutex
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let now = Instant::now();
    cache
        .entries
        .retain(|entry| now.duration_since(entry.inserted) < RESOLUTION_CACHE_TTL);
    cache
        .entries
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| entry.info.clone())
}

fn store_cached_playback_extra(url: &str, info: PlaybackExtraInfo) {
    let key = subtitle_cache_key(url);
    if key.len() > RESOLUTION_CACHE_MAX_URL_BYTES {
        return;
    }
    let cache_mutex =
        PLAYBACK_EXTRA_CACHE.get_or_init(|| Mutex::new(PlaybackExtraCache::default()));
    let mut cache = cache_mutex
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let now = Instant::now();
    cache.entries.retain(|entry| {
        entry.key != key && now.duration_since(entry.inserted) < RESOLUTION_CACHE_TTL
    });
    if cache.entries.len() >= RESOLUTION_CACHE_LIMIT {
        cache.entries.remove(0);
    }
    cache.entries.push(CachedPlaybackExtra {
        key,
        info,
        inserted: now,
    });
}

pub(super) fn cached_resolutions(
    cache: &Mutex<ResolutionCache>,
    url: &str,
    fetch: impl FnOnce() -> Result<Vec<u32>, String>,
) -> Result<Vec<u32>, String> {
    let (flight, leader) = {
        let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
        let now = Instant::now();
        cache
            .entries
            .retain(|entry| now.duration_since(entry.inserted) < RESOLUTION_CACHE_TTL);
        if let Some(entry) = cache.entries.iter().find(|entry| entry.url == url) {
            return Ok(entry.heights.clone());
        }
        if let Some(flight) = cache.in_flight.get(url) {
            (Arc::clone(flight), false)
        } else {
            let flight = Arc::new(InFlight::default());
            cache.in_flight.insert(url.to_owned(), Arc::clone(&flight));
            (flight, true)
        }
    };

    if !leader {
        let mut result = flight
            .result
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while result.is_none() {
            result = flight
                .ready
                .wait(result)
                .unwrap_or_else(|error| error.into_inner());
        }
        return result.as_ref().unwrap().clone();
    }

    let result = catch_unwind(AssertUnwindSafe(fetch));
    let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
    if let Ok(Ok(heights)) = &result
        && url.len() <= RESOLUTION_CACHE_MAX_URL_BYTES
        && !heights.is_empty()
        && heights.len() <= RESOLUTION_CACHE_MAX_HEIGHTS
    {
        let now = Instant::now();
        cache.entries.retain(|entry| {
            entry.url != url && now.duration_since(entry.inserted) < RESOLUTION_CACHE_TTL
        });
        if cache.entries.len() >= RESOLUTION_CACHE_LIMIT {
            cache.entries.remove(0);
        }
        cache.entries.push(CachedResolutions {
            url: url.to_owned(),
            heights: heights.clone(),
            inserted: now,
        });
    }
    let shared_result = match &result {
        Ok(result) => result.clone(),
        Err(_) => Err("A consulta de resoluções falhou inesperadamente.".to_owned()),
    };
    *flight
        .result
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(shared_result);
    cache.in_flight.remove(url);
    flight.ready.notify_all();
    drop(cache);
    match result {
        Ok(result) => result,
        Err(payload) => resume_unwind(payload),
    }
}

const SUBTITLE_MAX_BYTES: usize = 2 * 1024 * 1024;

const SUBTITLE_MAX_HLS_SEGMENTS: usize = 64;

fn validate_subtitle_url(value: &str) -> Result<url::Url, String> {
    let parsed = url::Url::parse(value).map_err(|_| "URL de legenda inválida.".to_owned())?;
    let host = parsed.host_str().unwrap_or("");
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || !(host == "youtube.com"
            || host.ends_with(".youtube.com")
            || host == "googlevideo.com"
            || host.ends_with(".googlevideo.com")
            || host == "ytimg.com"
            || host.ends_with(".ytimg.com"))
    {
        return Err("Origem da legenda não permitida.".into());
    }
    Ok(parsed)
}

pub(crate) fn is_allowed_subtitle_url(value: &str) -> bool {
    validate_subtitle_url(value).is_ok()
}

fn is_auto_translated_url(parsed: &url::Url) -> bool {
    parsed.query_pairs().any(|(key, _)| key == "tlang")
}

fn is_hls_playlist_format(format: &Value, parsed: &url::Url) -> bool {
    format["protocol"]
        .as_str()
        .is_some_and(|protocol| protocol.starts_with("m3u8"))
        || parsed.path().ends_with(".m3u8")
}

static SUBTITLE_HLS_FALLBACKS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

pub(super) fn subtitle_hls_fallbacks() -> &'static Mutex<HashMap<String, String>> {
    SUBTITLE_HLS_FALLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn canonical_subtitle_language(language: &str, automatic: bool) -> String {
    if automatic
        && language.len() > 5
        && language[language.len() - 5..].eq_ignore_ascii_case("-orig")
    {
        language[..language.len() - 5].to_owned()
    } else {
        language.to_owned()
    }
}

fn valid_sub_lang_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn url_has_asr_kind(parsed: &url::Url) -> bool {
    if parsed
        .query_pairs()
        .any(|(key, value)| key == "kind" && value.eq_ignore_ascii_case("asr"))
    {
        return true;
    }
    let lower_path = parsed.path().to_ascii_lowercase();
    lower_path.contains("kind%3dasr") || lower_path.contains("kind=asr")
}

fn register_hls_fallback(direct_url: &str, hls_url: &str) {
    if let Ok(mut map) = subtitle_hls_fallbacks().lock() {
        if map.len() > 128 {
            map.clear();
        }
        map.insert(direct_url.to_owned(), hls_url.to_owned());
    }
}

pub(crate) fn sanitize_subtitle_tracks(tracks: Vec<SubtitleTrack>) -> Vec<SubtitleTrack> {
    let mut best_by_key: HashMap<(bool, String), (bool, usize, SubtitleTrack)> = HashMap::new();
    for (idx, mut track) in tracks.into_iter().enumerate() {
        let canonical = canonical_subtitle_language(&track.language, track.automatic);
        let from_orig_alias = track.automatic && canonical != track.language;
        track.language = canonical;
        let key = (track.automatic, track.language.to_ascii_lowercase());
        match best_by_key.get(&key) {
            Some((existing_orig, existing_idx, _))
                if (*existing_orig, *existing_idx) <= (from_orig_alias, idx) => {}
            _ => {
                best_by_key.insert(key, (from_orig_alias, idx, track));
            }
        }
    }
    let mut out: Vec<SubtitleTrack> = best_by_key.into_values().map(|(_, _, t)| t).collect();
    out.sort_by(|left, right| {
        (
            left.automatic,
            left.language.to_ascii_lowercase(),
            &left.language,
        )
            .cmp(&(
                right.automatic,
                right.language.to_ascii_lowercase(),
                &right.language,
            ))
    });
    let mut seen_urls = HashSet::with_capacity(out.len());
    out.retain(|track| seen_urls.insert(track.url.clone()));
    out.truncate(80);
    out
}

#[derive(Clone)]
struct ParsedSubtitleCandidate {
    track: SubtitleTrack,
    translated: bool,
    is_hls_only: bool,
    has_asr_kind: bool,
    from_orig_alias: bool,
}

pub(super) fn parse_subtitles(value: &Value) -> Vec<SubtitleTrack> {
    let mut candidates = Vec::new();
    for (field, automatic) in [("subtitles", false), ("automatic_captions", true)] {
        let Some(languages) = value[field].as_object() else {
            continue;
        };
        for (language, formats) in languages {
            if !valid_sub_lang_token(language) {
                continue;
            }
            let Some((url, parsed, translated, is_hls_only)) =
                formats.as_array().and_then(|formats| {
                    let mut native_direct: Option<(&str, url::Url)> = None;
                    let mut native_hls: Option<(&str, url::Url)> = None;
                    let mut translated_direct: Option<(&str, url::Url)> = None;
                    let mut translated_hls: Option<(&str, url::Url)> = None;
                    for format in formats {
                        if format["ext"].as_str() != Some("vtt") {
                            continue;
                        }
                        let Some(value) = format["url"].as_str() else {
                            continue;
                        };
                        let Ok(parsed) = validate_subtitle_url(value) else {
                            continue;
                        };
                        let translated = is_auto_translated_url(&parsed);
                        let hls = is_hls_playlist_format(format, &parsed);
                        match (translated, hls) {
                            (false, false) if native_direct.is_none() => {
                                native_direct = Some((value, parsed));
                            }
                            (false, true) if native_hls.is_none() => {
                                native_hls = Some((value, parsed));
                            }
                            (true, false) if translated_direct.is_none() => {
                                translated_direct = Some((value, parsed));
                            }
                            (true, true) if translated_hls.is_none() => {
                                translated_hls = Some((value, parsed));
                            }
                            _ => {}
                        }
                    }
                    if let Some((d_url, d_parsed)) = native_direct {
                        if let Some((h_url, _)) = native_hls {
                            register_hls_fallback(d_url, h_url);
                        }
                        Some((d_url, d_parsed, false, false))
                    } else if let Some((h_url, h_parsed)) = native_hls {
                        Some((h_url, h_parsed, false, true))
                    } else if let Some((d_url, d_parsed)) = translated_direct {
                        if let Some((h_url, _)) = translated_hls {
                            register_hls_fallback(d_url, h_url);
                        }
                        Some((d_url, d_parsed, true, false))
                    } else if let Some((h_url, h_parsed)) = translated_hls {
                        Some((h_url, h_parsed, true, true))
                    } else {
                        None
                    }
                })
            else {
                continue;
            };
            let canonical_lang = canonical_subtitle_language(language, automatic);
            let from_orig_alias = automatic && canonical_lang != *language;
            let has_asr_kind = url_has_asr_kind(&parsed);
            candidates.push(ParsedSubtitleCandidate {
                track: SubtitleTrack {
                    language: canonical_lang,
                    automatic,
                    url: url.to_owned(),
                },
                translated,
                is_hls_only,
                has_asr_kind,
                from_orig_alias,
            });
        }
    }

    // YouTube injects non-ASR HLS mirrors of manual subtitles into `automatic_captions`.
    // Register them as HLS fallbacks for the matching manual track instead of listing duplicates.
    let mut manual_direct_urls: HashMap<String, Option<String>> = HashMap::new();
    for candidate in &candidates {
        if !candidate.track.automatic {
            let key = candidate.track.language.to_ascii_lowercase();
            let direct_url = (!candidate.is_hls_only).then(|| candidate.track.url.clone());
            manual_direct_urls
                .entry(key)
                .and_modify(|existing| {
                    if existing.is_none() {
                        *existing = direct_url.clone();
                    }
                })
                .or_insert(direct_url);
        }
    }
    candidates.retain(|candidate| {
        if candidate.track.automatic
            && !candidate.translated
            && candidate.is_hls_only
            && !candidate.has_asr_kind
            && let Some(maybe_direct) =
                manual_direct_urls.get(&candidate.track.language.to_ascii_lowercase())
        {
            if let Some(manual_direct_url) = maybe_direct {
                let already_registered = subtitle_hls_fallbacks()
                    .lock()
                    .map(|map| map.contains_key(manual_direct_url))
                    .unwrap_or(false);
                if !already_registered {
                    register_hls_fallback(manual_direct_url, &candidate.track.url);
                }
            }
            return false;
        }
        true
    });

    let has_native_tracks = candidates.iter().any(|candidate| !candidate.translated);
    let mut deduped: HashMap<(bool, String), ParsedSubtitleCandidate> = HashMap::new();
    for candidate in candidates {
        if has_native_tracks && candidate.translated {
            continue;
        }
        let key = (
            candidate.track.automatic,
            candidate.track.language.to_ascii_lowercase(),
        );
        let rank = (
            candidate.translated,
            candidate.is_hls_only,
            candidate.from_orig_alias,
        );
        match deduped.get(&key) {
            Some(existing)
                if (
                    existing.translated,
                    existing.is_hls_only,
                    existing.from_orig_alias,
                ) <= rank => {}
            _ => {
                deduped.insert(key, candidate);
            }
        }
    }

    let mut tracks: Vec<SubtitleTrack> = deduped.into_values().map(|c| c.track).collect();
    tracks.sort_by(|left, right| {
        (
            left.automatic,
            left.language.to_ascii_lowercase(),
            &left.language,
        )
            .cmp(&(
                right.automatic,
                right.language.to_ascii_lowercase(),
                &right.language,
            ))
    });
    let mut seen_urls = HashSet::with_capacity(tracks.len());
    tracks.retain(|track| seen_urls.insert(track.url.clone()));
    tracks.truncate(80);
    tracks
}

pub fn youtube_subtitles_fresh(
    video_url: &str,
    cancel: &Cancellation,
) -> Result<Vec<SubtitleTrack>, String> {
    let id = crate::history::youtube_video_id(video_url)
        .ok_or("Legendas disponíveis somente para vídeos do YouTube.")?;
    let url = crate::history::youtube_watch_url(&id).ok_or("URL de vídeo inválida.")?;
    let output = process::output(
        yt_dlp().args([
            "--dump-single-json",
            "--skip-download",
            "--no-playlist",
            "--no-warnings",
            "--",
            &url,
        ]),
        cancel,
        QUERY_TIMEOUT,
    )
    .map_err(|error| format!("Não foi possível consultar legendas: {error}"))?;
    if !output.status.success() {
        return Err("O yt-dlp não conseguiu consultar as legendas deste vídeo.".into());
    }
    let json: Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "Resposta de legendas inválida.")?;
    let tracks = parse_subtitles(&json);
    store_cached_subtitle_tracks(&url, &tracks);
    Ok(tracks)
}

pub fn youtube_subtitles(
    video_url: &str,
    cancel: &Cancellation,
) -> Result<Vec<SubtitleTrack>, String> {
    let id = crate::history::youtube_video_id(video_url)
        .ok_or("Legendas disponíveis somente para vídeos do YouTube.")?;
    let url = crate::history::youtube_watch_url(&id).ok_or("URL de vídeo inválida.")?;
    if let Some(cached) = get_cached_subtitle_tracks(&url) {
        return Ok(sanitize_subtitle_tracks(cached));
    }
    youtube_subtitles_fresh(video_url, cancel)
}

fn fetch_subtitle_text(
    client: &reqwest::blocking::Client,
    parsed: url::Url,
    remaining_bytes: usize,
) -> Result<String, String> {
    let response = client
        .get(parsed)
        .timeout(QUERY_TIMEOUT)
        .header(reqwest::header::USER_AGENT, SUBTITLE_USER_AGENT)
        .send()
        .map_err(|_| "Não foi possível baixar a legenda.".to_owned())?;
    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(
            "O YouTube limitou temporariamente o acesso às legendas (HTTP 429). Aguarde alguns instantes e tente novamente."
                .into(),
        );
    }
    if matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED
            | reqwest::StatusCode::FORBIDDEN
            | reqwest::StatusCode::NOT_FOUND
            | reqwest::StatusCode::GONE
    ) {
        return Err("A legenda expirou ou não está disponível. Clique em Buscar novamente.".into());
    }
    if !status.is_success() {
        return Err(format!(
            "Não foi possível baixar a legenda (HTTP {}).",
            status.as_u16()
        ));
    }
    let mut bytes = Vec::new();
    response
        .take(remaining_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Resposta de legenda inválida.".to_owned())?;
    if bytes.len() > remaining_bytes {
        return Err("Legenda acima do limite de 2 MiB.".into());
    }
    String::from_utf8(bytes).map_err(|_| "A legenda não está em UTF-8.".to_owned())
}

fn strip_vtt_header(segment: &str) -> &str {
    let trimmed = segment.trim_start_matches('\u{feff}');
    if !trimmed.starts_with("WEBVTT") {
        return trimmed;
    }
    let mut offset = 0;
    let mut seen_blank = false;
    for line in trimmed.split_inclusive('\n') {
        offset += line.len();
        if line.trim().is_empty() {
            seen_blank = true;
            break;
        }
    }
    if seen_blank { &trimmed[offset..] } else { "" }
}

pub(super) fn merge_vtt_segments<'a>(segments: impl IntoIterator<Item = &'a str>) -> String {
    let mut merged = String::new();
    for (index, segment) in segments.into_iter().enumerate() {
        if index == 0 {
            merged.push_str(segment.trim_start_matches('\u{feff}'));
            continue;
        }
        let body = strip_vtt_header(segment).trim_start_matches(['\r', '\n']);
        if !body.is_empty() {
            if !merged.ends_with('\n') {
                merged.push('\n');
            }
            merged.push('\n');
            merged.push_str(body);
        }
    }
    merged
}

pub(super) fn validate_vtt_payload(body: String) -> Result<String, String> {
    if body.trim().is_empty() {
        return Err("A faixa selecionada não contém texto de legenda.".into());
    }
    let cues = crate::transcript::parse_vtt(&body).map_err(str::to_owned)?;
    if cues.is_empty() {
        return Err("A faixa selecionada não contém texto de legenda.".into());
    }
    Ok(body)
}

fn download_vtt_single(url: &str) -> Result<String, String> {
    let parsed = validate_subtitle_url(url)?;
    let client = crate::http_client::no_redirect();
    let body = fetch_subtitle_text(client, parsed.clone(), SUBTITLE_MAX_BYTES)?;
    if !body.trim_start_matches('\u{feff}').starts_with("#EXTM3U") {
        return validate_vtt_payload(body);
    }
    let mut segment_urls = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if segment_urls.len() >= SUBTITLE_MAX_HLS_SEGMENTS {
            return Err("A legenda excede o limite de segmentos suportado.".into());
        }
        let joined = parsed
            .join(trimmed)
            .map_err(|_| "URL de segmento de legenda inválida.".to_owned())?;
        segment_urls.push(validate_subtitle_url(joined.as_str())?);
    }
    if segment_urls.is_empty() {
        return Err("A playlist da legenda não contém faixas disponíveis.".into());
    }
    let mut remaining = SUBTITLE_MAX_BYTES;
    let mut segments = Vec::with_capacity(segment_urls.len());
    for segment_url in segment_urls {
        let segment_text = fetch_subtitle_text(client, segment_url, remaining)?;
        remaining = remaining.saturating_sub(segment_text.len());
        segments.push(segment_text);
    }
    validate_vtt_payload(merge_vtt_segments(segments.iter().map(String::as_str)))
}

pub(super) fn resolve_yt_dlp_sub_request(
    video_url: Option<&str>,
    sub_url: &str,
    preferred_lang: Option<&str>,
    automatic: Option<bool>,
) -> Option<(String, String, &'static [&'static str])> {
    let parsed = validate_subtitle_url(sub_url).ok()?;
    let mut url_video_id = None;
    let mut url_lang = None;
    let mut url_tlang = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "v" if !v.is_empty() => url_video_id = Some(v.into_owned()),
            "lang" if !v.is_empty() => url_lang = Some(v.into_owned()),
            "tlang" if !v.is_empty() => url_tlang = Some(v.into_owned()),
            _ => {}
        }
    }
    let video_id = video_url
        .and_then(crate::history::youtube_video_id)
        .or(url_video_id)?;
    let watch_url = crate::history::youtube_watch_url(&video_id)?;
    let strip_orig = automatic != Some(false);
    let preferred_clean = preferred_lang
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| canonical_subtitle_language(s, strip_orig));
    let target_lang = url_tlang
        .filter(|l| valid_sub_lang_token(l))
        .or_else(|| preferred_clean.filter(|l| valid_sub_lang_token(l)))
        .or_else(|| url_lang.filter(|l| valid_sub_lang_token(l)))?;
    let write_flags: &'static [&'static str] = match automatic {
        Some(false) => &["--write-subs"],
        Some(true) => &["--write-auto-subs"],
        None => &["--write-subs", "--write-auto-subs"],
    };
    Some((watch_url, target_lang, write_flags))
}

fn download_vtt_via_yt_dlp_with_context(
    video_url: Option<&str>,
    sub_url: &str,
    preferred_lang: Option<&str>,
    automatic: Option<bool>,
    cancel: &Cancellation,
) -> Option<String> {
    if cancel.cancelled() {
        return None;
    }
    let (watch_url, target_lang, write_flags) =
        resolve_yt_dlp_sub_request(video_url, sub_url, preferred_lang, automatic)?;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp_dir =
        std::env::temp_dir().join(format!("fasttube-ytdlp-sub-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).ok()?;
    struct TempDirGuard(std::path::PathBuf);
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _guard = TempDirGuard(temp_dir.clone());
    let out_tmpl = temp_dir.join("sub.%(ext)s");
    let mut cmd = yt_dlp();
    cmd.arg("--skip-download");
    cmd.args(write_flags);
    cmd.args([
        "--sub-langs",
        &target_lang,
        "--sub-format",
        "vtt",
        "--no-playlist",
        "--no-warnings",
        "-o",
        &out_tmpl.to_string_lossy(),
        "--",
        &watch_url,
    ]);
    let _ = process::output(&mut cmd, cancel, QUERY_TIMEOUT).ok()?;
    if cancel.cancelled() {
        return None;
    }
    let expected_name = format!("sub.{target_lang}.vtt");
    let entries = std::fs::read_dir(&temp_dir).ok()?;
    let mut fallback = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("vtt") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(validated) = validate_vtt_payload(text) else {
            continue;
        };
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(&expected_name))
        {
            return Some(validated);
        }
        if fallback.is_none() {
            fallback = Some(validated);
        }
    }
    fallback
}

pub(super) fn find_matching_subtitle_track<'a>(
    tracks: &'a [SubtitleTrack],
    preferred_lang: Option<&str>,
    automatic: Option<bool>,
) -> Option<&'a SubtitleTrack> {
    let lang = preferred_lang.map(str::trim).filter(|s| !s.is_empty())?;
    let target = canonical_subtitle_language(lang, automatic != Some(false)).to_ascii_lowercase();
    let matches = |track: &SubtitleTrack, auto: Option<bool>| {
        auto.is_none_or(|expected| track.automatic == expected)
            && canonical_subtitle_language(&track.language, track.automatic).to_ascii_lowercase()
                == target
    };
    if automatic.is_none() {
        tracks
            .iter()
            .find(|track| !track.automatic && matches(track, None))
            .or_else(|| {
                tracks
                    .iter()
                    .find(|track| track.automatic && matches(track, None))
            })
    } else {
        tracks.iter().find(|track| matches(track, automatic))
    }
}

fn try_download_vtt_with_hls(url: &str, cancel: &Cancellation) -> Result<String, String> {
    if cancel.cancelled() {
        return Err("Download da legenda cancelado.".into());
    }
    match download_vtt_single(url) {
        Ok(vtt) => Ok(vtt),
        Err(err) => {
            if cancel.cancelled() {
                return Err("Download da legenda cancelado.".into());
            }
            let fallback_hls = subtitle_hls_fallbacks()
                .lock()
                .ok()
                .and_then(|map| map.get(url).cloned());
            if let Some(hls_url) = fallback_hls
                && let Ok(vtt) = download_vtt_single(&hls_url)
            {
                return Ok(vtt);
            }
            Err(err)
        }
    }
}

fn download_vtt_with_context(
    video_url: Option<&str>,
    sub_url: &str,
    preferred_lang: Option<&str>,
    automatic: Option<bool>,
    cancel: &Cancellation,
) -> Result<String, String> {
    if cancel.cancelled() {
        return Err("Download da legenda cancelado.".into());
    }
    let mut last_err = match try_download_vtt_with_hls(sub_url, cancel) {
        Ok(vtt) => return Ok(vtt),
        Err(err) => err,
    };
    let mut tried_urls = HashSet::from([sub_url.to_owned()]);
    if let Some(v_url) = video_url
        && preferred_lang.is_some_and(|lang| !lang.trim().is_empty())
    {
        if !cancel.cancelled()
            && let Some(cached_tracks) = get_cached_subtitle_tracks(v_url)
            && let Some(matched) =
                find_matching_subtitle_track(&cached_tracks, preferred_lang, automatic)
            && tried_urls.insert(matched.url.clone())
        {
            match try_download_vtt_with_hls(&matched.url, cancel) {
                Ok(vtt) => return Ok(vtt),
                Err(err) => last_err = err,
            }
        }
        if !cancel.cancelled()
            && let Ok(fresh_tracks) = youtube_subtitles_fresh(v_url, cancel)
            && let Some(matched) =
                find_matching_subtitle_track(&fresh_tracks, preferred_lang, automatic)
            && tried_urls.insert(matched.url.clone())
        {
            match try_download_vtt_with_hls(&matched.url, cancel) {
                Ok(vtt) => return Ok(vtt),
                Err(err) => last_err = err,
            }
        }
    }
    if cancel.cancelled() {
        return Err("Download da legenda cancelado.".into());
    }
    if let Some(vtt) =
        download_vtt_via_yt_dlp_with_context(video_url, sub_url, preferred_lang, automatic, cancel)
    {
        return Ok(vtt);
    }
    if cancel.cancelled() {
        Err("Download da legenda cancelado.".into())
    } else {
        Err(last_err)
    }
}

pub fn download_vtt_for_track(
    video_url: Option<&str>,
    track: &SubtitleTrack,
    cancel: &Cancellation,
) -> Result<String, String> {
    download_vtt_with_context(
        video_url,
        &track.url,
        Some(&track.language),
        Some(track.automatic),
        cancel,
    )
}

pub(crate) fn cached_playback_metadata(url: &str) -> Option<VideoPlaybackMetadata> {
    let target_id = crate::history::youtube_video_id(url);
    let heights = {
        let cache_mutex = RESOLUTION_CACHE.get_or_init(|| Mutex::new(ResolutionCache::default()));
        let mut cache = cache_mutex
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let now = Instant::now();
        cache
            .entries
            .retain(|entry| now.duration_since(entry.inserted) < RESOLUTION_CACHE_TTL);
        cache
            .entries
            .iter()
            .find(|entry| {
                entry.url == url
                    || (target_id.is_some()
                        && crate::history::youtube_video_id(&entry.url) == target_id)
            })
            .map(|entry| entry.heights.clone())
    }?;
    if heights.is_empty() {
        return None;
    }
    let extra = get_cached_playback_extra(url).unwrap_or_default();
    Some(VideoPlaybackMetadata {
        heights,
        subtitles: get_cached_subtitle_tracks(url).unwrap_or_default(),
        is_live: extra.is_live,
        title: extra.title,
        channel: extra.channel,
        channel_id: extra.channel_id,
        channel_url: extra.channel_url,
        description: extra.description,
        concurrent_view_count: extra.concurrent_view_count,
        storyboard: extra.storyboard,
    })
}

pub(crate) fn prime_playback_metadata_cache(
    url: &str,
    heights: &[u32],
    subtitles: &[SubtitleTrack],
    subtitles_parsed: bool,
    extra: PlaybackExtraInfo,
) {
    if !heights.is_empty()
        && url.len() <= RESOLUTION_CACHE_MAX_URL_BYTES
        && heights.len() <= RESOLUTION_CACHE_MAX_HEIGHTS
    {
        let cache_mutex = RESOLUTION_CACHE.get_or_init(|| Mutex::new(ResolutionCache::default()));
        let mut cache = cache_mutex
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let now = Instant::now();
        let target_id = crate::history::youtube_video_id(url);
        cache.entries.retain(|entry| {
            let same_video = entry.url == url
                || (target_id.is_some()
                    && crate::history::youtube_video_id(&entry.url) == target_id);
            !same_video && now.duration_since(entry.inserted) < RESOLUTION_CACHE_TTL
        });
        while cache.entries.len() >= RESOLUTION_CACHE_LIMIT {
            cache.entries.remove(0);
        }
        cache.entries.push(CachedResolutions {
            url: url.to_owned(),
            heights: heights.to_vec(),
            inserted: now,
        });
    }
    if subtitles_parsed {
        store_cached_subtitle_tracks(url, subtitles);
    }
    store_cached_playback_extra(url, extra);
}

pub fn playback_metadata_with_cancel(
    url: &str,
    cancellation: &Cancellation,
) -> Result<VideoPlaybackMetadata, String> {
    let heights = resolutions_with_cancel(url, cancellation)?;
    let extra = get_cached_playback_extra(url).unwrap_or_default();
    let subtitles = if crate::history::youtube_video_id(url).is_some() {
        if let Some(cached) = get_cached_subtitle_tracks(url) {
            cached
        } else {
            youtube_subtitles(url, cancellation).unwrap_or_default()
        }
    } else {
        Vec::new()
    };
    Ok(VideoPlaybackMetadata {
        heights,
        subtitles,
        is_live: extra.is_live,
        title: extra.title,
        channel: extra.channel,
        channel_id: extra.channel_id,
        channel_url: extra.channel_url,
        description: extra.description,
        concurrent_view_count: extra.concurrent_view_count,
        storyboard: extra.storyboard,
    })
}

pub fn resolutions_with_cancel(url: &str, cancellation: &Cancellation) -> Result<Vec<u32>, String> {
    let fetch = || {
        cached_resolutions(
            RESOLUTION_CACHE.get_or_init(|| Mutex::new(ResolutionCache::default())),
            url,
            || fetch_resolutions(url, cancellation),
        )
    };
    let result = fetch();
    if !cancellation.cancelled()
        && result
            .as_ref()
            .is_err_and(|error| error.contains("consulta cancelada"))
    {
        fetch()
    } else {
        result
    }
}

pub(super) fn parse_resolutions_from_json(value: &Value) -> Result<Vec<u32>, String> {
    let formats = value
        .get("formats")
        .and_then(Value::as_array)
        .ok_or("O vídeo não informou as resoluções disponíveis.")?;
    let mut heights: Vec<u32> = formats
        .iter()
        .filter_map(|format| {
            let codec = format.get("vcodec").and_then(Value::as_str)?;
            if codec == "none" || codec.is_empty() {
                return None;
            }
            let height = format.get("height").and_then(Value::as_u64)?;
            u32::try_from(height).ok().filter(|height| *height > 0)
        })
        .collect();
    heights.sort_unstable_by(|first, second| second.cmp(first));
    heights.dedup();
    if heights.is_empty() {
        Err("O vídeo não informou as resoluções disponíveis.".into())
    } else {
        Ok(heights)
    }
}

fn parse_storyboard_from_json(value: &Value) -> Option<StoryboardSpec> {
    let formats = value.get("formats").and_then(Value::as_array)?;
    let mut best: Option<(u32, StoryboardSpec)> = None;
    for fmt in formats {
        let is_sb = fmt
            .get("format_note")
            .and_then(Value::as_str)
            .is_some_and(|n| n.eq_ignore_ascii_case("storyboard"))
            || fmt
                .get("protocol")
                .and_then(Value::as_str)
                .is_some_and(|p| p == "mhtml");
        if !is_sb {
            continue;
        }
        let rows = fmt.get("rows").and_then(Value::as_u64).unwrap_or(0) as u32;
        let columns = fmt.get("columns").and_then(Value::as_u64).unwrap_or(0) as u32;
        if rows == 0 || columns == 0 {
            continue;
        }
        let thumb_width = fmt.get("width").and_then(Value::as_u64).unwrap_or(160) as u32;
        let thumb_height = fmt.get("height").and_then(Value::as_u64).unwrap_or(90) as u32;
        let Some(raw_frags) = fmt.get("fragments").and_then(Value::as_array) else {
            continue;
        };
        let fragments: Vec<StoryboardFragment> = raw_frags
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
                Some(StoryboardFragment {
                    url: url.to_owned(),
                    duration_secs,
                })
            })
            .collect();
        if fragments.is_empty() {
            continue;
        }
        let score = thumb_width;
        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score > *best_score)
        {
            best = Some((
                score,
                StoryboardSpec {
                    rows,
                    columns,
                    thumb_width,
                    thumb_height,
                    fragments,
                },
            ));
        }
    }
    best.map(|(_, spec)| spec)
}

fn parse_playback_extra_from_json(value: &Value) -> PlaybackExtraInfo {
    let live_status = value.get("live_status").and_then(Value::as_str);
    let is_live = value.get("is_live").and_then(Value::as_bool) == Some(true)
        || live_status == Some("is_live");
    let non_empty_str = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    PlaybackExtraInfo {
        is_live,
        title: non_empty_str("title"),
        channel: non_empty_str("channel").or_else(|| non_empty_str("uploader")),
        channel_id: non_empty_str("channel_id"),
        channel_url: non_empty_str("channel_url").or_else(|| non_empty_str("uploader_url")),
        description: non_empty_str("description"),
        concurrent_view_count: value.get("concurrent_view_count").and_then(Value::as_u64),
        storyboard: parse_storyboard_from_json(value),
    }
}

fn fetch_resolutions(url: &str, cancellation: &Cancellation) -> Result<Vec<u32>, String> {
    let output = process::output(
        yt_dlp().args([
            "--dump-single-json",
            "--skip-download",
            "--no-playlist",
            "--no-warnings",
            "--",
            url,
        ]),
        cancellation,
        QUERY_TIMEOUT,
    )
    .map_err(|error| format!("Não foi possível consultar as resoluções: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Não foi possível consultar as resoluções: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Resposta inválida do yt-dlp: {error}"))?;
    if crate::history::youtube_video_id(url).is_some() {
        let tracks = parse_subtitles(&value);
        store_cached_subtitle_tracks(url, &tracks);
    }
    store_cached_playback_extra(url, parse_playback_extra_from_json(&value));
    parse_resolutions_from_json(&value)
}

pub fn is_valid_playlist_id(playlist_id: &str) -> bool {
    !playlist_id.is_empty()
        && playlist_id.len() <= 128
        && playlist_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn youtube_playlist_url(playlist_id: &str) -> Option<String> {
    let trimmed = playlist_id.trim();
    if is_valid_playlist_id(trimmed) {
        Some(format!("https://www.youtube.com/playlist?list={trimmed}"))
    } else if is_playlist_url(trimmed) {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

pub(super) fn extract_playlist_id(value: &Value) -> Option<String> {
    if let Some(id) = nonempty_str(value, "id").filter(|id| is_valid_playlist_id(id))
        && (!crate::history::is_valid_video_id(id)
            || value
                .get("_type")
                .and_then(Value::as_str)
                .is_some_and(|t| t == "playlist" || t == "url")
            || value
                .get("ie_key")
                .and_then(Value::as_str)
                .is_some_and(|k| k.eq_ignore_ascii_case("YoutubeTab")))
    {
        return Some(id.to_owned());
    }
    for key in ["url", "webpage_url"] {
        if let Some(raw_url) = nonempty_str(value, key)
            && let Some(id) = raw_url.strip_prefix("https://www.youtube.com/playlist?list=")
        {
            let id = id.split(['&', '#', '/']).next().unwrap_or("");
            if is_valid_playlist_id(id) {
                return Some(id.to_owned());
            }
        }
    }
    None
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub struct PlaylistPageOutput {
    pub playlist_id: String,
    pub title: String,
    pub owner: String,
    pub description: Option<String>,
    pub total_videos: Option<u64>,
    pub thumbnail_url: Option<String>,
    pub videos: Vec<SearchResult>,
    pub set_video_ids: Vec<Option<String>>,
    pub next_continuation: Option<String>,
}

fn extract_playlist_id_from_input(playlist_id_or_url: &str) -> Option<String> {
    let trimmed = playlist_id_or_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let stripped = trimmed.strip_prefix("VL").unwrap_or(trimmed);
    if is_valid_playlist_id(stripped) {
        return Some(stripped.to_owned());
    }
    let parsed = url::Url::parse(trimmed).ok()?;
    let list_id = parsed
        .query_pairs()
        .find_map(|(k, v)| (k == "list").then(|| v.into_owned()))?;
    let clean = list_id.trim().strip_prefix("VL").unwrap_or(list_id.trim());
    is_valid_playlist_id(clean).then(|| clean.to_owned())
}

/// Fetches a page of playlist items via InnerTube `browseId = "VL<id>"` with continuations.
#[allow(dead_code)]
pub fn fetch_playlist_page_innertube(
    playlist_id_or_url: &str,
    continuation: Option<&str>,
    language: crate::i18n::UiLanguage,
    country: Option<&str>,
    session: Option<&crate::innertube::YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<PlaylistPageOutput, String> {
    if cancel.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let playlist_id = extract_playlist_id_from_input(playlist_id_or_url)
        .ok_or_else(|| "Identificador ou URL de playlist pública inválido.".to_owned())?;
    let ctx = crate::innertube::InnerTubeContext::from_language(language, country);
    let mut owned_session = session.cloned();
    let detail = crate::innertube::fetch_playlist_detail(
        &playlist_id,
        continuation,
        &ctx,
        owned_session.as_mut(),
        cancel,
    )
    .map_err(|err| err.into_message())?;
    let videos = detail
        .items
        .into_iter()
        .zip(detail.metadata)
        .map(|(video, metadata)| SearchResult { video, metadata })
        .collect();
    Ok(PlaylistPageOutput {
        playlist_id: detail.playlist_id,
        title: detail.title,
        owner: detail.owner,
        description: detail.description,
        total_videos: detail.total_videos,
        thumbnail_url: detail.thumbnail_url,
        videos,
        set_video_ids: detail.set_video_ids,
        next_continuation: detail.continuation,
    })
}

#[allow(dead_code)]
pub fn fetch_playlist_videos(
    playlist_id_or_url: &str,
    limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    fetch_public_playlist_videos_with_cancel(playlist_id_or_url, limit, cancellation)
}

pub fn fetch_playlist_videos_with_session(
    playlist_id_or_url: &str,
    limit: usize,
    language: crate::i18n::UiLanguage,
    session: Option<&crate::innertube::YouTubeWebSession>,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let effective_limit = limit.clamp(1, MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT);
    let playlist_id = extract_playlist_id_from_input(playlist_id_or_url)
        .ok_or_else(|| "Identificador ou URL de playlist inválido.".to_owned())?;
    let mut results = Vec::new();
    let mut seen_urls = HashSet::new();
    let mut next_token: Option<String> = None;
    let mut seen_tokens = HashSet::new();
    let mut fetched_any_page = false;
    let mut last_error = None;

    for _page in 0..8 {
        if cancellation.cancelled() || results.len() >= effective_limit {
            break;
        }
        match fetch_playlist_page_innertube(
            &playlist_id,
            next_token.as_deref(),
            language,
            None,
            session,
            cancellation,
        ) {
            Ok(page) => {
                fetched_any_page = true;
                for item in page.videos {
                    if results.len() >= effective_limit {
                        break;
                    }
                    if seen_urls.insert(item.video.url.clone()) {
                        results.push(item);
                    }
                }
                next_token = page
                    .next_continuation
                    .filter(|tok| seen_tokens.insert(tok.clone()));
                if next_token.is_none() {
                    break;
                }
            }
            Err(err) => {
                last_error = Some(err);
                break;
            }
        }
    }
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    if fetched_any_page {
        results.truncate(effective_limit);
        return Ok(results);
    }
    Err(last_error.unwrap_or_else(|| "Não foi possível carregar os vídeos da playlist.".to_owned()))
}

pub fn fetch_public_playlist_videos_with_cancel(
    playlist_id_or_url: &str,
    limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let url = youtube_playlist_url(playlist_id_or_url)
        .ok_or_else(|| "Identificador ou URL de playlist pública inválido.".to_owned())?;
    let effective_limit = limit.clamp(1, MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT);

    // Try InnerTube `VL<id>` first before spawning `yt-dlp`.
    if let Ok(results) = fetch_playlist_videos_with_session(
        playlist_id_or_url,
        effective_limit,
        crate::i18n::UiLanguage::default(),
        None,
        cancellation,
    ) && !results.is_empty()
    {
        return Ok(results);
    }
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }

    let limit_arg = effective_limit.to_string();
    let output = process::output(
        yt_dlp().args([
            "--flat-playlist",
            "--extractor-args",
            "youtubetab:approximate_date",
            "--playlist-end",
            &limit_arg,
            "--dump-single-json",
            "--skip-download",
            "--no-warnings",
            "--",
            &url,
        ]),
        cancellation,
        QUERY_TIMEOUT,
    )
    .map_err(|error| {
        format!("Não foi possível executar yt-dlp: {error}. Verifique se ele está ao lado do app ou no PATH.")
    })?;
    if !output.status.success() {
        return Err(format!(
            "Não foi possível consultar os vídeos da playlist pública: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_public_playlist_videos(&output.stdout, effective_limit)
}

pub(super) fn parse_public_playlist_videos(
    output: &[u8],
    limit: usize,
) -> Result<Vec<SearchResult>, String> {
    let effective_limit = limit.clamp(1, MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT);
    if output.iter().all(u8::is_ascii_whitespace) {
        return Ok(Vec::new());
    }
    if let Ok(single_value) = serde_json::from_slice::<Value>(output)
        && let Some(entries) = single_value.get("entries").and_then(Value::as_array)
    {
        let fallback_channel = [
            "channel",
            "uploader",
            "playlist_channel",
            "playlist_uploader",
        ]
        .into_iter()
        .filter_map(|key| nonempty_str(&single_value, key))
        .next();
        let fallback_id = ["channel_id", "playlist_channel_id"]
            .into_iter()
            .filter_map(|key| nonempty_str(&single_value, key))
            .find(|candidate| is_valid_channel_id(candidate));
        let fallback_url = ["channel_url", "uploader_url"]
            .into_iter()
            .filter_map(|key| nonempty_str(&single_value, key))
            .find(|u| parse_channel_target(u).is_some());
        let mut results = Vec::new();
        for entry in entries {
            if results.len() >= effective_limit {
                break;
            }
            if let Some(video) =
                parse_video_entry(entry, true, fallback_channel, fallback_id, fallback_url)
                && crate::history::youtube_video_id(&video.video.url).is_some()
            {
                results.push(video);
            }
        }
        return Ok(results);
    }
    let mut videos = parse_results(output, true)?;
    videos.retain(|item| crate::history::youtube_video_id(&item.video.url).is_some());
    videos.truncate(effective_limit);
    Ok(videos)
}

pub(super) fn clear_playback_caches() {
    if let Some(lock) = RESOLUTION_CACHE.get() {
        let mut cache = lock.lock().unwrap_or_else(|error| error.into_inner());
        cache.entries.clear();
    }
    if let Some(lock) = SUBTITLE_TRACK_CACHE.get() {
        let mut cache = lock.lock().unwrap_or_else(|error| error.into_inner());
        cache.entries.clear();
    }
    if let Some(lock) = PLAYBACK_EXTRA_CACHE.get() {
        let mut cache = lock.lock().unwrap_or_else(|error| error.into_inner());
        cache.entries.clear();
    }
}
