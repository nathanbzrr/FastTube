use crate::process::{self, Cancellation};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const QUERY_TIMEOUT: Duration = Duration::from_secs(45);
const BROWSER_FEED_TIMEOUT: Duration = Duration::from_secs(90);

const RESOLUTION_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const RESOLUTION_CACHE_LIMIT: usize = 128;
const RESOLUTION_CACHE_MAX_URL_BYTES: usize = 4096;
const RESOLUTION_CACHE_MAX_HEIGHTS: usize = 256;

struct CachedResolutions {
    url: String,
    heights: Vec<u32>,
    inserted: Instant,
}

#[derive(Default)]
struct ResolutionCache {
    entries: Vec<CachedResolutions>,
    in_flight: HashMap<String, Arc<InFlight>>,
}

#[derive(Default)]
struct InFlight {
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

fn get_cached_subtitle_tracks(url: &str) -> Option<Vec<SubtitleTrack>> {
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

fn store_cached_subtitle_tracks(url: &str, tracks: &[SubtitleTrack]) {
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlaybackExtraInfo {
    pub is_live: bool,
    pub title: Option<String>,
    pub channel: Option<String>,
    pub channel_id: Option<String>,
    pub channel_url: Option<String>,
    pub description: Option<String>,
    pub concurrent_view_count: Option<u64>,
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

fn cached_resolutions(
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
}

const SUBTITLE_MAX_BYTES: usize = 2 * 1024 * 1024;
const SUBTITLE_MAX_HLS_SEGMENTS: usize = 64;
const SUBTITLE_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

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

fn subtitle_hls_fallbacks() -> &'static Mutex<HashMap<String, String>> {
    SUBTITLE_HLS_FALLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn parse_subtitles(value: &Value) -> Vec<SubtitleTrack> {
    let mut candidates = Vec::new();
    for (field, automatic) in [("subtitles", false), ("automatic_captions", true)] {
        let Some(languages) = value[field].as_object() else {
            continue;
        };
        for (language, formats) in languages {
            if language.is_empty()
                || language.len() > 64
                || !language
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            {
                continue;
            }
            let Some((url, parsed)) = formats.as_array().and_then(|formats| {
                let mut direct = None;
                let mut hls = None;
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
                    if is_hls_playlist_format(format, &parsed) {
                        if hls.is_none() {
                            hls = Some((value, parsed));
                        }
                    } else if direct.is_none() {
                        direct = Some((value, parsed));
                    }
                }
                if let (Some((d_url, _)), Some((h_url, _))) = (&direct, &hls)
                    && let Ok(mut map) = subtitle_hls_fallbacks().lock()
                {
                    if map.len() > 512 {
                        map.clear();
                    }
                    map.insert((*d_url).to_owned(), (*h_url).to_owned());
                }
                direct.or(hls)
            }) else {
                continue;
            };
            let translated = is_auto_translated_url(&parsed);
            candidates.push((
                SubtitleTrack {
                    language: language.clone(),
                    automatic,
                    url: url.to_owned(),
                },
                translated,
            ));
        }
    }
    let has_native_tracks = candidates.iter().any(|(_, translated)| !translated);
    let mut tracks: Vec<SubtitleTrack> = candidates
        .into_iter()
        .filter_map(|(track, translated)| (!has_native_tracks || !translated).then_some(track))
        .collect();
    tracks.sort_by(|left, right| {
        (left.automatic, &left.language).cmp(&(right.automatic, &right.language))
    });
    tracks.truncate(80);
    tracks
}

#[cfg(test)]
#[test]
fn subtitle_parser_selects_vtt_and_rejects_untrusted_hosts() {
    let value = serde_json::json!({
        "subtitles": {"pt-BR": [{"ext":"vtt", "url":"https://www.youtube.com/api/timedtext?v=123"}], "bad": [{"ext":"vtt", "url":"https://youtube.com.evil.example/captions"}]},
        "automatic_captions": {"en": [{"ext":"vtt", "url":"https://www.youtube.com/api/timedtext?lang=en"}], "fr": [{"ext":"srt", "url":"https://www.youtube.com/subs"}]}
    });
    let tracks = parse_subtitles(&value);
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].language, "pt-BR");
    assert!(!tracks[0].automatic);
    assert!(tracks[1].automatic);
}

#[cfg(test)]
#[test]
fn subtitle_parser_prefers_direct_vtt_and_filters_tlang_when_native_exists() {
    let value = serde_json::json!({
        "subtitles": {},
        "automatic_captions": {
            "en": [
                {"ext": "vtt", "protocol": "m3u8_native", "url": "https://manifest.googlevideo.com/api/manifest/hls_timedtext_playlist/index.m3u8"},
                {"ext": "vtt", "url": "https://www.youtube.com/api/timedtext?v=123&lang=en&fmt=vtt"}
            ],
            "pt": [
                {"ext": "vtt", "url": "https://www.youtube.com/api/timedtext?v=123&lang=en&tlang=pt&fmt=vtt"}
            ],
            "it": [
                {"ext": "vtt", "url": "https://www.youtube.com/api/timedtext?v=123&lang=en&tlang=it&fmt=vtt"}
            ]
        }
    });
    let tracks = parse_subtitles(&value);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].language, "en");
    assert_eq!(
        tracks[0].url,
        "https://www.youtube.com/api/timedtext?v=123&lang=en&fmt=vtt"
    );
}

pub fn youtube_subtitles(
    video_url: &str,
    cancel: &Cancellation,
) -> Result<Vec<SubtitleTrack>, String> {
    let id = crate::history::youtube_video_id(video_url)
        .ok_or("Legendas disponíveis somente para vídeos do YouTube.")?;
    let url = crate::history::youtube_watch_url(&id).ok_or("URL de vídeo inválida.")?;
    if let Some(cached) = get_cached_subtitle_tracks(&url) {
        return Ok(cached);
    }
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

fn fetch_subtitle_text(
    client: &reqwest::blocking::Client,
    parsed: url::Url,
    remaining_bytes: usize,
) -> Result<String, String> {
    let response = client
        .get(parsed)
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

fn merge_vtt_segments<'a>(segments: impl IntoIterator<Item = &'a str>) -> String {
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

#[cfg(test)]
#[test]
fn hls_vtt_segment_merger_strips_duplicate_headers() {
    let seg1 = "WEBVTT\nKind: captions\nLanguage: pt-BR\n\n00:00:01.000 --> 00:00:02.000\nOi\n";
    let seg2 =
        "\u{feff}WEBVTT\nKind: captions\nLanguage: pt-BR\n\n00:00:02.000 --> 00:00:03.000\nMundo\n";
    let merged = merge_vtt_segments([seg1, seg2]);
    assert_eq!(merged.matches("WEBVTT").count(), 1);
    assert!(merged.contains("Oi"));
    assert!(merged.contains("Mundo"));
}

fn download_vtt_single(url: &str) -> Result<String, String> {
    let parsed = validate_subtitle_url(url)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(QUERY_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Não foi possível preparar o download da legenda.".to_owned())?;
    let body = fetch_subtitle_text(&client, parsed.clone(), SUBTITLE_MAX_BYTES)?;
    if !body.trim_start_matches('\u{feff}').starts_with("#EXTM3U") {
        return Ok(body);
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
        let segment_text = fetch_subtitle_text(&client, segment_url, remaining)?;
        remaining = remaining.saturating_sub(segment_text.len());
        segments.push(segment_text);
    }
    Ok(merge_vtt_segments(segments.iter().map(String::as_str)))
}

fn download_vtt_via_yt_dlp(url: &str) -> Option<String> {
    let parsed = validate_subtitle_url(url).ok()?;
    let mut video_id = None;
    let mut lang = None;
    let mut tlang = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "v" if !v.is_empty() => video_id = Some(v.into_owned()),
            "lang" if !v.is_empty() => lang = Some(v.into_owned()),
            "tlang" if !v.is_empty() => tlang = Some(v.into_owned()),
            _ => {}
        }
    }
    let video_id = video_id?;
    let watch_url = crate::history::youtube_watch_url(&video_id)?;
    let target_lang = tlang.or(lang)?;
    if !target_lang
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
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
    let cancel = Cancellation::default();
    let _ = process::output(
        yt_dlp().args([
            "--skip-download",
            "--write-subs",
            "--write-auto-subs",
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
        ]),
        &cancel,
        QUERY_TIMEOUT,
    )
    .ok()?;
    let entries = std::fs::read_dir(&temp_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("vtt")
            && let Ok(text) = std::fs::read_to_string(&path)
            && !text.trim().is_empty()
        {
            return Some(text);
        }
    }
    None
}

pub fn download_vtt(url: &str) -> Result<String, String> {
    match download_vtt_single(url) {
        Ok(vtt) => Ok(vtt),
        Err(primary_err) => {
            let fallback_hls = subtitle_hls_fallbacks()
                .lock()
                .ok()
                .and_then(|map| map.get(url).cloned());
            if let Some(hls_url) = fallback_hls
                && let Ok(vtt) = download_vtt_single(&hls_url)
            {
                return Ok(vtt);
            }
            if let Some(vtt) = download_vtt_via_yt_dlp(url) {
                return Ok(vtt);
            }
            Err(primary_err)
        }
    }
}

fn is_playlist_url(url: &str) -> bool {
    url.strip_prefix("https://www.youtube.com/playlist?list=")
        .is_some_and(|id| {
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
}

pub fn session_feed(
    cookies: &[Value],
    target: &str,
    cancel: &Cancellation,
) -> Result<HomeFeed, String> {
    use std::io::Write;
    if target != ":ytrec" && !is_playlist_url(target) {
        return Err("Endereço de playlist inválido.".into());
    }
    let profile = crate::session::profile_dir()?;
    let path = profile.join(format!(".fasttube-cookies-{}.txt", std::process::id()));
    struct TemporaryCookies(std::path::PathBuf);
    impl Drop for TemporaryCookies {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| "Não foi possível preparar a sessão do YouTube.")?;
    let _cleanup = TemporaryCookies(path.clone());
    writeln!(file, "# Netscape HTTP Cookie File")
        .map_err(|_| "Não foi possível preparar a sessão.")?;
    for cookie in cookies {
        let domain = cookie.get("domain").and_then(Value::as_str).unwrap_or("");
        let name = cookie.get("name").and_then(Value::as_str).unwrap_or("");
        let value = cookie.get("value").and_then(Value::as_str).unwrap_or("");
        let cookie_path = cookie.get("path").and_then(Value::as_str).unwrap_or("/");
        if !["youtube.com", ".youtube.com", "www.youtube.com"].contains(&domain)
            || name.is_empty()
            || [domain, name, value, cookie_path]
                .iter()
                .any(|s| s.contains(['\t', '\r', '\n']))
        {
            continue;
        }
        let expiry = cookie
            .get("expires")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .max(0.0) as u64;
        let secure = if cookie.get("secure").and_then(Value::as_bool) == Some(true) {
            "TRUE"
        } else {
            "FALSE"
        };
        let prefix = if cookie.get("httpOnly").and_then(Value::as_bool) == Some(true) {
            "#HttpOnly_"
        } else {
            ""
        };
        writeln!(
            file,
            "{prefix}{domain}\t{}\t{cookie_path}\t{secure}\t{expiry}\t{name}\t{value}",
            if domain.starts_with('.') {
                "TRUE"
            } else {
                "FALSE"
            }
        )
        .map_err(|_| "Não foi possível preparar a sessão.")?;
    }
    drop(file);
    let mut cmd = yt_dlp();
    cmd.args(["--ignore-config", "--cookies"]).arg(&path).args([
        "--flat-playlist",
        "--playlist-end",
        "50",
        "--dump-json",
        "--no-warnings",
        "--",
        target,
    ]);
    let output = process::output(&mut cmd, cancel, BROWSER_FEED_TIMEOUT)
        .map_err(|e| format!("Não foi possível consultar o YouTube: {e}"))?;
    if !output.status.success() {
        return Err("Não foi possível carregar o YouTube. Sua sessão pode ter expirado.".into());
    }
    parse_home_feed(&output.stdout)
}

fn parse_home_feed(output: &[u8]) -> Result<HomeFeed, String> {
    let mut feed = HomeFeed {
        items: Vec::new(),
        playlists: Vec::new(),
        channels: Vec::new(),
    };
    for line in output
        .split(|b| *b == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
    {
        let value: Value =
            serde_json::from_slice(line).map_err(|_| "Resposta inválida do YouTube.".to_owned())?;
        let playlist = value
            .get("ie_key")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case("YoutubeTab"));
        if playlist {
            let id = value.get("id").and_then(Value::as_str).unwrap_or("");
            let url = format!("https://www.youtube.com/playlist?list={id}");
            if !is_playlist_url(&url) {
                continue;
            }
            let mut entry = match parse_video_entry(&value, false, None, None, None) {
                Some(v) => v,
                None => continue,
            };
            entry.video.url = url.clone();
            entry.video.duration.clear();
            feed.playlists.push(url);
            feed.items.push(entry);
        } else if value
            .get("ie_key")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case("Youtube"))
        {
            let fallback_name = nonempty_str(&value, "playlist_channel")
                .or_else(|| nonempty_str(&value, "playlist_uploader"))
                .or_else(|| nonempty_str(&value, "playlist_title"))
                .and_then(clean_channel_title);
            let fallback_id = nonempty_str(&value, "playlist_channel_id")
                .or_else(|| nonempty_str(&value, "playlist_id"))
                .filter(|candidate| is_valid_channel_id(candidate));
            let fallback_url = fallback_id
                .map(|id| ChannelTarget::ChannelId(id.to_owned()).canonical_url())
                .or_else(|| {
                    nonempty_str(&value, "playlist_uploader_id")
                        .filter(|handle| is_valid_channel_handle(handle))
                        .map(|handle| ChannelTarget::Handle(handle.to_owned()).canonical_url())
                });
            if let Some(entry) = parse_video_entry(
                &value,
                true,
                fallback_name.as_deref(),
                fallback_id,
                fallback_url.as_deref(),
            ) && crate::history::youtube_video_id(&entry.video.url).is_some()
            {
                feed.items.push(entry);
            }
        }
    }
    Ok(feed)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HomeBatchRequest {
    pub video_queries: Vec<String>,
    pub channel_targets: Vec<ChannelTarget>,
    pub playlist_query: Option<String>,
    pub shorts_query: Option<String>,
}

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

pub fn build_playlist_search_url(query: &str) -> String {
    let trimmed = query.trim();
    let encoded = percent_encode_query_component(if trimmed.is_empty() {
        "mix musica playlist brasil"
    } else {
        trimmed
    });
    format!("https://www.youtube.com/results?search_query={encoded}&sp=EgIQAw%3D%3D")
}

fn fetch_home_lane(
    target: &str,
    playlist_end: Option<&str>,
    cancellation: &Cancellation,
) -> Result<HomeFeed, String> {
    if cancellation.cancelled() {
        return Err("Consulta cancelada.".into());
    }
    let mut cmd = yt_dlp();
    cmd.arg("--flat-playlist");
    if let Some(end) = playlist_end {
        cmd.args(["--playlist-end", end]);
    }
    cmd.args(["--dump-json", "--no-warnings", "--", target]);
    let output = process::output(&mut cmd, cancellation, QUERY_TIMEOUT)
        .map_err(|error| format!("Não foi possível executar yt-dlp: {error}."))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            "Não foi possível carregar o feed do YouTube.".into()
        } else {
            format!("Não foi possível carregar o feed: {stderr}")
        });
    }
    parse_home_feed(&output.stdout)
}

fn merge_home_feeds(feeds: Vec<HomeFeed>) -> HomeFeed {
    let mut merged = HomeFeed {
        items: Vec::new(),
        playlists: Vec::new(),
        channels: Vec::new(),
    };
    let mut seen_keys = std::collections::HashSet::new();
    let mut seen_playlists = std::collections::HashSet::new();
    for feed in feeds {
        for playlist in feed.playlists {
            if seen_playlists.insert(playlist.clone()) {
                merged.playlists.push(playlist);
            }
        }
        for item in feed.items {
            let key = crate::history::youtube_video_id(&item.video.url)
                .unwrap_or_else(|| item.video.url.clone());
            if seen_keys.insert(key) {
                merged.items.push(item);
            }
        }
    }
    merged
}

pub fn fetch_home_batch(
    request: &HomeBatchRequest,
    cancellation: &Cancellation,
) -> Result<HomeFeed, String> {
    if cancellation.cancelled() {
        return Err("Consulta cancelada.".into());
    }

    let mut targets: Vec<(String, Option<&'static str>)> = Vec::new();
    for query in &request.video_queries {
        let trimmed = query.trim();
        if !trimmed.is_empty() {
            targets.push((format!("ytsearch25:{trimmed}"), None));
        }
    }
    for target in &request.channel_targets {
        targets.push((target.videos_tab_url(), Some("10")));
    }
    if let Some(playlist_query) = request
        .playlist_query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
    {
        targets.push((build_playlist_search_url(playlist_query), Some("12")));
    }
    if let Some(shorts_query) = request
        .shorts_query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
    {
        targets.push((format!("ytsearch18:{shorts_query}"), None));
    }
    if targets.is_empty() {
        return Ok(HomeFeed {
            items: Vec::new(),
            playlists: Vec::new(),
            channels: Vec::new(),
        });
    }

    let results: Vec<Result<HomeFeed, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = targets
            .iter()
            .map(|(target, playlist_end)| {
                let target = target.as_str();
                let playlist_end = *playlist_end;
                scope.spawn(move || fetch_home_lane(target, playlist_end, cancellation))
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err("Falha interna ao carregar o feed.".into()))
            })
            .collect()
    });

    let mut feeds = Vec::new();
    let mut first_error = None;
    for result in results {
        match result {
            Ok(feed) => feeds.push(feed),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }

    if cancellation.cancelled() {
        return Err("Consulta cancelada.".into());
    }

    let merged = merge_home_feeds(feeds);
    if merged.items.is_empty()
        && let Some(error) = first_error
    {
        return Err(error);
    }
    Ok(merged)
}

pub const DEFAULT_CHANNEL_VIDEOS_LIMIT: usize = 20;
pub const CHANNEL_VIDEOS_PAGE_STEP: usize = 20;
pub const MAX_CHANNEL_VIDEOS_LIMIT: usize = 100;
pub const DEFAULT_CHANNEL_PLAYLISTS_LIMIT: usize = 30;
pub const MAX_CHANNEL_PLAYLISTS_LIMIT: usize = 50;
pub const MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT: usize = 100;
pub const MAX_CHANNEL_DESCRIPTION_CHARS: usize = 8192;
pub const MAX_CHANNEL_TAGS: usize = 30;
pub const MAX_CHANNEL_TAG_CHARS: usize = 64;

const CHANNEL_PAGE_CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const CHANNEL_PAGE_CACHE_CAPACITY: usize = 6;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelPlaylist {
    pub id: String,
    pub title: String,
    pub url: String,
    pub thumbnail: String,
    pub item_count: Option<u64>,
    pub description: Option<String>,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    pub has_shorts_tab: bool,
    pub has_streams_tab: bool,
    pub has_playlists_tab: bool,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub verified: Option<bool>,
    pub tags: Vec<String>,
    pub videos: Vec<SearchResult>,
}

#[derive(Clone, Debug)]
pub struct ChannelFetchOutcome {
    pub page: ChannelPage,
    pub from_cache: bool,
}

struct CachedChannelPage {
    key: String,
    page: ChannelPage,
    inserted: Instant,
    last_used: Instant,
}

#[derive(Default)]
struct ChannelPageCache {
    entries: Vec<CachedChannelPage>,
}

impl ChannelPageCache {
    fn evict_expired(&mut self, now: Instant) {
        self.entries
            .retain(|entry| now.saturating_duration_since(entry.inserted) < CHANNEL_PAGE_CACHE_TTL);
    }

    fn get(&mut self, key: &str, now: Instant) -> Option<ChannelPage> {
        self.evict_expired(now);
        let index = self.entries.iter().position(|entry| entry.key == key)?;
        let mut entry = self.entries.remove(index);
        entry.last_used = now;
        let page = entry.page.clone();
        self.entries.push(entry);
        Some(page)
    }

    fn insert(&mut self, key: String, page: ChannelPage, now: Instant) {
        self.evict_expired(now);
        self.entries.retain(|entry| entry.key != key);
        while self.entries.len() >= CHANNEL_PAGE_CACHE_CAPACITY {
            if let Some((lru_index, _)) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.last_used)
            {
                self.entries.remove(lru_index);
            } else {
                break;
            }
        }
        self.entries.push(CachedChannelPage {
            key,
            page,
            inserted: now,
            last_used: now,
        });
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

static CHANNEL_PAGE_CACHE: OnceLock<Mutex<ChannelPageCache>> = OnceLock::new();

fn channel_cache_key(target: &ChannelTarget, effective_limit: usize) -> String {
    format!("{}|{}", target.canonical_url(), effective_limit)
}

fn fetch_with_cache(
    cache: &Mutex<ChannelPageCache>,
    key: &str,
    force_refresh: bool,
    fetch: impl FnOnce() -> Result<ChannelPage, String>,
) -> Result<ChannelFetchOutcome, String> {
    if !force_refresh {
        let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(page) = guard.get(key, Instant::now()) {
            return Ok(ChannelFetchOutcome {
                page,
                from_cache: true,
            });
        }
    }

    let page = fetch()?;
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    guard.insert(key.to_owned(), page.clone(), Instant::now());
    drop(guard);
    Ok(ChannelFetchOutcome {
        page,
        from_cache: false,
    })
}

pub fn is_valid_channel_id(channel_id: &str) -> bool {
    channel_id.len() == 24
        && channel_id.starts_with("UC")
        && channel_id[2..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

pub fn is_valid_channel_handle(handle: &str) -> bool {
    let Some(rest) = handle.strip_prefix('@') else {
        return false;
    };
    (3..=30).contains(&rest.len())
        && rest.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' || byte == b'.'
        })
}

pub fn parse_channel_target(input: &str) -> Option<ChannelTarget> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_valid_channel_id(trimmed) {
        return Some(ChannelTarget::ChannelId(trimmed.to_owned()));
    }
    if is_valid_channel_handle(trimmed) {
        return Some(ChannelTarget::Handle(trimmed.to_owned()));
    }
    parse_youtube_channel_url(trimmed)
}

