use crate::process::{self, Cancellation};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VideoMetadata {
    pub view_count: Option<u64>,
    pub upload_date: Option<String>,
    pub description: Option<String>,
    pub channel_id: Option<String>,
    pub channel_url: Option<String>,
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
}

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

fn parse_subtitles(value: &Value) -> Vec<SubtitleTrack> {
    let mut tracks = Vec::new();
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
            let Some(url) = formats.as_array().and_then(|formats| {
                formats.iter().find_map(|format| {
                    if format["ext"].as_str() != Some("vtt") {
                        return None;
                    }
                    let value = format["url"].as_str()?;
                    let parsed = url::Url::parse(value).ok()?;
                    let host = parsed.host_str()?;
                    (parsed.scheme() == "https"
                        && parsed.username().is_empty()
                        && parsed.password().is_none()
                        && (host == "youtube.com"
                            || host.ends_with(".youtube.com")
                            || host == "googlevideo.com"
                            || host.ends_with(".googlevideo.com")
                            || host == "ytimg.com"
                            || host.ends_with(".ytimg.com")))
                    .then_some(value)
                })
            }) else {
                continue;
            };
            tracks.push(SubtitleTrack {
                language: language.clone(),
                automatic,
                url: url.to_owned(),
            });
        }
    }
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

pub fn youtube_subtitles(
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
    Ok(parse_subtitles(&json))
}

pub fn download_vtt(url: &str) -> Result<String, String> {
    let parsed = url::Url::parse(url).map_err(|_| "URL de legenda inválida.")?;
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
    let response = reqwest::blocking::Client::builder()
        .timeout(QUERY_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .and_then(|client| client.get(parsed).send())
        .map_err(|_| "Não foi possível baixar a legenda.")?;
    if !response.status().is_success() {
        return Err("A legenda expirou ou não está disponível.".into());
    }
    let mut bytes = Vec::new();
    response
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Resposta de legenda inválida.")?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Legenda acima do limite de 2 MiB.".into());
    }
    String::from_utf8(bytes).map_err(|_| "A legenda não está em UTF-8.".into())
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
            if let Some(entry) = parse_video_entry(&value, true, None, None, None) {
                if crate::history::youtube_video_id(&entry.video.url).is_some() {
                    feed.items.push(entry);
                }
            }
        }
    }
    Ok(feed)
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

impl ChannelTarget {
    pub fn canonical_url(&self) -> String {
        match self {
            Self::ChannelId(id) => format!("https://www.youtube.com/channel/{id}"),
            Self::Handle(handle) => format!("https://www.youtube.com/{handle}"),
        }
    }

    pub fn videos_tab_url(&self) -> String {
        format!("{}/videos", self.canonical_url())
    }

    pub fn playlists_tab_url(&self) -> String {
        format!("{}/playlists", self.canonical_url())
    }