#[cfg(test)]
pub fn youtube_channel_url(input: &str) -> Option<String> {
    parse_channel_target(input).map(|target| target.canonical_url())
}

#[cfg(test)]
pub fn youtube_channel_videos_url(input: &str) -> Option<String> {
    parse_channel_target(input).map(|target| target.videos_tab_url())
}

pub fn channel_target_for_metadata(metadata: &VideoMetadata) -> Option<ChannelTarget> {
    metadata
        .channel_id
        .as_deref()
        .and_then(parse_channel_target)
        .or_else(|| {
            metadata
                .channel_url
                .as_deref()
                .and_then(parse_channel_target)
        })
}

fn parse_youtube_channel_url(url: &str) -> Option<ChannelTarget> {
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

    if after_scheme
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return None;
    }

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
    if !matches!(
        host.as_str(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com"
    ) {
        return None;
    }

    let before_fragment = rest.split('#').next().unwrap_or("");
    let path = match before_fragment.split_once('?') {
        Some((path, _)) => path,
        None => before_fragment,
    };
    let path = path.strip_prefix('/')?;
    let path = path.strip_suffix('/').unwrap_or(path);
    if path.is_empty() {
        return None;
    }

    let is_allowed_tab = |tab: &str| {
        matches!(
            tab,
            "videos" | "featured" | "streams" | "shorts" | "releases" | "playlists" | "about"
        )
    };

    if let Some(after_channel) = path.strip_prefix("channel/") {
        let (candidate, tab) = match after_channel.split_once('/') {
            Some((id, rest)) => {
                if rest.contains('/') || !is_allowed_tab(rest) {
                    return None;
                }
                (id, Some(rest))
            }
            None => (after_channel, None),
        };
        let _ = tab;
        if is_valid_channel_id(candidate) {
            return Some(ChannelTarget::ChannelId(candidate.to_owned()));
        }
        return None;
    }

    let (candidate, tab) = match path.split_once('/') {
        Some((handle, rest)) => {
            if rest.contains('/') || !is_allowed_tab(rest) {
                return None;
            }
            (handle, Some(rest))
        }
        None => (path, None),
    };
    let _ = tab;
    if is_valid_channel_handle(candidate) {
        Some(ChannelTarget::Handle(candidate.to_owned()))
    } else {
        None
    }
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

fn yt_dlp() -> Command {
    let program = std::env::current_exe()
        .ok()
        .and_then(|executable| bundled_yt_dlp(&executable))
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

fn parse_resolutions_from_json(value: &Value) -> Result<Vec<u32>, String> {
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchSort {
    #[default]
    Relevance,
    Date,
    Views,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchDurationFilter {
    #[default]
    Any,
    Short,
    Medium,
    Long,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchTypeFilter {
    #[default]
    All,
    Videos,
    Shorts,
    Channels,
    Playlists,
    Movies,
    Live,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchUploadDateFilter {
    #[default]
    Any,
    Today,
    ThisWeek,
    ThisMonth,
    ThisYear,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchFeatureFilters {
    pub live: bool,
    pub four_k: bool,
    pub hd: bool,
    pub subtitles: bool,
    pub creative_commons: bool,
    pub three_sixty: bool,
    pub vr180: bool,
    pub three_d: bool,
    pub hdr: bool,
    pub location: bool,
    pub purchased: bool,
}

impl SearchFeatureFilters {
    pub fn is_empty(self) -> bool {
        self == Self::default()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchFilters {
    pub sort: SearchSort,
    pub duration: SearchDurationFilter,
    pub kind: SearchTypeFilter,
    pub upload_date: SearchUploadDateFilter,
    pub features: SearchFeatureFilters,
}

impl SearchFilters {
    pub fn is_default(self) -> bool {
        self.sort == SearchSort::default()
            && self.duration == SearchDurationFilter::default()
            && self.kind == SearchTypeFilter::default()
            && self.upload_date == SearchUploadDateFilter::default()
            && self.features.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchChip {
    #[default]
    All,
    Shorts,
    Unwatched,
    Videos,
    RecentlyUploaded,
    Live,
}

impl SearchChip {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Tudo",
            Self::Shorts => "Shorts",
            Self::Unwatched => "N\u{00e3}o assistidos",
            Self::Videos => "V\u{00ed}deos",
            Self::RecentlyUploaded => "Enviados recentemente",
            Self::Live => "Ao vivo",
        }
    }
}

pub fn effective_search_filters(mut base: SearchFilters, chip: SearchChip) -> SearchFilters {
    match chip {
        SearchChip::All | SearchChip::Unwatched => {}
        SearchChip::Shorts => {
            base.kind = SearchTypeFilter::Shorts;
        }
        SearchChip::Videos => {
            base.kind = SearchTypeFilter::Videos;
        }
        SearchChip::RecentlyUploaded => {
            if base.sort == SearchSort::Relevance {
                base.sort = SearchSort::Date;
            }
        }
        SearchChip::Live => {
            base.features.live = true;
        }
    }
    base
}

fn push_proto_varint(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn push_proto_field_varint(buf: &mut Vec<u8>, field_number: u32, value: u64) {
    push_proto_varint(buf, u64::from(field_number) << 3);
    push_proto_varint(buf, value);
}

/// Encodes `SearchFilters` into YouTube's `sp` base64 protobuf query parameter.
pub fn encode_search_sp(filters: SearchFilters) -> Option<String> {
    use base64::Engine as _;

    let mut msg = Vec::new();
    match filters.sort {
        SearchSort::Relevance => {}
        SearchSort::Date => push_proto_field_varint(&mut msg, 1, 2),
        SearchSort::Views => push_proto_field_varint(&mut msg, 1, 3),
    }

    let mut sub = Vec::new();
    match filters.upload_date {
        SearchUploadDateFilter::Any => {}
        SearchUploadDateFilter::Today => push_proto_field_varint(&mut sub, 1, 2),
        SearchUploadDateFilter::ThisWeek => push_proto_field_varint(&mut sub, 1, 3),
        SearchUploadDateFilter::ThisMonth => push_proto_field_varint(&mut sub, 1, 4),
        SearchUploadDateFilter::ThisYear => push_proto_field_varint(&mut sub, 1, 5),
    }
    match filters.kind {
        SearchTypeFilter::All => {}
        SearchTypeFilter::Videos => push_proto_field_varint(&mut sub, 2, 1),
        SearchTypeFilter::Channels => push_proto_field_varint(&mut sub, 2, 2),
        SearchTypeFilter::Playlists => push_proto_field_varint(&mut sub, 2, 3),
        SearchTypeFilter::Movies => push_proto_field_varint(&mut sub, 2, 4),
        SearchTypeFilter::Shorts => push_proto_field_varint(&mut sub, 2, 9),
        SearchTypeFilter::Live => push_proto_field_varint(&mut sub, 8, 1),
    }
    match filters.duration {
        SearchDurationFilter::Any => {}
        SearchDurationFilter::Long => push_proto_field_varint(&mut sub, 3, 2),
        SearchDurationFilter::Short => push_proto_field_varint(&mut sub, 3, 4),
        SearchDurationFilter::Medium => push_proto_field_varint(&mut sub, 3, 5),
    }

    let features = filters.features;
    if features.hd {
        push_proto_field_varint(&mut sub, 4, 1);
    }
    if features.subtitles {
        push_proto_field_varint(&mut sub, 5, 1);
    }
    if features.creative_commons {
        push_proto_field_varint(&mut sub, 6, 1);
    }
    if features.three_d {
        push_proto_field_varint(&mut sub, 7, 1);
    }
    if features.live && filters.kind != SearchTypeFilter::Live {
        push_proto_field_varint(&mut sub, 8, 1);
    }
    if features.purchased {
        push_proto_field_varint(&mut sub, 9, 1);
    }
    if features.four_k {
        push_proto_field_varint(&mut sub, 14, 1);
    }
    if features.three_sixty {
        push_proto_field_varint(&mut sub, 15, 1);
    }
    if features.location {
        push_proto_field_varint(&mut sub, 23, 1);
    }
    if features.hdr {
        push_proto_field_varint(&mut sub, 25, 1);
    }
    if features.vr180 {
        push_proto_field_varint(&mut sub, 26, 1);
    }

    if !sub.is_empty() {
        push_proto_varint(&mut msg, (2 << 3) | 2);
        push_proto_varint(&mut msg, sub.len() as u64);
        msg.extend_from_slice(&sub);
    }

    if msg.is_empty() {
        None
    } else {
        Some(base64::engine::general_purpose::STANDARD.encode(msg))
    }
}

pub fn parse_duration_label_secs(label: &str) -> Option<u64> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.split(':');
    let a = parts.next()?.parse::<u64>().ok()?;
    let b = parts.next()?.parse::<u64>().ok()?;
    match parts.next() {
        None => Some(a * 60 + b),
        Some(c_str) => {
            if parts.next().is_some() {
                return None;
            }
            let c = c_str.parse::<u64>().ok()?;
            Some(a * 3600 + b * 60 + c)
        }
    }
}

pub fn apply_search_filters(
    mut results: Vec<SearchResult>,
    filters: SearchFilters,
) -> Vec<SearchResult> {
    results.retain(|item| {
        let secs = parse_duration_label_secs(&item.video.duration);
        let is_short = item.metadata.kind == SearchResultKind::Short
            || item.video.url.contains("/shorts/")
            || secs.is_some_and(|s| s > 0 && s <= 60);
        let is_live = item.metadata.is_live
            || (item.metadata.kind == SearchResultKind::Video
                && item.video.duration.trim().is_empty());

        let kind_ok = match filters.kind {
            SearchTypeFilter::All => true,
            SearchTypeFilter::Videos => {
                item.metadata.kind == SearchResultKind::Video && !is_short && !is_live
            }
            SearchTypeFilter::Shorts => is_short,
            SearchTypeFilter::Channels => item.metadata.kind == SearchResultKind::Channel,
            SearchTypeFilter::Playlists => item.metadata.kind == SearchResultKind::Playlist,
            SearchTypeFilter::Movies => item.metadata.kind == SearchResultKind::Movie,
            SearchTypeFilter::Live => is_live,
        };
        if !kind_ok {
            return false;
        }
        if filters.features.live && !is_live {
            return false;
        }
        match filters.duration {
            SearchDurationFilter::Any => true,
            SearchDurationFilter::Short => secs.is_some_and(|s| s < 180),
            SearchDurationFilter::Medium => secs.is_some_and(|s| (180..=1200).contains(&s)),
            SearchDurationFilter::Long => secs.is_some_and(|s| s > 1200),
        }
    });
    if filters.sort == SearchSort::Views {
        results.sort_by(|a, b| {
            b.metadata
                .view_count
                .unwrap_or(0)
                .cmp(&a.metadata.view_count.unwrap_or(0))
        });
    }
    results
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChannelFeedTab {
    #[default]
    Videos,
    Shorts,
    Streams,
}

pub fn fetch_channel_tab_videos_with_cancel(
    input: &str,
    tab: ChannelFeedTab,
    limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let (target, effective_limit) = parse_and_clamp_channel_request(input, limit)?;
    let tab_url = match tab {
        ChannelFeedTab::Videos => target.videos_tab_url(),
        ChannelFeedTab::Shorts => target.shorts_tab_url(),
        ChannelFeedTab::Streams => target.streams_tab_url(),
    };
    if let Ok(items) = fetch_channel_tab_videos_html(&tab_url, tab, effective_limit, cancellation)
        && !items.is_empty()
    {
        return Ok(items);
    }
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let limit_arg = effective_limit.to_string();
    let output = process::output(
        yt_dlp().args([
            "--flat-playlist",
            "--playlist-end",
            &limit_arg,
            "--dump-single-json",
            "--skip-download",
            "--no-warnings",
            "--",
            &tab_url,
        ]),
        cancellation,
        QUERY_TIMEOUT,
    )
    .map_err(|error| {
        format!("Não foi possível executar yt-dlp: {error}. Verifique se ele está ao lado do app ou no PATH.")
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let trimmed = stderr.trim();
        if trimmed.contains("This channel does not have a")
            || trimmed.contains("streams tab")
            || trimmed.contains("shorts tab")
        {
            return Ok(Vec::new());
        }
        return Err(format!(
            "Não foi possível consultar esta aba do canal: {trimmed}"
        ));
    }
    parse_channel_page(&output.stdout, &target, effective_limit).map(|page| page.videos)
}

#[cfg(test)]
pub fn find(query: &str) -> Result<Vec<Video>, String> {
    find_with_metadata(query)
        .map(|results| results.into_iter().map(|result| result.video).collect())
}

#[cfg(test)]
pub fn find_with_metadata(query: &str) -> Result<Vec<SearchResult>, String> {
    find_with_cancel(query, &Cancellation::default())
}

#[cfg(test)]
pub fn find_with_cancel(
    query: &str,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    find_filtered_with_cancel(query, SearchFilters::default(), cancellation)
}

pub fn find_filtered_with_cancel(
    query: &str,
    filters: SearchFilters,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let is_url = query.starts_with("https://") || query.starts_with("http://");
    if !is_url
        && let Ok(html_results) = fetch_youtube_search_html(query, filters, cancellation)
        && !html_results.is_empty()
    {
        return Ok(html_results);
    }
    if cancellation.cancelled() {
        return Err("Pesquisa cancelada.".to_owned());
    }
    let target = if is_url {
        query.to_owned()
    } else if filters.sort == SearchSort::Date {
        format!("ytsearchdate20:{query}")
    } else if !filters.is_default() {
        format!("ytsearch30:{query}")
    } else {
        format!("ytsearch20:{query}")
    };
    let mut command = yt_dlp();
    command.args([
        "--flat-playlist",
        "--dump-json",
        "--no-warnings",
        "--",
        &target,
    ]);
    let output = process::output(&mut command, cancellation, QUERY_TIMEOUT).map_err(|error| {
        format!("Não foi possível executar yt-dlp: {error}. Verifique se ele está ao lado do app ou no PATH.")
    })?;
    if !output.status.success() {
        return Err(format!(
            "A pesquisa falhou: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let parsed = parse_results(&output.stdout, !is_url)?;
    Ok(if is_url {
        parsed
    } else {
        apply_search_filters(parsed, filters)
    })
}

fn fetch_youtube_search_html(
    query: &str,
    filters: SearchFilters,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    if cancellation.cancelled() {
        return Err("Pesquisa cancelada.".to_owned());
    }
    let mut url = url::Url::parse("https://www.youtube.com/results")
        .map_err(|_| "URL de pesquisa inválida.".to_owned())?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("search_query", query);
        if let Some(sp) = encode_search_sp(filters) {
            pairs.append_pair("sp", &sp);
        }
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|error| format!("Falha ao preparar cliente HTTP: {error}"))?;
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, SUBTITLE_USER_AGENT)
        .header(
            reqwest::header::ACCEPT_LANGUAGE,
            "pt-BR,pt;q=0.9,en-US;q=0.8,en;q=0.7",
        )
        .send()
        .map_err(|error| format!("Falha na consulta ao YouTube: {error}"))?;
    if cancellation.cancelled() {
        return Err("Pesquisa cancelada.".to_owned());
    }
    if !response.status().is_success() {
        return Err(format!("YouTube retornou HTTP {}", response.status()));
    }
    let body = response
        .text()
        .map_err(|error| format!("Falha ao ler resposta do YouTube: {error}"))?;
    let data = extract_yt_initial_data(&body)
        .ok_or_else(|| "Não foi possível ler os dados iniciais de busca do YouTube.".to_owned())?;
    Ok(parse_yt_initial_data_search_results(&data))
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

fn normalize_youtube_asset_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("//") {
        format!("https:{trimmed}")
    } else {
        trimmed.to_owned()
    }
}

fn yt_text(value: Option<&Value>) -> String {
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

fn select_yt_thumbnail(thumbnails: Option<&Value>, target_width: u64) -> String {
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

fn parse_pt_br_compact_count(raw: &str) -> Option<u64> {
    let lower = raw.trim().replace('\u{00a0}', " ").to_lowercase();
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

fn clean_pt_br_view_label(raw: &str) -> Option<String> {
    let trimmed = raw.trim().replace('\u{00a0}', " ");
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

fn channel_url_from_browse_endpoint(endpoint: Option<&Value>) -> (Option<String>, Option<String>) {
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

fn has_verified_owner_badge(owner_badges: Option<&Value>) -> bool {
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

fn parse_yt_video_renderer(v: &Value) -> Option<SearchResult> {
    let video_id = v
        .get("videoId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    let title = yt_text(v.get("title"));
    if title.is_empty() {
        return None;
    }
    let nav_url = v
        .pointer("/navigationEndpoint/commandMetadata/webCommandMetadata/url")
        .and_then(Value::as_str)
        .unwrap_or("");
    let is_short = nav_url.starts_with("/shorts/");
    let url = if is_short {
        format!("https://www.youtube.com/shorts/{video_id}")
    } else {
        format!("https://www.youtube.com/watch?v={video_id}")
    };
    let duration = yt_text(v.get("lengthText"));
    let thumbnail = select_yt_thumbnail(v.pointer("/thumbnail/thumbnails"), 480);

    let channel_text = yt_text(
        v.get("ownerText")
            .or_else(|| v.get("longBylineText"))
            .or_else(|| v.get("shortBylineText")),
    );

    let (channel_id, channel_url) = channel_url_from_browse_endpoint(
        v.pointer("/channelThumbnailSupportedRenderers/channelThumbnailWithLinkRenderer/navigationEndpoint")
            .or_else(|| v.pointer("/ownerText/runs/0/navigationEndpoint"))
            .or_else(|| v.pointer("/longBylineText/runs/0/navigationEndpoint")),
    );
    let avatar_url = select_yt_thumbnail(
        v.pointer("/channelThumbnailSupportedRenderers/channelThumbnailWithLinkRenderer/thumbnail/thumbnails"),
        68,
    );
    let channel_avatar_url = (!avatar_url.is_empty()).then_some(avatar_url);
    let channel_verified = has_verified_owner_badge(v.get("ownerBadges"));

    let mut is_live = false;
    let mut badges = Vec::new();
    if let Some(badge_arr) = v.get("badges").and_then(Value::as_array) {
        for badge in badge_arr {
            let renderer = badge.get("metadataBadgeRenderer").unwrap_or(badge);
            let style = renderer.get("style").and_then(Value::as_str).unwrap_or("");
            let icon = renderer
                .pointer("/icon/iconType")
                .and_then(Value::as_str)
                .unwrap_or("");
            let label = renderer
                .get("label")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if style.contains("LIVE_NOW") || icon == "LIVE" || label.eq_ignore_ascii_case("ao vivo")
            {
                is_live = true;
            } else if !label.is_empty() && !badges.iter().any(|existing: &String| existing == label)
            {
                badges.push(label.to_owned());
            }
        }
    }
    if duration.is_empty()
        && let Some(overlays) = v.get("thumbnailOverlays").and_then(Value::as_array)
        && overlays.iter().any(|ov| {
            ov.pointer("/thumbnailOverlayTimeStatusRenderer/style")
                .and_then(Value::as_str)
                == Some("LIVE")
        })
    {
        is_live = true;
    }

    let short_views_raw = yt_text(v.get("shortViewCountText"));
    let full_views_raw = yt_text(v.get("viewCountText"));
    let view_count = parse_pt_br_compact_count(&full_views_raw)
        .or_else(|| parse_pt_br_compact_count(&short_views_raw));
    let view_count_text = clean_pt_br_view_label(&short_views_raw)
        .or_else(|| clean_pt_br_view_label(&full_views_raw));
    let published_time_raw = yt_text(v.get("publishedTimeText"));
    let published_time_text = (!published_time_raw.is_empty()).then_some(published_time_raw);

    let description = v
        .pointer("/detailedMetadataSnippets/0/snippetText")
        .or_else(|| v.get("descriptionSnippet"))
        .map(|val| yt_text(Some(val)))
        .filter(|text| !text.is_empty());

    Some(SearchResult {
        video: Video {
            title,
            channel: channel_text,
            url,
            thumbnail,
            duration: if is_live { String::new() } else { duration },
        },
        metadata: VideoMetadata {
            view_count,
            upload_date: None,
            description,
            channel_id,
            channel_url,
            kind: if is_short {
                SearchResultKind::Short
            } else {
                SearchResultKind::Video
            },
            is_live,
            published_time_text,
            view_count_text,
            channel_avatar_url,
            channel_verified,
            badges,
            playlist_count: None,
            subscriber_count_text: None,
        },
    })
}

fn parse_yt_shorts_lockup(s: &Value) -> Option<SearchResult> {
    let watch_ep = s.pointer("/onTap/innertubeCommand/reelWatchEndpoint");
    let video_id = watch_ep
        .and_then(|ep| ep.get("videoId"))
        .and_then(Value::as_str)
        .or_else(|| {
            s.get("entityId")
                .and_then(Value::as_str)
                .and_then(|id| id.strip_prefix("shorts-shelf-item-"))
        })?
        .trim();
    if video_id.is_empty() {
        return None;
    }
    let title = s
        .pointer("/overlayMetadata/primaryText/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .or_else(|| {
            s.get("accessibilityText")
                .and_then(Value::as_str)
                .map(str::trim)
        })?
        .to_owned();
    let views_raw = s
        .pointer("/overlayMetadata/secondaryText/content")
        .and_then(Value::as_str)
        .unwrap_or("");
    let view_count = parse_pt_br_compact_count(views_raw);
    let view_count_text = clean_pt_br_view_label(views_raw);
    let thumbnail = select_yt_thumbnail(
        s.pointer("/thumbnailViewModel/thumbnailViewModel/image/sources")
            .or_else(|| watch_ep.and_then(|ep| ep.pointer("/thumbnail/thumbnails")))
            .or_else(|| s.pointer("/thumbnail/sources")),
        480,
    );
    Some(SearchResult {
        video: Video {
            title,
            channel: String::new(),
            url: format!("https://www.youtube.com/shorts/{video_id}"),
            thumbnail,
            duration: "0:59".to_owned(),
        },
        metadata: VideoMetadata {
            view_count,
            view_count_text,
            kind: SearchResultKind::Short,
            ..Default::default()
        },
    })
}

fn parse_yt_channel_renderer(c: &Value) -> Option<SearchResult> {
    let channel_id = c
        .get("channelId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_channel_id(id))?
        .to_owned();
    let title = yt_text(c.get("title"));
    if title.is_empty() {
        return None;
    }
    let (_, resolved_url) = channel_url_from_browse_endpoint(c.get("navigationEndpoint"));
    let canonical_url =
        resolved_url.unwrap_or_else(|| format!("https://www.youtube.com/channel/{channel_id}"));
    let avatar = select_yt_thumbnail(c.pointer("/thumbnail/thumbnails"), 176);
    let handle_part = yt_text(c.get("subscriberCountText"));
    let subs_part = yt_text(c.get("videoCountText"));
    let combined_sub = match (handle_part.is_empty(), subs_part.is_empty()) {
        (false, false) => Some(format!("{handle_part} \u{2022} {subs_part}")),
        (false, true) => Some(handle_part),
        (true, false) => Some(subs_part),
        (true, true) => None,
    };
    let description = {
        let text = yt_text(c.get("descriptionSnippet"));
        (!text.is_empty()).then_some(text)
    };
    let verified = has_verified_owner_badge(c.get("ownerBadges"));
    Some(SearchResult {
        video: Video {
            title: title.clone(),
            channel: title,
            url: canonical_url.clone(),
            thumbnail: avatar.clone(),
            duration: String::new(),
        },
        metadata: VideoMetadata {
            description,
            channel_id: Some(channel_id),
            channel_url: Some(canonical_url),
            kind: SearchResultKind::Channel,
            channel_avatar_url: (!avatar.is_empty()).then_some(avatar),
            channel_verified: verified,
            subscriber_count_text: combined_sub,
            ..Default::default()
        },
    })
}

fn parse_yt_lockup_video(l: &Value) -> Option<SearchResult> {
    let content_type = l.get("contentType").and_then(Value::as_str).unwrap_or("");
    let content_id = l
        .get("contentId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    if content_type != "LOCKUP_CONTENT_TYPE_VIDEO"
        && (!content_type.is_empty() || !crate::history::is_valid_video_id(content_id))
    {
        return None;
    }
    let title = l
        .pointer("/metadata/lockupMetadataViewModel/title/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())?
        .to_owned();
    let thumbnail = select_yt_thumbnail(
        l.pointer("/contentImage/thumbnailViewModel/image/sources"),
        480,
    );
    let mut duration = String::new();
    let mut is_live = false;
    if let Some(overlays) = l
        .pointer("/contentImage/thumbnailViewModel/overlays")
        .and_then(Value::as_array)
    {
        for ov in overlays {
            if let Some(badges) = ov
                .pointer("/thumbnailOverlayBadgeViewModel/thumbnailBadges")
                .and_then(Value::as_array)
            {
                for badge_entry in badges {
                    let badge = badge_entry
                        .get("thumbnailBadgeViewModel")
                        .unwrap_or(badge_entry);
                    let style = badge
                        .get("badgeStyle")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let text = badge
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or("");
                    if style.contains("LIVE") || text.eq_ignore_ascii_case("ao vivo") {
                        is_live = true;
                    } else if !text.is_empty() && duration.is_empty() {
                        duration = text.to_owned();
                    }
                }
            }
        }
    }

    let mut channel = String::new();
    let mut channel_id = None;
    let mut channel_url = None;
    let mut channel_verified = false;
    let mut view_count = None;
    let mut view_count_text = None;
    let mut published_time_text = None;

    if let Some(rows) = l
        .pointer("/metadata/lockupMetadataViewModel/metadata/contentMetadataViewModel/metadataRows")
        .and_then(Value::as_array)
    {
        for row in rows {
            let Some(parts) = row.get("metadataParts").and_then(Value::as_array) else {
                continue;
            };
            for part in parts {
                let text_obj = part.get("text").unwrap_or(part);
                let text = text_obj
                    .get("content")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("");
                if text.is_empty() {
                    continue;
                }
                if let Some(cmd) = text_obj.pointer("/commandRuns/0/onTap/innertubeCommand") {
                    let (cid, curl) = channel_url_from_browse_endpoint(Some(cmd));
                    if cid.is_some() || curl.is_some() {
                        channel_id = cid;
                        channel_url = curl;
                        if channel.is_empty() {
                            channel = text.to_owned();
                        }
                        if text_obj
                            .get("attachmentRuns")
                            .and_then(Value::as_array)
                            .is_some_and(|runs| !runs.is_empty())
                        {
                            channel_verified = true;
                        }
                        continue;
                    }
                }
                let lower = text.to_lowercase();
                if lower.contains("visualiza")
                    || lower.contains("view")
                    || lower.contains("assistindo")
                {
                    view_count = parse_pt_br_compact_count(text);
                    view_count_text = clean_pt_br_view_label(text);
                } else if lower.starts_with("há ")
                    || lower.starts_with("transmitido ")
                    || lower.starts_with("estreou ")
                    || lower.contains(" ago")
                    || lower.contains("streamed ")
                {
                    let cleaned = text
                        .strip_prefix("Transmitido ")
                        .or_else(|| text.strip_prefix("Estreou "))
                        .unwrap_or(text)
                        .trim();
                    published_time_text = Some(cleaned.to_owned());
                } else if channel.is_empty()
                    && view_count.is_none()
                    && published_time_text.is_none()
                {
                    channel = text.to_owned();
                }
            }
        }
    }

    let avatar_url = select_yt_thumbnail(
        l.pointer(
            "/metadata/lockupMetadataViewModel/image/decoratedAvatarViewModel/avatar/avatarViewModel/image/sources",
        ),
        68,
    );
    let channel_avatar_url = (!avatar_url.is_empty()).then_some(avatar_url);

    Some(SearchResult {
        video: Video {
            title,
            channel,
            url: format!("https://www.youtube.com/watch?v={content_id}"),
            thumbnail,
            duration: if is_live { String::new() } else { duration },
        },
        metadata: VideoMetadata {
            view_count,
            upload_date: None,
            description: None,
            channel_id,
            channel_url,
            kind: SearchResultKind::Video,
            is_live,
            published_time_text,
            view_count_text,
            channel_avatar_url,
            channel_verified,
            ..Default::default()
        },
    })
}

fn parse_yt_lockup_playlist(l: &Value) -> Option<SearchResult> {
    let content_type = l.get("contentType").and_then(Value::as_str).unwrap_or("");
    let content_id = l
        .get("contentId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    if content_type != "LOCKUP_CONTENT_TYPE_PLAYLIST"
        && !content_id.starts_with("PL")
        && !content_id.starts_with("RD")
        && !content_id.starts_with("OL")
    {
        return None;
    }
    let title = l
        .pointer("/metadata/lockupMetadataViewModel/title/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())?
        .to_owned();
    let first_part = l.pointer(
        "/metadata/lockupMetadataViewModel/metadata/contentMetadataViewModel/metadataRows/0/metadataParts/0/text",
    );
    let channel = first_part
        .and_then(|p| p.get("content"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_owned();
    let (channel_id, channel_url) = channel_url_from_browse_endpoint(
        first_part.and_then(|p| p.pointer("/commandRuns/0/onTap/innertubeCommand")),
    );
    let playlist_count = l
        .pointer("/contentImage/collectionThumbnailViewModel/primaryThumbnail/thumbnailViewModel/overlays")
        .and_then(Value::as_array)
        .and_then(|ovs| {
            ovs.iter().find_map(|ov| {
                ov.pointer("/thumbnailOverlayBadgeViewModel/thumbnailBadges/0/thumbnailBadgeViewModel/text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            })
        });
    let thumbnail = select_yt_thumbnail(
        l.pointer("/contentImage/collectionThumbnailViewModel/primaryThumbnail/thumbnailViewModel/image/sources"),
        480,
    );
    Some(SearchResult {
        video: Video {
            title,
            channel,
            url: format!("https://www.youtube.com/playlist?list={content_id}"),
            thumbnail,
            duration: String::new(),
        },
        metadata: VideoMetadata {
            channel_id,
            channel_url,
            kind: SearchResultKind::Playlist,
            playlist_count,
            ..Default::default()
        },
    })
}

fn parse_yt_playlist_renderer(p: &Value) -> Option<SearchResult> {
    let playlist_id = p
        .get("playlistId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    let title = yt_text(p.get("title"));
    if title.is_empty() {
        return None;
    }
    let channel = yt_text(p.get("shortBylineText").or_else(|| p.get("longBylineText")));
    let (channel_id, channel_url) = channel_url_from_browse_endpoint(
        p.pointer("/shortBylineText/runs/0/navigationEndpoint")
            .or_else(|| p.pointer("/longBylineText/runs/0/navigationEndpoint")),
    );
    let thumbnail = select_yt_thumbnail(p.pointer("/thumbnails/0/thumbnails"), 480);
    let count_num = p.get("videoCount").and_then(Value::as_str).map(str::trim);
    let playlist_count = count_num
        .filter(|c| !c.is_empty())
        .map(|c| format!("{c} v\u{00ed}deos"));
    Some(SearchResult {
        video: Video {
            title,
            channel,
            url: format!("https://www.youtube.com/playlist?list={playlist_id}"),
            thumbnail,
            duration: String::new(),
        },
        metadata: VideoMetadata {
            channel_id,
            channel_url,
            kind: SearchResultKind::Playlist,
            playlist_count,
            ..Default::default()
        },
    })
}

fn collect_yt_search_items(
    value: &Value,
    out: &mut Vec<SearchResult>,
    seen: &mut std::collections::HashSet<String>,
) {
    let Some(obj) = value.as_object() else {
        if let Some(arr) = value.as_array() {
            for item in arr {
                collect_yt_search_items(item, out, seen);
            }
        }
        return;
    };
    if let Some(v) = obj
        .get("videoRenderer")
        .or_else(|| obj.get("gridVideoRenderer"))
        .or_else(|| obj.get("compactVideoRenderer"))
    {
        if let Some(res) = parse_yt_video_renderer(v)
            && seen.insert(res.video.url.clone())
        {
            out.push(res);
        }
        return;
    }
    if let Some(s) = obj.get("shortsLockupViewModel") {
        if let Some(res) = parse_yt_shorts_lockup(s)
            && seen.insert(res.video.url.clone())
        {
            out.push(res);
        }
        return;
    }
    if let Some(c) = obj.get("channelRenderer") {
        if let Some(res) = parse_yt_channel_renderer(c)
            && seen.insert(res.video.url.clone())
        {
            out.push(res);
        }
        return;
    }
    if let Some(l) = obj.get("lockupViewModel") {
        if let Some(res) = parse_yt_lockup_video(l).or_else(|| parse_yt_lockup_playlist(l))
            && seen.insert(res.video.url.clone())
        {
            out.push(res);
        }
        return;
    }
    if let Some(p) = obj
        .get("playlistRenderer")
        .or_else(|| obj.get("gridPlaylistRenderer"))
    {
        if let Some(res) = parse_yt_playlist_renderer(p)
            && seen.insert(res.video.url.clone())
        {
            out.push(res);
        }
        return;
    }
    for (key, child) in obj {
        // Skip secondary side rails (e.g., ads or watch-card playlists) when primary results exist.
        if key == "secondaryContents" {
            continue;
        }
        collect_yt_search_items(child, out, seen);
    }
}

pub fn parse_yt_initial_data_search_results(data: &Value) -> Vec<SearchResult> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let root = data
        .pointer("/contents/twoColumnSearchResultsRenderer/primaryContents")
        .unwrap_or(data);
    collect_yt_search_items(root, &mut out, &mut seen);
    out
}

#[cfg(test)]
pub fn fetch_channel_page(input: &str) -> Result<ChannelPage, String> {
    fetch_channel_page_with_limit(input, DEFAULT_CHANNEL_VIDEOS_LIMIT)
}

#[cfg(test)]
pub fn fetch_channel_page_with_limit(input: &str, limit: usize) -> Result<ChannelPage, String> {
    let (target, effective_limit) = parse_and_clamp_channel_request(input, limit)?;
    fetch_channel_page_for_target(&target, effective_limit)
}

#[cfg(test)]
pub fn fetch_channel_page_with_cache(
    input: &str,
    limit: usize,
    force_refresh: bool,
) -> Result<ChannelFetchOutcome, String> {
    fetch_channel_page_with_cancel(input, limit, force_refresh, &Cancellation::default())
}

pub fn fetch_channel_page_with_cancel(
    input: &str,
    limit: usize,
    force_refresh: bool,
    cancellation: &Cancellation,
) -> Result<ChannelFetchOutcome, String> {
    let cache = CHANNEL_PAGE_CACHE.get_or_init(|| Mutex::new(ChannelPageCache::default()));
    fetch_channel_page_cached_with(cache, input, limit, force_refresh, |target, limit| {
        fetch_channel_page_for_target_cancel(target, limit, cancellation)
    })
}

fn parse_and_clamp_channel_request(
    input: &str,
    limit: usize,
) -> Result<(ChannelTarget, usize), String> {
    let target = parse_channel_target(input).ok_or_else(|| {
        "Identificador ou URL de canal do YouTube inválido ou não suportado.".to_owned()
    })?;
    let effective_limit = limit.clamp(1, MAX_CHANNEL_VIDEOS_LIMIT);
    Ok((target, effective_limit))
}

fn fetch_channel_page_cached_with(
    cache: &Mutex<ChannelPageCache>,
    input: &str,
    limit: usize,
    force_refresh: bool,
    fetch: impl FnOnce(&ChannelTarget, usize) -> Result<ChannelPage, String>,
) -> Result<ChannelFetchOutcome, String> {
    let (target, effective_limit) = parse_and_clamp_channel_request(input, limit)?;
    let key = channel_cache_key(&target, effective_limit);
    fetch_with_cache(cache, &key, force_refresh, || {
        fetch(&target, effective_limit)
    })
}

#[cfg(test)]
fn fetch_channel_page_for_target(
    target: &ChannelTarget,
    effective_limit: usize,
) -> Result<ChannelPage, String> {
    fetch_channel_page_for_target_cancel(target, effective_limit, &Cancellation::default())
}

fn fetch_channel_page_for_target_cancel(
    target: &ChannelTarget,
    effective_limit: usize,
    cancellation: &Cancellation,
) -> Result<ChannelPage, String> {
    if let Ok(page) = fetch_channel_page_html(target, effective_limit, cancellation)
        && (!page.videos.is_empty()
            || !page.home_shelves.is_empty()
            || page.featured_video.is_some())
    {
        return Ok(page);
    }
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let limit_arg = effective_limit.to_string();
    let videos_url = target.videos_tab_url();
    let output = process::output(
        yt_dlp().args([
            "--flat-playlist",
            "--playlist-end",
            &limit_arg,
            "--dump-single-json",
            "--skip-download",
            "--no-warnings",
            "--",
            &videos_url,
        ]),
        cancellation,
        QUERY_TIMEOUT,
    )
    .map_err(|error| {
        format!("Não foi possível executar yt-dlp: {error}. Verifique se ele está ao lado do app ou no PATH.")
    })?;
    if !output.status.success() {
        return Err(format!(
            "Não foi possível consultar o canal: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_channel_page(&output.stdout, target, effective_limit)
}

fn fetch_youtube_initial_data_url(url: &str, cancellation: &Cancellation) -> Result<Value, String> {
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|error| format!("Falha ao preparar cliente HTTP: {error}"))?;
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, SUBTITLE_USER_AGENT)
        .header(
            reqwest::header::ACCEPT_LANGUAGE,
            "pt-BR,pt;q=0.9,en-US;q=0.8,en;q=0.7",
        )
        .send()
        .map_err(|error| format!("Falha na consulta ao YouTube: {error}"))?;
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    if !response.status().is_success() {
        return Err(format!("YouTube retornou HTTP {}", response.status()));
    }
    let body = response
        .text()
        .map_err(|error| format!("Falha ao ler resposta do YouTube: {error}"))?;
    extract_yt_initial_data(&body)
        .ok_or_else(|| "Não foi possível ler os dados iniciais da página do YouTube.".to_owned())
}

fn fetch_youtube_innertube_browse(
    continuation: &str,
    cancellation: &Cancellation,
) -> Result<Value, String> {
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| format!("Falha ao preparar cliente HTTP: {error}"))?;
    let payload = serde_json::json!({
        "context": {
            "client": {
                "hl": "pt",
                "gl": "BR",
                "clientName": "WEB",
                "clientVersion": "2.20260101.00.00"
            }
        },
        "continuation": continuation
    });
    let response = client
        .post("https://www.youtube.com/youtubei/v1/browse?prettyPrint=false")
        .header(reqwest::header::USER_AGENT, SUBTITLE_USER_AGENT)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(
            reqwest::header::ACCEPT_LANGUAGE,
            "pt-BR,pt;q=0.9,en-US;q=0.8,en;q=0.7",
        )
        .json(&payload)
        .send()
        .map_err(|error| format!("Falha na consulta browse ao YouTube: {error}"))?;
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    if !response.status().is_success() {
        return Err(format!(
            "YouTube browse retornou HTTP {}",
            response.status()
        ));
    }
    response
        .json::<Value>()
        .map_err(|error| format!("Falha ao decodificar resposta browse do YouTube: {error}"))
}

fn fetch_youtube_innertube_player_duration(
    video_id: &str,
    cancellation: &Cancellation,
) -> Option<String> {
    if cancellation.cancelled() || !crate::history::is_valid_video_id(video_id) {
        return None;
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .ok()?;
    let payload = serde_json::json!({
        "context": {
            "client": {
                "hl": "pt",
                "gl": "BR",
                "clientName": "WEB",
                "clientVersion": "2.20260101.00.00"
            }
        },
        "videoId": video_id
    });
    let response = client
        .post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false")
        .header(reqwest::header::USER_AGENT, SUBTITLE_USER_AGENT)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&payload)
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let val = response.json::<Value>().ok()?;
    let secs = val
        .pointer("/videoDetails/lengthSeconds")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<u64>().ok())?;
    let formatted = format_duration_seconds(secs);
    (!formatted.is_empty()).then_some(formatted)
}

pub fn decode_youtube_redirect_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let normalized = if trimmed.starts_with("/redirect") {
        format!("https://www.youtube.com{trimmed}")
    } else {
        trimmed.to_owned()
    };
    if (normalized.starts_with("https://www.youtube.com/redirect")
        || normalized.starts_with("http://www.youtube.com/redirect"))
        && let Ok(parsed) = url::Url::parse(&normalized)
    {
        for (k, v) in parsed.query_pairs() {
            if k == "q" && !v.trim().is_empty() {
                let target = v.trim();
                if target.starts_with("http://") || target.starts_with("https://") {
                    return target.to_owned();
                }
                return format!("https://{}", target.trim_start_matches('/'));
            }
        }
    }
    if normalized.starts_with("http://") || normalized.starts_with("https://") {
        normalized
    } else {
        format!("https://{}", normalized.trim_start_matches('/'))
    }
}

fn infer_external_link_title(url: &str) -> String {
    let lower = url.to_lowercase();
    if lower.contains("twitch.tv") {
        "Twitch".to_owned()
    } else if lower.contains("twitter.com") || lower.contains("x.com") {
        "Twitter".to_owned()
    } else if lower.contains("discord.gg") || lower.contains("discord.com") {
        "Discord".to_owned()
    } else if lower.contains("instagram.com") {
        "Instagram".to_owned()
    } else if lower.contains("tiktok.com") {
        "TikTok".to_owned()
    } else if lower.contains("facebook.com") {
        "Facebook".to_owned()
    } else if lower.contains("github.com") {
        "GitHub".to_owned()
    } else if lower.contains("reddit.com") {
        "Reddit".to_owned()
    } else if let Ok(parsed) = url::Url::parse(url)
        && let Some(host) = parsed.host_str()
    {
        host.strip_prefix("www.").unwrap_or(host).to_owned()
    } else {
        url.trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("www.")
            .to_owned()
    }
}

fn find_first_continuation_token(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(token) = map
                .get("continuationCommand")
                .and_then(|c| c.get("token"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                return Some(token.to_owned());
            }
            for child in map.values() {
                if let Some(found) = find_first_continuation_token(child) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(find_first_continuation_token),
        _ => None,
    }
}

fn find_about_channel_view_model(value: &Value) -> Option<&Value> {
    match value {
        Value::Object(map) => {
            if let Some(vm) = map.get("aboutChannelViewModel") {
                return Some(vm);
            }
            for child in map.values() {
                if let Some(found) = find_about_channel_view_model(child) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(find_about_channel_view_model),
        _ => None,
    }
}

fn apply_about_continuation_data(page: &mut ChannelPage, about_resp: &Value) {
    let Some(vm) = find_about_channel_view_model(about_resp) else {
        return;
    };
    if let Some(desc) = nonempty_str(vm, "description") {
        page.description = sanitize_channel_description(Some(desc));
    }
    if let Some(country) = nonempty_str(vm, "country") {
        page.country = Some(country.to_owned());
    }
    let joined = yt_text(vm.get("joinedDateText"));
    if !joined.is_empty() {
        page.joined_date_text = Some(joined);
    }
    if let Some(subs) = nonempty_str(vm, "subscriberCountText") {
        if page.subscriber_count.is_none() {
            page.subscriber_count = parse_pt_br_compact_count(subs);
        }
        if page.subscriber_count_text.is_none() {
            page.subscriber_count_text = Some(subs.to_owned());
        }
    }
    if let Some(videos_txt) = nonempty_str(vm, "videoCountText")
        && page.video_count_text.is_none()
    {
        page.video_count_text = Some(videos_txt.to_owned());
    }
    if let Some(views_txt) = nonempty_str(vm, "viewCountText") {
        page.view_count_text = Some(views_txt.to_owned());
        if page.view_count.is_none() {
            page.view_count = parse_pt_br_compact_count(views_txt);
        }
    }
    if let Some(links_arr) = vm.get("links").and_then(Value::as_array) {
        let mut parsed_links = Vec::new();
        for item in links_arr {
            let link_vm = item.get("channelExternalLinkViewModel").unwrap_or(item);
            let title = link_vm
                .pointer("/title/content")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            let raw_url = link_vm
                .pointer("/link/commandRuns/0/onTap/innertubeCommand/urlEndpoint/url")
                .or_else(|| link_vm.pointer("/link/content"))
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if !raw_url.is_empty() {
                let decoded = decode_youtube_redirect_url(raw_url);
                let label = if title.is_empty() {
                    infer_external_link_title(&decoded)
                } else {
                    title.to_owned()
                };
                parsed_links.push(ChannelExternalLink {
                    title: label,
                    url: decoded,
                });
            }
        }
        if !parsed_links.is_empty() {
            page.links = parsed_links;
        }
    }
}

fn fill_channel_defaults_on_result(
    res: &mut SearchResult,
    channel_title: Option<&str>,
    channel_id: Option<&str>,
    canonical_url: &str,
    verified: bool,
) {
    if res.video.channel.is_empty()
        && let Some(title) = channel_title
    {
        res.video.channel = title.to_owned();
    }
    if res.metadata.channel_id.is_none()
        && let Some(cid) = channel_id
    {
        res.metadata.channel_id = Some(cid.to_owned());
    }
    if res.metadata.channel_url.is_none() && !canonical_url.is_empty() {
        res.metadata.channel_url = Some(canonical_url.to_owned());
    }
    if verified {
        res.metadata.channel_verified = true;
    }
}

pub fn parse_channel_initial_data(
    featured_data: &Value,
    videos_data: Option<&Value>,
    requested_target: &ChannelTarget,
    limit: usize,
) -> Result<ChannelPage, String> {
    let effective_limit = limit.clamp(1, MAX_CHANNEL_VIDEOS_LIMIT);
    let data = featured_data;
    let meta = data.pointer("/metadata/channelMetadataRenderer");
    let page_header = data.pointer("/header/pageHeaderRenderer");
    let header_vm = page_header.and_then(|h| h.pointer("/content/pageHeaderViewModel"));

    let channel_id = meta
        .and_then(|m| nonempty_str(m, "externalId"))
        .filter(|id| is_valid_channel_id(id))
        .map(str::to_owned)
        .or_else(|| match requested_target {
            ChannelTarget::ChannelId(id) => Some(id.clone()),
            ChannelTarget::Handle(_) => None,
        });

    let mut handle = meta
        .and_then(|m| nonempty_str(m, "vanityChannelUrl"))
        .and_then(parse_youtube_channel_url)
        .and_then(|t| match t {
            ChannelTarget::Handle(h) => Some(h),
            ChannelTarget::ChannelId(_) => None,
        })
        .or_else(|| match requested_target {
            ChannelTarget::Handle(h) => Some(h.clone()),
            ChannelTarget::ChannelId(_) => None,
        });

    let mut subscriber_count = None;
    let mut subscriber_count_text = None;
    let mut video_count_text = None;

    if let Some(rows) = header_vm
        .and_then(|vm| vm.pointer("/metadata/contentMetadataViewModel/metadataRows"))
        .and_then(Value::as_array)
    {
        for row in rows {
            let Some(parts) = row.get("metadataParts").and_then(Value::as_array) else {
                continue;
            };
            for part in parts {
                let text = part
                    .pointer("/text/content")
                    .or_else(|| part.get("content"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("");
                if text.is_empty() {
                    continue;
                }
                if text.starts_with('@') && is_valid_channel_handle(text) {
                    handle = Some(text.to_owned());
                } else {
                    let lower = text.to_lowercase();
                    if lower.contains("inscrito") || lower.contains("subscriber") {
                        subscriber_count_text = Some(text.to_owned());
                        subscriber_count = parse_pt_br_compact_count(text);
                    } else if lower.contains("vídeo") || lower.contains("video") {
                        video_count_text = Some(text.to_owned());
                    }
                }
            }
        }
    }

    let resolved_target = if let Some(id) = &channel_id {
        ChannelTarget::ChannelId(id.clone())
    } else if let Some(h) = &handle {
        ChannelTarget::Handle(h.clone())
    } else {
        requested_target.clone()
    };
    let canonical_url = resolved_target.canonical_url();

    let title = page_header
        .and_then(|h| nonempty_str(h, "pageTitle"))
        .or_else(|| {
            header_vm
                .and_then(|vm| vm.pointer("/title/dynamicTextViewModel/text/content"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .or_else(|| meta.and_then(|m| nonempty_str(m, "title")))
        .map(str::to_owned);

    let verified = header_vm
        .and_then(|vm| vm.pointer("/title/dynamicTextViewModel/text/attachmentRuns"))
        .and_then(Value::as_array)
        .is_some_and(|runs| !runs.is_empty());

    let banner_url = header_vm
        .map(|vm| {
            select_yt_thumbnail(
                vm.pointer("/banner/imageBannerViewModel/image/sources"),
                2560,
            )
        })
        .filter(|s| !s.is_empty());

    let avatar_url = header_vm
        .map(|vm| {
            select_yt_thumbnail(
                vm.pointer("/image/decoratedAvatarViewModel/avatar/avatarViewModel/image/sources"),
                160,
            )
        })
        .filter(|s| !s.is_empty())
        .or_else(|| {
            meta.map(|m| select_yt_thumbnail(m.pointer("/avatar/thumbnails"), 160))
                .filter(|s| !s.is_empty())
        });

    let description_preview = header_vm
        .and_then(|vm| {
            vm.pointer("/description/descriptionPreviewViewModel/description/content")
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let description = meta
        .and_then(|m| nonempty_str(m, "description"))
        .and_then(|d| sanitize_channel_description(Some(d)))
        .or_else(|| description_preview.clone());

    let about_continuation = header_vm.and_then(|vm| {
        vm.get("description")
            .and_then(find_first_continuation_token)
            .or_else(|| {
                vm.get("attribution")
                    .and_then(find_first_continuation_token)
            })
    });

    let mut links = Vec::new();
    let mut more_links_label = None;
    if let Some(attr) = header_vm.and_then(|vm| vm.get("attribution")) {
        let attr_vm = attr.get("attributionViewModel").unwrap_or(attr);
        let raw_url = attr_vm
            .pointer("/text/commandRuns/0/onTap/innertubeCommand/urlEndpoint/url")
            .or_else(|| attr_vm.pointer("/text/content"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if !raw_url.is_empty() {
            let decoded = decode_youtube_redirect_url(raw_url);
            let title = infer_external_link_title(&decoded);
            links.push(ChannelExternalLink {
                title,
                url: decoded,
            });
        }
        if let Some(suffix) = attr_vm
            .pointer("/suffix/content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            more_links_label = Some(suffix.trim_start_matches("...").trim().to_owned());
        }
    }
    if let Some(same_as) = data
        .pointer("/microformat/microformatDataRenderer/channelProfileMicroformatDetails/profilePage/mainEntity/sameAs")
        .and_then(Value::as_array)
    {
        for item in same_as {
            if let Some(raw) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                let decoded = decode_youtube_redirect_url(raw);
                if !links.iter().any(|existing| existing.url.eq_ignore_ascii_case(&decoded)) {
                    links.push(ChannelExternalLink {
                        title: infer_external_link_title(&decoded),
                        url: decoded,
                    });
                }
            }
        }
    }

    let mut has_shorts_tab = false;
    let mut has_streams_tab = false;
    let mut has_playlists_tab = false;
    let mut featured_video: Option<SearchResult> = None;
    let mut home_shelves: Vec<ChannelHomeShelf> = Vec::new();
    let mut videos: Vec<SearchResult> = Vec::new();
    let mut videos_popular_continuation = None;
    let mut videos_oldest_continuation = None;

    if let Some(tabs) = data
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for tab_entry in tabs {
            let Some(tab) = tab_entry.get("tabRenderer") else {
                continue;
            };
            let tab_url = tab
                .pointer("/endpoint/commandMetadata/webCommandMetadata/url")
                .and_then(Value::as_str)
                .unwrap_or("");
            if tab_url.ends_with("/shorts") {
                has_shorts_tab = true;
            } else if tab_url.ends_with("/streams") {
                has_streams_tab = true;
            } else if tab_url.ends_with("/playlists") {
                has_playlists_tab = true;
            }

            if let Some(sections) = tab
                .pointer("/content/sectionListRenderer/contents")
                .and_then(Value::as_array)
            {
                for sec in sections {
                    let Some(item_sec) = sec
                        .pointer("/itemSectionRenderer/contents")
                        .and_then(Value::as_array)
                    else {
                        continue;
                    };
                    for item in item_sec {
                        if let Some(cvp) = item.get("channelVideoPlayerRenderer") {
                            if let Some(vid) = nonempty_str(cvp, "videoId") {
                                let v_title = yt_text(cvp.get("title"));
                                let v_desc = {
                                    let d = yt_text(cvp.get("description"));
                                    (!d.is_empty()).then_some(d)
                                };
                                let views_raw = yt_text(cvp.get("viewCountText"));
                                let view_count = parse_pt_br_compact_count(&views_raw);
                                let view_count_text = clean_pt_br_view_label(&views_raw);
                                let pub_raw = yt_text(cvp.get("publishedTimeText"));
                                let published_time_text = (!pub_raw.is_empty()).then_some(pub_raw);
                                featured_video = Some(SearchResult {
                                    video: Video {
                                        title: v_title,
                                        channel: title.clone().unwrap_or_default(),
                                        url: format!("https://www.youtube.com/watch?v={vid}"),
                                        thumbnail: format!(
                                            "https://i.ytimg.com/vi/{vid}/hqdefault.jpg"
                                        ),
                                        duration: String::new(),
                                    },
                                    metadata: VideoMetadata {
                                        view_count,
                                        description: v_desc,
                                        channel_id: channel_id.clone(),
                                        channel_url: Some(canonical_url.clone()),
                                        kind: SearchResultKind::Video,
                                        published_time_text,
                                        view_count_text,
                                        channel_verified: verified,
                                        ..Default::default()
                                    },
                                });
                            }
                        } else if let Some(shelf) = item.get("shelfRenderer") {
                            let shelf_title = yt_text(shelf.get("title"));
                            let subtitle = {
                                let sub = yt_text(shelf.get("subtitle"));
                                (!sub.is_empty()).then_some(sub)
                            };
                            let playlist_id = shelf
                                .pointer(
                                    "/playAllButton/buttonRenderer/navigationEndpoint/watchEndpoint/playlistId",
                                )
                                .and_then(Value::as_str)
                                .or_else(|| {
                                    shelf
                                        .pointer("/endpoint/browseEndpoint/browseId")
                                        .and_then(Value::as_str)
                                        .and_then(|b| b.strip_prefix("VL"))
                                })
                                .map(str::trim)
                                .filter(|id| is_valid_playlist_id(id) && !id.starts_with("UU"))
                                .map(str::to_owned);
                            let mut shelf_items = Vec::new();
                            let mut seen = std::collections::HashSet::new();
                            if let Some(content) = shelf.get("content") {
                                collect_yt_search_items(content, &mut shelf_items, &mut seen);
                            }
                            for entry in &mut shelf_items {
                                fill_channel_defaults_on_result(
                                    entry,
                                    title.as_deref(),
                                    channel_id.as_deref(),
                                    &canonical_url,
                                    verified,
                                );
                            }
                            if !shelf_items.is_empty() {
                                home_shelves.push(ChannelHomeShelf {
                                    title: shelf_title,
                                    subtitle,
                                    kind: ChannelShelfKind::Videos,
                                    playlist_id,
                                    items: shelf_items,
                                });
                            }
                        } else if let Some(reel) = item.get("reelShelfRenderer") {
                            let mut shelf_title = yt_text(reel.get("title"));
                            if shelf_title.is_empty() {
                                shelf_title = "Shorts".to_owned();
                            }
                            let mut shelf_items = Vec::new();
                            let mut seen = std::collections::HashSet::new();
                            if let Some(items_val) = reel.get("items") {
                                collect_yt_search_items(items_val, &mut shelf_items, &mut seen);
                            }
                            for entry in &mut shelf_items {
                                fill_channel_defaults_on_result(
                                    entry,
                                    title.as_deref(),
                                    channel_id.as_deref(),
                                    &canonical_url,
                                    verified,
                                );
                            }
                            if !shelf_items.is_empty() {
                                has_shorts_tab = true;
                                home_shelves.push(ChannelHomeShelf {
                                    title: shelf_title,
                                    subtitle: None,
                                    kind: ChannelShelfKind::Shorts,
                                    playlist_id: None,
                                    items: shelf_items,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(v_data) = videos_data
        && let Some(tabs) = v_data
            .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
            .and_then(Value::as_array)
    {
        for tab_entry in tabs {
            let Some(grid) = tab_entry.pointer("/tabRenderer/content/richGridRenderer") else {
                continue;
            };
            let mut seen = std::collections::HashSet::new();
            if let Some(contents) = grid.get("contents") {
                collect_yt_search_items(contents, &mut videos, &mut seen);
            }
            if let Some(chips) = grid
                .pointer("/header/chipBarViewModel/chips")
                .and_then(Value::as_array)
            {
                if let Some(chip_pop) = chips.get(1) {
                    videos_popular_continuation = find_first_continuation_token(chip_pop);
                }
                if let Some(chip_old) = chips.get(2) {
                    videos_oldest_continuation = find_first_continuation_token(chip_old);
                }
            }
            break;
        }
    }

    if videos.is_empty() {
        for shelf in &home_shelves {
            if shelf.kind == ChannelShelfKind::Videos {
                videos = shelf.items.clone();
                break;
            }
        }
    }

    for v in &mut videos {
        fill_channel_defaults_on_result(
            v,
            title.as_deref(),
            channel_id.as_deref(),
            &canonical_url,
            verified,
        );
    }
    videos.truncate(effective_limit);

    Ok(ChannelPage {
        target: resolved_target,
        channel_id,
        title,
        handle,
        canonical_url,
        description,
        description_preview,
        subscriber_count,
        view_count: None,
        subscriber_count_text,
        video_count_text,
        view_count_text: None,
        country: None,
        joined_date_text: None,
        links,
        more_links_label,
        about_continuation,
        featured_video,
        home_shelves,
        videos_popular_continuation,
        videos_oldest_continuation,
        has_shorts_tab,
        has_streams_tab,
        has_playlists_tab,
        avatar_url,
        banner_url,
        verified: Some(verified),
        tags: Vec::new(),
        videos,
    })
}

fn fetch_channel_page_html(
    target: &ChannelTarget,
    effective_limit: usize,
    cancellation: &Cancellation,
) -> Result<ChannelPage, String> {
    let featured_url = target.featured_tab_url();
    let videos_url = target.videos_tab_url();

    let (featured_res, videos_res) = std::thread::scope(|s| {
        let f_handle = s.spawn(|| fetch_youtube_initial_data_url(&featured_url, cancellation));
        let v_handle = s.spawn(|| fetch_youtube_initial_data_url(&videos_url, cancellation));
        (
            f_handle
                .join()
                .unwrap_or_else(|_| Err("Worker panic".to_owned())),
            v_handle
                .join()
                .unwrap_or_else(|_| Err("Worker panic".to_owned())),
        )
    });

    let featured_data = match featured_res {
        Ok(val) => val,
        Err(err) => {
            if let Ok(v_val) = videos_res {
                return parse_channel_initial_data(&v_val, Some(&v_val), target, effective_limit);
            }
            return Err(err);
        }
    };

    let mut page = parse_channel_initial_data(
        &featured_data,
        videos_res.as_ref().ok(),
        target,
        effective_limit,
    )?;

    let featured_vid_id = page
        .featured_video
        .as_ref()
        .filter(|v| v.video.duration.is_empty())
        .and_then(|v| crate::history::youtube_video_id(&v.video.url));
    let about_token = page.about_continuation.clone();

    if featured_vid_id.is_some() || about_token.is_some() {
        let (dur_opt, about_opt) = std::thread::scope(|s| {
            let d_handle = s.spawn(|| {
                featured_vid_id
                    .as_deref()
                    .and_then(|vid| fetch_youtube_innertube_player_duration(vid, cancellation))
            });
            let a_handle = s.spawn(|| {
                about_token
                    .as_deref()
                    .and_then(|tok| fetch_youtube_innertube_browse(tok, cancellation).ok())
            });
            (
                d_handle.join().ok().flatten(),
                a_handle.join().ok().flatten(),
            )
        });
        if let (Some(dur), Some(fv)) = (dur_opt, page.featured_video.as_mut()) {
            fv.video.duration = dur;
        }
        if let Some(about_resp) = about_opt {
            apply_about_continuation_data(&mut page, &about_resp);
        }
    }

    Ok(page)
}

fn fetch_channel_tab_videos_html(
    tab_url: &str,
    tab: ChannelFeedTab,
    effective_limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let data = fetch_youtube_initial_data_url(tab_url, cancellation)?;
    let meta = data.pointer("/metadata/channelMetadataRenderer");
    let channel_title = meta.and_then(|m| nonempty_str(m, "title"));
    let channel_id = meta
        .and_then(|m| nonempty_str(m, "externalId"))
        .filter(|id| is_valid_channel_id(id));
    let canonical_url = channel_id
        .map(|id| format!("https://www.youtube.com/channel/{id}"))
        .unwrap_or_default();
    let verified = data
        .pointer("/header/pageHeaderRenderer/content/pageHeaderViewModel/title/dynamicTextViewModel/text/attachmentRuns")
        .and_then(Value::as_array)
        .is_some_and(|runs| !runs.is_empty());

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(tabs) = data
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for tab_entry in tabs {
            let Some(tr) = tab_entry.get("tabRenderer") else {
                continue;
            };
            if tr.get("selected").and_then(Value::as_bool) != Some(true)
                && tr.get("content").is_none()
            {
                continue;
            }
            if let Some(content) = tr.get("content") {
                collect_yt_search_items(content, &mut out, &mut seen);
            }
        }
    }
    for item in &mut out {
        if tab == ChannelFeedTab::Shorts {
            item.metadata.kind = SearchResultKind::Short;
        }
        fill_channel_defaults_on_result(item, channel_title, channel_id, &canonical_url, verified);
    }
    out.truncate(effective_limit);
    Ok(out)
}

pub fn fetch_channel_sorted_videos_with_cancel(
    continuation_token: &str,
    channel_title: Option<&str>,
    channel_id: Option<&str>,
    canonical_url: &str,
    verified: bool,
    limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let effective_limit = limit.clamp(1, MAX_CHANNEL_VIDEOS_LIMIT);
    let resp = fetch_youtube_innertube_browse(continuation_token, cancellation)?;
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    collect_yt_search_items(&resp, &mut out, &mut seen);
    for item in &mut out {
        fill_channel_defaults_on_result(item, channel_title, channel_id, canonical_url, verified);
    }
    out.truncate(effective_limit);
    Ok(out)
}

pub fn fetch_channel_search_with_cancel(
    input: &str,
    query: &str,
    limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let (target, effective_limit) = parse_and_clamp_channel_request(input, limit)?;
    let trimmed_query = query.trim();
    if trimmed_query.is_empty() {
        return Ok(Vec::new());
    }
    let search_url = target
        .search_url(trimmed_query)
        .ok_or_else(|| "URL de busca do canal inválida.".to_owned())?;
    let data = fetch_youtube_initial_data_url(&search_url, cancellation)?;
    let meta = data.pointer("/metadata/channelMetadataRenderer");
    let channel_title = meta.and_then(|m| nonempty_str(m, "title"));
    let channel_id = meta
        .and_then(|m| nonempty_str(m, "externalId"))
        .filter(|id| is_valid_channel_id(id));
    let canonical_url = target.canonical_url();
    let verified = data
        .pointer("/header/pageHeaderRenderer/content/pageHeaderViewModel/title/dynamicTextViewModel/text/attachmentRuns")
        .and_then(Value::as_array)
        .is_some_and(|runs| !runs.is_empty());

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(tabs) = data
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for tab_entry in tabs {
            if let Some(expandable) = tab_entry.pointer("/expandableTabRenderer/content") {
                collect_yt_search_items(expandable, &mut out, &mut seen);
            } else if let Some(tr) = tab_entry.get("tabRenderer")
                && tr.get("selected").and_then(Value::as_bool) == Some(true)
                && let Some(content) = tr.get("content")
            {
                collect_yt_search_items(content, &mut out, &mut seen);
            }
        }
    }
    for item in &mut out {
        fill_channel_defaults_on_result(item, channel_title, channel_id, &canonical_url, verified);
    }
    out.truncate(effective_limit);
    Ok(out)
}

fn nonempty_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn valid_http_url(url: &str) -> Option<&str> {
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

fn format_duration_seconds(seconds: u64) -> String {
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
            upload_date: nonempty("upload_date").map(str::to_owned),
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

fn parse_results(output: &[u8], youtube_search: bool) -> Result<Vec<SearchResult>, String> {
    let mut videos = Vec::new();
    for line in output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
    {
        let value: Value = serde_json::from_slice(line)
            .map_err(|error| format!("Resposta inválida do yt-dlp: {error}"))?;
        if let Some(result) = parse_video_entry(&value, youtube_search, None, None, None) {
            videos.push(result);
        }
    }
    Ok(videos)
}

fn clean_channel_title(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    for suffix in [
        " - Videos",
        " - Vídeos",
        " - Playlists",
        " - About",
        " - Sobre",
    ] {
        if let Some(stripped) = trimmed.strip_suffix(suffix) {
            let stripped = stripped.trim();
            if !stripped.is_empty() {
                return Some(stripped.to_owned());
            }
        }
    }
    Some(trimmed.to_owned())
}

fn sanitize_channel_description(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(
            trimmed
                .chars()
                .take(MAX_CHANNEL_DESCRIPTION_CHARS)
                .collect(),
        )
    }
}

fn parse_channel_tags(value: &Value) -> Vec<String> {
    let Some(items) = value.get("tags").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut tags = Vec::new();
    for item in items {
        if tags.len() >= MAX_CHANNEL_TAGS {
            break;
        }
        let Some(raw) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        let sanitized: String = raw.chars().take(MAX_CHANNEL_TAG_CHARS).collect();
        if !sanitized.is_empty() && !tags.contains(&sanitized) {
            tags.push(sanitized);
        }
    }
    tags
}

fn select_channel_thumbnails(value: &Value) -> (Option<String>, Option<String>) {
    let Some(items) = value.get("thumbnails").and_then(Value::as_array) else {
        return (None, None);
    };

    let mut avatar_url: Option<String> = None;
    let mut banner_url: Option<String> = None;

    for item in items {
        let Some(url) = item
            .get("url")
            .and_then(Value::as_str)
            .and_then(valid_http_url)
        else {
            continue;
        };
        let id = item.get("id").and_then(Value::as_str).unwrap_or("");
        let width = item.get("width").and_then(Value::as_u64);
        let height = item.get("height").and_then(Value::as_u64);

        if id.contains("avatar") || (width.is_some() && width == height) {
            avatar_url = Some(url.to_owned());
        } else if id.contains("banner")
            || matches!((width, height), (Some(w), Some(h)) if h > 0 && w >= h * 2)
        {
            banner_url = Some(url.to_owned());
        }
    }

    (avatar_url, banner_url)
}

fn parse_channel_page(
    output: &[u8],
    requested_target: &ChannelTarget,
    limit: usize,
) -> Result<ChannelPage, String> {
    let effective_limit = limit.clamp(1, MAX_CHANNEL_VIDEOS_LIMIT);
    if output.iter().all(u8::is_ascii_whitespace) {
        return Err("Resposta vazia do yt-dlp ao consultar o canal.".to_owned());
    }

    if let Ok(single_value) = serde_json::from_slice::<Value>(output) {
        if single_value.get("entries").is_some()
            || single_value
                .get("_type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "playlist")
        {
            return parse_channel_playlist_json(&single_value, requested_target, effective_limit);
        }
        return parse_channel_json_lines(&[output], requested_target, effective_limit);
    }

    let trimmed_lines: Vec<&[u8]> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
        .collect();
    if trimmed_lines.is_empty() {
        return Err("Resposta vazia do yt-dlp ao consultar o canal.".to_owned());
    }

    parse_channel_json_lines(&trimmed_lines, requested_target, effective_limit)
}

fn parse_channel_playlist_json(
    root: &Value,
    requested_target: &ChannelTarget,
    limit: usize,
) -> Result<ChannelPage, String> {
    let entries = root
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| "Resposta inválida do yt-dlp: lista de vídeos ausente.".to_owned())?;
    let channel_id = ["channel_id", "id", "playlist_channel_id", "playlist_id"]
        .into_iter()
        .filter_map(|key| nonempty_str(root, key))
        .find(|candidate| is_valid_channel_id(candidate))
        .map(str::to_owned)
        .or_else(|| match requested_target {
            ChannelTarget::ChannelId(id) => Some(id.clone()),
            ChannelTarget::Handle(_) => None,
        });

    let handle = ["uploader_id", "playlist_uploader_id"]
        .into_iter()
        .filter_map(|key| nonempty_str(root, key))
        .find(|candidate| is_valid_channel_handle(candidate))
        .map(str::to_owned)
        .or_else(|| {
            ["uploader_url", "channel_url", "webpage_url"]
                .into_iter()
                .filter_map(|key| nonempty_str(root, key))
                .find_map(|url| match parse_youtube_channel_url(url) {
                    Some(ChannelTarget::Handle(handle)) => Some(handle),
                    _ => None,
                })
        })
        .or_else(|| match requested_target {
            ChannelTarget::Handle(handle) => Some(handle.clone()),
            ChannelTarget::ChannelId(_) => None,
        });

    let resolved_target = if let Some(id) = &channel_id {
        ChannelTarget::ChannelId(id.clone())
    } else if let Some(handle) = &handle {
        ChannelTarget::Handle(handle.clone())
    } else {
        requested_target.clone()
    };
    let canonical_url = resolved_target.canonical_url();

    let title = [
        "channel",
        "uploader",
        "playlist_channel",
        "playlist_uploader",
    ]
    .into_iter()
    .filter_map(|key| nonempty_str(root, key))
    .next()
    .map(str::to_owned)
    .or_else(|| nonempty_str(root, "title").and_then(clean_channel_title));

    let description = sanitize_channel_description(nonempty_str(root, "description"));
    let subscriber_count = root.get("channel_follower_count").and_then(Value::as_u64);
    let view_count = root.get("view_count").and_then(Value::as_u64);
    let verified = root.get("channel_is_verified").and_then(Value::as_bool);
    let (avatar_url, banner_url) = select_channel_thumbnails(root);
    let tags = parse_channel_tags(root);

    let mut videos = Vec::new();
    {
        let mut flat_entries: Vec<&Value> = Vec::new();
        for entry in entries {
            if let Some(nested) = entry.get("entries").and_then(Value::as_array) {
                flat_entries.extend(nested.iter());
            } else {
                flat_entries.push(entry);
            }
        }
        for entry in flat_entries {
            if videos.len() >= limit {
                break;
            }
            if let Some(video) = parse_video_entry(
                entry,
                true,
                title.as_deref(),
                channel_id.as_deref(),
                Some(&canonical_url),
            ) {
                videos.push(video);
            }
        }
    }

    Ok(ChannelPage {
        target: resolved_target,
        channel_id,
        title,
        handle,
        canonical_url,
        description,
        subscriber_count,
        view_count,
        avatar_url,
        banner_url,
        verified,
        tags,
        videos,
        ..Default::default()
    })
}

fn parse_channel_json_lines(
    lines: &[&[u8]],
    requested_target: &ChannelTarget,
    limit: usize,
) -> Result<ChannelPage, String> {
    let mut parsed_values = Vec::with_capacity(lines.len().min(limit));
    for line in lines {
        let value: Value = serde_json::from_slice(line)
            .map_err(|error| format!("Resposta inválida do yt-dlp: {error}"))?;
        parsed_values.push(value);
    }

    let first = &parsed_values[0];
    let channel_id = ["playlist_channel_id", "playlist_id"]
        .into_iter()
        .filter_map(|key| nonempty_str(first, key))
        .find(|candidate| is_valid_channel_id(candidate))
        .map(str::to_owned)
        .or_else(|| match requested_target {
            ChannelTarget::ChannelId(id) => Some(id.clone()),
            ChannelTarget::Handle(_) => None,
        });

    let handle = ["playlist_uploader_id"]
        .into_iter()
        .filter_map(|key| nonempty_str(first, key))
        .find(|candidate| is_valid_channel_handle(candidate))
        .map(str::to_owned)
        .or_else(|| match requested_target {
            ChannelTarget::Handle(handle) => Some(handle.clone()),
            ChannelTarget::ChannelId(_) => None,
        });

    let resolved_target = if let Some(id) = &channel_id {
        ChannelTarget::ChannelId(id.clone())
    } else if let Some(handle) = &handle {
        ChannelTarget::Handle(handle.clone())
    } else {
        requested_target.clone()
    };
    let canonical_url = resolved_target.canonical_url();

    let title = ["playlist_channel", "playlist_uploader"]
        .into_iter()
        .filter_map(|key| nonempty_str(first, key))
        .next()
        .map(str::to_owned)
        .or_else(|| nonempty_str(first, "playlist_title").and_then(clean_channel_title))
        .or_else(|| nonempty_str(first, "playlist").and_then(clean_channel_title));

    let mut videos = Vec::new();
    for value in &parsed_values {
        if videos.len() >= limit {
            break;
        }
        if let Some(video) = parse_video_entry(
            value,
            true,
            title.as_deref(),
            channel_id.as_deref(),
            Some(&canonical_url),
        ) {
            videos.push(video);
        }
    }

    Ok(ChannelPage {
        target: resolved_target,
        channel_id,
        title,
        handle,
        canonical_url,
        description: None,
        subscriber_count: None,
        view_count: None,
        avatar_url: None,
        banner_url: None,
        verified: None,
        tags: Vec::new(),
        videos,
        ..Default::default()
    })
}

pub fn update_cached_channel_videos(
    target: &ChannelTarget,
    effective_limit: usize,
    videos: &[SearchResult],
) {
    let Some(cache) = CHANNEL_PAGE_CACHE.get() else {
        return;
    };
    let key = channel_cache_key(target, effective_limit);
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    guard.evict_expired(now);
    if let Some(entry) = guard.entries.iter_mut().find(|entry| entry.key == key) {
        entry.page.videos = videos.to_vec();
        entry.last_used = now;
    }
}

pub fn fetch_channel_videos_batch_with_cancel(
    input: &str,
    start: usize,
    count: usize,
    cancellation: &Cancellation,
) -> Result<ChannelVideosBatch, String> {
    let target = parse_channel_target(input).ok_or_else(|| {
        "Identificador ou URL de canal do YouTube inválido ou não suportado.".to_owned()
    })?;
    if start == 0 || start > MAX_CHANNEL_VIDEOS_LIMIT {
        return Err(format!(
            "Limite máximo de {MAX_CHANNEL_VIDEOS_LIMIT} vídeos por canal atingido."
        ));
    }
    let effective_count = count.clamp(1, CHANNEL_VIDEOS_PAGE_STEP.max(MAX_CHANNEL_VIDEOS_LIMIT));
    let end = start
        .saturating_add(effective_count.saturating_sub(1))
        .min(MAX_CHANNEL_VIDEOS_LIMIT);
    let expected_slice_len = end.saturating_sub(start).saturating_add(1);
    let start_arg = start.to_string();
    let end_arg = end.to_string();
    let videos_url = target.videos_tab_url();
    let output = process::output(
        yt_dlp().args([
            "--flat-playlist",
            "--playlist-start",
            &start_arg,
            "--playlist-end",
            &end_arg,
            "--dump-single-json",
            "--skip-download",
            "--no-warnings",
            "--",
            &videos_url,
        ]),
        cancellation,
        QUERY_TIMEOUT,
    )
    .map_err(|error| {
        format!("Não foi possível executar yt-dlp: {error}. Verifique se ele está ao lado do app ou no PATH.")
    })?;
    if !output.status.success() {
        return Err(format!(
            "Não foi possível carregar mais vídeos do canal: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_channel_videos_batch(&output.stdout, &target, start, end, expected_slice_len)
}

fn parse_channel_videos_batch(
    output: &[u8],
    requested_target: &ChannelTarget,
    start: usize,
    end: usize,
    expected_slice_len: usize,
) -> Result<ChannelVideosBatch, String> {
    let page = parse_channel_page(output, requested_target, expected_slice_len)?;
    let reached_max_limit = end >= MAX_CHANNEL_VIDEOS_LIMIT;
    let has_more = !reached_max_limit && page.videos.len() >= expected_slice_len;
    Ok(ChannelVideosBatch {
        videos: page.videos,
        start,
        end,
        has_more,
        reached_max_limit,
    })
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

fn extract_playlist_id(value: &Value) -> Option<String> {
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

fn parse_channel_playlist_entry(value: &Value) -> Option<ChannelPlaylist> {
    let id = extract_playlist_id(value)?;
    let url = format!("https://www.youtube.com/playlist?list={id}");
    if !is_playlist_url(&url) {
        return None;
    }
    let title: String = nonempty_str(value, "title")
        .unwrap_or(&id)
        .chars()
        .take(512)
        .collect();
    let thumbnail = select_video_thumbnail(value);
    let item_count = value
        .get("playlist_count")
        .or_else(|| value.get("n_entries"))
        .or_else(|| value.get("entry_count"))
        .and_then(Value::as_u64);
    let description = sanitize_channel_description(nonempty_str(value, "description"));
    Some(ChannelPlaylist {
        id,
        title,
        url,
        thumbnail,
        item_count,
        description,
    })
}

fn fetch_channel_playlists_html(
    playlists_url: &str,
    effective_limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<ChannelPlaylist>, String> {
    let data = fetch_youtube_initial_data_url(playlists_url, cancellation)?;
    let mut raw_items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(tabs) = data
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for tab_entry in tabs {
            let Some(tr) = tab_entry.get("tabRenderer") else {
                continue;
            };
            if tr.get("selected").and_then(Value::as_bool) != Some(true)
                && tr.get("content").is_none()
            {
                continue;
            }
            if let Some(content) = tr.get("content") {
                collect_yt_search_items(content, &mut raw_items, &mut seen);
            }
        }
    }
    let mut playlists = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    for item in raw_items {
        if playlists.len() >= effective_limit {
            break;
        }
        if item.metadata.kind != SearchResultKind::Playlist {
            continue;
        }
        let Some(id) = item
            .video
            .url
            .strip_prefix("https://www.youtube.com/playlist?list=")
            .and_then(|rest| rest.split(['&', '#', '/']).next())
            .filter(|id| is_valid_playlist_id(id))
            .map(str::to_owned)
        else {
            continue;
        };
        if !seen_ids.insert(id.clone()) {
            continue;
        }
        let item_count = item
            .metadata
            .playlist_count
            .as_deref()
            .and_then(parse_pt_br_compact_count);
        playlists.push(ChannelPlaylist {
            id,
            title: item.video.title,
            url: item.video.url,
            thumbnail: item.video.thumbnail,
            item_count,
            description: item.metadata.description,
        });
    }
    Ok(playlists)
}

pub fn fetch_channel_playlists_with_cancel(
    input: &str,
    limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<ChannelPlaylist>, String> {
    let target = parse_channel_target(input).ok_or_else(|| {
        "Identificador ou URL de canal do YouTube inválido ou não suportado.".to_owned()
    })?;
    let effective_limit = limit.clamp(1, MAX_CHANNEL_PLAYLISTS_LIMIT);
    let playlists_url = target.playlists_tab_url();
    if let Ok(playlists) =
        fetch_channel_playlists_html(&playlists_url, effective_limit, cancellation)
        && !playlists.is_empty()
    {
        return Ok(playlists);
    }
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let limit_arg = effective_limit.to_string();
    let output = process::output(
        yt_dlp().args([
            "--flat-playlist",
            "--playlist-end",
            &limit_arg,
            "--dump-single-json",
            "--skip-download",
            "--no-warnings",
            "--",
            &playlists_url,
        ]),
        cancellation,
        QUERY_TIMEOUT,
    )
    .map_err(|error| {
        format!("Não foi possível executar yt-dlp: {error}. Verifique se ele está ao lado do app ou no PATH.")
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let trimmed = stderr.trim();
        if trimmed.contains("This channel does not have a")
            || trimmed.contains("playlists tab")
            || trimmed.contains("does not have any playlists")
        {
            return Ok(Vec::new());
        }
        return Err(format!(
            "Não foi possível consultar as playlists públicas do canal: {trimmed}"
        ));
    }
    parse_channel_playlists(&output.stdout, effective_limit)
}

fn parse_channel_playlists(output: &[u8], limit: usize) -> Result<Vec<ChannelPlaylist>, String> {
    let effective_limit = limit.clamp(1, MAX_CHANNEL_PLAYLISTS_LIMIT);
    if output.iter().all(u8::is_ascii_whitespace) {
        return Ok(Vec::new());
    }

    let mut playlists = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    if let Ok(single_value) = serde_json::from_slice::<Value>(output) {
        if let Some(entries) = single_value.get("entries").and_then(Value::as_array) {
            let mut flat_entries: Vec<&Value> = Vec::new();
            for entry in entries {
                if let Some(nested) = entry.get("entries").and_then(Value::as_array) {
                    flat_entries.extend(nested.iter());
                } else {
                    flat_entries.push(entry);
                }
            }
            for entry in flat_entries {
                if playlists.len() >= effective_limit {
                    break;
                }
                if let Some(playlist) = parse_channel_playlist_entry(entry)
                    && seen_ids.insert(playlist.id.clone())
                {
                    playlists.push(playlist);
                }
            }
            return Ok(playlists);
        }
        if let Some(playlist) = parse_channel_playlist_entry(&single_value) {
            playlists.push(playlist);
        }
        return Ok(playlists);
    }

    for line in output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
    {
        if playlists.len() >= effective_limit {
            break;
        }
        let value: Value = serde_json::from_slice(line)
            .map_err(|error| format!("Resposta inválida do yt-dlp: {error}"))?;
        if let Some(playlist) = parse_channel_playlist_entry(&value)
            && seen_ids.insert(playlist.id.clone())
        {
            playlists.push(playlist);
        }
    }
    Ok(playlists)
}

pub fn fetch_public_playlist_videos_with_cancel(
    playlist_id_or_url: &str,
    limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let url = youtube_playlist_url(playlist_id_or_url)
        .ok_or_else(|| "Identificador ou URL de playlist pública inválido.".to_owned())?;
    let effective_limit = limit.clamp(1, MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT);
    let limit_arg = effective_limit.to_string();
    let output = process::output(
        yt_dlp().args([
            "--flat-playlist",
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

fn parse_public_playlist_videos(output: &[u8], limit: usize) -> Result<Vec<SearchResult>, String> {
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

fn fetch_channel_about_html(
    target: &ChannelTarget,
    cancellation: &Cancellation,
) -> Result<ChannelAboutInfo, String> {
    let featured_url = target.featured_tab_url();
    let data = fetch_youtube_initial_data_url(&featured_url, cancellation)?;
    let mut page = parse_channel_initial_data(&data, None, target, 1)?;
    if let Some(token) = page.about_continuation.clone()
        && let Ok(about_resp) = fetch_youtube_innertube_browse(&token, cancellation)
    {
        apply_about_continuation_data(&mut page, &about_resp);
    }
    let tags = data
        .pointer("/microformat/microformatDataRenderer/tags")
        .and_then(Value::as_array)
        .map(|items| {
            let mut out = Vec::new();
            for item in items.iter().take(MAX_CHANNEL_TAGS) {
                if let Some(raw) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    let s: String = raw.chars().take(MAX_CHANNEL_TAG_CHARS).collect();
                    if !s.is_empty() && !out.contains(&s) {
                        out.push(s);
                    }
                }
            }
            out
        })
        .unwrap_or_default();
    Ok(ChannelAboutInfo {
        channel_id: page.channel_id,
        title: page.title,
        handle: page.handle,
        canonical_url: Some(page.canonical_url),
        description: page.description,
        subscriber_count: page.subscriber_count,
        view_count: page.view_count,
        subscriber_count_text: page.subscriber_count_text,
        video_count_text: page.video_count_text,
        view_count_text: page.view_count_text,
        country: page.country,
        joined_date_text: page.joined_date_text,
        links: page.links,
        avatar_url: page.avatar_url,
        banner_url: page.banner_url,
        verified: page.verified,
        tags,
    })
}

pub fn fetch_channel_about_with_cancel(
    input: &str,
    cancellation: &Cancellation,
) -> Result<ChannelAboutInfo, String> {
    let target = parse_channel_target(input).ok_or_else(|| {
        "Identificador ou URL de canal do YouTube inválido ou não suportado.".to_owned()
    })?;
    if let Ok(about) = fetch_channel_about_html(&target, cancellation) {
        return Ok(about);
    }
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let about_url = target.about_tab_url();
    let output = process::output(
        yt_dlp().args([
            "--flat-playlist",
            "--playlist-items",
            "0",
            "--dump-single-json",
            "--skip-download",
            "--no-warnings",
            "--",
            &about_url,
        ]),
        cancellation,
        QUERY_TIMEOUT,
    )
    .map_err(|error| {
        format!("Não foi possível executar yt-dlp: {error}. Verifique se ele está ao lado do app ou no PATH.")
    })?;
    if !output.status.success() {
        return Err(format!(
            "Não foi possível consultar as informações do canal: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_channel_about(&output.stdout, &target)
}

fn parse_channel_about(
    output: &[u8],
    requested_target: &ChannelTarget,
) -> Result<ChannelAboutInfo, String> {
    if output.iter().all(u8::is_ascii_whitespace) {
        return Err("Resposta vazia do yt-dlp ao consultar informações do canal.".to_owned());
    }
    let root: Value = serde_json::from_slice(output)
        .map_err(|error| format!("Resposta inválida do yt-dlp: {error}"))?;

    let channel_id = ["channel_id", "id", "playlist_channel_id", "playlist_id"]
        .into_iter()
        .filter_map(|key| nonempty_str(&root, key))
        .find(|candidate| is_valid_channel_id(candidate))
        .map(str::to_owned)
        .or_else(|| match requested_target {
            ChannelTarget::ChannelId(id) => Some(id.clone()),
            ChannelTarget::Handle(_) => None,
        });

    let handle = ["uploader_id", "playlist_uploader_id"]
        .into_iter()
        .filter_map(|key| nonempty_str(&root, key))
        .find(|candidate| is_valid_channel_handle(candidate))
        .map(str::to_owned)
        .or_else(|| {
            ["uploader_url", "channel_url", "webpage_url"]
                .into_iter()
                .filter_map(|key| nonempty_str(&root, key))
                .find_map(|url| match parse_youtube_channel_url(url) {
                    Some(ChannelTarget::Handle(handle)) => Some(handle),
                    _ => None,
                })
        })
        .or_else(|| match requested_target {
            ChannelTarget::Handle(handle) => Some(handle.clone()),
            ChannelTarget::ChannelId(_) => None,
        });

    let resolved_target = if let Some(id) = &channel_id {
        ChannelTarget::ChannelId(id.clone())
    } else if let Some(handle) = &handle {
        ChannelTarget::Handle(handle.clone())
    } else {
        requested_target.clone()
    };
    let canonical_url = Some(resolved_target.canonical_url());

    let title = [
        "channel",
        "uploader",
        "playlist_channel",
        "playlist_uploader",
    ]
    .into_iter()
    .filter_map(|key| nonempty_str(&root, key))
    .next()
    .map(str::to_owned)
    .or_else(|| nonempty_str(&root, "title").and_then(clean_channel_title));

    let description = sanitize_channel_description(nonempty_str(&root, "description"));
    let subscriber_count = root.get("channel_follower_count").and_then(Value::as_u64);
    let view_count = root.get("view_count").and_then(Value::as_u64);
    let verified = root.get("channel_is_verified").and_then(Value::as_bool);
    let (avatar_url, banner_url) = select_channel_thumbnails(&root);
    let tags = parse_channel_tags(&root);

    Ok(ChannelAboutInfo {
        channel_id,
        title,
        handle,
        canonical_url,
        description,
        subscriber_count,
        view_count,
        avatar_url,
        banner_url,
        verified,
        tags,
        ..Default::default()
    })
}

pub const YOUTUBE_SUGGEST_ENDPOINT: &str = "https://suggestqueries.google.com/complete/search";
const SEARCH_SUGGEST_TIMEOUT: Duration = Duration::from_secs(4);
const MAX_SEARCH_SUGGEST_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchSuggestionItem {
    pub text: String,
    pub is_recent: bool,
}

pub fn parse_youtube_suggest_json(body: &str) -> Result<Vec<String>, String> {
    let parsed: Value = serde_json::from_str(body)
        .map_err(|err| format!("Resposta inválida de sugestões de busca: {err}"))?;
    let suggestions = parsed
        .as_array()
        .and_then(|arr| arr.get(1))
        .and_then(Value::as_array)
        .ok_or_else(|| "Formato inesperado nas sugestões do YouTube.".to_owned())?;

    let mut out = Vec::new();
    let mut seen_lower = std::collections::HashSet::new();
    for entry in suggestions {
        let Some(raw) = entry.as_str() else {
            continue;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if seen_lower.insert(lower) {
            out.push(trimmed.to_owned());
            if out.len() >= 10 {
                break;
            }
        }
    }
    Ok(out)
}

pub fn merge_search_suggestions(
    query: &str,
    recent_searches: &[String],
    remote_suggestions: &[String],
    max_items: usize,
) -> Vec<SearchSuggestionItem> {
    if max_items == 0 {
        return Vec::new();
    }
    let trimmed = query.trim();
    let needle = trimmed.to_lowercase();
    let mut merged = Vec::new();
    let mut seen_lower = std::collections::HashSet::new();

    let mut matching_recent: Vec<(&str, bool)> = recent_searches
        .iter()
        .filter_map(|raw| {
            let clean = raw.trim();
            if clean.is_empty() {
                return None;
            }
            if needle.is_empty() {
                return Some((clean, true));
            }
            let lower = clean.to_lowercase();
            if lower.starts_with(&needle) {
                Some((clean, true))
            } else if lower.contains(&needle) {
                Some((clean, false))
            } else {
                None
            }
        })
        .collect();
    if !needle.is_empty() {
        matching_recent.sort_by_key(|(_, is_prefix)| !*is_prefix);
    }

    for (recent, _) in matching_recent {
        if merged.len() >= max_items {
            break;
        }
        let lower = recent.to_lowercase();
        if seen_lower.insert(lower) {
            merged.push(SearchSuggestionItem {
                text: recent.to_owned(),
                is_recent: true,
            });
        }
    }

    if !needle.is_empty() {
        for suggestion in remote_suggestions {
            if merged.len() >= max_items {
                break;
            }
            let clean = suggestion.trim();
            if clean.is_empty() {
                continue;
            }
            let lower = clean.to_lowercase();
            if seen_lower.insert(lower) {
                merged.push(SearchSuggestionItem {
                    text: clean.to_owned(),
                    is_recent: false,
                });
            }
        }
    }

    merged
}

pub fn fetch_search_suggestions_at(
    endpoint: &str,
    query: &str,
    cancellation: &Cancellation,
) -> Result<Vec<String>, String> {
    if cancellation.cancelled() {
        return Err("Consulta de sugestões cancelada.".to_owned());
    }
    let trimmed = query.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with(':')
    {
        return Ok(Vec::new());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(SEARCH_SUGGEST_TIMEOUT)
        .build()
        .map_err(|err| format!("Falha ao preparar cliente HTTP de sugestões: {err}"))?;
    let response = client
        .get(endpoint)
        .query(&[
            ("client", "firefox"),
            ("ds", "yt"),
            ("hl", "pt"),
            ("q", trimmed),
        ])
        .header(reqwest::header::USER_AGENT, SUBTITLE_USER_AGENT)
        .header(
            reqwest::header::ACCEPT_LANGUAGE,
            "pt-BR,pt;q=0.9,en-US;q=0.8,en;q=0.7",
        )
        .send()
        .map_err(|err| format!("Falha ao buscar sugestões: {err}"))?;
    if cancellation.cancelled() {
        return Err("Consulta de sugestões cancelada.".to_owned());
    }
    if !response.status().is_success() {
        return Err(format!(
            "Servidor de sugestões retornou HTTP {}",
            response.status().as_u16()
        ));
    }
    let mut limited = response.take((MAX_SEARCH_SUGGEST_BYTES + 1) as u64);
    let mut bytes = Vec::new();
    limited
        .read_to_end(&mut bytes)
        .map_err(|err| format!("Falha ao ler sugestões: {err}"))?;
    if cancellation.cancelled() {
        return Err("Consulta de sugestões cancelada.".to_owned());
    }
    if bytes.len() > MAX_SEARCH_SUGGEST_BYTES {
        return Err("Resposta de sugestões excedeu o limite permitido.".to_owned());
    }
    let body = String::from_utf8_lossy(&bytes);
    parse_youtube_suggest_json(&body)
}

pub fn fetch_search_suggestions(
    query: &str,
    cancellation: &Cancellation,
) -> Result<Vec<String>, String> {
    fetch_search_suggestions_at(YOUTUBE_SUGGEST_ENDPOINT, query, cancellation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_yt_dlp_is_resolved_from_executable_directory() {
        let root = std::env::temp_dir().join(format!(
            "fasttube-ytdlp-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let app_dir = root.join("portable");
        std::fs::create_dir_all(&app_dir).unwrap();
        let app = app_dir.join("fasttube.exe");
        std::fs::write(&app, b"fixture executable").unwrap();

        #[cfg(windows)]
        let helper_name = "yt-dlp.exe";
        #[cfg(not(windows))]
        let helper_name = "yt-dlp";
        assert_eq!(bundled_yt_dlp(&app), None);
        std::fs::write(app_dir.join(helper_name), b"fixture helper").unwrap();
        assert_eq!(bundled_yt_dlp(&app), Some(app_dir.join(helper_name)));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn libmpv_script_option_uses_the_bundled_helper_path() {
        assert_eq!(
            ytdl_hook_path_option(std::path::Path::new(r"D:\FastTube, portable\yt-dlp.exe")),
            r"ytdl_hook-ytdl_path=D:\\FastTube\, portable\\yt-dlp.exe"
        );
    }

    #[test]
    fn home_feed_distinguishes_videos_playlists_and_untrusted_entries() {
        let output = br#"{"id":"dQw4w9WgXcQ","title":"Video","ie_key":"Youtube","url":"https://www.youtube.com/watch?v=dQw4w9WgXcQ"}
{"id":"PLabc_123","title":"Playlist","ie_key":"YoutubeTab","url":"https://www.youtube.com/playlist?list=PLabc_123"}
{"id":"bad&x=1","title":"Bad","ie_key":"YoutubeTab","url":"https://example.org/"}
{"id":"dQw4w9WgXcQ","ie_key":"Youtube","url":"https://example.org/video"}
{"id":"something","ie_key":"Other","url":"https://example.org/"}
"#;
        let feed = parse_home_feed(output).unwrap();
        assert_eq!(feed.items.len(), 2);
        assert_eq!(
            feed.playlists,
            ["https://www.youtube.com/playlist?list=PLabc_123"]
        );
        assert_eq!(feed.items[1].video.url, feed.playlists[0]);
        assert!(parse_home_feed(b"not json").is_err());
        assert!(!is_playlist_url(
            "https://www.youtube.com/playlist?list=a&x=b"
        ));
    }

    #[test]
    fn home_batch_playlist_url_encoding_and_deduplication_work() {
        assert_eq!(
            build_playlist_search_url("The Strokes mix"),
            "https://www.youtube.com/results?search_query=The+Strokes+mix&sp=EgIQAw%3D%3D"
        );
        let fallback_line = br#"{"id":"abcDEF12345","title":"Video","ie_key":"Youtube","url":"https://www.youtube.com/watch?v=abcDEF12345","playlist_channel":"Canal X","playlist_channel_id":"UC1234567890123456789012"}"#;
        let parsed = parse_home_feed(fallback_line).unwrap();
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].video.channel, "Canal X");
        assert_eq!(
            parsed.items[0].metadata.channel_id.as_deref(),
            Some("UC1234567890123456789012")
        );

        let f1 = parse_home_feed(
            br#"{"id":"dQw4w9WgXcQ","title":"V1","ie_key":"Youtube","url":"https://www.youtube.com/watch?v=dQw4w9WgXcQ"}
{"id":"PLabc_123","title":"Mix One","ie_key":"YoutubeTab","url":"https://www.youtube.com/playlist?list=PLabc_123"}"#,
        )
        .unwrap();
        let f2 = parse_home_feed(
            br#"{"id":"dQw4w9WgXcQ","title":"V1 dup","ie_key":"Youtube","url":"https://youtu.be/dQw4w9WgXcQ"}
{"id":"xyz12345678","title":"V2","ie_key":"Youtube","url":"https://www.youtube.com/watch?v=xyz12345678"}
{"id":"PLabc_123","title":"Mix One dup","ie_key":"YoutubeTab","url":"https://www.youtube.com/playlist?list=PLabc_123"}"#,
        )
        .unwrap();
        let merged = merge_home_feeds(vec![f1, f2]);
        assert_eq!(merged.items.len(), 3);
        assert_eq!(merged.playlists.len(), 1);
    }

    use std::sync::mpsc;

    #[test]
    fn resolution_cache_reuses_success_and_expires() {
        let cache = Mutex::new(ResolutionCache::default());
        assert_eq!(
            cached_resolutions(&cache, "video", || Ok(vec![1080, 720])),
            Ok(vec![1080, 720])
        );
        assert_eq!(
            cached_resolutions(&cache, "video", || panic!("cache miss")),
            Ok(vec![1080, 720])
        );
        cache.lock().unwrap().entries[0].inserted = Instant::now() - RESOLUTION_CACHE_TTL;
        assert_eq!(
            cached_resolutions(&cache, "video", || Ok(vec![480])),
            Ok(vec![480])
        );
    }

    #[test]
    fn resolution_cache_does_not_store_errors_or_oversized_values() {
        let cache = Mutex::new(ResolutionCache::default());
        assert!(cached_resolutions(&cache, "video", || Err("failed".into())).is_err());
        assert_eq!(
            cached_resolutions(&cache, "video", || Ok(Vec::new())),
            Ok(Vec::new())
        );
        let heights = vec![1; RESOLUTION_CACHE_MAX_HEIGHTS + 1];
        assert_eq!(
            cached_resolutions(&cache, "video", || Ok(heights.clone())),
            Ok(heights)
        );
        let url = "x".repeat(RESOLUTION_CACHE_MAX_URL_BYTES + 1);
        assert_eq!(
            cached_resolutions(&cache, &url, || Ok(vec![720])),
            Ok(vec![720])
        );
        assert!(cache.lock().unwrap().entries.is_empty());
        assert_eq!(
            cached_resolutions(&cache, "video", || Ok(vec![720])),
            Ok(vec![720])
        );
    }

    #[test]
    fn resolution_cache_evicts_oldest_entry() {
        let cache = Mutex::new(ResolutionCache::default());
        for index in 0..=RESOLUTION_CACHE_LIMIT {
            cached_resolutions(&cache, &index.to_string(), || Ok(vec![720])).unwrap();
        }
        let guard = cache.lock().unwrap();
        let entries = &guard.entries;
        assert_eq!(entries.len(), RESOLUTION_CACHE_LIMIT);
        assert!(!entries.iter().any(|entry| entry.url == "0"));
        assert!(
            entries
                .iter()
                .any(|entry| entry.url == RESOLUTION_CACHE_LIMIT.to_string())
        );
    }

    fn wait_for_followers(cache: &Mutex<ResolutionCache>, url: &str, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let followers = cache
                .lock()
                .unwrap()
                .in_flight
                .get(url)
                .map_or(0, |flight| Arc::strong_count(flight).saturating_sub(2));
            if followers >= count {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "followers did not join in-flight query"
            );
            std::thread::yield_now();
        }
    }

    fn concurrent_result(result: Result<Vec<u32>, String>, url: String) {
        let cache = Arc::new(Mutex::new(ResolutionCache::default()));
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let leader_cache = Arc::clone(&cache);
        let leader_url = url.clone();
        let expected = result.clone();
        let leader = std::thread::spawn(move || {
            cached_resolutions(&leader_cache, &leader_url, || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                result
            })
        });
        started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let followers: Vec<_> = (0..4)
            .map(|_| {
                let cache = Arc::clone(&cache);
                let url = url.clone();
                std::thread::spawn(move || {
                    cached_resolutions(&cache, &url, || panic!("duplicate fetch"))
                })
            })
            .collect();
        wait_for_followers(&cache, &url, followers.len());
        release_tx.send(()).unwrap();
        assert_eq!(leader.join().unwrap(), expected);
        for follower in followers {
            assert_eq!(follower.join().unwrap(), expected);
        }
        assert!(cache.lock().unwrap().in_flight.is_empty());
        if expected.is_err()
            || expected.as_ref().unwrap().is_empty()
            || url.len() > RESOLUTION_CACHE_MAX_URL_BYTES
        {
            assert_eq!(
                cached_resolutions(&cache, &url, || Ok(vec![480])),
                Ok(vec![480])
            );
        }
    }

    #[test]
    fn concurrent_same_url_shares_results_and_errors() {
        concurrent_result(Ok(vec![1080]), "video".to_owned());
        concurrent_result(Err("failed".to_owned()), "video".to_owned());
        concurrent_result(Ok(Vec::new()), "video".to_owned());
        concurrent_result(
            Ok(vec![720]),
            "x".repeat(RESOLUTION_CACHE_MAX_URL_BYTES + 1),
        );
    }

    #[test]
    fn distinct_urls_fetch_in_parallel() {
        let cache = Arc::new(Mutex::new(ResolutionCache::default()));
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first_cache = Arc::clone(&cache);
        let first = std::thread::spawn(move || {
            cached_resolutions(&first_cache, "first", || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(vec![1080])
            })
        });
        started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let second_cache = Arc::clone(&cache);
        let second = std::thread::spawn(move || {
            done_tx
                .send(cached_resolutions(&second_cache, "second", || {
                    Ok(vec![720])
                }))
                .unwrap();
        });
        let result = done_rx.recv_timeout(Duration::from_secs(10));
        release_tx.send(()).unwrap();
        assert_eq!(result.unwrap(), Ok(vec![720]));
        assert_eq!(first.join().unwrap(), Ok(vec![1080]));
        second.join().unwrap();
    }

    #[test]
    fn panic_wakes_followers_and_allows_retry() {
        let cache = Arc::new(Mutex::new(ResolutionCache::default()));
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let leader_cache = Arc::clone(&cache);
        let leader = std::thread::spawn(move || {
            cached_resolutions(&leader_cache, "video", || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                panic!("fetch panic")
            })
        });
        started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let follower_cache = Arc::clone(&cache);
        let follower = std::thread::spawn(move || {
            cached_resolutions(&follower_cache, "video", || panic!("duplicate fetch"))
        });
        wait_for_followers(&cache, "video", 1);
        release_tx.send(()).unwrap();
        assert!(leader.join().is_err());
        assert!(follower.join().unwrap().is_err());
        assert!(cache.lock().unwrap().in_flight.is_empty());
        assert_eq!(
            cached_resolutions(&cache, "video", || Ok(vec![480])),
            Ok(vec![480])
        );
    }

    #[test]
    fn parses_flat_youtube_results_and_metadata() {
        let input = br#"{"id":"abc_123","title":"Example","channel":null,"uploader":"Creator","channel_id":"UC8butISFwT-Wl7EV0hUK0BQ","channel_url":"https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ","duration":3661.8,"view_count":0,"upload_date":"20261106","description":"Description","thumbnails":[{"width":320},{"width":300,"url":"https://example.com/image.jpg"}]}"#;
        let results = parse_results(input, true).unwrap();
        let result = &results[0];
        assert_eq!(result.video.url, "https://www.youtube.com/watch?v=abc_123");
        assert_eq!(result.video.channel, "Creator");
        assert_eq!(result.video.duration, "1:01:01");
        assert_eq!(result.video.thumbnail, "https://example.com/image.jpg");
        assert_eq!(result.metadata.view_count, Some(0));
        assert_eq!(result.metadata.upload_date.as_deref(), Some("20261106"));
        assert_eq!(result.metadata.description.as_deref(), Some("Description"));
        assert_eq!(
            result.metadata.channel_id.as_deref(),
            Some("UC8butISFwT-Wl7EV0hUK0BQ")
        );
        assert_eq!(
            result.metadata.channel_url.as_deref(),
            Some("https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ")
        );
        assert_eq!(
            channel_target_for_metadata(&result.metadata),
            Some(ChannelTarget::ChannelId(
                "UC8butISFwT-Wl7EV0hUK0BQ".to_owned()
            ))
        );
    }

    #[test]
    fn preserves_urls_and_absent_metadata() {
        let input = br#"{"id":"123","webpage_url":"https://example.com/watch/123","thumbnail":"https://example.com/image.jpg","view_count":null,"description":""}

{"id":"other"}"#;
        let results = parse_results(input, false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].video.url, "https://example.com/watch/123");
        assert_eq!(results[0].video.thumbnail, "https://example.com/image.jpg");
        assert!(results[0].metadata.view_count.is_none());
        assert!(results[0].metadata.upload_date.is_none());
        assert!(results[0].metadata.description.is_none());
        assert!(results[0].metadata.channel_id.is_none());
        assert!(results[0].metadata.channel_url.is_none());
        assert!(channel_target_for_metadata(&results[0].metadata).is_none());
    }

    #[test]
    fn handles_flat_urls_youtube_ids_and_invalid_json() {
        let input = br#"{"id":"x","url":"https://example.com/x"}
{"id":"abc-123","ie_key":"Youtube"}
{"id":"bad?id","extractor_key":"Youtube"}"#;
        let results = parse_results(input, false).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].video.url, "https://example.com/x");
        assert_eq!(
            results[1].video.url,
            "https://www.youtube.com/watch?v=abc-123"
        );
        assert!(parse_results(b"{invalid}", true).is_err());
    }

    #[test]
    fn validates_channel_ids_handles_and_urls_strictly() {
        let valid_id = "UC8butISFwT-Wl7EV0hUK0BQ";
        assert!(is_valid_channel_id(valid_id));
        assert!(!is_valid_channel_id("UC8butISFwT-Wl7EV0hUK0B")); // 23 chars
        assert!(!is_valid_channel_id("UC8butISFwT-Wl7EV0hUK0BQQ")); // 25 chars
        assert!(!is_valid_channel_id("AB8butISFwT-Wl7EV0hUK0BQ")); // wrong prefix
        assert!(!is_valid_channel_id("UC8butISFwT-Wl7EV0hUK0B?")); // invalid char

        assert!(is_valid_channel_handle("@freecodecamp"));
        assert!(is_valid_channel_handle("@rust_lang.org-1"));
        assert!(!is_valid_channel_handle("freecodecamp")); // missing @
        assert!(!is_valid_channel_handle("@ab")); // too short (<3 after @)
        assert!(!is_valid_channel_handle(&format!("@{}", "a".repeat(31)))); // too long (>30 after @)
        assert!(!is_valid_channel_handle("@bad/handle"));
        assert!(!is_valid_channel_handle("@bad handle"));

        assert_eq!(
            parse_channel_target(valid_id),
            Some(ChannelTarget::ChannelId(valid_id.to_owned()))
        );
        assert_eq!(
            parse_channel_target("  @freecodecamp  "),
            Some(ChannelTarget::Handle("@freecodecamp".to_owned()))
        );
        assert_eq!(
            parse_channel_target(
                "https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ/videos?view=0#top"
            ),
            Some(ChannelTarget::ChannelId(valid_id.to_owned()))
        );
        assert_eq!(
            parse_channel_target("https://m.youtube.com/@freecodecamp/streams/"),
            Some(ChannelTarget::Handle("@freecodecamp".to_owned()))
        );
        assert_eq!(
            youtube_channel_url("http://youtube.com:80/@freecodecamp"),
            Some("https://www.youtube.com/@freecodecamp".to_owned())
        );
        assert_eq!(
            youtube_channel_videos_url(valid_id),
            Some("https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ/videos".to_owned())
        );

        // Reject video URLs, external domains, credentials, arbitrary ports, and unknown paths
        assert!(parse_channel_target("https://www.youtube.com/watch?v=dQw4w9WgXcQ").is_none());
        assert!(parse_channel_target("https://youtu.be/dQw4w9WgXcQ").is_none());
        assert!(
            parse_channel_target("https://evil.com/channel/UC8butISFwT-Wl7EV0hUK0BQ").is_none()
        );
        assert!(
            parse_channel_target("https://user@www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ")
                .is_none()
        );
        assert!(
            parse_channel_target("https://www.youtube.com:8080/channel/UC8butISFwT-Wl7EV0hUK0BQ")
                .is_none()
        );
        assert!(parse_channel_target("https://www.youtube.com/@freecodecamp/unknown").is_none());
        assert!(parse_channel_target("-option-injection").is_none());
        assert!(fetch_channel_page("invalid-channel").is_err());
    }

    #[test]
    fn extracts_channel_association_only_when_reliable() {
        // 1. Valid channel_id + handle URL -> prefers canonical /channel/<UC...> URL
        let with_id = br#"{"id":"v1","ie_key":"Youtube","title":"V1","channel":"Ch","channel_id":"UC8butISFwT-Wl7EV0hUK0BQ","uploader_url":"https://www.youtube.com/@freecodecamp"}"#;
        let res = parse_results(with_id, true).unwrap();
        assert_eq!(
            res[0].metadata.channel_id.as_deref(),
            Some("UC8butISFwT-Wl7EV0hUK0BQ")
        );
        assert_eq!(
            res[0].metadata.channel_url.as_deref(),
            Some("https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ")
        );

        // 2. Only handle available (no UC id) -> preserves handle URL without inventing UC id
        let handle_only = br#"{"id":"v2","ie_key":"Youtube","title":"V2","channel":"Ch","uploader_id":"@freecodecamp"}"#;
        let res_handle = parse_results(handle_only, true).unwrap();
        assert!(res_handle[0].metadata.channel_id.is_none());
        assert_eq!(
            res_handle[0].metadata.channel_url.as_deref(),
            Some("https://www.youtube.com/@freecodecamp")
        );
        assert_eq!(
            channel_target_for_metadata(&res_handle[0].metadata),
            Some(ChannelTarget::Handle("@freecodecamp".to_owned()))
        );

        // 3. Only display name or invalid channel_id/channel_url -> does not invent association
        let unreliable = br#"{"id":"v3","ie_key":"Youtube","title":"V3","channel":"Just A Name","channel_id":"invalid_id","channel_url":"https://example.com/c/foo"}"#;
        let res_unreliable = parse_results(unreliable, true).unwrap();
        assert_eq!(res_unreliable[0].video.channel, "Just A Name");
        assert!(res_unreliable[0].metadata.channel_id.is_none());
        assert!(res_unreliable[0].metadata.channel_url.is_none());
        assert!(channel_target_for_metadata(&res_unreliable[0].metadata).is_none());

        // 4. Playlist-level fields (playlist_channel_id / playlist_uploader_id / playlist_channel)
        // on a search or playlist entry must NOT be treated as the video's own channel association.
        let playlist_only = br#"{"id":"v4","ie_key":"Youtube","title":"V4","playlist_channel":"Playlist Owner","playlist_uploader":"Playlist Owner","playlist_channel_id":"UC8butISFwT-Wl7EV0hUK0BQ","playlist_uploader_id":"@freecodecamp"}"#;
        let res_playlist = parse_results(playlist_only, true).unwrap();
        assert_eq!(res_playlist[0].video.channel, "");
        assert!(res_playlist[0].metadata.channel_id.is_none());
        assert!(res_playlist[0].metadata.channel_url.is_none());
        assert!(channel_target_for_metadata(&res_playlist[0].metadata).is_none());
    }

    #[test]
    fn parses_channel_playlist_json_with_limit_and_fallbacks() {
        let payload = br#"{
            "id": "UC8butISFwT-Wl7EV0hUK0BQ",
            "channel": "freeCodeCamp.org",
            "channel_id": "UC8butISFwT-Wl7EV0hUK0BQ",
            "title": "freeCodeCamp.org - Videos",
            "channel_follower_count": 12000000,
            "description": "Learn to code for free.",
            "uploader_id": "@freecodecamp",
            "uploader_url": "https://www.youtube.com/@freecodecamp",
            "channel_is_verified": true,
            "_type": "playlist",
            "thumbnails": [
                {"url": "https://yt3.googleusercontent.com/banner=w2560", "width": 2560, "height": 424, "id": "5"},
                {"url": "https://yt3.googleusercontent.com/avatar=s900", "width": 900, "height": 900, "id": "avatar_uncropped"}
            ],
            "entries": [
                {
                    "id": "eHHxRJulX74",
                    "ie_key": "Youtube",
                    "title": "First Video",
                    "duration": 3661,
                    "view_count": null,
                    "thumbnails": [{"url": "https://i.ytimg.com/vi/eHHxRJulX74/hq720.jpg", "width": 360, "height": 202}]
                },
                {
                    "id": "lR931Iz8Z6U",
                    "ie_key": "Youtube",
                    "title": "Second Video",
                    "duration": 125,
                    "view_count": 102000,
                    "thumbnails": [{"url": "https://i.ytimg.com/vi/lR931Iz8Z6U/hq720.jpg", "width": 360, "height": 202}]
                },
                {
                    "id": "third_video1",
                    "ie_key": "Youtube",
                    "title": "Third Video",
                    "duration": 60
                }
            ]
        }"#;

        let target = ChannelTarget::Handle("@freecodecamp".to_owned());
        let page = parse_channel_page(payload, &target, 2).unwrap();

        assert_eq!(
            page.target,
            ChannelTarget::ChannelId("UC8butISFwT-Wl7EV0hUK0BQ".to_owned())
        );
        assert_eq!(page.channel_id.as_deref(), Some("UC8butISFwT-Wl7EV0hUK0BQ"));
        assert_eq!(page.title.as_deref(), Some("freeCodeCamp.org"));
        assert_eq!(page.handle.as_deref(), Some("@freecodecamp"));
        assert_eq!(
            page.canonical_url,
            "https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ"
        );
        assert_eq!(page.description.as_deref(), Some("Learn to code for free."));
        assert_eq!(page.subscriber_count, Some(12_000_000));
        assert_eq!(page.verified, Some(true));
        assert_eq!(
            page.avatar_url.as_deref(),
            Some("https://yt3.googleusercontent.com/avatar=s900")
        );
        assert_eq!(
            page.banner_url.as_deref(),
            Some("https://yt3.googleusercontent.com/banner=w2560")
        );
        assert_eq!(page.videos.len(), 2);
        assert_eq!(page.videos[0].video.title, "First Video");
        assert_eq!(page.videos[0].video.channel, "freeCodeCamp.org");
        assert_eq!(page.videos[0].video.duration, "1:01:01");
        assert_eq!(
            page.videos[0].video.url,
            "https://www.youtube.com/watch?v=eHHxRJulX74"
        );
        assert!(page.videos[0].metadata.view_count.is_none());
        assert_eq!(
            page.videos[0].metadata.channel_id.as_deref(),
            Some("UC8butISFwT-Wl7EV0hUK0BQ")
        );
        assert_eq!(
            page.videos[0].metadata.channel_url.as_deref(),
            Some("https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ")
        );
        assert_eq!(page.videos[1].video.channel, "freeCodeCamp.org");
        assert_eq!(
            page.videos[1].metadata.channel_id.as_deref(),
            Some("UC8butISFwT-Wl7EV0hUK0BQ")
        );
        assert_eq!(
            page.videos[1].metadata.channel_url.as_deref(),
            Some("https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ")
        );
        assert_eq!(page.videos[1].metadata.view_count, Some(102_000));
    }

    #[test]
    fn parses_channel_page_with_missing_optional_fields_and_jsonl_fallback() {
        let minimal_playlist = r#"{
            "_type": "playlist",
            "title": "Canal Sem Extras - Vídeos",
            "entries": []
        }"#
        .as_bytes();
        let requested = ChannelTarget::Handle("@canalsemextras".to_owned());
        let page = parse_channel_page(minimal_playlist, &requested, 20).unwrap();
        assert_eq!(page.target, requested);
        assert!(page.channel_id.is_none());
        assert_eq!(page.title.as_deref(), Some("Canal Sem Extras"));
        assert_eq!(page.handle.as_deref(), Some("@canalsemextras"));
        assert_eq!(
            page.canonical_url,
            "https://www.youtube.com/@canalsemextras"
        );
        assert!(page.description.is_none());
        assert!(page.subscriber_count.is_none());
        assert!(page.avatar_url.is_none());
        assert!(page.banner_url.is_none());
        assert!(page.verified.is_none());
        assert!(page.videos.is_empty());

        let jsonl = br#"{"id":"dQw4w9WgXcQ","ie_key":"Youtube","title":"Video 1","playlist_channel":"Canal JSONL","playlist_channel_id":"UC8butISFwT-Wl7EV0hUK0BQ","playlist_uploader_id":"@canaljsonl"}
{"id":"abc_123-XYZ","ie_key":"Youtube","title":"Video 2"}"#;
        let jsonl_page = parse_channel_page(jsonl, &requested, 2).unwrap();
        assert_eq!(
            jsonl_page.channel_id.as_deref(),
            Some("UC8butISFwT-Wl7EV0hUK0BQ")
        );
        assert_eq!(jsonl_page.title.as_deref(), Some("Canal JSONL"));
        assert_eq!(jsonl_page.handle.as_deref(), Some("@canaljsonl"));
        assert_eq!(jsonl_page.videos.len(), 2);
        for video in &jsonl_page.videos {
            assert_eq!(video.video.channel, "Canal JSONL");
            assert_eq!(
                video.metadata.channel_id.as_deref(),
                Some("UC8butISFwT-Wl7EV0hUK0BQ")
            );
            assert_eq!(
                video.metadata.channel_url.as_deref(),
                Some("https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ")
            );
        }
        let jsonl_limited = parse_channel_page(jsonl, &requested, 1).unwrap();
        assert_eq!(jsonl_limited.videos.len(), 1);

        assert!(parse_channel_page(b"   \n  ", &requested, 10).is_err());
        assert!(parse_channel_page(b"{not-json}", &requested, 10).is_err());
    }

    fn sample_channel_page(handle: &str, title: &str, video_count: usize) -> ChannelPage {
        let target = ChannelTarget::Handle(handle.to_owned());
        let canonical_url = target.canonical_url();
        let videos = (0..video_count)
            .map(|idx| SearchResult {
                video: Video {
                    title: format!("{title} #{idx}"),
                    channel: title.to_owned(),
                    url: format!("https://www.youtube.com/watch?v=video_{idx:05}"),
                    thumbnail: String::new(),
                    duration: "1:00".to_owned(),
                },
                metadata: VideoMetadata {
                    view_count: Some(idx as u64),
                    upload_date: None,
                    description: None,
                    channel_id: None,
                    channel_url: Some(canonical_url.clone()),
                    ..Default::default()
                },
            })
            .collect();
        ChannelPage {
            target,
            channel_id: None,
            title: Some(title.to_owned()),
            handle: Some(handle.to_owned()),
            canonical_url,
            description: Some(format!("Description for {title}")),
            subscriber_count: Some(1_000),
            view_count: None,
            avatar_url: None,
            banner_url: None,
            verified: Some(true),
            tags: Vec::new(),
            videos,
            ..Default::default()
        }
    }

    #[test]
    fn decode_youtube_redirect_url_extracts_target_query_param() {
        let raw = "https://www.youtube.com/redirect?event=channel_header&redir_token=QUFFLUhqa18&q=http%3A%2F%2Fwww.twitch.tv%2Fthebausffs";
        assert_eq!(
            decode_youtube_redirect_url(raw),
            "http://www.twitch.tv/thebausffs"
        );
        assert_eq!(
            decode_youtube_redirect_url("discord.gg/thebausffs"),
            "https://discord.gg/thebausffs"
        );
    }

    #[test]
    fn parse_yt_lockup_video_parses_channel_grid_video_item() {
        let sample = serde_json::json!({
            "contentType": "LOCKUP_CONTENT_TYPE_VIDEO",
            "contentId": "o582gceJ3uY",
            "contentImage": {
                "thumbnailViewModel": {
                    "image": {
                        "sources": [
                            {"url": "https://i.ytimg.com/vi/o582gceJ3uY/hqdefault.jpg", "width": 336, "height": 188}
                        ]
                    },
                    "overlays": [{
                        "thumbnailOverlayBadgeViewModel": {
                            "thumbnailBadges": [{
                                "thumbnailBadgeViewModel": {
                                    "text": "14:39",
                                    "badgeStyle": "THUMBNAIL_OVERLAY_BADGE_STYLE_DEFAULT"
                                }
                            }]
                        }
                    }]
                }
            },
            "metadata": {
                "lockupMetadataViewModel": {
                    "title": {"content": "THE BEST GRAGAS YOU WILL EVER SEE!"},
                    "metadata": {
                        "contentMetadataViewModel": {
                            "metadataRows": [{
                                "metadataParts": [
                                    {"text": {"content": "314 mil visualizações"}},
                                    {"text": {"content": "há 1 ano"}}
                                ]
                            }]
                        }
                    }
                }
            }
        });
        let parsed = parse_yt_lockup_video(&sample).expect("should parse lockup video");
        assert_eq!(parsed.video.title, "THE BEST GRAGAS YOU WILL EVER SEE!");
        assert_eq!(
            parsed.video.url,
            "https://www.youtube.com/watch?v=o582gceJ3uY"
        );
        assert_eq!(parsed.video.duration, "14:39");
        assert_eq!(parsed.metadata.view_count, Some(314_000));
        assert_eq!(parsed.metadata.view_count_text.as_deref(), Some("314 mil"));
        assert_eq!(
            parsed.metadata.published_time_text.as_deref(),
            Some("há 1 ano")
        );
    }

    #[test]
    fn channel_cache_hit_returns_clone_without_calling_fetch() {
        use std::cell::Cell;

        let cache = Mutex::new(ChannelPageCache::default());
        let target = ChannelTarget::Handle("@freecodecamp".to_owned());
        let key = channel_cache_key(&target, 20);
        let calls = Cell::new(0usize);

        let first = fetch_with_cache(&cache, &key, false, || {
            calls.set(calls.get() + 1);
            Ok(sample_channel_page("@freecodecamp", "freeCodeCamp", 2))
        })
        .unwrap();
        assert!(!first.from_cache);
        assert_eq!(calls.get(), 1);
        assert_eq!(first.page.title.as_deref(), Some("freeCodeCamp"));

        let second = fetch_with_cache(&cache, &key, false, || {
            calls.set(calls.get() + 1);
            panic!("fetch closure should not be called on cache hit");
        })
        .unwrap();
        assert!(second.from_cache);
        assert_eq!(calls.get(), 1);
        assert_eq!(second.page, first.page);
    }

    #[test]
    fn channel_cache_expired_entry_is_miss_and_triggers_new_fetch() {
        use std::cell::Cell;

        let now = Instant::now();
        let mut direct_cache = ChannelPageCache::default();
        let key = "https://www.youtube.com/@freecodecamp|20".to_owned();
        direct_cache.insert(
            key.clone(),
            sample_channel_page("@freecodecamp", "Initial", 1),
            now,
        );
        assert!(
            direct_cache
                .get(&key, now + CHANNEL_PAGE_CACHE_TTL - Duration::from_secs(1))
                .is_some()
        );
        assert!(
            direct_cache
                .get(&key, now + CHANNEL_PAGE_CACHE_TTL)
                .is_none()
        );
        assert_eq!(direct_cache.len(), 0);

        let cache = Mutex::new(ChannelPageCache::default());
        let calls = Cell::new(0usize);
        let first = fetch_with_cache(&cache, &key, false, || {
            calls.set(calls.get() + 1);
            Ok(sample_channel_page("@freecodecamp", "V1", 1))
        })
        .unwrap();
        assert!(!first.from_cache);
        assert_eq!(calls.get(), 1);

        cache.lock().unwrap().entries[0].inserted = Instant::now() - CHANNEL_PAGE_CACHE_TTL;

        let refreshed = fetch_with_cache(&cache, &key, false, || {
            calls.set(calls.get() + 1);
            Ok(sample_channel_page("@freecodecamp", "V2", 2))
        })
        .unwrap();
        assert!(!refreshed.from_cache);
        assert_eq!(calls.get(), 2);
        assert_eq!(refreshed.page.title.as_deref(), Some("V2"));
    }

    #[test]
    fn channel_cache_capacity_lru_evicts_least_recently_used_and_get_refreshes_recency() {
        let base = Instant::now();
        let mut cache = ChannelPageCache::default();

        for idx in 1..=6 {
            let handle = format!("@channel_{idx}");
            let key = format!("https://www.youtube.com/{handle}|20");
            cache.insert(
                key,
                sample_channel_page(&handle, &format!("Channel {idx}"), 1),
                base + Duration::from_secs(idx as u64),
            );
        }
        assert_eq!(cache.len(), CHANNEL_PAGE_CACHE_CAPACITY);

        // Touch @channel_1 and @channel_2 so @channel_3 becomes the least recently used.
        assert!(
            cache
                .get(
                    "https://www.youtube.com/@channel_1|20",
                    base + Duration::from_secs(10)
                )
                .is_some()
        );
        assert!(
            cache
                .get(
                    "https://www.youtube.com/@channel_2|20",
                    base + Duration::from_secs(11)
                )
                .is_some()
        );

        // Insert 7th and 8th entries.
        cache.insert(
            "https://www.youtube.com/@channel_7|20".to_owned(),
            sample_channel_page("@channel_7", "Channel 7", 1),
            base + Duration::from_secs(12),
        );
        assert_eq!(cache.len(), CHANNEL_PAGE_CACHE_CAPACITY);

        cache.insert(
            "https://www.youtube.com/@channel_8|20".to_owned(),
            sample_channel_page("@channel_8", "Channel 8", 1),
            base + Duration::from_secs(13),
        );
        assert_eq!(cache.len(), CHANNEL_PAGE_CACHE_CAPACITY);
        assert!(cache.len() <= 6);

        let check_time = base + Duration::from_secs(14);
        // @channel_3 and @channel_4 had the oldest last_used timestamps and must be evicted.
        assert!(
            cache
                .get("https://www.youtube.com/@channel_3|20", check_time)
                .is_none()
        );
        assert!(
            cache
                .get("https://www.youtube.com/@channel_4|20", check_time)
                .is_none()
        );

        // Refreshed (@channel_1, @channel_2) and newer (@channel_5..8) entries survive.
        for surviving in [1, 2, 5, 6, 7, 8] {
            let key = format!("https://www.youtube.com/@channel_{surviving}|20");
            assert!(
                cache.get(&key, check_time).is_some(),
                "expected {key} to remain in cache"
            );
        }
    }

    #[test]
    fn channel_cache_different_limits_produce_distinct_entries() {
        use std::cell::Cell;

        let cache = Mutex::new(ChannelPageCache::default());
        let calls = Cell::new(0usize);

        let out_10 = fetch_channel_page_cached_with(
            &cache,
            "@freecodecamp",
            10,
            false,
            |target, effective_limit| {
                calls.set(calls.get() + 1);
                assert_eq!(target, &ChannelTarget::Handle("@freecodecamp".to_owned()));
                assert_eq!(effective_limit, 10);
                Ok(sample_channel_page("@freecodecamp", "FCC-10", 10))
            },
        )
        .unwrap();
        assert!(!out_10.from_cache);
        assert_eq!(out_10.page.videos.len(), 10);
        assert_eq!(calls.get(), 1);

        let out_25 = fetch_channel_page_cached_with(
            &cache,
            "https://www.youtube.com/@freecodecamp/videos",
            25,
            false,
            |_target, effective_limit| {
                calls.set(calls.get() + 1);
                assert_eq!(effective_limit, 25);
                Ok(sample_channel_page("@freecodecamp", "FCC-25", 25))
            },
        )
        .unwrap();
        assert!(!out_25.from_cache);
        assert_eq!(out_25.page.videos.len(), 25);
        assert_eq!(calls.get(), 2);
        assert_eq!(cache.lock().unwrap().len(), 2);

        let hit_10 = fetch_channel_page_cached_with(
            &cache,
            "https://www.youtube.com/@freecodecamp",
            10,
            false,
            |_, _| panic!("expected cache hit for limit 10"),
        )
        .unwrap();
        assert!(hit_10.from_cache);
        assert_eq!(hit_10.page.videos.len(), 10);

        let hit_25 = fetch_channel_page_cached_with(&cache, "@freecodecamp", 25, false, |_, _| {
            panic!("expected cache hit for limit 25")
        })
        .unwrap();
        assert!(hit_25.from_cache);
        assert_eq!(hit_25.page.videos.len(), 25);
    }

    #[test]
    fn channel_cache_force_refresh_bypasses_fresh_entry_and_stores_new_page() {
        use std::cell::Cell;

        let cache = Mutex::new(ChannelPageCache::default());
        let key = "https://www.youtube.com/@freecodecamp|20";
        let calls = Cell::new(0usize);

        let initial = fetch_with_cache(&cache, key, false, || {
            calls.set(calls.get() + 1);
            Ok(sample_channel_page("@freecodecamp", "Old Title", 1))
        })
        .unwrap();
        assert!(!initial.from_cache);
        assert_eq!(initial.page.title.as_deref(), Some("Old Title"));

        let forced = fetch_with_cache(&cache, key, true, || {
            calls.set(calls.get() + 1);
            Ok(sample_channel_page("@freecodecamp", "Refreshed Title", 3))
        })
        .unwrap();
        assert!(!forced.from_cache);
        assert_eq!(calls.get(), 2);
        assert_eq!(forced.page.title.as_deref(), Some("Refreshed Title"));
        assert_eq!(forced.page.videos.len(), 3);
        assert_eq!(cache.lock().unwrap().len(), 1);

        let subsequent_hit = fetch_with_cache(&cache, key, false, || {
            panic!("should hit updated cache entry")
        })
        .unwrap();
        assert!(subsequent_hit.from_cache);
        assert_eq!(
            subsequent_hit.page.title.as_deref(),
            Some("Refreshed Title")
        );
        assert_eq!(subsequent_hit.page.videos.len(), 3);
    }

    #[test]
    fn channel_cache_does_not_store_errors() {
        use std::cell::Cell;

        let cache = Mutex::new(ChannelPageCache::default());
        let key = "https://www.youtube.com/@freecodecamp|20";
        let calls = Cell::new(0usize);

        let err = fetch_with_cache(&cache, key, false, || {
            calls.set(calls.get() + 1);
            Err("yt-dlp failed".to_owned())
        });
        assert_eq!(err.unwrap_err(), "yt-dlp failed");
        assert_eq!(calls.get(), 1);
        assert_eq!(cache.lock().unwrap().len(), 0);

        let recovered = fetch_with_cache(&cache, key, false, || {
            calls.set(calls.get() + 1);
            Ok(sample_channel_page("@freecodecamp", "Recovered", 2))
        })
        .unwrap();
        assert!(!recovered.from_cache);
        assert_eq!(calls.get(), 2);
        assert_eq!(recovered.page.title.as_deref(), Some("Recovered"));
        assert_eq!(cache.lock().unwrap().len(), 1);
    }

    #[test]
    fn channel_cache_rejects_invalid_input_before_cache_or_fetch() {
        use std::cell::Cell;

        let cache = Mutex::new(ChannelPageCache::default());
        let calls = Cell::new(0usize);

        let err = fetch_channel_page_cached_with(
            &cache,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            20,
            false,
            |_, _| {
                calls.set(calls.get() + 1);
                Ok(sample_channel_page("@freecodecamp", "Never", 1))
            },
        )
        .unwrap_err();
        assert_eq!(
            err,
            "Identificador ou URL de canal do YouTube inválido ou não suportado."
        );
        assert_eq!(calls.get(), 0);
        assert_eq!(cache.lock().unwrap().len(), 0);

        let public_err = fetch_channel_page_with_cache("invalid-channel", 20, false).unwrap_err();
        assert_eq!(
            public_err,
            "Identificador ou URL de canal do YouTube inválido ou não suportado."
        );
    }

    #[test]
    fn parses_channel_videos_batch_and_enforces_pagination_limits() {
        let payload = br#"{
            "_type": "playlist",
            "id": "UC8butISFwT-Wl7EV0hUK0BQ",
            "channel": "freeCodeCamp.org",
            "channel_id": "UC8butISFwT-Wl7EV0hUK0BQ",
            "entries": [
                {"id": "dQw4w9WgXcQ", "ie_key": "Youtube", "title": "Batch Video 1", "duration": 60},
                {"id": "0123456789A", "ie_key": "Youtube", "title": "Batch Video 2", "duration": 120}
            ]
        }"#;
        let target = ChannelTarget::ChannelId("UC8butISFwT-Wl7EV0hUK0BQ".to_owned());

        let batch = parse_channel_videos_batch(payload, &target, 21, 22, 2).unwrap();
        assert_eq!(batch.videos.len(), 2);
        assert_eq!(batch.start, 21);
        assert_eq!(batch.end, 22);
        assert!(batch.has_more);
        assert!(!batch.reached_max_limit);

        let partial = parse_channel_videos_batch(payload, &target, 21, 40, 20).unwrap();
        assert_eq!(partial.videos.len(), 2);
        assert!(!partial.has_more);
        assert!(!partial.reached_max_limit);

        let capped = parse_channel_videos_batch(payload, &target, 99, 100, 2).unwrap();
        assert_eq!(capped.videos.len(), 2);
        assert!(!capped.has_more);
        assert!(capped.reached_max_limit);

        let cancel = Cancellation::default();
        assert!(fetch_channel_videos_batch_with_cancel("@freecodecamp", 0, 20, &cancel).is_err());
        assert!(
            fetch_channel_videos_batch_with_cancel(
                "@freecodecamp",
                MAX_CHANNEL_VIDEOS_LIMIT + 1,
                20,
                &cancel
            )
            .is_err()
        );
        assert!(
            fetch_channel_videos_batch_with_cancel("invalid-channel", 21, 20, &cancel).is_err()
        );
    }

    #[test]
    fn parses_public_channel_playlists_and_playlist_videos_safely() {
        let playlists_payload = br#"{
            "_type": "playlist",
            "title": "freeCodeCamp.org - Playlists",
            "entries": [
                {
                    "_type": "url",
                    "ie_key": "YoutubeTab",
                    "id": "PLWKjhJtqVAbnupFp3_281",
                    "title": "Python Tutorials",
                    "url": "https://www.youtube.com/playlist?list=PLWKjhJtqVAbnupFp3_281",
                    "playlist_count": 15,
                    "description": "Learn Python",
                    "thumbnails": [{"url": "https://i.ytimg.com/vi/abc/hqdefault.jpg", "width": 320, "height": 180}]
                },
                {
                    "_type": "url",
                    "ie_key": "YoutubeTab",
                    "id": "PLWKjhJtqVAbnupFp3_281",
                    "title": "Duplicate Playlist"
                },
                {
                    "_type": "url",
                    "id": "bad&id=1",
                    "url": "https://evil.example/playlist?list=bad"
                }
            ]
        }"#;
        let playlists = parse_channel_playlists(playlists_payload, 30).unwrap();
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].id, "PLWKjhJtqVAbnupFp3_281");
        assert_eq!(playlists[0].title, "Python Tutorials");
        assert_eq!(
            playlists[0].url,
            "https://www.youtube.com/playlist?list=PLWKjhJtqVAbnupFp3_281"
        );
        assert_eq!(playlists[0].item_count, Some(15));
        assert_eq!(playlists[0].description.as_deref(), Some("Learn Python"));

        let playlist_videos_payload = br#"{
            "_type": "playlist",
            "id": "PLWKjhJtqVAbnupFp3_281",
            "title": "Python Tutorials",
            "channel": "freeCodeCamp.org",
            "channel_id": "UC8butISFwT-Wl7EV0hUK0BQ",
            "entries": [
                {"id": "dQw4w9WgXcQ", "ie_key": "Youtube", "title": "Lesson 1", "duration": 300},
                {"id": "bad_id", "ie_key": "Youtube", "title": "Invalid ID"},
                {"id": "0123456789A", "ie_key": "Youtube", "title": "Lesson 2", "duration": 450}
            ]
        }"#;
        let videos = parse_public_playlist_videos(playlist_videos_payload, 50).unwrap();
        assert_eq!(videos.len(), 2);
        assert_eq!(
            videos[0].video.url,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
        assert_eq!(videos[0].video.channel, "freeCodeCamp.org");
        assert_eq!(
            videos[0].metadata.channel_id.as_deref(),
            Some("UC8butISFwT-Wl7EV0hUK0BQ")
        );
        assert_eq!(
            videos[1].video.url,
            "https://www.youtube.com/watch?v=0123456789A"
        );

        assert!(youtube_playlist_url("PLWKjhJtqVAbnupFp3_281").is_some());
        assert!(youtube_playlist_url("bad&injection").is_none());
    }

    #[test]
    fn parses_channel_about_info_and_truncates_oversized_fields() {
        let long_desc = "A".repeat(MAX_CHANNEL_DESCRIPTION_CHARS + 500);
        let payload = serde_json::json!({
            "id": "UC8butISFwT-Wl7EV0hUK0BQ",
            "channel": "freeCodeCamp.org",
            "channel_id": "UC8butISFwT-Wl7EV0hUK0BQ",
            "title": "freeCodeCamp.org - About",
            "uploader_id": "@freecodecamp",
            "description": long_desc,
            "channel_follower_count": 10500000u64,
            "view_count": 850000000u64,
            "channel_is_verified": true,
            "tags": ["programming", "rust", "programming", "web"],
            "thumbnails": [
                {"url": "https://yt3.googleusercontent.com/avatar=s900", "width": 900, "height": 900, "id": "avatar"}
            ]
        });
        let target = ChannelTarget::Handle("@freecodecamp".to_owned());
        let about = parse_channel_about(&serde_json::to_vec(&payload).unwrap(), &target).unwrap();
        assert_eq!(
            about.channel_id.as_deref(),
            Some("UC8butISFwT-Wl7EV0hUK0BQ")
        );
        assert_eq!(about.title.as_deref(), Some("freeCodeCamp.org"));
        assert_eq!(about.handle.as_deref(), Some("@freecodecamp"));
        assert_eq!(
            about.canonical_url.as_deref(),
            Some("https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ")
        );
        assert_eq!(
            about.description.as_ref().map(|d| d.chars().count()),
            Some(MAX_CHANNEL_DESCRIPTION_CHARS)
        );
        assert_eq!(about.subscriber_count, Some(10_500_000));
        assert_eq!(about.view_count, Some(850_000_000));
        assert_eq!(about.verified, Some(true));
        assert_eq!(about.tags, vec!["programming", "rust", "web"]);
        assert_eq!(
            about.avatar_url.as_deref(),
            Some("https://yt3.googleusercontent.com/avatar=s900")
        );
    }

    #[test]
    fn channel_target_exposes_shorts_and_streams_tab_urls() {
        let target = ChannelTarget::Handle("@rustlang".to_owned());
        assert_eq!(
            target.shorts_tab_url(),
            "https://www.youtube.com/@rustlang/shorts"
        );
        assert_eq!(
            target.streams_tab_url(),
            "https://www.youtube.com/@rustlang/streams"
        );
    }

    #[test]
    fn search_filters_filter_by_type_duration_and_sort_by_views() {
        let mk = |title: &str, url: &str, dur: &str, views: Option<u64>| SearchResult {
            video: Video {
                title: title.into(),
                channel: "Canal".into(),
                url: url.into(),
                thumbnail: String::new(),
                duration: dur.into(),
            },
            metadata: VideoMetadata {
                view_count: views,
                ..Default::default()
            },
        };
        let items = vec![
            mk(
                "Short",
                "https://www.youtube.com/shorts/aaa",
                "0:45",
                Some(100),
            ),
            mk(
                "Medium A",
                "https://www.youtube.com/watch?v=bbb",
                "8:10",
                Some(500),
            ),
            mk(
                "Medium B",
                "https://www.youtube.com/watch?v=ccc",
                "12:00",
                Some(1200),
            ),
            mk(
                "Long",
                "https://www.youtube.com/watch?v=ddd",
                "1:05:00",
                Some(3000),
            ),
            mk("Live", "https://www.youtube.com/watch?v=eee", "", Some(900)),
        ];

        assert_eq!(parse_duration_label_secs("8:10"), Some(490));
        assert_eq!(parse_duration_label_secs("1:05:00"), Some(3900));
        assert_eq!(parse_duration_label_secs(""), None);

        let shorts = apply_search_filters(
            items.clone(),
            SearchFilters {
                kind: SearchTypeFilter::Shorts,
                ..Default::default()
            },
        );
        assert_eq!(shorts.len(), 1);
        assert_eq!(shorts[0].video.title, "Short");

        let live = apply_search_filters(
            items.clone(),
            SearchFilters {
                kind: SearchTypeFilter::Live,
                ..Default::default()
            },
        );
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].video.title, "Live");

        let medium_by_views = apply_search_filters(
            items,
            SearchFilters {
                sort: SearchSort::Views,
                duration: SearchDurationFilter::Medium,
                kind: SearchTypeFilter::Videos,
                ..Default::default()
            },
        );
        assert_eq!(medium_by_views.len(), 2);
        assert_eq!(medium_by_views[0].video.title, "Medium B");
        assert_eq!(medium_by_views[1].video.title, "Medium A");
    }

    #[test]
    fn encode_search_sp_matches_official_youtube_tokens() {
        assert_eq!(encode_search_sp(SearchFilters::default()), None);
        assert_eq!(
            encode_search_sp(SearchFilters {
                sort: SearchSort::Views,
                ..Default::default()
            })
            .as_deref(),
            Some("CAM=")
        );
        assert_eq!(
            encode_search_sp(SearchFilters {
                sort: SearchSort::Date,
                ..Default::default()
            })
            .as_deref(),
            Some("CAI=")
        );
        assert_eq!(
            encode_search_sp(SearchFilters {
                kind: SearchTypeFilter::Shorts,
                ..Default::default()
            })
            .as_deref(),
            Some("EgIQCQ==")
        );
        assert_eq!(
            encode_search_sp(SearchFilters {
                kind: SearchTypeFilter::Channels,
                ..Default::default()
            })
            .as_deref(),
            Some("EgIQAg==")
        );
        assert_eq!(
            encode_search_sp(SearchFilters {
                kind: SearchTypeFilter::Playlists,
                ..Default::default()
            })
            .as_deref(),
            Some("EgIQAw==")
        );
        assert_eq!(
            encode_search_sp(SearchFilters {
                duration: SearchDurationFilter::Short,
                ..Default::default()
            })
            .as_deref(),
            Some("EgIYBA==")
        );
        assert_eq!(
            encode_search_sp(SearchFilters {
                upload_date: SearchUploadDateFilter::Today,
                ..Default::default()
            })
            .as_deref(),
            Some("EgIIAg==")
        );
        assert_eq!(
            encode_search_sp(SearchFilters {
                features: SearchFeatureFilters {
                    live: true,
                    ..Default::default()
                },
                ..Default::default()
            })
            .as_deref(),
            Some("EgJAAQ==")
        );
        assert_eq!(
            encode_search_sp(SearchFilters {
                sort: SearchSort::Views,
                kind: SearchTypeFilter::Videos,
                ..Default::default()
            })
            .as_deref(),
            Some("CAMSAhAB")
        );
    }

    #[test]
    fn parse_yt_initial_data_extracts_live_video_channel_playlist_and_short() {
        let payload = serde_json::json!({
            "contents": {
                "twoColumnSearchResultsRenderer": {
                    "primaryContents": {
                        "sectionListRenderer": {
                            "contents": [{
                                "itemSectionRenderer": {
                                    "contents": [
                                        {
                                            "videoRenderer": {
                                                "videoId": "live1234567",
                                                "title": { "runs": [{ "text": "GEN 1x1 BLG | Worlds 2026" }] },
                                                "viewCountText": { "runs": [{ "text": "64.100" }, { "text": " assistindo" }] },
                                                "shortViewCountText": { "runs": [{ "text": "64\u{00a0}mil" }, { "text": " assistindo" }] },
                                                "ownerText": {
                                                    "runs": [{
                                                        "text": "CBLOL",
                                                        "navigationEndpoint": {
                                                            "browseEndpoint": {
                                                                "browseId": "UC48rkW4j4z7N8h3r1bB9wXA",
                                                                "canonicalBaseUrl": "/@CBLOL"
                                                            }
                                                        }
                                                    }]
                                                },
                                                "ownerBadges": [{
                                                    "metadataBadgeRenderer": {
                                                        "style": "BADGE_STYLE_TYPE_VERIFIED",
                                                        "tooltip": "Verificado"
                                                    }
                                                }],
                                                "badges": [
                                                    { "metadataBadgeRenderer": { "style": "BADGE_STYLE_TYPE_LIVE_NOW", "label": "AO VIVO" } },
                                                    { "metadataBadgeRenderer": { "style": "BADGE_STYLE_TYPE_SIMPLE", "label": "Novo" } }
                                                ],
                                                "detailedMetadataSnippets": [{
                                                    "snippetText": { "runs": [{ "text": "Final do Worlds 2026 ao vivo." }] }
                                                }],
                                                "thumbnail": { "thumbnails": [{ "url": "https://i.ytimg.com/vi/live1234567/hq720.jpg", "width": 720 }] },
                                                "channelThumbnailSupportedRenderers": {
                                                    "channelThumbnailWithLinkRenderer": {
                                                        "thumbnail": { "thumbnails": [{ "url": "//yt3.ggpht.com/cblol=s68", "width": 68 }] },
                                                        "navigationEndpoint": {
                                                            "browseEndpoint": {
                                                                "browseId": "UC48rkW4j4z7N8h3r1bB9wXA",
                                                                "canonicalBaseUrl": "/@CBLOL"
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        },
                                        {
                                            "channelRenderer": {
                                                "channelId": "UCA-UimI-hyK_YVWNgKJsJ_w",
                                                "title": { "simpleText": "IOM Worlds 2026" },
                                                "thumbnail": { "thumbnails": [{ "url": "//yt3.ggpht.com/iom=s176", "width": 176 }] },
                                                "subscriberCountText": { "simpleText": "@IOMWorlds2026" },
                                                "videoCountText": { "simpleText": "989 inscritos" }
                                            }
                                        },
                                        {
                                            "lockupViewModel": {
                                                "contentId": "PLuiPiC1Hk8eLgTHu-INIEjm12_czwvMq4",
                                                "contentType": "LOCKUP_CONTENT_TYPE_PLAYLIST",
                                                "metadata": {
                                                    "lockupMetadataViewModel": {
                                                        "title": { "content": "Worlds 2026 Jogos" },
                                                        "metadata": {
                                                            "contentMetadataViewModel": {
                                                                "metadataRows": [{
                                                                    "metadataParts": [{
                                                                        "text": { "content": "Ilha das Lendas" }
                                                                    }]
                                                                }]
                                                            }
                                                        }
                                                    }
                                                },
                                                "contentImage": {
                                                    "collectionThumbnailViewModel": {
                                                        "primaryThumbnail": {
                                                            "thumbnailViewModel": {
                                                                "image": { "sources": [{ "url": "https://i.ytimg.com/vi/abc/hq720.jpg", "width": 720 }] },
                                                                "overlays": [{
                                                                    "thumbnailOverlayBadgeViewModel": {
                                                                        "thumbnailBadges": [{
                                                                            "thumbnailBadgeViewModel": { "text": "91 v\u{00ed}deos" }
                                                                        }]
                                                                    }
                                                                }]
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        },
                                        {
                                            "shortsLockupViewModel": {
                                                "entityId": "shorts-shelf-item-GtxjVvdMG90",
                                                "overlayMetadata": {
                                                    "primaryText": { "content": "Worlds 2026 Short" },
                                                    "secondaryText": { "content": "4\u{00a0}mil visualiza\u{00e7}\u{00f5}es" }
                                                }
                                            }
                                        }
                                    ]
                                }
                            }]
                        }
                    }
                }
            }
        });
        let items = parse_yt_initial_data_search_results(&payload);
        assert_eq!(items.len(), 4);

        assert_eq!(items[0].metadata.kind, SearchResultKind::Video);
        assert!(items[0].metadata.is_live);
        assert!(items[0].metadata.channel_verified);
        assert_eq!(
            items[0].metadata.view_count_text.as_deref(),
            Some("64 mil assistindo")
        );
        assert_eq!(items[0].metadata.badges, vec!["Novo"]);
        assert_eq!(
            items[0].metadata.channel_avatar_url.as_deref(),
            Some("https://yt3.ggpht.com/cblol=s68")
        );

        assert_eq!(items[1].metadata.kind, SearchResultKind::Channel);
        assert_eq!(
            items[1].metadata.subscriber_count_text.as_deref(),
            Some("@IOMWorlds2026 \u{2022} 989 inscritos")
        );

        assert_eq!(items[2].metadata.kind, SearchResultKind::Playlist);
        assert_eq!(
            items[2].metadata.playlist_count.as_deref(),
            Some("91 v\u{00ed}deos")
        );

        assert_eq!(items[3].metadata.kind, SearchResultKind::Short);
        assert_eq!(
            items[3].video.url,
            "https://www.youtube.com/shorts/GtxjVvdMG90"
        );
    }

    #[test]
    fn subtitle_track_cache_and_resolutions_json_parsing_work_together() {
        let json = serde_json::json!({
            "formats": [
                { "vcodec": "none", "height": 0 },
                { "vcodec": "avc1", "height": 720 },
                { "vcodec": "vp9", "height": 1080 },
                { "vcodec": "av01", "height": 1080 },
                { "vcodec": "avc1", "height": 480 }
            ],
            "subtitles": {
                "pt": [{ "ext": "vtt", "url": "https://www.youtube.com/api/timedtext?v=subcache123&lang=pt&fmt=vtt" }]
            },
            "automatic_captions": {
                "en": [{ "ext": "vtt", "url": "https://www.youtube.com/api/timedtext?v=subcache123&lang=en&fmt=vtt" }]
            }
        });

        let heights = parse_resolutions_from_json(&json).unwrap();
        assert_eq!(heights, vec![1080, 720, 480]);

        let tracks = parse_subtitles(&json);
        assert_eq!(tracks.len(), 2);

        let watch_url = "https://www.youtube.com/watch?v=subcache123";
        let short_url = "https://youtu.be/subcache123?t=10";
        store_cached_subtitle_tracks(watch_url, &tracks);
        assert_eq!(get_cached_subtitle_tracks(short_url), Some(tracks));
        assert_eq!(
            get_cached_subtitle_tracks("https://example.com/not-yt"),
            None
        );
    }

    #[test]
    fn parse_youtube_suggest_json_deduplicates_and_validates_shape() {
        let raw = r#"["rust",["rust tutorial","  Rust Tutorial ","rust egui","","cargo clippy"]]"#;
        let parsed = parse_youtube_suggest_json(raw).unwrap();
        assert_eq!(parsed, vec!["rust tutorial", "rust egui", "cargo clippy"]);

        assert!(parse_youtube_suggest_json("{}").is_err());
        assert!(parse_youtube_suggest_json("[\"rust\"]").is_err());
    }

    #[test]
    fn merge_search_suggestions_prioritizes_recent_prefix_and_deduplicates_remote() {
        let recent = vec![
            "aprender rust".to_owned(),
            "rust egui".to_owned(),
            "mpv player".to_owned(),
        ];
        let remote = vec![
            "Rust Egui".to_owned(),
            "rust async tokio".to_owned(),
            "rust lang brasil".to_owned(),
        ];
        let merged = merge_search_suggestions("rust", &recent, &remote, 5);
        assert_eq!(
            merged,
            vec![
                SearchSuggestionItem {
                    text: "rust egui".into(),
                    is_recent: true,
                },
                SearchSuggestionItem {
                    text: "aprender rust".into(),
                    is_recent: true,
                },
                SearchSuggestionItem {
                    text: "rust async tokio".into(),
                    is_recent: false,
                },
                SearchSuggestionItem {
                    text: "rust lang brasil".into(),
                    is_recent: false,
                },
            ]
        );
    }

    #[test]
    fn fetch_search_suggestions_respects_cancellation_and_parses_local_http_response() {
        let cancelled = Cancellation::default();
        cancelled.cancel();
        assert!(fetch_search_suggestions_at("http://127.0.0.1:9", "rust", &cancelled).is_err());

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            use std::io::{Read as _, Write as _};
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let body = r#"["rust egui",["rust egui tutorial","rust egui table"]]"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });

        let endpoint = format!("http://{addr}/complete/search");
        let cancel = Cancellation::default();
        let results = fetch_search_suggestions_at(&endpoint, "rust egui", &cancel).unwrap();
        assert_eq!(results, vec!["rust egui tutorial", "rust egui table"]);
        let _ = handle.join();
    }
}