    pub fn about_tab_url(&self) -> String {
        format!("{}/about", self.canonical_url())
    }
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelPage {
    pub target: ChannelTarget,
    pub channel_id: Option<String>,
    pub title: Option<String>,
    pub handle: Option<String>,
    pub canonical_url: String,
    pub description: Option<String>,
    pub subscriber_count: Option<u64>,
    pub view_count: Option<u64>,
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

pub fn youtube_channel_url(input: &str) -> Option<String> {
    parse_channel_target(input).map(|target| target.canonical_url())
}

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

fn yt_dlp() -> Command {
    let program = std::env::current_exe()
        .ok()
        .and_then(|executable| bundled_yt_dlp(&executable))
        .unwrap_or_else(|| "yt-dlp".into());
    let mut command = Command::new(program);
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

pub fn resolutions(url: &str) -> Result<Vec<u32>, String> {
    resolutions_with_cancel(url, &Cancellation::default())
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

pub fn find(query: &str) -> Result<Vec<Video>, String> {
    find_with_metadata(query)
        .map(|results| results.into_iter().map(|result| result.video).collect())
}

pub fn find_with_metadata(query: &str) -> Result<Vec<SearchResult>, String> {
    find_with_cancel(query, &Cancellation::default())
}

pub fn find_with_cancel(
    query: &str,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let target = if query.starts_with("https://") || query.starts_with("http://") {
        query.to_owned()
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
    parse_results(&output.stdout, target.starts_with("ytsearch20:"))
}

pub fn fetch_channel_page(input: &str) -> Result<ChannelPage, String> {
    fetch_channel_page_with_limit(input, DEFAULT_CHANNEL_VIDEOS_LIMIT)
}

pub fn fetch_channel_page_with_limit(input: &str, limit: usize) -> Result<ChannelPage, String> {
    let (target, effective_limit) = parse_and_clamp_channel_request(input, limit)?;
    fetch_channel_page_for_target(&target, effective_limit)
}

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
                .filter(|item| {
                    item.get("url")
                        .and_then(Value::as_str)
                        .and_then(valid_http_url)
                        .is_some()
                })
                .min_by_key(|item| {
                    item.get("width")
                        .and_then(Value::as_u64)
                        .unwrap_or(0)
                        .abs_diff(320)
                })
        })
        .and_then(|item| item.get("url"))
        .and_then(Value::as_str)
        .and_then(valid_http_url)
        .or_else(|| nonempty_str(value, "thumbnail").and_then(valid_http_url))
        .unwrap_or("")
        .to_owned()
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

    Some(SearchResult {
        video: Video {
            title: text("title"),
            channel,
            url,
            thumbnail,
            duration,
        },
        metadata: VideoMetadata {
            view_count: value.get("view_count").and_then(Value::as_u64),
            upload_date: nonempty("upload_date").map(str::to_owned),
            description: nonempty("description").map(str::to_owned),
            channel_id,
            channel_url,
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
    if let Some(id) = nonempty_str(value, "id").filter(|id| is_valid_playlist_id(id)) {
        if !crate::history::is_valid_video_id(id)
            || value
                .get("_type")
                .and_then(Value::as_str)
                .is_some_and(|t| t == "playlist" || t == "url")
            || value
                .get("ie_key")
                .and_then(Value::as_str)
                .is_some_and(|k| k.eq_ignore_ascii_case("YoutubeTab"))
        {
            return Some(id.to_owned());
        }
    }
    for key in ["url", "webpage_url"] {
        if let Some(raw_url) = nonempty_str(value, key) {
            if let Some(id) = raw_url.strip_prefix("https://www.youtube.com/playlist?list=") {
                let id = id.split(['&', '#', '/']).next().unwrap_or("");
                if is_valid_playlist_id(id) {
                    return Some(id.to_owned());
                }
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

pub fn fetch_channel_playlists_with_cancel(
    input: &str,
    limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<ChannelPlaylist>, String> {
    let target = parse_channel_target(input).ok_or_else(|| {
        "Identificador ou URL de canal do YouTube inválido ou não suportado.".to_owned()
    })?;
    let effective_limit = limit.clamp(1, MAX_CHANNEL_PLAYLISTS_LIMIT);
    let limit_arg = effective_limit.to_string();
    let playlists_url = target.playlists_tab_url();
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
                if let Some(playlist) = parse_channel_playlist_entry(entry) {
                    if seen_ids.insert(playlist.id.clone()) {
                        playlists.push(playlist);
                    }
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
        if let Some(playlist) = parse_channel_playlist_entry(&value) {
            if seen_ids.insert(playlist.id.clone()) {
                playlists.push(playlist);
            }
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
    if let Ok(single_value) = serde_json::from_slice::<Value>(output) {
        if let Some(entries) = single_value.get("entries").and_then(Value::as_array) {
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
                {
                    if crate::history::youtube_video_id(&video.video.url).is_some() {
                        results.push(video);
                    }
                }
            }
            return Ok(results);
        }
    }
    let mut videos = parse_results(output, true)?;
    videos.retain(|item| crate::history::youtube_video_id(&item.video.url).is_some());
    videos.truncate(effective_limit);
    Ok(videos)
}

pub fn fetch_channel_about_with_cancel(
    input: &str,
    cancellation: &Cancellation,
) -> Result<ChannelAboutInfo, String> {
    let target = parse_channel_target(input).ok_or_else(|| {
        "Identificador ou URL de canal do YouTube inválido ou não suportado.".to_owned()
    })?;
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
    })
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
        }
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
}
