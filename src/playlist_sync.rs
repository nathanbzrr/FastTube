use crate::comment_actions::{
    FALLBACK_CLIENT_VERSION, INNERTUBE_BASE_URL, InnerTubeError, WEB_USER_AGENT, YOUTUBE_ORIGIN,
    YouTubeWebSession, http_client, parse_innertube_error_json,
};
use crate::history::is_valid_video_id;
use crate::oauth;
use crate::process::Cancellation;
use crate::search::{
    self, SearchResult, SearchResultKind, Video, VideoMetadata, channel_url_from_browse_endpoint,
    clean_published_time_text, has_verified_owner_badge, parse_video_info_views_and_date,
    select_yt_thumbnail, yt_text,
};
use eframe::egui;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::{SystemTime, UNIX_EPOCH};

pub const PLAYLISTS_FEED_BROWSE_ID: &str = "FEplaylist_aggregation";
const MAX_PLAYLISTS_FEED_PAGES: usize = 6;
const MAX_PLAYLIST_FEED_CARDS: usize = 250;
const MAX_PLAYLIST_DETAIL_PAGES: usize = 50;
const MAX_MIX_DETAIL_PAGES: usize = 6;
const MAX_PLAYLIST_DETAIL_VIDEOS: usize = 5_000;
pub const MIX_SEED_CONTINUATION_PREFIX: &str = "mix_seed:";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlaylistSort {
    #[default]
    RecentlyAdded,
    AlphabeticalAZ,
}

impl PlaylistSort {
    pub fn label(self, pt: bool) -> &'static str {
        match self {
            Self::RecentlyAdded => {
                if pt {
                    "Adicionadas recentemente"
                } else {
                    "Recently added"
                }
            }
            Self::AlphabeticalAZ => "A-Z",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlaylistCategoryFilter {
    #[default]
    All,
    Playlists,
    Music,
    Mixes,
    Owned,
    Saved,
}

impl PlaylistCategoryFilter {
    pub const CHIPS: [Self; 5] = [
        Self::Playlists,
        Self::Music,
        Self::Mixes,
        Self::Owned,
        Self::Saved,
    ];

    pub fn label(self, pt: bool) -> &'static str {
        match self {
            Self::All => {
                if pt {
                    "Tudo"
                } else {
                    "All"
                }
            }
            Self::Playlists => "Playlists",
            Self::Music => {
                if pt {
                    "Música"
                } else {
                    "Music"
                }
            }
            Self::Mixes => "Mixes",
            Self::Owned => {
                if pt {
                    "Minhas"
                } else {
                    "Owned"
                }
            }
            Self::Saved => {
                if pt {
                    "Salvas"
                } else {
                    "Saved"
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlaylistPrivacy {
    Public,
    Unlisted,
    Private,
    #[default]
    Unknown,
}

impl PlaylistPrivacy {
    pub fn label(self, pt: bool) -> &'static str {
        match self {
            Self::Public => {
                if pt {
                    "Público"
                } else {
                    "Public"
                }
            }
            Self::Unlisted => {
                if pt {
                    "Não listado"
                } else {
                    "Unlisted"
                }
            }
            Self::Private => {
                if pt {
                    "Privado"
                } else {
                    "Private"
                }
            }
            Self::Unknown => "",
        }
    }

    #[allow(dead_code)]
    pub fn as_innertube_str(self) -> Option<&'static str> {
        match self {
            Self::Public => Some("PUBLIC"),
            Self::Unlisted => Some("UNLISTED"),
            Self::Private => Some("PRIVATE"),
            Self::Unknown => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlaylistKind {
    #[default]
    Playlist,
    Mix,
    Music,
    WatchLater,
    LikedVideos,
    Podcast,
    Local,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaylistChipAction {
    BrowseParams(String),
    Continuation(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaylistSortOption {
    pub sort: PlaylistSort,
    pub label: String,
    pub action: PlaylistChipAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncedPlaylistCard {
    pub id: String,
    pub title: String,
    pub thumbnail_url: String,
    pub video_count: Option<u64>,
    pub badge_text: String,
    pub owner_label: String,
    pub subtitle_line: String,
    pub updated_text: Option<String>,
    pub privacy: PlaylistPrivacy,
    pub kind: PlaylistKind,
    pub is_mix: bool,
    pub is_music: bool,
    pub is_owned: bool,
    pub is_saved: bool,
    pub seed_video_id: Option<String>,
    pub browse_id: Option<String>,
    pub has_full_playlist_link: bool,
    pub stack_color_dark: Option<u32>,
    pub stack_color_light: Option<u32>,
}

impl SyncedPlaylistCard {
    pub fn matches_filter(&self, filter: PlaylistCategoryFilter) -> bool {
        match filter {
            PlaylistCategoryFilter::All => true,
            PlaylistCategoryFilter::Playlists => !self.is_mix && self.kind != PlaylistKind::Mix,
            PlaylistCategoryFilter::Music => self.is_music || self.kind == PlaylistKind::Music,
            PlaylistCategoryFilter::Mixes => self.is_mix || self.kind == PlaylistKind::Mix,
            PlaylistCategoryFilter::Owned => self.is_owned,
            PlaylistCategoryFilter::Saved => self.is_saved,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemotePlaylistsFeedPage {
    pub cards: Vec<SyncedPlaylistCard>,
    pub chips: HashMap<PlaylistCategoryFilter, PlaylistChipAction>,
    pub sort_options: Vec<PlaylistSortOption>,
    pub selected_sort: Option<PlaylistSort>,
    pub next_continuation: Option<String>,
    pub signed_out: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemotePlaylistDetailMetadata {
    pub playlist_id: String,
    pub title: String,
    pub owner: String,
    pub privacy: PlaylistPrivacy,
    pub total_videos: Option<u64>,
    pub thumbnail_url: Option<String>,
    pub is_mix: bool,
    pub is_editable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemotePlaylistDetailItem {
    pub result: SearchResult,
    pub set_video_id: Option<String>,
    pub index_text: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemotePlaylistDetailPage {
    pub meta: Option<RemotePlaylistDetailMetadata>,
    pub items: Vec<RemotePlaylistDetailItem>,
    pub next_continuation: Option<String>,
    pub signed_out: bool,
}

pub enum PlaylistsSyncMessage {
    SessionUpdated(YouTubeWebSession),
    Page {
        request_id: u64,
        filter: PlaylistCategoryFilter,
        sort: PlaylistSort,
        page_index: usize,
        page: RemotePlaylistsFeedPage,
    },
    Finished {
        request_id: u64,
        filter: PlaylistCategoryFilter,
        sort: PlaylistSort,
        auth_expired: bool,
        result: Result<usize, String>,
    },
}

pub enum PlaylistDetailSyncMessage {
    SessionUpdated(YouTubeWebSession),
    Page {
        request_id: u64,
        playlist_id: String,
        page_index: usize,
        page: RemotePlaylistDetailPage,
    },
    Finished {
        request_id: u64,
        playlist_id: String,
        auth_expired: bool,
        result: Result<usize, String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaylistsFetchMode {
    Initial,
    Filter(PlaylistCategoryFilter, PlaylistSort, PlaylistChipAction),
    Sort(PlaylistCategoryFilter, PlaylistSort, PlaylistChipAction),
}

pub struct RemotePlaylistActionEvent {
    pub web_session: Option<YouTubeWebSession>,
    pub auth_expired: bool,
    pub playlist_id: String,
    pub video_id: String,
    pub result: Result<(), String>,
}

pub fn is_valid_playlist_id(id: &str) -> bool {
    (2..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn parse_cookie_header_map(cookie_header: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for segment in cookie_header.split(';') {
        let trimmed = segment.trim();
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if !name.is_empty() && !value.is_empty() {
            map.insert(name.to_owned(), value.to_owned());
        }
    }
    map
}

pub(crate) fn to_innertube_web_session(
    session: &YouTubeWebSession,
    now_unix: u64,
) -> Result<crate::innertube::YouTubeWebSession, crate::innertube::InnerTubeError> {
    let cookies = parse_cookie_header_map(&session.cookie_header());
    let expires_at = now_unix.saturating_add(30 * 24 * 60 * 60);
    let mut it = crate::innertube::YouTubeWebSession::new(
        cookies,
        session.visitor_data.clone(),
        now_unix,
        Some(expires_at),
        session.auth_user_index,
    )?;
    if !session.client_version.trim().is_empty() {
        it.client_version = session.client_version.clone();
    }
    it.delegated_session_id = session.delegated_session_id.clone();
    it.api_key = session.api_key.clone();
    Ok(it)
}

pub(crate) fn from_innertube_web_session(
    it_session: &crate::innertube::YouTubeWebSession,
) -> Result<YouTubeWebSession, String> {
    let raw: Vec<Value> = it_session
        .cookies
        .iter()
        .map(|(k, v)| json!({ "name": k, "value": v, "domain": ".youtube.com" }))
        .collect();
    let mut session = YouTubeWebSession::from_devtools_cookies(&raw)?;
    if !it_session.client_version.trim().is_empty() {
        session.client_version = it_session.client_version.clone();
    }
    if it_session.visitor_data.is_some() {
        session.visitor_data = it_session.visitor_data.clone();
    }
    session.delegated_session_id = it_session.delegated_session_id.clone();
    session.api_key = it_session.api_key.clone();
    session.auth_user_index = it_session.auth_user_index;
    session.client_config_refreshed = it_session.visitor_data.is_some()
        || (it_session.client_version != FALLBACK_CLIENT_VERSION
            && !it_session.client_version.trim().is_empty());
    Ok(session)
}

fn persist_web_session_dpapi(session: &YouTubeWebSession) {
    let now = unix_now_secs();
    if let Ok(it_session) = to_innertube_web_session(session, now) {
        let _ = crate::innertube::save_web_session(&it_session);
    }
}

pub fn build_innertube_authorization_header(
    session: &YouTubeWebSession,
    origin: &str,
    timestamp_secs: u64,
) -> Result<String, InnerTubeError> {
    let cookies = parse_cookie_header_map(&session.cookie_header());
    let sapisid = cookies
        .get("SAPISID")
        .or_else(|| cookies.get("__Secure-3PAPISID"))
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            InnerTubeError::AuthExpired("Cookie de autenticação do YouTube ausente.".into())
        })?;
    let mut parts = vec![crate::innertube::sapisidhash_header(
        sapisid,
        origin,
        timestamp_secs,
    )];
    if let Some(p1) = cookies.get("__Secure-1PAPISID").filter(|v| !v.is_empty()) {
        parts.push(format!(
            "SAPISID1PHASH {}",
            crate::innertube::sapisidhash_value(p1, origin, timestamp_secs)
        ));
    }
    if let Some(p3) = cookies.get("__Secure-3PAPISID").filter(|v| !v.is_empty()) {
        parts.push(format!(
            "SAPISID3PHASH {}",
            crate::innertube::sapisidhash_value(p3, origin, timestamp_secs)
        ));
    }
    Ok(parts.join(" "))
}

fn should_retry_interactive(error: &str) -> bool {
    error.contains("Entre na sua conta") || error.contains("sessão expirou")
}

/// Acquires a `YouTubeWebSession`, checking the DPAPI-persisted `web-session.dpapi`
/// before spawning headless Chromium, and persisting newly extracted sessions to disk.
pub fn acquire_web_session_with_dpapi_cache_at<F>(
    cached: Option<YouTubeWebSession>,
    session_path: Option<&Path>,
    now_unix: u64,
    extract_cookies: F,
) -> Result<YouTubeWebSession, String>
where
    F: FnOnce() -> Result<Vec<Value>, String>,
{
    if let Some(session) = cached {
        return Ok(session);
    }
    let persisted_res = match session_path {
        Some(path) => crate::innertube::load_web_session_from(path, now_unix),
        None => crate::innertube::load_web_session(),
    };
    if let Ok(Some(persisted)) = persisted_res
        && let Ok(session) = from_innertube_web_session(&persisted)
    {
        return Ok(session);
    }

    let cookies = extract_cookies()?;
    let it_session = crate::innertube::YouTubeWebSession::from_devtools_cookies(&cookies, now_unix)
        .map_err(|err| err.into_message())?;
    match session_path {
        Some(path) => {
            let _ = crate::innertube::save_web_session_to(path, &it_session);
        }
        None => {
            let _ = crate::innertube::save_web_session(&it_session);
        }
    }
    from_innertube_web_session(&it_session)
}

pub fn acquire_web_session(
    cached: Option<YouTubeWebSession>,
    allow_interactive: bool,
    cancel: &Cancellation,
) -> Result<YouTubeWebSession, String> {
    if let Some(session) = cached {
        return Ok(session);
    }
    if let Ok(Some(persisted)) = crate::innertube::load_web_session()
        && let Ok(session) = from_innertube_web_session(&persisted)
    {
        return Ok(session);
    }
    let _guard = crate::session::web_session_acquire_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cancel.cancelled() {
        return Err("Operação cancelada.".into());
    }
    acquire_web_session_with_dpapi_cache_at(None, None, unix_now_secs(), || {
        match crate::session::youtube_cookies(false, cancel) {
            Ok(cookies) => Ok(cookies),
            Err(err) if allow_interactive && should_retry_interactive(&err) => {
                crate::session::youtube_cookies(true, cancel)
            }
            Err(err) => Err(err),
        }
    })
}

pub fn post_innertube_with_locale_at(
    base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    endpoint: &str,
    mut body: Value,
    hl: &str,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    if cancel.cancelled() {
        return Err(InnerTubeError::Cancelled("Operação cancelada.".into()));
    }
    let ts = unix_now_secs();
    let auth = build_innertube_authorization_header(session, YOUTUBE_ORIGIN, ts)?;
    let mut client_ctx = json!({
        "clientName": "WEB",
        "clientVersion": session.client_version,
        "hl": hl,
        "gl": "BR",
    });
    if let Some(visitor) = &session.visitor_data
        && let Some(obj) = client_ctx.as_object_mut()
    {
        obj.insert("visitorData".into(), Value::String(visitor.clone()));
    }
    if let Some(obj) = body.as_object_mut() {
        let mut ctx_val = json!({ "client": client_ctx });
        if let Some(page_id) = session
            .delegated_session_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            ctx_val["user"] = json!({
                "lockedSafetyMode": false,
                "onBehalfOfUser": page_id,
            });
        }
        obj.insert("context".into(), ctx_val);
    }
    let url = format!("{base_url}/{endpoint}?prettyPrint=false");
    let mut request = client
        .post(&url)
        .header(reqwest::header::USER_AGENT, WEB_USER_AGENT)
        .header(reqwest::header::COOKIE, session.cookie_header())
        .header(reqwest::header::AUTHORIZATION, auth)
        .header("Origin", YOUTUBE_ORIGIN)
        .header("X-Origin", YOUTUBE_ORIGIN)
        .header(reqwest::header::REFERER, format!("{YOUTUBE_ORIGIN}/"))
        .header("X-Goog-AuthUser", session.auth_user_index.to_string())
        .header("X-Youtube-Bootstrap-Logged-In", "true")
        .header("X-Youtube-Client-Name", "1")
        .header("X-Youtube-Client-Version", &session.client_version)
        .json(&body);
    if let Some(visitor) = &session.visitor_data {
        request = request.header("X-Goog-Visitor-Id", visitor);
    }
    if let Some(page_id) = session
        .delegated_session_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        request = request.header("X-Goog-PageId", page_id);
    }

    let response = request
        .send()
        .map_err(|_| InnerTubeError::Other("Falha de conexão com o YouTube.".into()))?;
    if cancel.cancelled() {
        return Err(InnerTubeError::Cancelled("Operação cancelada.".into()));
    }
    session.absorb_set_cookies(response.headers());
    let status = response.status();
    let text = response.text().unwrap_or_default();
    let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);

    if matches!(status.as_u16(), 401 | 403) {
        let _ = crate::innertube::invalidate_web_session();
        return Err(InnerTubeError::AuthExpired(
            "Sua sessão do YouTube expirou. Entre novamente.".into(),
        ));
    }
    if !status.is_success() {
        if let Some(err) = parse_innertube_error_json(&json, status.as_u16()) {
            if matches!(err, InnerTubeError::AuthExpired(_)) {
                let _ = crate::innertube::invalidate_web_session();
            }
            return Err(err);
        }
        return Err(InnerTubeError::Other(format!(
            "O YouTube recusou a ação (HTTP {}).",
            status.as_u16()
        )));
    }
    if json.is_null() && !text.trim().is_empty() {
        return Err(InnerTubeError::Other(
            "Resposta inválida do YouTube.".into(),
        ));
    }
    if let Some(err) = parse_innertube_error_json(&json, status.as_u16()) {
        if matches!(err, InnerTubeError::AuthExpired(_)) {
            let _ = crate::innertube::invalidate_web_session();
        }
        return Err(err);
    }
    Ok(json)
}

fn post_innertube_unauth_with_locale_at(
    base_url: &str,
    client: &reqwest::blocking::Client,
    endpoint: &str,
    mut body: Value,
    hl: &str,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    if cancel.cancelled() {
        return Err(InnerTubeError::Cancelled("Operação cancelada.".into()));
    }
    let client_ctx = json!({
        "clientName": "WEB",
        "clientVersion": FALLBACK_CLIENT_VERSION,
        "hl": hl,
        "gl": "BR",
    });
    if let Some(obj) = body.as_object_mut() {
        obj.insert("context".into(), json!({ "client": client_ctx }));
    }
    let url = format!("{base_url}/{endpoint}?prettyPrint=false");
    let response = client
        .post(&url)
        .header(reqwest::header::USER_AGENT, WEB_USER_AGENT)
        .header("Origin", YOUTUBE_ORIGIN)
        .header("X-Origin", YOUTUBE_ORIGIN)
        .header(reqwest::header::REFERER, format!("{YOUTUBE_ORIGIN}/"))
        .header("X-Youtube-Client-Name", "1")
        .header("X-Youtube-Client-Version", FALLBACK_CLIENT_VERSION)
        .json(&body)
        .send()
        .map_err(|_| InnerTubeError::Other("Falha de conexão com o YouTube.".into()))?;
    if cancel.cancelled() {
        return Err(InnerTubeError::Cancelled("Operação cancelada.".into()));
    }
    let status = response.status();
    let text = response.text().unwrap_or_default();
    let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        if let Some(err) = parse_innertube_error_json(&json, status.as_u16()) {
            return Err(err);
        }
        return Err(InnerTubeError::Other(format!(
            "O YouTube recusou a ação (HTTP {}).",
            status.as_u16()
        )));
    }
    if json.is_null() && !text.trim().is_empty() {
        return Err(InnerTubeError::Other(
            "Resposta inválida do YouTube.".into(),
        ));
    }
    if let Some(err) = parse_innertube_error_json(&json, status.as_u16()) {
        return Err(err);
    }
    Ok(json)
}

fn extract_first_grouped_number(raw: &str) -> Option<u64> {
    let mut digits = String::new();
    let mut started = false;
    for ch in raw.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            started = true;
        } else if started && matches!(ch, '.' | ',' | ' ' | '\u{00a0}') {
            continue;
        } else if started {
            break;
        }
    }
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u64>().ok()
    }
}

fn parse_privacy_label(raw: &str) -> Option<PlaylistPrivacy> {
    match raw.trim().to_lowercase().as_str() {
        "particular" | "privada" | "privado" | "private" => Some(PlaylistPrivacy::Private),
        "não listada" | "nao listada" | "não listado" | "nao listado" | "unlisted" => {
            Some(PlaylistPrivacy::Unlisted)
        }
        "pública" | "publica" | "público" | "publico" | "public" => Some(PlaylistPrivacy::Public),
        _ => None,
    }
}

fn is_updated_text(raw: &str) -> bool {
    let lower = raw.trim().to_lowercase();
    lower.starts_with("atualiz") || lower.starts_with("updated")
}

fn is_full_playlist_link_text(raw: &str) -> bool {
    let lower = raw.trim().to_lowercase();
    lower.contains("ver playlist completa") || lower.contains("view full playlist")
}

fn map_playlist_chip_label(label: &str) -> Option<PlaylistCategoryFilter> {
    match label.trim().to_lowercase().as_str() {
        "tudo" | "all" => Some(PlaylistCategoryFilter::All),
        "playlists" | "playlist" => Some(PlaylistCategoryFilter::Playlists),
        "música" | "musica" | "music" => Some(PlaylistCategoryFilter::Music),
        "mixes" | "mix" => Some(PlaylistCategoryFilter::Mixes),
        "minhas" | "owned" | "criadas por você" | "criadas por voce" => {
            Some(PlaylistCategoryFilter::Owned)
        }
        "salvas" | "saved" => Some(PlaylistCategoryFilter::Saved),
        _ => None,
    }
}

fn map_playlist_sort_label(label: &str) -> Option<PlaylistSort> {
    let lower = label.trim().to_lowercase();
    if lower.contains("a-z")
        || lower.contains("a\u{2013}z")
        || lower.contains("alphabet")
        || lower.contains("alfab")
    {
        Some(PlaylistSort::AlphabeticalAZ)
    } else if lower.contains("recent") || lower.contains("adicionad") {
        Some(PlaylistSort::RecentlyAdded)
    } else {
        None
    }
}

fn extract_chip_action_from_command(node: &Value, depth: u8) -> Option<PlaylistChipAction> {
    if depth >= 6 {
        return None;
    }
    if let Some(token) = node
        .pointer("/continuationCommand/token")
        .or_else(|| node.pointer("/reloadContinuationData/continuation"))
        .or_else(|| node.pointer("/serviceEndpoint/continuationCommand/token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(PlaylistChipAction::Continuation(token.to_owned()));
    }
    if let Some(browse) = node
        .get("browseEndpoint")
        .or_else(|| node.pointer("/navigationEndpoint/browseEndpoint"))
        .or_else(|| node.pointer("/serviceEndpoint/browseEndpoint"))
    {
        if let Some(params) = browse
            .get("params")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(PlaylistChipAction::BrowseParams(params.to_owned()));
        }
        if let Some(bid) = browse
            .get("browseId")
            .and_then(Value::as_str)
            .map(str::trim)
            && (bid == PLAYLISTS_FEED_BROWSE_ID || bid == "FEplaylists")
        {
            return Some(PlaylistChipAction::BrowseParams(String::new()));
        }
    }
    for key in [
        "innertubeCommand",
        "onTap",
        "tapCommand",
        "serviceEndpoint",
        "navigationEndpoint",
        "onSelectCommand",
        "endpoint",
        "command",
        "continuation",
    ] {
        if let Some(sub) = node.get(key)
            && let Some(action) = extract_chip_action_from_command(sub, depth + 1)
        {
            return Some(action);
        }
    }
    for arr_ptr in [
        "/commandExecutorCommand/commands",
        "/serialCommand/commands",
        "/parallelCommand/commands",
    ] {
        if let Some(cmds) = node.pointer(arr_ptr).and_then(Value::as_array) {
            for cmd in cmds {
                if let Some(action) = extract_chip_action_from_command(cmd, depth + 1) {
                    return Some(action);
                }
            }
        }
    }
    None
}

fn extract_chip_action_from_endpoint(endpoint: Option<&Value>) -> Option<PlaylistChipAction> {
    extract_chip_action_from_command(endpoint?, 0)
}

fn value_suggests_selected_item(node: &Value) -> bool {
    if node.get("isSelected").and_then(Value::as_bool) == Some(true)
        || node.get("selected").and_then(Value::as_bool) == Some(true)
    {
        return true;
    }
    for ptr in [
        "/trailingImage/sources/0/clientResource/imageName",
        "/leadingImage/sources/0/clientResource/imageName",
        "/trailingAccessory/iconViewModel/image/sources/0/clientResource/imageName",
    ] {
        if let Some(name) = node.pointer(ptr).and_then(Value::as_str)
            && name.to_ascii_uppercase().contains("CHECK")
        {
            return true;
        }
    }
    false
}

fn collect_playlist_chips_and_sorts(
    root: &Value,
    chips: &mut HashMap<PlaylistCategoryFilter, PlaylistChipAction>,
    sort_options: &mut Vec<PlaylistSortOption>,
    selected_sort: &mut Option<PlaylistSort>,
) {
    match root {
        Value::Object(map) => {
            if let Some(chip) = map.get("chipCloudChipRenderer") {
                let label = yt_text(chip.get("text"));
                if let Some(filter) = map_playlist_chip_label(&label)
                    && let Some(action) = extract_chip_action_from_endpoint(
                        chip.get("navigationEndpoint")
                            .or_else(|| chip.get("endpoint"))
                            .or_else(|| chip.get("serviceEndpoint")),
                    )
                {
                    chips.entry(filter).or_insert(action);
                }
            }
            if let Some(chip) = map.get("chipViewModel") {
                let label = chip
                    .pointer("/text/content")
                    .and_then(Value::as_str)
                    .or_else(|| chip.get("text").and_then(Value::as_str))
                    .or_else(|| {
                        chip.pointer("/chipButtonViewModel/title")
                            .and_then(Value::as_str)
                    })
                    .or_else(|| {
                        chip.pointer("/chipButtonViewModel/text/content")
                            .and_then(Value::as_str)
                    })
                    .or_else(|| chip.get("content").and_then(Value::as_str))
                    .unwrap_or("");
                if let Some(filter) = map_playlist_chip_label(label) {
                    let ep = chip
                        .pointer("/onTap/innertubeCommand")
                        .or_else(|| chip.pointer("/tapCommand/innertubeCommand"))
                        .or_else(|| chip.get("onTap"))
                        .or_else(|| chip.get("tapCommand"))
                        .or_else(|| chip.pointer("/chipButtonViewModel/onTap/innertubeCommand"))
                        .or_else(|| chip.pointer("/chipButtonViewModel/onTap"))
                        .or_else(|| chip.get("navigationEndpoint"));
                    if let Some(action) = extract_chip_action_from_endpoint(ep) {
                        chips.entry(filter).or_insert(action);
                    }
                }
            }
            if let Some(sub_menu) = map.get("sortFilterSubMenuRenderer")
                && let Some(items) = sub_menu.get("subMenuItems").and_then(Value::as_array)
            {
                for item in items {
                    let title = item
                        .get("title")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .map(str::to_owned)
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| yt_text(item.get("title")));
                    let Some(sort) = map_playlist_sort_label(&title) else {
                        continue;
                    };
                    if item.get("selected").and_then(Value::as_bool) == Some(true)
                        && selected_sort.is_none()
                    {
                        *selected_sort = Some(sort);
                    }
                    let action = extract_chip_action_from_endpoint(
                        item.get("serviceEndpoint")
                            .or_else(|| item.get("continuation"))
                            .or_else(|| item.get("navigationEndpoint"))
                            .or(Some(item)),
                    );
                    if let Some(action) = action
                        && !sort_options.iter().any(|opt| opt.sort == sort)
                    {
                        sort_options.push(PlaylistSortOption {
                            sort,
                            label: title,
                            action,
                        });
                    }
                }
            }
            if let Some(dropdown) = map.get("dropdownRenderer")
                && let Some(entries) = dropdown.get("entries").and_then(Value::as_array)
            {
                for entry in entries {
                    let Some(di) = entry.get("dropdownItemRenderer") else {
                        continue;
                    };
                    let title = di
                        .get("label")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .map(str::to_owned)
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| yt_text(di.get("label")));
                    let Some(sort) = map_playlist_sort_label(&title) else {
                        continue;
                    };
                    if value_suggests_selected_item(di) && selected_sort.is_none() {
                        *selected_sort = Some(sort);
                    }
                    let action = extract_chip_action_from_endpoint(
                        di.get("onSelectCommand")
                            .or_else(|| di.get("serviceEndpoint"))
                            .or_else(|| di.get("continuation"))
                            .or(Some(di)),
                    );
                    if let Some(action) = action
                        && !sort_options.iter().any(|opt| opt.sort == sort)
                    {
                        sort_options.push(PlaylistSortOption {
                            sort,
                            label: title,
                            action,
                        });
                    }
                }
            }
            if let Some(item_vm) = map.get("listItemViewModel") {
                let title = item_vm
                    .pointer("/title/content")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .map(str::to_owned)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| yt_text(item_vm.get("title")));
                if let Some(sort) = map_playlist_sort_label(&title) {
                    if value_suggests_selected_item(item_vm) && selected_sort.is_none() {
                        *selected_sort = Some(sort);
                    }
                    let action = extract_chip_action_from_endpoint(
                        item_vm
                            .pointer("/rendererContext/commandContext/onTap")
                            .or_else(|| item_vm.get("onTap"))
                            .or(Some(item_vm)),
                    );
                    if let Some(action) = action
                        && !sort_options.iter().any(|opt| opt.sort == sort)
                    {
                        sort_options.push(PlaylistSortOption {
                            sort,
                            label: title,
                            action,
                        });
                    }
                }
            }
            for child in map.values() {
                collect_playlist_chips_and_sorts(child, chips, sort_options, selected_sort);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_playlist_chips_and_sorts(item, chips, sort_options, selected_sort);
            }
        }
        _ => {}
    }
}

fn parse_color_argb_u32(val: Option<&Value>) -> Option<u32> {
    let v = val?;
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .or_else(|| v.as_i64().map(|n| n as u32))
}

fn parse_playlist_lockup_view_model(l: &Value) -> Option<SyncedPlaylistCard> {
    let content_type = l.get("contentType").and_then(Value::as_str).unwrap_or("");
    let tap_cmd = l
        .pointer("/rendererContext/commandContext/onTap/innertubeCommand")
        .or_else(|| l.pointer("/onTap/innertubeCommand"));

    let watch_ep = tap_cmd.and_then(|c| c.get("watchEndpoint"));
    let browse_ep = tap_cmd.and_then(|c| c.get("browseEndpoint"));

    let mut browse_id = browse_ep
        .and_then(|b| b.get("browseId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let seed_video_id = watch_ep
        .and_then(|w| w.get("videoId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_video_id(id))
        .map(str::to_owned);

    let raw_id = l
        .get("contentId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            watch_ep
                .and_then(|w| w.get("playlistId"))
                .and_then(Value::as_str)
                .map(str::trim)
        })
        .or_else(|| {
            browse_id
                .as_deref()
                .map(|b| b.strip_prefix("VL").unwrap_or(b))
        })?;

    let id = raw_id.strip_prefix("VL").unwrap_or(raw_id).to_owned();
    if !is_valid_playlist_id(&id) {
        return None;
    }

    let upper_type = content_type.to_ascii_uppercase();
    if !upper_type.is_empty()
        && !upper_type.contains("PLAYLIST")
        && !upper_type.contains("MIX")
        && !upper_type.contains("PODCAST")
        && watch_ep.and_then(|w| w.get("playlistId")).is_none()
        && !browse_id.as_deref().is_some_and(|b| b.starts_with("VL"))
    {
        return None;
    }

    let title = l
        .pointer("/metadata/lockupMetadataViewModel/title/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| yt_text(l.pointer("/metadata/lockupMetadataViewModel/title")));
    if title.is_empty() {
        return None;
    }

    let thumbnail_sources = l
        .pointer("/contentImage/collectionThumbnailViewModel/primaryThumbnail/thumbnailViewModel/image/sources")
        .or_else(|| l.pointer("/contentImage/thumbnailViewModel/image/sources"))
        .or_else(|| {
            l.pointer(
                "/contentImage/collectionThumbnailViewModel/stackThumbnail/thumbnailViewModel/image/sources",
            )
        });
    let thumbnail_url = select_yt_thumbnail(thumbnail_sources, 480);
    let stack_color_dark = parse_color_argb_u32(
        l.pointer("/contentImage/collectionThumbnailViewModel/stackColor/darkTheme")
            .or_else(|| {
                l.pointer(
                    "/contentImage/collectionThumbnailViewModel/darkThemePalette/baseBackground",
                )
            }),
    );
    let stack_color_light = parse_color_argb_u32(
        l.pointer("/contentImage/collectionThumbnailViewModel/stackColor/lightTheme")
            .or_else(|| {
                l.pointer(
                    "/contentImage/collectionThumbnailViewModel/lightThemePalette/baseBackground",
                )
            }),
    );

    let mut badge_text = String::new();
    let mut video_count: Option<u64> = None;
    let mut icon_suggests_mix = false;
    let mut icon_suggests_music = false;

    for overlay_ptr in [
        "/contentImage/collectionThumbnailViewModel/primaryThumbnail/thumbnailViewModel/overlays",
        "/contentImage/thumbnailViewModel/overlays",
    ] {
        let Some(overlays) = l.pointer(overlay_ptr).and_then(Value::as_array) else {
            continue;
        };
        for ov in overlays {
            let badges = ov
                .pointer("/thumbnailOverlayBadgeViewModel/thumbnailBadges")
                .or_else(|| ov.pointer("/thumbnailBottomOverlayViewModel/badges"))
                .and_then(Value::as_array);
            let Some(badges) = badges else {
                continue;
            };
            for badge_entry in badges {
                let badge = badge_entry
                    .get("thumbnailBadgeViewModel")
                    .unwrap_or(badge_entry);
                let text = badge
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("");
                let style = badge
                    .get("badgeStyle")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_ascii_uppercase();
                let icon_name = badge
                    .pointer("/icon/sources/0/clientResource/imageName")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_ascii_uppercase();
                if icon_name.contains("MIX") || icon_name.contains("RADIO") || style.contains("MIX")
                {
                    icon_suggests_mix = true;
                }
                if icon_name.contains("MUSIC") || icon_name.contains("AUDIO") {
                    icon_suggests_music = true;
                }
                if !text.is_empty() && badge_text.is_empty() {
                    badge_text = text.to_owned();
                }
                if video_count.is_none() && !text.is_empty() {
                    video_count = extract_first_grouped_number(text);
                }
            }
        }
    }

    let mut owner_label = String::new();
    let mut privacy = PlaylistPrivacy::Unknown;
    let mut updated_text: Option<String> = None;
    let mut has_external_channel = false;
    let mut explicit_full_link = false;

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
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| yt_text(Some(text_obj)));
                if text.is_empty() {
                    continue;
                }
                if is_full_playlist_link_text(&text) {
                    explicit_full_link = true;
                    if let Some(cmd_runs) = text_obj.get("commandRuns").and_then(Value::as_array) {
                        for run in cmd_runs {
                            if let Some(bid) = run
                                .pointer("/onTap/innertubeCommand/browseEndpoint/browseId")
                                .and_then(Value::as_str)
                                .filter(|s| !s.is_empty())
                                && browse_id.is_none()
                            {
                                browse_id = Some(bid.to_owned());
                            }
                        }
                    }
                    continue;
                }
                if let Some(cmd_runs) = text_obj.get("commandRuns").and_then(Value::as_array) {
                    for run in cmd_runs {
                        let cmd = run.pointer("/onTap/innertubeCommand");
                        let (cid, curl) = channel_url_from_browse_endpoint(cmd);
                        if cid.is_some() || curl.is_some() {
                            has_external_channel = true;
                            if owner_label.is_empty() {
                                let clean_owner = text
                                    .split(['\u{00b7}', '\u{2022}'])
                                    .next()
                                    .map(str::trim)
                                    .filter(|s| {
                                        !s.is_empty()
                                            && !s.eq_ignore_ascii_case("playlist")
                                            && !s.eq_ignore_ascii_case("mix")
                                    })
                                    .unwrap_or(&text);
                                owner_label = clean_owner.to_owned();
                            }
                        }
                        if let Some(bid) = cmd
                            .and_then(|c| c.pointer("/browseEndpoint/browseId"))
                            .and_then(Value::as_str)
                            && bid.starts_with("VL")
                            && browse_id.is_none()
                        {
                            browse_id = Some(bid.to_owned());
                        }
                    }
                }
                for seg in text
                    .split(['\u{00b7}', '\u{2022}'])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    if is_full_playlist_link_text(seg) {
                        explicit_full_link = true;
                        continue;
                    }
                    if let Some(p) = parse_privacy_label(seg) {
                        privacy = p;
                        if owner_label.is_empty() && p == PlaylistPrivacy::Private {
                            owner_label = seg.to_owned();
                        }
                        continue;
                    }
                    if is_updated_text(seg) {
                        if updated_text.is_none() {
                            updated_text = Some(seg.to_owned());
                        }
                        continue;
                    }
                    if seg.eq_ignore_ascii_case("playlist") || seg.eq_ignore_ascii_case("mix") {
                        continue;
                    }
                    if owner_label.is_empty() {
                        owner_label = seg.to_owned();
                        if seg.starts_with('@') {
                            has_external_channel = true;
                        }
                    }
                }
            }
        }
    }

    let title_lower = title.to_lowercase();
    let is_mix = upper_type.contains("MIX")
        || id.starts_with("RD")
        || icon_suggests_mix
        || badge_text.eq_ignore_ascii_case("mix")
        || watch_ep.and_then(|w| w.get("startRadio")).is_some()
        || (!id.starts_with("PL")
            && !matches!(id.as_str(), "WL" | "LL")
            && (title_lower.starts_with("mix de ")
                || title_lower.starts_with("mix - ")
                || title_lower.starts_with("my mix")));
    let is_music = is_mix
        || icon_suggests_music
        || id.starts_with("OLAK5uy_")
        || title_lower.contains("music")
        || title_lower.contains("música")
        || title_lower.contains("musica")
        || title_lower.contains("songs");

    let kind = if id == "WL" {
        PlaylistKind::WatchLater
    } else if id == "LL" {
        PlaylistKind::LikedVideos
    } else if is_mix {
        PlaylistKind::Mix
    } else if upper_type.contains("PODCAST") {
        PlaylistKind::Podcast
    } else if is_music {
        PlaylistKind::Music
    } else {
        PlaylistKind::Playlist
    };

    if matches!(kind, PlaylistKind::WatchLater | PlaylistKind::LikedVideos)
        && privacy == PlaylistPrivacy::Unknown
    {
        privacy = PlaylistPrivacy::Private;
    }

    let is_saved = !is_mix
        && !matches!(kind, PlaylistKind::WatchLater | PlaylistKind::LikedVideos)
        && has_external_channel;
    let is_owned = !is_mix && !is_saved;

    let badge_text = if !badge_text.is_empty() {
        badge_text
    } else if is_mix {
        "Mix".to_owned()
    } else if let Some(count) = video_count {
        format!("{count} vídeos")
    } else {
        "Playlist".to_owned()
    };

    let subtitle_line = if !owner_label.is_empty() {
        format!("{owner_label} · Playlist")
    } else if is_mix {
        "Mix · Playlist".to_owned()
    } else {
        "Playlist".to_owned()
    };

    Some(SyncedPlaylistCard {
        id,
        title,
        thumbnail_url,
        video_count,
        badge_text,
        owner_label,
        subtitle_line,
        updated_text,
        privacy,
        kind,
        is_mix,
        is_music,
        is_owned,
        is_saved,
        seed_video_id,
        browse_id,
        has_full_playlist_link: explicit_full_link || !is_mix,
        stack_color_dark,
        stack_color_light,
    })
}

fn parse_legacy_playlist_renderer(p: &Value, is_radio: bool) -> Option<SyncedPlaylistCard> {
    let raw_id = p
        .get("playlistId")
        .and_then(Value::as_str)
        .map(str::trim)
        .or_else(|| {
            p.pointer("/navigationEndpoint/watchEndpoint/playlistId")
                .and_then(Value::as_str)
                .map(str::trim)
        })?;
    let id = raw_id.strip_prefix("VL").unwrap_or(raw_id).to_owned();
    if !is_valid_playlist_id(&id) {
        return None;
    }
    let title = yt_text(p.get("title"));
    if title.is_empty() {
        return None;
    }
    let thumb_sources = p
        .pointer("/thumbnails/0/thumbnails")
        .or_else(|| p.pointer("/thumbnail/thumbnails"));
    let thumbnail_url = select_yt_thumbnail(thumb_sources, 480);
    let count_text = yt_text(
        p.get("videoCountShortText")
            .or_else(|| p.get("videoCountText")),
    );
    let video_count = extract_first_grouped_number(&count_text);
    let owner_obj = p.get("shortBylineText").or_else(|| p.get("longBylineText"));
    let owner_label = yt_text(owner_obj);
    let has_external = owner_obj
        .and_then(|o| o.pointer("/runs/0/navigationEndpoint"))
        .is_some_and(|ep| {
            let (cid, curl) = channel_url_from_browse_endpoint(Some(ep));
            cid.is_some() || curl.is_some()
        })
        || owner_label.starts_with('@');
    let seed_video_id = p
        .pointer("/navigationEndpoint/watchEndpoint/videoId")
        .and_then(Value::as_str)
        .filter(|vid| is_valid_video_id(vid))
        .map(str::to_owned);
    let browse_id = p
        .pointer("/navigationEndpoint/browseEndpoint/browseId")
        .or_else(|| {
            p.pointer("/viewPlaylistText/runs/0/navigationEndpoint/browseEndpoint/browseId")
        })
        .and_then(Value::as_str)
        .map(str::to_owned);
    let is_mix = is_radio || id.starts_with("RD");
    let title_lower = title.to_lowercase();
    let is_music = is_mix
        || id.starts_with("OLAK5uy_")
        || title_lower.contains("music")
        || title_lower.contains("música");
    let kind = if id == "WL" {
        PlaylistKind::WatchLater
    } else if id == "LL" {
        PlaylistKind::LikedVideos
    } else if is_mix {
        PlaylistKind::Mix
    } else if is_music {
        PlaylistKind::Music
    } else {
        PlaylistKind::Playlist
    };
    let privacy = if matches!(kind, PlaylistKind::WatchLater | PlaylistKind::LikedVideos) {
        PlaylistPrivacy::Private
    } else {
        parse_privacy_label(&owner_label).unwrap_or(PlaylistPrivacy::Unknown)
    };
    let is_saved = !is_mix
        && !matches!(kind, PlaylistKind::WatchLater | PlaylistKind::LikedVideos)
        && has_external;
    let is_owned = !is_mix && !is_saved;
    let badge_text = if is_mix {
        "Mix".to_owned()
    } else if !count_text.is_empty() {
        if count_text.chars().all(|c| c.is_ascii_digit()) {
            format!("{count_text} vídeos")
        } else {
            count_text
        }
    } else {
        "Playlist".to_owned()
    };
    let subtitle_line = if !owner_label.is_empty() {
        format!("{owner_label} · Playlist")
    } else {
        "Playlist".to_owned()
    };
    let updated_text = {
        let t = yt_text(p.get("publishedTimeText"));
        (!t.is_empty()).then_some(t)
    };

    Some(SyncedPlaylistCard {
        id,
        title,
        thumbnail_url,
        video_count,
        badge_text,
        owner_label,
        subtitle_line,
        updated_text,
        privacy,
        kind,
        is_mix,
        is_music,
        is_owned,
        is_saved,
        seed_video_id,
        browse_id,
        has_full_playlist_link: !is_mix,
        stack_color_dark: None,
        stack_color_light: None,
    })
}

fn collect_playlist_cards(
    node: &Value,
    cards: &mut Vec<SyncedPlaylistCard>,
    seen_ids: &mut HashSet<String>,
    next_continuation: &mut Option<String>,
) {
    let Some(obj) = node.as_object() else {
        if let Some(arr) = node.as_array() {
            for item in arr {
                collect_playlist_cards(item, cards, seen_ids, next_continuation);
            }
        }
        return;
    };

    if let Some(cont) = obj.get("continuationItemRenderer") {
        if next_continuation.is_none()
            && let Some(token) = cont
                .pointer("/continuationEndpoint/continuationCommand/token")
                .or_else(|| {
                    cont.pointer("/button/buttonRenderer/command/continuationCommand/token")
                })
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            *next_continuation = Some(token.to_owned());
        }
        return;
    }

    if let Some(l) = obj.get("lockupViewModel") {
        if let Some(card) = parse_playlist_lockup_view_model(l)
            && seen_ids.insert(card.id.clone())
        {
            cards.push(card);
        }
        return;
    }
    if let Some(p) = obj
        .get("gridPlaylistRenderer")
        .or_else(|| obj.get("playlistRenderer"))
    {
        if let Some(card) = parse_legacy_playlist_renderer(p, false)
            && seen_ids.insert(card.id.clone())
        {
            cards.push(card);
        }
        return;
    }
    if let Some(r) = obj
        .get("gridRadioRenderer")
        .or_else(|| obj.get("radioRenderer"))
    {
        if let Some(card) = parse_legacy_playlist_renderer(r, true)
            && seen_ids.insert(card.id.clone())
        {
            cards.push(card);
        }
        return;
    }

    for child in obj.values() {
        collect_playlist_cards(child, cards, seen_ids, next_continuation);
    }
}

fn contains_signed_out_promo(root: &Value) -> bool {
    match root {
        Value::Object(map) => {
            if let Some(promo) = map.get("backgroundPromoRenderer") {
                let text = yt_text(promo.get("title")).to_lowercase()
                    + " "
                    + &yt_text(promo.get("bodyText")).to_lowercase();
                if text.contains("faça login") || text.contains("sign in") || text.contains("entre")
                {
                    return true;
                }
                if promo
                    .pointer("/ctaButton/buttonRenderer/navigationEndpoint/signInEndpoint")
                    .is_some()
                {
                    return true;
                }
            }
            map.values().any(contains_signed_out_promo)
        }
        Value::Array(arr) => arr.iter().any(contains_signed_out_promo),
        _ => false,
    }
}

pub fn parse_playlists_feed_response(json: &Value) -> RemotePlaylistsFeedPage {
    let mut page = RemotePlaylistsFeedPage::default();
    collect_playlist_chips_and_sorts(
        json,
        &mut page.chips,
        &mut page.sort_options,
        &mut page.selected_sort,
    );
    let mut seen_ids = HashSet::new();

    for tabs_ptr in [
        "/contents/twoColumnBrowseResultsRenderer/tabs",
        "/contents/singleColumnBrowseResultsRenderer/tabs",
    ] {
        if let Some(tabs) = json.pointer(tabs_ptr).and_then(Value::as_array) {
            for tab in tabs {
                if let Some(content) = tab.pointer("/tabRenderer/content") {
                    collect_playlist_cards(
                        content,
                        &mut page.cards,
                        &mut seen_ids,
                        &mut page.next_continuation,
                    );
                }
            }
        }
    }

    for action_key in ["onResponseReceivedActions", "onResponseReceivedEndpoints"] {
        if let Some(actions) = json.get(action_key).and_then(Value::as_array) {
            for action in actions {
                if let Some(items) = action
                    .pointer("/appendContinuationItemsAction/continuationItems")
                    .or_else(|| action.pointer("/reloadContinuationItemsCommand/continuationItems"))
                {
                    collect_playlist_cards(
                        items,
                        &mut page.cards,
                        &mut seen_ids,
                        &mut page.next_continuation,
                    );
                }
            }
        }
    }

    if let Some(cont_obj) = json.get("continuationContents") {
        for key in [
            "gridContinuation",
            "sectionListContinuation",
            "itemSectionContinuation",
            "richGridContinuation",
        ] {
            if let Some(sub) = cont_obj.get(key) {
                collect_playlist_cards(
                    sub,
                    &mut page.cards,
                    &mut seen_ids,
                    &mut page.next_continuation,
                );
            }
        }
    }

    if page.cards.is_empty() && contains_signed_out_promo(json) {
        page.signed_out = true;
    }
    page
}

fn find_object_by_key<'a>(root: &'a Value, target: &str) -> Option<&'a Value> {
    match root {
        Value::Object(map) => {
            if let Some(v) = map.get(target) {
                return Some(v);
            }
            for child in map.values() {
                if let Some(found) = find_object_by_key(child, target) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(|item| find_object_by_key(item, target)),
        _ => None,
    }
}

fn parse_playlist_header_metadata(
    json: &Value,
    playlist_id: &str,
) -> Option<RemotePlaylistDetailMetadata> {
    if let Some(vm) = find_object_by_key(json, "pageHeaderViewModel") {
        let title = vm
            .pointer("/title/dynamicTextViewModel/text/content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| yt_text(vm.pointer("/title/dynamicTextViewModel/text")));

        let mut owner = String::new();
        let mut privacy = PlaylistPrivacy::Unknown;
        let mut total_videos = None;

        if let Some(rows) = vm
            .pointer("/metadata/contentMetadataViewModel/metadataRows")
            .and_then(Value::as_array)
        {
            for row in rows {
                let Some(parts) = row.get("metadataParts").and_then(Value::as_array) else {
                    continue;
                };
                for part in parts {
                    let text = part
                        .pointer("/text/content")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .unwrap_or_else(|| yt_text(part.get("text")));
                    if text.is_empty() {
                        continue;
                    }
                    let lower = text.to_lowercase();
                    if let Some(p) = parse_privacy_label(&text) {
                        privacy = p;
                    } else if lower.contains("vídeo") || lower.contains("video") {
                        if total_videos.is_none() {
                            total_videos = extract_first_grouped_number(&text);
                        }
                    } else if owner.is_empty()
                        && !lower.contains("visualiza")
                        && !lower.contains("view")
                        && !is_updated_text(&text)
                    {
                        let cleaned = text
                            .strip_prefix("por ")
                            .or_else(|| text.strip_prefix("by "))
                            .unwrap_or(&text)
                            .trim();
                        if !cleaned.is_empty() {
                            owner = cleaned.to_owned();
                        }
                    }
                }
            }
        }

        let thumbnail_url = Some(select_yt_thumbnail(
            vm.pointer("/heroImage/contentPreviewImageViewModel/image/sources"),
            480,
        ))
        .filter(|u| !u.is_empty());

        let is_editable = find_object_by_key(json, "playlistHeaderRenderer")
            .and_then(|h| h.get("isEditable"))
            .and_then(Value::as_bool)
            .unwrap_or(playlist_id == "WL");

        if !title.is_empty() || total_videos.is_some() || !owner.is_empty() {
            return Some(RemotePlaylistDetailMetadata {
                playlist_id: playlist_id.to_owned(),
                title,
                owner,
                privacy,
                total_videos,
                thumbnail_url,
                is_mix: playlist_id.starts_with("RD"),
                is_editable,
            });
        }
    }

    if let Some(hdr) = find_object_by_key(json, "playlistHeaderRenderer") {
        let title = yt_text(hdr.get("title"));
        let owner = yt_text(hdr.get("ownerText"));
        let privacy = hdr
            .get("privacy")
            .and_then(Value::as_str)
            .and_then(|s| match s.to_ascii_uppercase().as_str() {
                "PRIVATE" => Some(PlaylistPrivacy::Private),
                "UNLISTED" => Some(PlaylistPrivacy::Unlisted),
                "PUBLIC" => Some(PlaylistPrivacy::Public),
                _ => None,
            })
            .or_else(|| {
                parse_privacy_label(&yt_text(
                    hdr.pointer("/privacyBadge/metadataBadgeRenderer/label"),
                ))
            })
            .unwrap_or(if playlist_id == "WL" || playlist_id == "LL" {
                PlaylistPrivacy::Private
            } else {
                PlaylistPrivacy::Unknown
            });
        let num_text = yt_text(
            hdr.get("numVideosText")
                .or_else(|| hdr.pointer("/byline/0/playlistBylineRenderer/text")),
        );
        let total_videos = extract_first_grouped_number(&num_text);
        let thumbnail_url = Some(select_yt_thumbnail(
            hdr.pointer("/playlistHeaderBanner/heroPlaylistThumbnailRenderer/thumbnail/thumbnails"),
            480,
        ))
        .filter(|u| !u.is_empty());
        let is_editable = hdr
            .get("isEditable")
            .and_then(Value::as_bool)
            .unwrap_or(playlist_id == "WL");

        return Some(RemotePlaylistDetailMetadata {
            playlist_id: playlist_id.to_owned(),
            title,
            owner,
            privacy,
            total_videos,
            thumbnail_url,
            is_mix: playlist_id.starts_with("RD"),
            is_editable,
        });
    }

    None
}

fn parse_playlist_video_renderer(v: &Value) -> Option<RemotePlaylistDetailItem> {
    if v.get("isPlayable").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let video_id = v
        .get("videoId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_video_id(id))?;
    let title = yt_text(v.get("title"));
    if title.is_empty()
        || title == "[Deleted video]"
        || title == "[Private video]"
        || title == "[Vídeo excluído]"
        || title == "[Vídeo privado]"
    {
        return None;
    }

    let owner_obj = v.get("shortBylineText").or_else(|| v.get("longBylineText"));
    let channel = yt_text(owner_obj);
    let (channel_id, channel_url) = channel_url_from_browse_endpoint(
        owner_obj.and_then(|o| o.pointer("/runs/0/navigationEndpoint")),
    );
    let channel_verified = has_verified_owner_badge(v.get("ownerBadges"));
    let thumbnail = select_yt_thumbnail(v.pointer("/thumbnail/thumbnails"), 480);

    let mut is_short = v.pointer("/navigationEndpoint/reelWatchEndpoint").is_some();
    let mut is_live = false;
    let mut duration = yt_text(v.get("lengthText"));
    if duration.is_empty()
        && let Some(secs) = v
            .get("lengthSeconds")
            .and_then(|s| {
                s.as_str()
                    .and_then(|t| t.parse::<u64>().ok())
                    .or_else(|| s.as_u64())
            })
            .filter(|&s| s > 0)
    {
        duration = search::format_duration_seconds(secs);
    }

    if let Some(overlays) = v.get("thumbnailOverlays").and_then(Value::as_array) {
        for ov in overlays {
            if let Some(ts) = ov.get("thumbnailOverlayTimeStatusRenderer") {
                let style = ts.get("style").and_then(Value::as_str).unwrap_or("");
                let text = yt_text(ts.get("text"));
                if style.eq_ignore_ascii_case("SHORTS") {
                    is_short = true;
                } else if style.eq_ignore_ascii_case("LIVE")
                    || text.eq_ignore_ascii_case("ao vivo")
                    || text.eq_ignore_ascii_case("live")
                {
                    is_live = true;
                } else if duration.is_empty() && !text.is_empty() {
                    duration = text;
                }
            }
        }
    }

    let video_info = yt_text(v.get("videoInfo"));
    let (view_count, view_count_text, video_info_date) =
        parse_video_info_views_and_date(&video_info);
    let published_time_text =
        video_info_date.or_else(|| clean_published_time_text(&yt_text(v.get("publishedTimeText"))));

    let url = if is_short {
        format!("https://www.youtube.com/shorts/{video_id}")
    } else {
        format!("https://www.youtube.com/watch?v={video_id}")
    };

    let set_video_id = v
        .get("setVideoId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let index_text = {
        let t = yt_text(v.get("index"));
        (!t.is_empty()).then_some(t)
    };

    Some(RemotePlaylistDetailItem {
        result: SearchResult {
            video: Video {
                title,
                channel,
                url,
                thumbnail,
                duration: if is_live { String::new() } else { duration },
            },
            metadata: VideoMetadata {
                view_count,
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
                channel_verified,
                ..Default::default()
            },
        },
        set_video_id,
        index_text,
    })
}

fn collect_playlist_detail_items(
    node: &Value,
    items: &mut Vec<RemotePlaylistDetailItem>,
    seen_keys: &mut HashSet<String>,
    next_continuation: &mut Option<String>,
) {
    let Some(obj) = node.as_object() else {
        if let Some(arr) = node.as_array() {
            for child in arr {
                collect_playlist_detail_items(child, items, seen_keys, next_continuation);
            }
        }
        return;
    };

    if let Some(cont) = obj.get("continuationItemRenderer") {
        if next_continuation.is_none()
            && let Some(token) = cont
                .pointer("/continuationEndpoint/continuationCommand/token")
                .or_else(|| {
                    cont.pointer("/button/buttonRenderer/command/continuationCommand/token")
                })
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            *next_continuation = Some(token.to_owned());
        }
        return;
    }

    if let Some(v) = obj.get("playlistVideoRenderer") {
        if let Some(item) = parse_playlist_video_renderer(v) {
            let dedup_key = item
                .set_video_id
                .clone()
                .unwrap_or_else(|| item.result.video.url.clone());
            if seen_keys.insert(dedup_key) {
                items.push(item);
            }
        }
        return;
    }

    if let Some(l) = obj.get("lockupViewModel") {
        if let Some(result) = crate::innertube::parse_lockup_video_vm(l)
            && seen_keys.insert(result.video.url.clone())
        {
            items.push(RemotePlaylistDetailItem {
                result,
                set_video_id: None,
                index_text: None,
            });
        }
        return;
    }

    for child in obj.values() {
        collect_playlist_detail_items(child, items, seen_keys, next_continuation);
    }
}

pub fn parse_playlist_detail_response(json: &Value, playlist_id: &str) -> RemotePlaylistDetailPage {
    let mut page = RemotePlaylistDetailPage {
        meta: parse_playlist_header_metadata(json, playlist_id),
        ..Default::default()
    };
    let mut seen_keys = HashSet::new();

    if let Some(tabs) = json
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for tab in tabs {
            if let Some(content) = tab.pointer("/tabRenderer/content") {
                collect_playlist_detail_items(
                    content,
                    &mut page.items,
                    &mut seen_keys,
                    &mut page.next_continuation,
                );
            }
        }
    }

    if let Some(actions) = json
        .get("onResponseReceivedActions")
        .and_then(Value::as_array)
    {
        for action in actions {
            if let Some(items) = action
                .pointer("/appendContinuationItemsAction/continuationItems")
                .or_else(|| action.pointer("/reloadContinuationItemsCommand/continuationItems"))
            {
                collect_playlist_detail_items(
                    items,
                    &mut page.items,
                    &mut seen_keys,
                    &mut page.next_continuation,
                );
            }
        }
    }

    if page.items.is_empty() && page.meta.is_none() && contains_signed_out_promo(json) {
        page.signed_out = true;
    }
    page
}

fn parse_playlist_panel_video_renderer(v: &Value) -> Option<RemotePlaylistDetailItem> {
    let video_id = v
        .get("videoId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_video_id(id))?;
    let title = yt_text(v.get("title"));
    if title.is_empty() {
        return None;
    }
    let owner_obj = v.get("longBylineText").or_else(|| v.get("shortBylineText"));
    let channel = yt_text(owner_obj);
    let (channel_id, channel_url) = channel_url_from_browse_endpoint(
        owner_obj.and_then(|o| o.pointer("/runs/0/navigationEndpoint")),
    );
    let duration = yt_text(v.get("lengthText"));
    let thumbnail = select_yt_thumbnail(v.pointer("/thumbnail/thumbnails"), 480);
    let index_text = {
        let t = yt_text(v.get("indexText"));
        (!t.is_empty()).then_some(t)
    };
    let set_video_id = v
        .get("playlistSetVideoId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    Some(RemotePlaylistDetailItem {
        result: SearchResult {
            video: Video {
                title,
                channel,
                url: format!("https://www.youtube.com/watch?v={video_id}"),
                thumbnail,
                duration,
            },
            metadata: VideoMetadata {
                channel_id,
                channel_url,
                kind: SearchResultKind::Video,
                ..Default::default()
            },
        },
        set_video_id,
        index_text,
    })
}

/// Extracts the seed video ID from a synthetic `mix_seed:<video_id>` continuation token.
pub fn parse_mix_seed_continuation(token: &str) -> Option<&str> {
    token
        .trim()
        .strip_prefix(MIX_SEED_CONTINUATION_PREFIX)
        .map(str::trim)
        .filter(|vid| is_valid_video_id(vid))
}

fn extract_watch_next_continuations_array(arr: &[Value]) -> Option<String> {
    for entry in arr {
        for key in [
            "nextContinuationData",
            "nextRadioContinuationData",
            "reloadContinuationData",
        ] {
            if let Some(tok) = entry
                .get(key)
                .and_then(|d| d.get("continuation"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(tok.to_owned());
            }
        }
    }
    None
}

fn extract_continuation_item_token_local(cont: &Value) -> Option<String> {
    cont.pointer("/continuationEndpoint/continuationCommand/token")
        .or_else(|| cont.pointer("/button/buttonRenderer/command/continuationCommand/token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

pub fn parse_watch_next_playlist_response(
    json: &Value,
    fallback_playlist_id: &str,
) -> RemotePlaylistDetailPage {
    let mut page = RemotePlaylistDetailPage::default();
    let mut seen = HashSet::new();
    let mut is_mix = fallback_playlist_id.starts_with("RD");

    if let Some(pl) = json.pointer("/contents/twoColumnWatchNextResults/playlist/playlist") {
        let playlist_id = pl
            .get("playlistId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| is_valid_playlist_id(s))
            .unwrap_or(fallback_playlist_id)
            .to_owned();
        let title = pl
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| yt_text(pl.get("titleText")));
        let owner = yt_text(pl.get("ownerName"));
        let is_infinite = pl.get("isInfinite").and_then(Value::as_bool) == Some(true);
        is_mix = playlist_id.starts_with("RD") || is_infinite;
        let total_videos = if is_infinite {
            None
        } else {
            pl.get("totalVideos")
                .and_then(Value::as_u64)
                .or_else(|| extract_first_grouped_number(&yt_text(pl.get("totalVideosText"))))
        };

        page.meta = Some(RemotePlaylistDetailMetadata {
            playlist_id: playlist_id.clone(),
            title,
            owner,
            privacy: if playlist_id == "WL" || playlist_id == "LL" {
                PlaylistPrivacy::Private
            } else {
                PlaylistPrivacy::Unknown
            },
            total_videos,
            thumbnail_url: None,
            is_mix,
            is_editable: false,
        });

        if let Some(contents) = pl.get("contents").and_then(Value::as_array) {
            for entry in contents {
                if let Some(panel_video) = entry.get("playlistPanelVideoRenderer")
                    && let Some(item) = parse_playlist_panel_video_renderer(panel_video)
                    && seen.insert(item.result.video.url.clone())
                {
                    page.items.push(item);
                } else if let Some(cont) = entry.get("continuationItemRenderer")
                    && page.next_continuation.is_none()
                {
                    page.next_continuation = extract_continuation_item_token_local(cont);
                }
            }
        }

        if page.next_continuation.is_none()
            && let Some(conts) = pl.get("continuations").and_then(Value::as_array)
        {
            page.next_continuation = extract_watch_next_continuations_array(conts);
        }
    }

    if let Some(panel_cont) = json.pointer("/continuationContents/playlistPanelContinuation") {
        if let Some(contents) = panel_cont.get("contents").and_then(Value::as_array) {
            for entry in contents {
                if let Some(panel_video) = entry.get("playlistPanelVideoRenderer")
                    && let Some(item) = parse_playlist_panel_video_renderer(panel_video)
                    && seen.insert(item.result.video.url.clone())
                {
                    page.items.push(item);
                } else if let Some(cont) = entry.get("continuationItemRenderer")
                    && page.next_continuation.is_none()
                {
                    page.next_continuation = extract_continuation_item_token_local(cont);
                }
            }
        }
        if page.next_continuation.is_none()
            && let Some(conts) = panel_cont.get("continuations").and_then(Value::as_array)
        {
            page.next_continuation = extract_watch_next_continuations_array(conts);
        }
    }

    for action_key in ["onResponseReceivedEndpoints", "onResponseReceivedActions"] {
        if let Some(actions) = json.get(action_key).and_then(Value::as_array) {
            for action in actions {
                if let Some(items) = action
                    .pointer("/appendContinuationItemsAction/continuationItems")
                    .or_else(|| action.pointer("/reloadContinuationItemsCommand/continuationItems"))
                    .and_then(Value::as_array)
                {
                    for entry in items {
                        if let Some(panel_video) = entry.get("playlistPanelVideoRenderer")
                            && let Some(item) = parse_playlist_panel_video_renderer(panel_video)
                            && seen.insert(item.result.video.url.clone())
                        {
                            page.items.push(item);
                        } else if let Some(cont) = entry.get("continuationItemRenderer")
                            && page.next_continuation.is_none()
                        {
                            page.next_continuation = extract_continuation_item_token_local(cont);
                        }
                    }
                }
            }
        }
    }

    // For infinite YouTube Mixes (`RD...`) that omit explicit continuation tokens, synthesize
    // a next-video seed token from the tail track so pagination continues beyond page 0.
    if page.next_continuation.is_none()
        && is_mix
        && let Some(last_vid) = page
            .items
            .last()
            .and_then(|item| crate::history::youtube_video_id(&item.result.video.url))
    {
        page.next_continuation = Some(format!("{MIX_SEED_CONTINUATION_PREFIX}{last_vid}"));
    }

    page
}

pub fn fetch_playlists_feed_page_at(
    innertube_base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    browse_params: Option<&str>,
    continuation: Option<&str>,
    hl: &str,
    cancel: &Cancellation,
) -> Result<RemotePlaylistsFeedPage, InnerTubeError> {
    let body = if let Some(token) = continuation.map(str::trim).filter(|t| !t.is_empty()) {
        json!({ "continuation": token })
    } else if let Some(params) = browse_params.map(str::trim).filter(|p| !p.is_empty()) {
        json!({ "browseId": PLAYLISTS_FEED_BROWSE_ID, "params": params })
    } else {
        json!({ "browseId": PLAYLISTS_FEED_BROWSE_ID })
    };
    let json = post_innertube_with_locale_at(
        innertube_base_url,
        client,
        session,
        "browse",
        body,
        hl,
        cancel,
    )?;
    let page = parse_playlists_feed_response(&json);
    if page.signed_out {
        let _ = crate::innertube::invalidate_web_session();
        return Err(InnerTubeError::AuthExpired(
            "Sua sessão do YouTube expirou. Entre novamente.".into(),
        ));
    }
    Ok(page)
}

pub fn fetch_playlist_detail_page_at(
    innertube_base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    playlist_id: &str,
    continuation: Option<&str>,
    hl: &str,
    cancel: &Cancellation,
) -> Result<RemotePlaylistDetailPage, InnerTubeError> {
    if !is_valid_playlist_id(playlist_id) {
        return Err(InnerTubeError::Other("Playlist inválida.".into()));
    }
    let body = if let Some(token) = continuation {
        json!({ "continuation": token })
    } else {
        let browse_id = if playlist_id.starts_with("VL") {
            playlist_id.to_owned()
        } else {
            format!("VL{playlist_id}")
        };
        json!({ "browseId": browse_id })
    };
    let json = post_innertube_with_locale_at(
        innertube_base_url,
        client,
        session,
        "browse",
        body,
        hl,
        cancel,
    )?;
    let page = parse_playlist_detail_response(&json, playlist_id);
    if page.signed_out {
        let _ = crate::innertube::invalidate_web_session();
        return Err(InnerTubeError::AuthExpired(
            "Sua sessão do YouTube expirou. Entre novamente.".into(),
        ));
    }
    Ok(page)
}

#[allow(clippy::too_many_arguments)]
pub fn fetch_watch_next_playlist_page_at(
    innertube_base_url: &str,
    client: &reqwest::blocking::Client,
    session_opt: Option<&mut YouTubeWebSession>,
    video_id: Option<&str>,
    playlist_id: &str,
    continuation: Option<&str>,
    hl: &str,
    cancel: &Cancellation,
) -> Result<RemotePlaylistDetailPage, InnerTubeError> {
    if !is_valid_playlist_id(playlist_id) {
        return Err(InnerTubeError::Other("Playlist inválida.".into()));
    }
    let body = if let Some(tok) = continuation.map(str::trim).filter(|t| !t.is_empty()) {
        if let Some(seed_vid) = parse_mix_seed_continuation(tok) {
            json!({
                "playlistId": playlist_id,
                "videoId": seed_vid,
            })
        } else {
            json!({ "continuation": tok })
        }
    } else {
        let mut b = json!({ "playlistId": playlist_id });
        if let Some(vid) = video_id.filter(|v| is_valid_video_id(v))
            && let Some(obj) = b.as_object_mut()
        {
            obj.insert("videoId".into(), Value::String(vid.to_owned()));
        }
        b
    };
    let json = match session_opt {
        Some(session) => post_innertube_with_locale_at(
            innertube_base_url,
            client,
            session,
            "next",
            body,
            hl,
            cancel,
        )?,
        None => post_innertube_unauth_with_locale_at(
            innertube_base_url,
            client,
            "next",
            body,
            hl,
            cancel,
        )?,
    };
    Ok(parse_watch_next_playlist_response(&json, playlist_id))
}

pub fn fetch_watch_next_playlist_at(
    innertube_base_url: &str,
    client: &reqwest::blocking::Client,
    session_opt: Option<&mut YouTubeWebSession>,
    video_id: Option<&str>,
    playlist_id: &str,
    hl: &str,
    cancel: &Cancellation,
) -> Result<RemotePlaylistDetailPage, InnerTubeError> {
    fetch_watch_next_playlist_page_at(
        innertube_base_url,
        client,
        session_opt,
        video_id,
        playlist_id,
        None,
        hl,
        cancel,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn remove_video_from_playlist_at(
    innertube_base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    playlist_id: &str,
    video_id: &str,
    set_video_id: Option<&str>,
    hl: &str,
    cancel: &Cancellation,
) -> Result<(), InnerTubeError> {
    if !is_valid_playlist_id(playlist_id) || !is_valid_video_id(video_id) {
        return Err(InnerTubeError::Other("Playlist ou vídeo inválido.".into()));
    }
    let action = if let Some(svid) = set_video_id.filter(|s| !s.trim().is_empty()) {
        json!({
            "action": "ACTION_REMOVE_VIDEO",
            "setVideoId": svid,
        })
    } else {
        json!({
            "action": "ACTION_REMOVE_VIDEO_BY_VIDEO_ID",
            "removedVideoId": video_id,
        })
    };
    let body = json!({
        "playlistId": playlist_id,
        "actions": [action],
    });
    let json = post_innertube_with_locale_at(
        innertube_base_url,
        client,
        session,
        "browse/edit_playlist",
        body,
        hl,
        cancel,
    )?;
    if let Some(status) = json.get("status").and_then(Value::as_str)
        && status != "STATUS_SUCCEEDED"
    {
        return Err(InnerTubeError::Other(
            "O YouTube não conseguiu remover o vídeo da playlist.".into(),
        ));
    }
    Ok(())
}

/// Adapter trait allowing `add_video_to_playlist_innertube` to accept either
/// `crate::innertube::YouTubeWebSession` or `crate::comment_actions::YouTubeWebSession`
/// (by mutable ref, shared ref, value, or `Option`).
pub trait PlaylistWebSessionMut {
    fn with_innertube_session<R>(
        self,
        cancel: &Cancellation,
        f: impl FnOnce(
            &mut crate::innertube::YouTubeWebSession,
        ) -> Result<R, crate::innertube::InnerTubeError>,
    ) -> Result<R, crate::innertube::InnerTubeError>;
}

impl PlaylistWebSessionMut for &mut crate::innertube::YouTubeWebSession {
    fn with_innertube_session<R>(
        self,
        _cancel: &Cancellation,
        f: impl FnOnce(
            &mut crate::innertube::YouTubeWebSession,
        ) -> Result<R, crate::innertube::InnerTubeError>,
    ) -> Result<R, crate::innertube::InnerTubeError> {
        f(self)
    }
}

impl PlaylistWebSessionMut for &crate::innertube::YouTubeWebSession {
    fn with_innertube_session<R>(
        self,
        _cancel: &Cancellation,
        f: impl FnOnce(
            &mut crate::innertube::YouTubeWebSession,
        ) -> Result<R, crate::innertube::InnerTubeError>,
    ) -> Result<R, crate::innertube::InnerTubeError> {
        let mut cloned = self.clone();
        f(&mut cloned)
    }
}

impl PlaylistWebSessionMut for crate::innertube::YouTubeWebSession {
    fn with_innertube_session<R>(
        mut self,
        _cancel: &Cancellation,
        f: impl FnOnce(
            &mut crate::innertube::YouTubeWebSession,
        ) -> Result<R, crate::innertube::InnerTubeError>,
    ) -> Result<R, crate::innertube::InnerTubeError> {
        f(&mut self)
    }
}

impl PlaylistWebSessionMut for &mut YouTubeWebSession {
    fn with_innertube_session<R>(
        self,
        _cancel: &Cancellation,
        f: impl FnOnce(
            &mut crate::innertube::YouTubeWebSession,
        ) -> Result<R, crate::innertube::InnerTubeError>,
    ) -> Result<R, crate::innertube::InnerTubeError> {
        let mut it_sess = to_innertube_web_session(self, unix_now_secs())?;
        let out = f(&mut it_sess)?;
        if let Ok(updated) = from_innertube_web_session(&it_sess) {
            *self = updated;
        }
        Ok(out)
    }
}

impl PlaylistWebSessionMut for &YouTubeWebSession {
    fn with_innertube_session<R>(
        self,
        _cancel: &Cancellation,
        f: impl FnOnce(
            &mut crate::innertube::YouTubeWebSession,
        ) -> Result<R, crate::innertube::InnerTubeError>,
    ) -> Result<R, crate::innertube::InnerTubeError> {
        let mut it_sess = to_innertube_web_session(self, unix_now_secs())?;
        f(&mut it_sess)
    }
}

impl PlaylistWebSessionMut for YouTubeWebSession {
    fn with_innertube_session<R>(
        self,
        _cancel: &Cancellation,
        f: impl FnOnce(
            &mut crate::innertube::YouTubeWebSession,
        ) -> Result<R, crate::innertube::InnerTubeError>,
    ) -> Result<R, crate::innertube::InnerTubeError> {
        let mut it_sess = to_innertube_web_session(&self, unix_now_secs())?;
        f(&mut it_sess)
    }
}

impl<T: PlaylistWebSessionMut> PlaylistWebSessionMut for Option<T> {
    fn with_innertube_session<R>(
        self,
        cancel: &Cancellation,
        f: impl FnOnce(
            &mut crate::innertube::YouTubeWebSession,
        ) -> Result<R, crate::innertube::InnerTubeError>,
    ) -> Result<R, crate::innertube::InnerTubeError> {
        match self {
            Some(inner) => inner.with_innertube_session(cancel, f),
            None => {
                let acquired = acquire_web_session(None, false, cancel)
                    .map_err(crate::innertube::InnerTubeError::AuthExpired)?;
                let mut it_sess = to_innertube_web_session(&acquired, unix_now_secs())?;
                f(&mut it_sess)
            }
        }
    }
}

/// Zero-quota InnerTube helper to add a video to `WL` (Watch Later) or any user playlist,
/// delegating directly to `crate::innertube::mutate_playlist_add_video`.
#[allow(dead_code)]
pub fn add_video_to_playlist_innertube<S: PlaylistWebSessionMut>(
    playlist_id: &str,
    video_id: &str,
    session: S,
    cancel: &Cancellation,
) -> Result<Value, String> {
    let ctx = crate::innertube::InnerTubeContext::default();
    let res = session.with_innertube_session(cancel, |it_sess| {
        crate::innertube::mutate_playlist_add_video(playlist_id, video_id, &ctx, it_sess, cancel)
    });
    match res {
        Ok(val) => Ok(val),
        Err(err) => {
            if err.is_auth_expired() {
                let _ = crate::innertube::invalidate_web_session();
            }
            Err(err.into_message())
        }
    }
}

/// Zero-quota InnerTube helper to rename a user playlist (`ACTION_SET_PLAYLIST_NAME`).
#[allow(dead_code)]
pub fn rename_playlist_innertube<S: PlaylistWebSessionMut>(
    playlist_id: &str,
    new_title: &str,
    session: S,
    cancel: &Cancellation,
) -> Result<Value, String> {
    let ctx = crate::innertube::InnerTubeContext::default();
    let res = session.with_innertube_session(cancel, |it_sess| {
        crate::innertube::mutate_playlist_rename(playlist_id, new_title, &ctx, it_sess, cancel)
    });
    match res {
        Ok(val) => Ok(val),
        Err(err) => {
            if err.is_auth_expired() {
                let _ = crate::innertube::invalidate_web_session();
            }
            Err(err.into_message())
        }
    }
}

/// Zero-quota InnerTube helper to update a user playlist's privacy (`ACTION_SET_PLAYLIST_PRIVACY`).
#[allow(dead_code)]
pub fn set_playlist_privacy_innertube<S: PlaylistWebSessionMut>(
    playlist_id: &str,
    privacy: PlaylistPrivacy,
    session: S,
    cancel: &Cancellation,
) -> Result<Value, String> {
    let Some(privacy_str) = privacy.as_innertube_str() else {
        return Err("Privacidade da playlist inválida.".into());
    };
    let ctx = crate::innertube::InnerTubeContext::default();
    let res = session.with_innertube_session(cancel, |it_sess| {
        crate::innertube::mutate_playlist_privacy(playlist_id, privacy_str, &ctx, it_sess, cancel)
    });
    match res {
        Ok(val) => Ok(val),
        Err(err) => {
            if err.is_auth_expired() {
                let _ = crate::innertube::invalidate_web_session();
            }
            Err(err.into_message())
        }
    }
}

/// Zero-quota InnerTube helper to delete a user playlist (`playlist/delete`).
#[allow(dead_code)]
pub fn delete_playlist_innertube<S: PlaylistWebSessionMut>(
    playlist_id: &str,
    session: S,
    cancel: &Cancellation,
) -> Result<Value, String> {
    let ctx = crate::innertube::InnerTubeContext::default();
    let res = session.with_innertube_session(cancel, |it_sess| {
        crate::innertube::mutate_playlist_delete(playlist_id, &ctx, it_sess, cancel)
    });
    match res {
        Ok(val) => Ok(val),
        Err(err) => {
            if err.is_auth_expired() {
                let _ = crate::innertube::invalidate_web_session();
            }
            Err(err.into_message())
        }
    }
}

/// Zero-quota InnerTube helper to reorder a playlist item (`ACTION_MOVE_VIDEO_AFTER`).
#[allow(dead_code)]
pub fn move_playlist_video_innertube<S: PlaylistWebSessionMut>(
    playlist_id: &str,
    set_video_id: &str,
    predecessor_set_video_id: Option<&str>,
    session: S,
    cancel: &Cancellation,
) -> Result<Value, String> {
    let ctx = crate::innertube::InnerTubeContext::default();
    let res = session.with_innertube_session(cancel, |it_sess| {
        crate::innertube::mutate_playlist_move_video(
            playlist_id,
            set_video_id,
            predecessor_set_video_id,
            &ctx,
            it_sess,
            cancel,
        )
    });
    match res {
        Ok(val) => Ok(val),
        Err(err) => {
            if err.is_auth_expired() {
                let _ = crate::innertube::invalidate_web_session();
            }
            Err(err.into_message())
        }
    }
}

#[allow(dead_code)]
pub fn add_video_to_playlist_at(
    innertube_base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    playlist_id: &str,
    video_id: &str,
    hl: &str,
    cancel: &Cancellation,
) -> Result<(), InnerTubeError> {
    let body = crate::innertube::build_playlist_add_video_payload(playlist_id, video_id)
        .map_err(|err| InnerTubeError::Other(err.into_message()))?;
    let json = post_innertube_with_locale_at(
        innertube_base_url,
        client,
        session,
        "browse/edit_playlist",
        body,
        hl,
        cancel,
    )?;
    if let Some(status) = json.get("status").and_then(Value::as_str)
        && status != "STATUS_SUCCEEDED"
    {
        return Err(InnerTubeError::Other(
            "O YouTube não conseguiu adicionar o vídeo à playlist.".into(),
        ));
    }
    Ok(())
}

#[allow(dead_code)]
pub fn start_playlist_add_video(
    playlist_id: String,
    video_id: String,
    cached_session: Option<YouTubeWebSession>,
    hl: String,
    ctx: &egui::Context,
) -> (Receiver<RemotePlaylistActionEvent>, Cancellation) {
    let (tx, rx) = mpsc::channel();
    let cancel = Cancellation::default();
    let cancel_bg = cancel.clone();
    let ctx_bg = ctx.clone();

    std::thread::spawn(move || {
        let client = match http_client() {
            Ok(c) => c,
            Err(err) => {
                let _ = tx.send(RemotePlaylistActionEvent {
                    web_session: cached_session,
                    auth_expired: false,
                    playlist_id,
                    video_id,
                    result: Err(err),
                });
                ctx_bg.request_repaint();
                return;
            }
        };
        let mut session = match acquire_web_session(cached_session, false, &cancel_bg) {
            Ok(s) => s,
            Err(err) => {
                let _ = tx.send(RemotePlaylistActionEvent {
                    web_session: None,
                    auth_expired: true,
                    playlist_id,
                    video_id,
                    result: Err(err),
                });
                ctx_bg.request_repaint();
                return;
            }
        };
        session.refresh_client_config_if_needed(&client, &cancel_bg);
        persist_web_session_dpapi(&session);
        let _ = hl;
        match add_video_to_playlist_innertube(&playlist_id, &video_id, &mut session, &cancel_bg) {
            Ok(_) => {
                persist_web_session_dpapi(&session);
                let _ = tx.send(RemotePlaylistActionEvent {
                    web_session: Some(session),
                    auth_expired: false,
                    playlist_id,
                    video_id,
                    result: Ok(()),
                });
            }
            Err(err) => {
                let auth_expired =
                    err.contains("expired") || err.contains("expirou") || err.contains("rejected");
                if auth_expired {
                    let _ = crate::innertube::invalidate_web_session();
                }
                let _ = tx.send(RemotePlaylistActionEvent {
                    web_session: (!auth_expired).then_some(session),
                    auth_expired,
                    playlist_id,
                    video_id,
                    result: Err(err),
                });
            }
        }
        ctx_bg.request_repaint();
    });

    (rx, cancel)
}

pub fn start_playlists_feed_sync(
    request_id: u64,
    mode: PlaylistsFetchMode,
    cached_session: Option<YouTubeWebSession>,
    allow_interactive: bool,
    hl: String,
    ctx: &egui::Context,
) -> (Receiver<PlaylistsSyncMessage>, Cancellation) {
    let (tx, rx) = mpsc::channel();
    let cancel = Cancellation::default();
    let cancel_bg = cancel.clone();
    let ctx_bg = ctx.clone();

    let (filter, sort, action) = match mode {
        PlaylistsFetchMode::Initial => (
            PlaylistCategoryFilter::All,
            PlaylistSort::RecentlyAdded,
            None,
        ),
        PlaylistsFetchMode::Filter(f, s, act) => (f, s, Some(act)),
        PlaylistsFetchMode::Sort(f, s, act) => (f, s, Some(act)),
    };

    std::thread::spawn(move || {
        let client = match http_client() {
            Ok(c) => c,
            Err(err) => {
                let _ = tx.send(PlaylistsSyncMessage::Finished {
                    request_id,
                    filter,
                    sort,
                    auth_expired: false,
                    result: Err(err),
                });
                ctx_bg.request_repaint();
                return;
            }
        };

        let had_cached = cached_session.is_some();
        let mut session = match acquire_web_session(cached_session, allow_interactive, &cancel_bg) {
            Ok(s) => s,
            Err(err) => {
                let auth_expired = err.contains("expirou")
                    || err.contains("Entre na sua conta")
                    || err.contains("expirada")
                    || err.contains("ausente")
                    || err.contains("login");
                let _ = tx.send(PlaylistsSyncMessage::Finished {
                    request_id,
                    filter,
                    sort,
                    auth_expired,
                    result: Err(err),
                });
                ctx_bg.request_repaint();
                return;
            }
        };
        session.refresh_client_config_if_needed(&client, &cancel_bg);
        persist_web_session_dpapi(&session);
        let _ = tx.send(PlaylistsSyncMessage::SessionUpdated(session.clone()));

        let (initial_params, initial_cont) = match &action {
            Some(PlaylistChipAction::BrowseParams(p)) => (Some(p.as_str()), None),
            Some(PlaylistChipAction::Continuation(c)) => (None, Some(c.as_str())),
            None => (None, None),
        };

        let first_page = match fetch_playlists_feed_page_at(
            INNERTUBE_BASE_URL,
            &client,
            &mut session,
            initial_params,
            initial_cont,
            &hl,
            &cancel_bg,
        ) {
            Ok(page) => page,
            Err(InnerTubeError::AuthExpired(_)) if had_cached && !cancel_bg.cancelled() => {
                let _ = crate::innertube::invalidate_web_session();
                match acquire_web_session(None, allow_interactive, &cancel_bg) {
                    Ok(mut refreshed) => {
                        refreshed.refresh_client_config_if_needed(&client, &cancel_bg);
                        persist_web_session_dpapi(&refreshed);
                        session = refreshed;
                        let _ = tx.send(PlaylistsSyncMessage::SessionUpdated(session.clone()));
                        match fetch_playlists_feed_page_at(
                            INNERTUBE_BASE_URL,
                            &client,
                            &mut session,
                            initial_params,
                            initial_cont,
                            &hl,
                            &cancel_bg,
                        ) {
                            Ok(page) => page,
                            Err(err) => {
                                let auth_expired = matches!(err, InnerTubeError::AuthExpired(_));
                                let _ = tx.send(PlaylistsSyncMessage::Finished {
                                    request_id,
                                    filter,
                                    sort,
                                    auth_expired,
                                    result: Err(err.into_message()),
                                });
                                ctx_bg.request_repaint();
                                return;
                            }
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(PlaylistsSyncMessage::Finished {
                            request_id,
                            filter,
                            sort,
                            auth_expired: true,
                            result: Err(err),
                        });
                        ctx_bg.request_repaint();
                        return;
                    }
                }
            }
            Err(err) => {
                let auth_expired = matches!(err, InnerTubeError::AuthExpired(_));
                let _ = tx.send(PlaylistsSyncMessage::Finished {
                    request_id,
                    filter,
                    sort,
                    auth_expired,
                    result: Err(err.into_message()),
                });
                ctx_bg.request_repaint();
                return;
            }
        };

        let mut next_cont = first_page.next_continuation.clone();
        let mut seen_tokens = HashSet::new();
        if let Some(tok) = &next_cont {
            seen_tokens.insert(tok.clone());
        }
        let mut total_cards = first_page.cards.len();
        let _ = tx.send(PlaylistsSyncMessage::Page {
            request_id,
            filter,
            sort,
            page_index: 0,
            page: first_page,
        });
        ctx_bg.request_repaint();

        for page_index in 1..MAX_PLAYLISTS_FEED_PAGES {
            if cancel_bg.cancelled() || total_cards >= MAX_PLAYLIST_FEED_CARDS {
                break;
            }
            let Some(token) = next_cont.take() else {
                break;
            };
            match fetch_playlists_feed_page_at(
                INNERTUBE_BASE_URL,
                &client,
                &mut session,
                None,
                Some(&token),
                &hl,
                &cancel_bg,
            ) {
                Ok(page) => {
                    total_cards += page.cards.len();
                    next_cont = page
                        .next_continuation
                        .clone()
                        .filter(|t| seen_tokens.insert(t.clone()));
                    let _ = tx.send(PlaylistsSyncMessage::Page {
                        request_id,
                        filter,
                        sort,
                        page_index,
                        page,
                    });
                    ctx_bg.request_repaint();
                }
                Err(_) => break,
            }
        }

        if !cancel_bg.cancelled() {
            let _ = tx.send(PlaylistsSyncMessage::Finished {
                request_id,
                filter,
                sort,
                auth_expired: false,
                result: Ok(total_cards),
            });
            ctx_bg.request_repaint();
        }
    });

    (rx, cancel)
}

#[allow(clippy::too_many_arguments)]
pub fn start_playlist_detail_sync(
    request_id: u64,
    playlist_id: String,
    seed_video_id: Option<String>,
    cached_session: Option<YouTubeWebSession>,
    oauth_token: Option<String>,
    allow_interactive: bool,
    hl: String,
    ctx: &egui::Context,
) -> (Receiver<PlaylistDetailSyncMessage>, Cancellation) {
    let (tx, rx) = mpsc::channel();
    let cancel = Cancellation::default();
    let cancel_bg = cancel.clone();
    let ctx_bg = ctx.clone();

    std::thread::spawn(move || {
        let client = match http_client() {
            Ok(c) => c,
            Err(err) => {
                let _ = tx.send(PlaylistDetailSyncMessage::Finished {
                    request_id,
                    playlist_id,
                    auth_expired: false,
                    result: Err(err),
                });
                ctx_bg.request_repaint();
                return;
            }
        };

        let is_mix = playlist_id.starts_with("RD");
        let had_cached = cached_session.is_some();
        let mut web_session = acquire_web_session(cached_session, allow_interactive, &cancel_bg)
            .ok()
            .map(|mut sess| {
                sess.refresh_client_config_if_needed(&client, &cancel_bg);
                persist_web_session_dpapi(&sess);
                let _ = tx.send(PlaylistDetailSyncMessage::SessionUpdated(sess.clone()));
                sess
            });

        if is_mix {
            let first_attempt = fetch_watch_next_playlist_at(
                INNERTUBE_BASE_URL,
                &client,
                web_session.as_mut(),
                seed_video_id.as_deref(),
                &playlist_id,
                &hl,
                &cancel_bg,
            );
            let mut use_unauth_mix = false;
            let page = match first_attempt {
                Ok(p) => Ok(p),
                Err(InnerTubeError::AuthExpired(_)) => {
                    use_unauth_mix = true;
                    fetch_watch_next_playlist_at(
                        INNERTUBE_BASE_URL,
                        &client,
                        None,
                        seed_video_id.as_deref(),
                        &playlist_id,
                        &hl,
                        &cancel_bg,
                    )
                }
                Err(e) => Err(e),
            };
            match page {
                Ok(first_page) => {
                    let mut seen_urls: HashSet<String> = first_page
                        .items
                        .iter()
                        .map(|item| item.result.video.url.clone())
                        .collect();
                    let mut seen_tokens: HashSet<String> = HashSet::new();
                    let mut seen_seeds: HashSet<String> = HashSet::new();
                    if let Some(seed) = seed_video_id.as_deref() {
                        seen_seeds.insert(seed.to_owned());
                    }
                    let mut next_cont = first_page.next_continuation.clone();
                    if let Some(tok) = &next_cont {
                        seen_tokens.insert(tok.clone());
                    }
                    let mut total_items = first_page.items.len();
                    let _ = tx.send(PlaylistDetailSyncMessage::Page {
                        request_id,
                        playlist_id: playlist_id.clone(),
                        page_index: 0,
                        page: first_page,
                    });
                    ctx_bg.request_repaint();

                    for page_index in 1..MAX_MIX_DETAIL_PAGES {
                        if cancel_bg.cancelled() || total_items >= MAX_PLAYLIST_DETAIL_VIDEOS {
                            break;
                        }
                        let Some(token) = next_cont.take() else {
                            break;
                        };
                        if let Some(seed_vid) = parse_mix_seed_continuation(&token)
                            && !seen_seeds.insert(seed_vid.to_owned())
                        {
                            break;
                        }
                        let sess_arg = if use_unauth_mix {
                            None
                        } else {
                            web_session.as_mut()
                        };
                        let next_res = match fetch_watch_next_playlist_page_at(
                            INNERTUBE_BASE_URL,
                            &client,
                            sess_arg,
                            None,
                            &playlist_id,
                            Some(&token),
                            &hl,
                            &cancel_bg,
                        ) {
                            Ok(p) => Ok(p),
                            Err(InnerTubeError::AuthExpired(_)) if !use_unauth_mix => {
                                use_unauth_mix = true;
                                fetch_watch_next_playlist_page_at(
                                    INNERTUBE_BASE_URL,
                                    &client,
                                    None,
                                    None,
                                    &playlist_id,
                                    Some(&token),
                                    &hl,
                                    &cancel_bg,
                                )
                            }
                            Err(e) => Err(e),
                        };
                        match next_res {
                            Ok(mut next_page) => {
                                next_page
                                    .items
                                    .retain(|item| seen_urls.insert(item.result.video.url.clone()));
                                if next_page.items.is_empty() {
                                    break;
                                }
                                total_items += next_page.items.len();
                                next_cont = next_page
                                    .next_continuation
                                    .clone()
                                    .filter(|t| seen_tokens.insert(t.clone()));
                                let _ = tx.send(PlaylistDetailSyncMessage::Page {
                                    request_id,
                                    playlist_id: playlist_id.clone(),
                                    page_index,
                                    page: next_page,
                                });
                                ctx_bg.request_repaint();
                            }
                            Err(_) => break,
                        }
                    }

                    if !cancel_bg.cancelled() {
                        let _ = tx.send(PlaylistDetailSyncMessage::Finished {
                            request_id,
                            playlist_id,
                            auth_expired: false,
                            result: Ok(total_items),
                        });
                    }
                }
                Err(err) => {
                    let _ = tx.send(PlaylistDetailSyncMessage::Finished {
                        request_id,
                        playlist_id,
                        auth_expired: false,
                        result: Err(err.into_message()),
                    });
                }
            }
            ctx_bg.request_repaint();
            return;
        }

        if let Some(mut session) = web_session.take() {
            let mut first_res = fetch_playlist_detail_page_at(
                INNERTUBE_BASE_URL,
                &client,
                &mut session,
                &playlist_id,
                None,
                &hl,
                &cancel_bg,
            );
            if matches!(first_res, Err(InnerTubeError::AuthExpired(_)))
                && had_cached
                && !cancel_bg.cancelled()
            {
                let _ = crate::innertube::invalidate_web_session();
                if let Ok(mut refreshed) = acquire_web_session(None, allow_interactive, &cancel_bg)
                {
                    refreshed.refresh_client_config_if_needed(&client, &cancel_bg);
                    persist_web_session_dpapi(&refreshed);
                    session = refreshed;
                    let _ = tx.send(PlaylistDetailSyncMessage::SessionUpdated(session.clone()));
                    first_res = fetch_playlist_detail_page_at(
                        INNERTUBE_BASE_URL,
                        &client,
                        &mut session,
                        &playlist_id,
                        None,
                        &hl,
                        &cancel_bg,
                    );
                }
            }

            match first_res {
                Ok(first_page) => {
                    let mut next_cont = first_page.next_continuation.clone();
                    let mut seen_tokens = HashSet::new();
                    if let Some(tok) = &next_cont {
                        seen_tokens.insert(tok.clone());
                    }
                    let mut total_items = first_page.items.len();
                    let _ = tx.send(PlaylistDetailSyncMessage::Page {
                        request_id,
                        playlist_id: playlist_id.clone(),
                        page_index: 0,
                        page: first_page,
                    });
                    ctx_bg.request_repaint();

                    for page_index in 1..MAX_PLAYLIST_DETAIL_PAGES {
                        if cancel_bg.cancelled() || total_items >= MAX_PLAYLIST_DETAIL_VIDEOS {
                            break;
                        }
                        let Some(token) = next_cont.take() else {
                            break;
                        };
                        match fetch_playlist_detail_page_at(
                            INNERTUBE_BASE_URL,
                            &client,
                            &mut session,
                            &playlist_id,
                            Some(&token),
                            &hl,
                            &cancel_bg,
                        ) {
                            Ok(page) => {
                                total_items += page.items.len();
                                next_cont = page
                                    .next_continuation
                                    .clone()
                                    .filter(|t| seen_tokens.insert(t.clone()));
                                let _ = tx.send(PlaylistDetailSyncMessage::Page {
                                    request_id,
                                    playlist_id: playlist_id.clone(),
                                    page_index,
                                    page,
                                });
                                ctx_bg.request_repaint();
                            }
                            Err(_) => break,
                        }
                    }

                    if !cancel_bg.cancelled() {
                        let _ = tx.send(PlaylistDetailSyncMessage::Finished {
                            request_id,
                            playlist_id,
                            auth_expired: false,
                            result: Ok(total_items),
                        });
                        ctx_bg.request_repaint();
                    }
                    return;
                }
                Err(InnerTubeError::AuthExpired(msg)) => {
                    if playlist_id == "WL" {
                        let _ = tx.send(PlaylistDetailSyncMessage::Finished {
                            request_id,
                            playlist_id,
                            auth_expired: true,
                            result: Err(msg),
                        });
                        ctx_bg.request_repaint();
                        return;
                    }
                }
                Err(err) => {
                    if playlist_id == "WL" || oauth_token.is_none() {
                        let _ = tx.send(PlaylistDetailSyncMessage::Finished {
                            request_id,
                            playlist_id,
                            auth_expired: false,
                            result: Err(err.into_message()),
                        });
                        ctx_bg.request_repaint();
                        return;
                    }
                }
            }
        }

        if playlist_id == "WL" {
            let _ = tx.send(PlaylistDetailSyncMessage::Finished {
                request_id,
                playlist_id,
                auth_expired: true,
                result: Err(
                    "Entre na sua conta do YouTube para sincronizar Assistir mais tarde.".into(),
                ),
            });
            ctx_bg.request_repaint();
            return;
        }

        if let Some(token) = oauth_token.as_deref() {
            let mut page_idx = 0usize;
            let progress_tx = tx.clone();
            let progress_pl = playlist_id.clone();
            let progress_ctx = ctx_bg.clone();
            let outcome = if playlist_id == "LL" {
                oauth::liked_videos_feed_progressive(token, &cancel_bg, move |prog| {
                    let page = RemotePlaylistDetailPage {
                        meta: None,
                        items: prog
                            .batch
                            .into_iter()
                            .map(|res| RemotePlaylistDetailItem {
                                result: res,
                                set_video_id: None,
                                index_text: None,
                            })
                            .collect(),
                        next_continuation: None,
                        signed_out: false,
                    };
                    let _ = progress_tx.send(PlaylistDetailSyncMessage::Page {
                        request_id,
                        playlist_id: progress_pl.clone(),
                        page_index: page_idx,
                        page,
                    });
                    page_idx += 1;
                    progress_ctx.request_repaint();
                })
            } else {
                oauth::playlist_videos_feed_progressive(
                    token,
                    &playlist_id,
                    &cancel_bg,
                    move |prog| {
                        let page = RemotePlaylistDetailPage {
                            meta: None,
                            items: prog
                                .batch
                                .into_iter()
                                .map(|res| RemotePlaylistDetailItem {
                                    result: res,
                                    set_video_id: None,
                                    index_text: None,
                                })
                                .collect(),
                            next_continuation: None,
                            signed_out: false,
                        };
                        let _ = progress_tx.send(PlaylistDetailSyncMessage::Page {
                            request_id,
                            playlist_id: progress_pl.clone(),
                            page_index: page_idx,
                            page,
                        });
                        page_idx += 1;
                        progress_ctx.request_repaint();
                    },
                )
            };
            match outcome {
                Ok(out) => {
                    let _ = tx.send(PlaylistDetailSyncMessage::Finished {
                        request_id,
                        playlist_id,
                        auth_expired: false,
                        result: Ok(out.items.len()),
                    });
                    ctx_bg.request_repaint();
                    return;
                }
                Err(err) => {
                    if playlist_id == "LL" || cancel_bg.cancelled() {
                        let _ = tx.send(PlaylistDetailSyncMessage::Finished {
                            request_id,
                            playlist_id,
                            auth_expired: false,
                            result: Err(err),
                        });
                        ctx_bg.request_repaint();
                        return;
                    }
                }
            }
        }

        match search::fetch_public_playlist_videos_with_cancel(&playlist_id, 200, &cancel_bg) {
            Ok(results) => {
                let count = results.len();
                let page = RemotePlaylistDetailPage {
                    meta: None,
                    items: results
                        .into_iter()
                        .map(|res| RemotePlaylistDetailItem {
                            result: res,
                            set_video_id: None,
                            index_text: None,
                        })
                        .collect(),
                    next_continuation: None,
                    signed_out: false,
                };
                let _ = tx.send(PlaylistDetailSyncMessage::Page {
                    request_id,
                    playlist_id: playlist_id.clone(),
                    page_index: 0,
                    page,
                });
                let _ = tx.send(PlaylistDetailSyncMessage::Finished {
                    request_id,
                    playlist_id,
                    auth_expired: false,
                    result: Ok(count),
                });
            }
            Err(err) => {
                let _ = tx.send(PlaylistDetailSyncMessage::Finished {
                    request_id,
                    playlist_id,
                    auth_expired: false,
                    result: Err(err),
                });
            }
        }
        ctx_bg.request_repaint();
    });

    (rx, cancel)
}

pub fn start_playlist_remove_video(
    playlist_id: String,
    video_id: String,
    set_video_id: Option<String>,
    cached_session: Option<YouTubeWebSession>,
    hl: String,
    ctx: &egui::Context,
) -> (Receiver<RemotePlaylistActionEvent>, Cancellation) {
    let (tx, rx) = mpsc::channel();
    let cancel = Cancellation::default();
    let cancel_bg = cancel.clone();
    let ctx_bg = ctx.clone();

    std::thread::spawn(move || {
        let client = match http_client() {
            Ok(c) => c,
            Err(err) => {
                let _ = tx.send(RemotePlaylistActionEvent {
                    web_session: cached_session,
                    auth_expired: false,
                    playlist_id,
                    video_id,
                    result: Err(err),
                });
                ctx_bg.request_repaint();
                return;
            }
        };
        let mut session = match acquire_web_session(cached_session, false, &cancel_bg) {
            Ok(s) => s,
            Err(err) => {
                let _ = tx.send(RemotePlaylistActionEvent {
                    web_session: None,
                    auth_expired: true,
                    playlist_id,
                    video_id,
                    result: Err(err),
                });
                ctx_bg.request_repaint();
                return;
            }
        };
        session.refresh_client_config_if_needed(&client, &cancel_bg);
        persist_web_session_dpapi(&session);
        match remove_video_from_playlist_at(
            INNERTUBE_BASE_URL,
            &client,
            &mut session,
            &playlist_id,
            &video_id,
            set_video_id.as_deref(),
            &hl,
            &cancel_bg,
        ) {
            Ok(()) => {
                persist_web_session_dpapi(&session);
                let _ = tx.send(RemotePlaylistActionEvent {
                    web_session: Some(session),
                    auth_expired: false,
                    playlist_id,
                    video_id,
                    result: Ok(()),
                });
            }
            Err(InnerTubeError::AuthExpired(err)) => {
                let _ = crate::innertube::invalidate_web_session();
                let _ = tx.send(RemotePlaylistActionEvent {
                    web_session: None,
                    auth_expired: true,
                    playlist_id,
                    video_id,
                    result: Err(err),
                });
            }
            Err(err) => {
                let _ = tx.send(RemotePlaylistActionEvent {
                    web_session: Some(session),
                    auth_expired: false,
                    playlist_id,
                    video_id,
                    result: Err(err.into_message()),
                });
            }
        }
        ctx_bg.request_repaint();
    });

    (rx, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn parses_fe_playlists_lockups_chips_sorts_continuation_and_signed_out() {
        let sample: Value = serde_json::from_str(
            r#"{
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "header": {
                                        "chipBarViewModel": {
                                            "chips": [
                                                { "chipViewModel": { "text": { "content": "Playlists" }, "onTap": { "innertubeCommand": { "browseEndpoint": { "browseId": "FEplaylist_aggregation", "params": "p_playlists" } } } } },
                                                { "chipViewModel": { "text": { "content": "Música" }, "onTap": { "innertubeCommand": { "browseEndpoint": { "browseId": "FEplaylist_aggregation", "params": "p_music" } } } } },
                                                { "chipViewModel": { "text": { "content": "Mixes" }, "onTap": { "innertubeCommand": { "browseEndpoint": { "browseId": "FEplaylist_aggregation", "params": "p_mixes" } } } } },
                                                { "chipViewModel": { "text": { "content": "Minhas" }, "onTap": { "innertubeCommand": { "browseEndpoint": { "browseId": "FEplaylist_aggregation", "params": "p_owned" } } } } },
                                                { "chipViewModel": { "text": { "content": "Salvas" }, "onTap": { "innertubeCommand": { "browseEndpoint": { "browseId": "FEplaylist_aggregation", "params": "p_saved" } } } } }
                                            ]
                                        }
                                    },
                                    "contents": [{
                                        "itemSectionRenderer": {
                                            "header": {
                                                "sortFilterSubMenuRenderer": {
                                                    "subMenuItems": [
                                                        { "title": "A-Z", "serviceEndpoint": { "continuationCommand": { "token": "sort_az" } } },
                                                        { "title": "Adicionadas recentemente", "serviceEndpoint": { "continuationCommand": { "token": "sort_recent" } } }
                                                    ]
                                                }
                                            },
                                            "contents": [
                                                {
                                                    "lockupViewModel": {
                                                        "contentId": "PLlilpeep001",
                                                        "contentType": "LOCKUP_CONTENT_TYPE_PLAYLIST",
                                                        "contentImage": {
                                                            "collectionThumbnailViewModel": {
                                                                "primaryThumbnail": {
                                                                    "thumbnailViewModel": {
                                                                        "image": { "sources": [{ "url": "https://i.ytimg.com/vi/abcDEF12345/hqdefault.jpg", "width": 480 }] },
                                                                        "overlays": [{
                                                                            "thumbnailOverlayBadgeViewModel": {
                                                                                "thumbnailBadges": [{ "thumbnailBadgeViewModel": { "text": "162 vídeos", "icon": { "sources": [{ "clientResource": { "imageName": "PLAYLISTS" } }] } } }]
                                                                            }
                                                                        }]
                                                                    }
                                                                }
                                                            }
                                                        },
                                                        "metadata": {
                                                            "lockupMetadataViewModel": {
                                                                "title": { "content": "ALL LIL PEEP SONGS" },
                                                                "metadata": {
                                                                    "contentMetadataViewModel": {
                                                                        "metadataRows": [
                                                                            { "metadataParts": [
                                                                                { "text": { "content": "@LILPEEPENERGY", "commandRuns": [{ "onTap": { "innertubeCommand": { "browseEndpoint": { "browseId": "UC1234567890123456789012", "canonicalBaseUrl": "/@LILPEEPENERGY" } } } }] } },
                                                                                { "text": { "content": "Playlist" } }
                                                                            ]},
                                                                            { "metadataParts": [{ "text": { "content": "Ver playlist completa" } }] }
                                                                        ]
                                                                    }
                                                                }
                                                            }
                                                        },
                                                        "rendererContext": { "commandContext": { "onTap": { "innertubeCommand": { "browseEndpoint": { "browseId": "VLPLlilpeep001" } } } } }
                                                    }
                                                },
                                                {
                                                    "lockupViewModel": {
                                                        "contentId": "WL",
                                                        "contentType": "LOCKUP_CONTENT_TYPE_PLAYLIST",
                                                        "contentImage": {
                                                            "collectionThumbnailViewModel": {
                                                                "primaryThumbnail": {
                                                                    "thumbnailViewModel": {
                                                                        "image": { "sources": [{ "url": "https://i.ytimg.com/vi/xyzXYZ12345/hqdefault.jpg", "width": 480 }] },
                                                                        "overlays": [{
                                                                            "thumbnailOverlayBadgeViewModel": {
                                                                                "thumbnailBadges": [{ "thumbnailBadgeViewModel": { "text": "33 vídeos" } }]
                                                                            }
                                                                        }]
                                                                    }
                                                                }
                                                            }
                                                        },
                                                        "metadata": {
                                                            "lockupMetadataViewModel": {
                                                                "title": { "content": "Assistir mais tarde" },
                                                                "metadata": {
                                                                    "contentMetadataViewModel": {
                                                                        "metadataRows": [
                                                                            { "metadataParts": [{ "text": { "content": "Particular" } }, { "text": { "content": "Playlist" } }] },
                                                                            { "metadataParts": [{ "text": { "content": "Ver playlist completa" } }] }
                                                                        ]
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                },
                                                {
                                                    "lockupViewModel": {
                                                        "contentId": "RDMMseed1234567",
                                                        "contentType": "LOCKUP_CONTENT_TYPE_MIX",
                                                        "contentImage": {
                                                            "collectionThumbnailViewModel": {
                                                                "primaryThumbnail": {
                                                                    "thumbnailViewModel": {
                                                                        "image": { "sources": [{ "url": "https://i.ytimg.com/vi/seed1234567/hqdefault.jpg", "width": 480 }] },
                                                                        "overlays": [{
                                                                            "thumbnailOverlayBadgeViewModel": {
                                                                                "thumbnailBadges": [{ "thumbnailBadgeViewModel": { "text": "Mix", "icon": { "sources": [{ "clientResource": { "imageName": "MIX" } }] } } }]
                                                                            }
                                                                        }]
                                                                    }
                                                                }
                                                            }
                                                        },
                                                        "metadata": {
                                                            "lockupMetadataViewModel": {
                                                                "title": { "content": "Mix de Passionfruit" },
                                                                "metadata": {
                                                                    "contentMetadataViewModel": {
                                                                        "metadataRows": [{ "metadataParts": [{ "text": { "content": "Drake, Jay Rock, Chris Browne outros" } }, { "text": { "content": "Playlist" } }] }]
                                                                    }
                                                                }
                                                            }
                                                        },
                                                        "rendererContext": { "commandContext": { "onTap": { "innertubeCommand": { "watchEndpoint": { "videoId": "seed1234567", "playlistId": "RDMMseed1234567" } } } } }
                                                    }
                                                },
                                                {
                                                    "continuationItemRenderer": {
                                                        "continuationEndpoint": { "continuationCommand": { "token": "next_pl_page" } }
                                                    }
                                                }
                                            ]
                                        }
                                    }]
                                }
                            }
                        }
                    }]
                }
            }
        }"#,
        )
        .unwrap();

        let page = parse_playlists_feed_response(&sample);
        assert!(!page.signed_out);
        assert_eq!(page.next_continuation.as_deref(), Some("next_pl_page"));
        assert_eq!(page.sort_options.len(), 2);
        assert_eq!(page.chips.len(), 5);
        assert_eq!(page.cards.len(), 3);

        let saved = &page.cards[0];
        assert_eq!(saved.id, "PLlilpeep001");
        assert_eq!(saved.title, "ALL LIL PEEP SONGS");
        assert_eq!(saved.video_count, Some(162));
        assert_eq!(saved.badge_text, "162 vídeos");
        assert!(saved.is_saved);
        assert!(!saved.is_owned);
        assert!(saved.has_full_playlist_link);
        assert!(saved.matches_filter(PlaylistCategoryFilter::Saved));
        assert!(saved.matches_filter(PlaylistCategoryFilter::Playlists));

        let wl = &page.cards[1];
        assert_eq!(wl.id, "WL");
        assert_eq!(wl.kind, PlaylistKind::WatchLater);
        assert_eq!(wl.privacy, PlaylistPrivacy::Private);
        assert_eq!(wl.video_count, Some(33));
        assert!(wl.is_owned);
        assert!(!wl.is_saved);
        assert!(wl.matches_filter(PlaylistCategoryFilter::Owned));

        let mix = &page.cards[2];
        assert_eq!(mix.id, "RDMMseed1234567");
        assert!(mix.is_mix);
        assert!(mix.is_music);
        assert_eq!(mix.seed_video_id.as_deref(), Some("seed1234567"));
        assert!(!mix.has_full_playlist_link);
        assert!(mix.matches_filter(PlaylistCategoryFilter::Mixes));
        assert!(mix.matches_filter(PlaylistCategoryFilter::Music));
        assert!(!mix.matches_filter(PlaylistCategoryFilter::Playlists));

        let signed_out: Value = serde_json::from_str(
            r#"{
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "backgroundPromoRenderer": {
                                            "title": { "simpleText": "Faça login para ver suas playlists" }
                                        }
                                    }]
                                }
                            }
                        }
                    }]
                }
            }
        }"#,
        )
        .unwrap();
        assert!(parse_playlists_feed_response(&signed_out).signed_out);
    }

    #[test]
    fn parses_playlist_detail_page_header_videos_shorts_and_watch_next_mix() {
        let detail_json: Value = serde_json::from_str(
            r#"{
            "header": {
                "pageHeaderRenderer": {
                    "content": {
                        "pageHeaderViewModel": {
                            "title": { "dynamicTextViewModel": { "text": { "content": "Assistir mais tarde" } } },
                            "metadata": {
                                "contentMetadataViewModel": {
                                    "metadataRows": [
                                        { "metadataParts": [{ "text": { "content": "nathan" } }, { "text": { "content": "Privado" } }] },
                                        { "metadataParts": [{ "text": { "content": "33 vídeos" } }] }
                                    ]
                                }
                            }
                        }
                    }
                }
            },
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "itemSectionRenderer": {
                                            "contents": [{
                                                "playlistVideoListRenderer": {
                                                    "contents": [
                                                        {
                                                            "playlistVideoRenderer": {
                                                                "videoId": "dQw4w9WgXcQ",
                                                                "title": { "runs": [{ "text": "faço R$10K/Mês com IA" }] },
                                                                "shortBylineText": { "runs": [{ "text": "rixardê", "navigationEndpoint": { "browseEndpoint": { "browseId": "UCuAXFkgsw1L7xaCfnd5JJOw" } } }] },
                                                                "lengthText": { "simpleText": "2:19:27" },
                                                                "thumbnail": { "thumbnails": [{ "url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg", "width": 480 }] },
                                                                "setVideoId": "SET_VID_1",
                                                                "index": { "simpleText": "1" },
                                                                "isPlayable": true
                                                            }
                                                        },
                                                        {
                                                            "playlistVideoRenderer": {
                                                                "videoId": "M7lc1UVf-VE",
                                                                "title": { "runs": [{ "text": "BG3 dublado" }] },
                                                                "shortBylineText": { "runs": [{ "text": "ALT+F4" }] },
                                                                "lengthText": { "simpleText": "0:43" },
                                                                "thumbnail": { "thumbnails": [{ "url": "https://i.ytimg.com/vi/M7lc1UVf-VE/hqdefault.jpg", "width": 480 }] },
                                                                "thumbnailOverlays": [{ "thumbnailOverlayTimeStatusRenderer": { "style": "SHORTS" } }],
                                                                "setVideoId": "SET_VID_2"
                                                            }
                                                        },
                                                        {
                                                            "playlistVideoRenderer": {
                                                                "videoId": "bad00000001",
                                                                "title": { "simpleText": "[Deleted video]" },
                                                                "isPlayable": false
                                                            }
                                                        },
                                                        {
                                                            "continuationItemRenderer": {
                                                                "continuationEndpoint": { "continuationCommand": { "token": "pl_cont_2" } }
                                                            }
                                                        }
                                                    ]
                                                }
                                            }]
                                        }
                                    }]
                                }
                            }
                        }
                    }]
                }
            }
        }"#,
        )
        .unwrap();

        let detail = parse_playlist_detail_response(&detail_json, "WL");
        let meta = detail.meta.expect("playlist meta");
        assert_eq!(meta.title, "Assistir mais tarde");
        assert_eq!(meta.owner, "nathan");
        assert_eq!(meta.privacy, PlaylistPrivacy::Private);
        assert_eq!(meta.total_videos, Some(33));
        assert_eq!(detail.items.len(), 2);
        assert_eq!(detail.items[0].set_video_id.as_deref(), Some("SET_VID_1"));
        assert_eq!(detail.items[0].result.video.duration, "2:19:27");
        assert_eq!(
            detail.items[1].result.metadata.kind,
            SearchResultKind::Short
        );
        assert_eq!(detail.next_continuation.as_deref(), Some("pl_cont_2"));

        let next_json: Value = serde_json::from_str(
            r#"{
            "contents": {
                "twoColumnWatchNextResults": {
                    "playlist": {
                        "playlist": {
                            "title": "Mix de Passionfruit",
                            "playlistId": "RDMMseed1234567",
                            "ownerName": { "simpleText": "YouTube" },
                            "isInfinite": true,
                            "contents": [
                                {
                                    "playlistPanelVideoRenderer": {
                                        "videoId": "seed1234567",
                                        "title": { "simpleText": "Passionfruit" },
                                        "longBylineText": { "runs": [{ "text": "Drake" }] },
                                        "lengthText": { "simpleText": "4:58" },
                                        "thumbnail": { "thumbnails": [{ "url": "https://i.ytimg.com/vi/seed1234567/hqdefault.jpg", "width": 480 }] }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        }"#,
        )
        .unwrap();
        let mix_page = parse_watch_next_playlist_response(&next_json, "RDMMseed1234567");
        assert!(mix_page.meta.as_ref().unwrap().is_mix);
        assert_eq!(mix_page.items.len(), 1);
        assert_eq!(mix_page.items[0].result.video.title, "Passionfruit");
        assert_eq!(mix_page.items[0].result.video.channel, "Drake");
    }

    #[test]
    fn remove_video_from_playlist_posts_expected_innertube_payload() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let body = r#"{"status":"STATUS_SUCCEEDED"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).unwrap();
            req
        });
        let client = crate::http_client::shared();
        let mut session = YouTubeWebSession::test_session();
        let cancel = Cancellation::default();
        let base_url = format!("http://127.0.0.1:{port}/youtubei/v1");
        remove_video_from_playlist_at(
            &base_url,
            client,
            &mut session,
            "WL",
            "dQw4w9WgXcQ",
            Some("SET_VID_1"),
            "pt",
            &cancel,
        )
        .unwrap();
        let req = handle.join().unwrap();
        assert!(req.contains("/youtubei/v1/browse/edit_playlist"));
        assert!(req.contains("ACTION_REMOVE_VIDEO"));
        assert!(req.contains("SET_VID_1"));
    }

    #[test]
    fn dpapi_web_session_cache_reused_without_headless_chromium_and_invalidated_on_auth_expiry() {
        use std::cell::Cell;

        let temp_dir = std::env::temp_dir().join(format!(
            "fasttube-playlist-dpapi-test-{}-{}",
            std::process::id(),
            unix_now_secs()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let session_path = temp_dir.join("web-session.dpapi");
        let now_unix = 1_750_000_000;

        let extractor_calls = Cell::new(0usize);
        let sample_cookies = || {
            vec![
                json!({
                    "name": "SAPISID",
                    "value": "sapisid_dpapi_playlist_token",
                    "domain": ".youtube.com",
                    "expires": (now_unix + 86_400) as f64
                }),
                json!({
                    "name": "SID",
                    "value": "sid_dpapi_playlist_token",
                    "domain": ".youtube.com",
                    "expires": (now_unix + 86_400) as f64
                }),
                json!({
                    "name": "__Secure-3PAPISID",
                    "value": "p3_dpapi_playlist_token",
                    "domain": ".youtube.com",
                    "expires": (now_unix + 86_400) as f64
                }),
            ]
        };

        // 1. Cold start: no DPAPI file on disk -> invokes cookie extractor once and saves DPAPI file.
        let first =
            acquire_web_session_with_dpapi_cache_at(None, Some(&session_path), now_unix, || {
                extractor_calls.set(extractor_calls.get() + 1);
                Ok(sample_cookies())
            })
            .expect("cold extraction should succeed");
        assert_eq!(extractor_calls.get(), 1);
        assert!(session_path.exists());
        assert!(
            first
                .cookie_header()
                .contains("SAPISID=sapisid_dpapi_playlist_token")
        );

        // Verify SAPISIDHASH header uses crate::innertube::sapisidhash_header.
        let auth_hdr =
            build_innertube_authorization_header(&first, YOUTUBE_ORIGIN, now_unix).unwrap();
        let expected_sapisid = crate::innertube::sapisidhash_header(
            "sapisid_dpapi_playlist_token",
            YOUTUBE_ORIGIN,
            now_unix,
        );
        assert!(auth_hdr.starts_with(&expected_sapisid));

        // 2. Warm start: valid non-expired DPAPI session on disk -> zero headless browser launch!
        let warm = acquire_web_session_with_dpapi_cache_at(
            None,
            Some(&session_path),
            now_unix + 60,
            || {
                panic!(
                    "headless Chromium extractor must not run when valid DPAPI session is cached"
                );
            },
        )
        .expect("warm DPAPI session should load");
        assert_eq!(extractor_calls.get(), 1);
        assert_eq!(warm.cookie_header(), first.cookie_header());

        // 3. On HTTP 401/403 auth failure, invalidate_web_session removes the DPAPI file.
        crate::innertube::invalidate_web_session_at(&session_path).unwrap();
        assert!(!session_path.exists());

        // Next acquisition extracts fresh cookies and re-persists.
        let after_invalidation = acquire_web_session_with_dpapi_cache_at(
            None,
            Some(&session_path),
            now_unix + 120,
            || {
                extractor_calls.set(extractor_calls.get() + 1);
                Ok(sample_cookies())
            },
        )
        .expect("re-extraction after invalidation should succeed");
        assert_eq!(extractor_calls.get(), 2);
        assert!(session_path.exists());
        assert_eq!(after_invalidation.cookie_header(), first.cookie_header());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn add_video_to_playlist_innertube_wires_action_add_video_payload() {
        let payload =
            crate::innertube::build_playlist_add_video_payload("WL", "dQw4w9WgXcQ").unwrap();
        assert_eq!(payload["playlistId"], "WL");
        assert_eq!(payload["actions"][0]["action"], "ACTION_ADD_VIDEO");
        assert_eq!(payload["actions"][0]["addedVideoId"], "dQw4w9WgXcQ");

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let body = r#"{"status":"STATUS_SUCCEEDED"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).unwrap();
            req
        });

        let client = crate::http_client::shared();
        let mut session = YouTubeWebSession::test_session();
        let cancel = Cancellation::default();
        let base_url = format!("http://127.0.0.1:{port}/youtubei/v1");
        add_video_to_playlist_at(
            &base_url,
            client,
            &mut session,
            "WL",
            "dQw4w9WgXcQ",
            "pt",
            &cancel,
        )
        .unwrap();

        let req = handle.join().unwrap();
        assert!(req.contains("/youtubei/v1/browse/edit_playlist"));
        assert!(req.contains("ACTION_ADD_VIDEO"));
        assert!(req.contains("dQw4w9WgXcQ"));
        assert!(req.contains("\"playlistId\":\"WL\""));
    }

    #[test]
    fn parses_mix_explicit_continuations_and_synthesizes_seed_token_for_infinite_rd_mixes() {
        let radio_fixture = json!({
            "contents": {
                "twoColumnWatchNextResults": {
                    "playlist": {
                        "playlist": {
                            "playlistId": "RDdQw4w9WgXcQ",
                            "title": "Mix - Rust Beats",
                            "isInfinite": true,
                            "contents": [
                                {
                                    "playlistPanelVideoRenderer": {
                                        "videoId": "dQw4w9WgXcQ",
                                        "title": { "simpleText": "Track 1" },
                                        "shortBylineText": { "runs": [{ "text": "Artist 1" }] },
                                        "lengthText": { "simpleText": "3:33" }
                                    }
                                },
                                {
                                    "playlistPanelVideoRenderer": {
                                        "videoId": "9bZkp7q19f0",
                                        "title": { "simpleText": "Track 2" },
                                        "shortBylineText": { "runs": [{ "text": "Artist 2" }] },
                                        "lengthText": { "simpleText": "4:12" }
                                    }
                                }
                            ],
                            "continuations": [
                                { "nextRadioContinuationData": { "continuation": "RD_RADIO_CONT_2" } }
                            ]
                        }
                    }
                }
            }
        });

        let page_explicit = parse_watch_next_playlist_response(&radio_fixture, "RDdQw4w9WgXcQ");
        assert_eq!(page_explicit.items.len(), 2);
        assert_eq!(
            page_explicit.next_continuation.as_deref(),
            Some("RD_RADIO_CONT_2")
        );

        let seedless_mix = json!({
            "contents": {
                "twoColumnWatchNextResults": {
                    "playlist": {
                        "playlist": {
                            "playlistId": "RDdQw4w9WgXcQ",
                            "title": "Mix - Endless",
                            "isInfinite": true,
                            "contents": [
                                {
                                    "playlistPanelVideoRenderer": {
                                        "videoId": "dQw4w9WgXcQ",
                                        "title": { "simpleText": "Track 1" },
                                        "shortBylineText": { "runs": [{ "text": "Artist 1" }] }
                                    }
                                },
                                {
                                    "playlistPanelVideoRenderer": {
                                        "videoId": "9bZkp7q19f0",
                                        "title": { "simpleText": "Track 2" },
                                        "shortBylineText": { "runs": [{ "text": "Artist 2" }] }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        });

        let page_seed = parse_watch_next_playlist_response(&seedless_mix, "RDdQw4w9WgXcQ");
        assert_eq!(
            page_seed.next_continuation.as_deref(),
            Some("mix_seed:9bZkp7q19f0")
        );
        assert_eq!(
            parse_mix_seed_continuation(page_seed.next_continuation.as_deref().unwrap()),
            Some("9bZkp7q19f0")
        );

        let cont_page_fixture = json!({
            "continuationContents": {
                "playlistPanelContinuation": {
                    "contents": [
                        {
                            "playlistPanelVideoRenderer": {
                                "videoId": "kJQP7kiw5Fk",
                                "title": { "simpleText": "Track 3" },
                                "shortBylineText": { "runs": [{ "text": "Artist 3" }] }
                            }
                        }
                    ],
                    "continuations": [
                        { "nextContinuationData": { "continuation": "RD_CONT_3" } }
                    ]
                }
            }
        });
        let page_cont = parse_watch_next_playlist_response(&cont_page_fixture, "RDdQw4w9WgXcQ");
        assert_eq!(page_cont.items.len(), 1);
        assert_eq!(page_cont.items[0].result.video.title, "Track 3");
        assert_eq!(page_cont.next_continuation.as_deref(), Some("RD_CONT_3"));
    }

    #[test]
    fn fetch_watch_next_playlist_page_sends_seed_video_and_continuation_tokens() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let mut captured = Vec::new();
            for (idx, body) in [
                json!({
                    "contents": {
                        "twoColumnWatchNextResults": {
                            "playlist": {
                                "playlist": {
                                    "playlistId": "RDdQw4w9WgXcQ",
                                    "isInfinite": true,
                                    "contents": [{
                                        "playlistPanelVideoRenderer": {
                                            "videoId": "kJQP7kiw5Fk",
                                            "title": { "simpleText": "Seeded Track 3" }
                                        }
                                    }]
                                }
                            }
                        }
                    }
                })
                .to_string(),
                json!({
                    "continuationContents": {
                        "playlistPanelContinuation": {
                            "contents": [{
                                "playlistPanelVideoRenderer": {
                                    "videoId": "M7lc1UVf-VE",
                                    "title": { "simpleText": "Continuation Track 4" }
                                }
                            }]
                        }
                    }
                })
                .to_string(),
            ]
            .into_iter()
            .enumerate()
            {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                captured.push((idx, req));
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(resp.as_bytes()).unwrap();
            }
            captured
        });

        let client = crate::http_client::shared();
        let cancel = Cancellation::default();
        let base_url = format!("http://127.0.0.1:{port}/youtubei/v1");

        let seeded = fetch_watch_next_playlist_page_at(
            &base_url,
            client,
            None,
            None,
            "RDdQw4w9WgXcQ",
            Some("mix_seed:9bZkp7q19f0"),
            "pt",
            &cancel,
        )
        .unwrap();
        assert_eq!(seeded.items.len(), 1);
        assert_eq!(seeded.items[0].result.video.title, "Seeded Track 3");

        let cont = fetch_watch_next_playlist_page_at(
            &base_url,
            client,
            None,
            None,
            "RDdQw4w9WgXcQ",
            Some("RD_RADIO_CONT_2"),
            "pt",
            &cancel,
        )
        .unwrap();
        assert_eq!(cont.items.len(), 1);
        assert_eq!(cont.items[0].result.video.title, "Continuation Track 4");

        let reqs = handle.join().unwrap();
        assert!(reqs[0].1.contains("\"playlistId\":\"RDdQw4w9WgXcQ\""));
        assert!(reqs[0].1.contains("\"videoId\":\"9bZkp7q19f0\""));
        assert!(reqs[1].1.contains("\"continuation\":\"RD_RADIO_CONT_2\""));
    }

    #[test]
    fn playlist_privacy_innertube_mapping_and_mutation_validation() {
        assert_eq!(PlaylistPrivacy::Public.as_innertube_str(), Some("PUBLIC"));
        assert_eq!(
            PlaylistPrivacy::Unlisted.as_innertube_str(),
            Some("UNLISTED")
        );
        assert_eq!(PlaylistPrivacy::Private.as_innertube_str(), Some("PRIVATE"));
        assert_eq!(PlaylistPrivacy::Unknown.as_innertube_str(), None);

        let cancel = Cancellation::default();
        let session = YouTubeWebSession::test_session();
        assert!(
            set_playlist_privacy_innertube(
                "PLtest1234",
                PlaylistPrivacy::Unknown,
                &session,
                &cancel
            )
            .is_err()
        );
        assert!(rename_playlist_innertube("PLtest1234", "   ", &session, &cancel).is_err());
        assert!(delete_playlist_innertube("WL", &session, &cancel).is_err());
        assert!(
            move_playlist_video_innertube("PLtest1234", "SET_A", Some("SET_A"), &session, &cancel)
                .is_err()
        );
    }

    #[test]
    fn parse_playlist_detail_extracts_dates_from_video_info_and_lockup_items() {
        let json = serde_json::json!({
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "itemSectionRenderer": {
                                            "contents": [{
                                                "playlistVideoListRenderer": {
                                                    "contents": [
                                                        {
                                                            "playlistVideoRenderer": {
                                                                "videoId": "dQw4w9WgXcQ",
                                                                "title": { "simpleText": "Playlist Video Bullet" },
                                                                "shortBylineText": { "runs": [{ "text": "Canal A" }] },
                                                                "lengthText": { "simpleText": "4:00" },
                                                                "videoInfo": { "runs": [{ "text": "12\u{00a0}mil visualizações • há\u{00a0}7\u{00a0}meses" }] }
                                                            }
                                                        },
                                                        {
                                                            "lockupViewModel": {
                                                                "contentId": "AbxnXTCqdBc",
                                                                "contentType": "LOCKUP_CONTENT_TYPE_VIDEO",
                                                                "metadata": {
                                                                    "lockupMetadataViewModel": {
                                                                        "title": { "content": "Playlist Lockup Video" },
                                                                        "metadata": {
                                                                            "contentMetadataViewModel": {
                                                                                "metadataRows": [{
                                                                                    "metadataParts": [
                                                                                        { "text": { "content": "Canal B" } },
                                                                                        { "text": { "content": "50\u{00a0}mil" }, "accessibilityLabel": "50 mil visualizações", "leadingIcon": { "name": "PLAY_ARROW_OUTLINED" } },
                                                                                        { "text": { "content": "Estreou há\u{00a0}3\u{00a0}dias" } }
                                                                                    ]
                                                                                }]
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    ]
                                                }
                                            }]
                                        }
                                    }]
                                }
                            }
                        }
                    }]
                }
            }
        });
        let detail = parse_playlist_detail_response(&json, "PLtest");
        assert_eq!(detail.items.len(), 2);
        assert_eq!(detail.items[0].result.metadata.view_count, Some(12_000));
        assert_eq!(
            detail.items[0]
                .result
                .metadata
                .published_time_text
                .as_deref(),
            Some("há 7 meses")
        );
        assert_eq!(detail.items[1].result.video.channel, "Canal B");
        assert_eq!(detail.items[1].result.metadata.view_count, Some(50_000));
        assert_eq!(
            detail.items[1]
                .result
                .metadata
                .published_time_text
                .as_deref(),
            Some("Estreou há 3 dias")
        );
    }

    #[test]
    fn fetch_playlists_feed_page_posts_fe_playlist_aggregation_browse_id() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut buf = Vec::new();
            let mut chunk = [0u8; 2048];
            let mut expected_total = None;
            loop {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                if expected_total.is_none()
                    && let Some(hdr_end) = buf.windows(4).position(|w| w == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&buf[..hdr_end]);
                    let content_len = headers
                        .lines()
                        .find_map(|line| {
                            let (k, v) = line.split_once(':')?;
                            k.trim()
                                .eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    expected_total = Some(hdr_end + 4 + content_len);
                }
                if let Some(total) = expected_total
                    && buf.len() >= total
                {
                    break;
                }
            }
            let req = String::from_utf8_lossy(&buf).to_string();
            let body = r#"{"contents":{"twoColumnBrowseResultsRenderer":{"tabs":[]}}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).unwrap();
            req
        });

        let client = crate::http_client::shared();
        let mut session = YouTubeWebSession::test_session();
        let cancel = Cancellation::default();
        let base_url = format!("http://127.0.0.1:{port}/youtubei/v1");
        let page = fetch_playlists_feed_page_at(
            &base_url,
            client,
            &mut session,
            None,
            None,
            "pt",
            &cancel,
        )
        .unwrap();
        assert!(page.cards.is_empty());

        let req = handle.join().unwrap();
        assert!(
            req.contains("POST /youtubei/v1/browse"),
            "expected browse endpoint request, got:\n{req}"
        );
        assert!(
            req.contains("\"browseId\":\"FEplaylist_aggregation\""),
            "expected FEplaylist_aggregation browseId in payload, got:\n{req}"
        );
        assert!(req.contains("\"hl\":\"pt\""));
    }

    #[test]
    fn parses_fe_playlist_aggregation_stack_colors_dropdown_sort_endpoints_and_compound_metadata() {
        let sample = json!({
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "subMenu": {
                                        "sortFilterSubMenuRenderer": {
                                            "subMenuItems": []
                                        }
                                    },
                                    "contents": [{
                                        "itemSectionRenderer": {
                                            "header": {
                                                "chipBarViewModel": {
                                                    "chips": [
                                                        {
                                                            "chipViewModel": {
                                                                "text": { "content": "Adicionadas recentemente" },
                                                                "onTap": {
                                                                    "innertubeCommand": {
                                                                        "showSheetCommand": {
                                                                            "panelLoadingStrategy": {
                                                                                "inlineContent": {
                                                                                    "sheetViewModel": {
                                                                                        "content": {
                                                                                            "listViewModel": {
                                                                                                "listItems": [
                                                                                                    {
                                                                                                        "listItemViewModel": {
                                                                                                            "title": { "content": "A-Z" },
                                                                                                            "trailingImage": {
                                                                                                                "sources": [{ "clientResource": { "imageName": "CHECK" } }]
                                                                                                            },
                                                                                                            "rendererContext": {
                                                                                                                "commandContext": {
                                                                                                                    "onTap": {
                                                                                                                        "innertubeCommand": {
                                                                                                                            "continuationCommand": { "token": "sort_az_sheet_tok" }
                                                                                                                        }
                                                                                                                    }
                                                                                                                }
                                                                                                            }
                                                                                                        }
                                                                                                    }
                                                                                                ]
                                                                                            }
                                                                                        }
                                                                                    }
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    ]
                                                },
                                                "dropdownRenderer": {
                                                    "entries": [
                                                        {
                                                            "dropdownItemRenderer": {
                                                                "label": "Adicionadas recentemente",
                                                                "isSelected": false,
                                                                "onSelectCommand": {
                                                                    "continuationCommand": { "token": "sort_recent_tok" }
                                                                }
                                                            }
                                                        }
                                                    ]
                                                }
                                            },
                                            "contents": [{
                                                "gridRenderer": {
                                                    "items": [
                                                        {
                                                            "lockupViewModel": {
                                                                "contentId": "WL",
                                                                "contentType": "LOCKUP_CONTENT_TYPE_PLAYLIST",
                                                                "contentImage": {
                                                                    "collectionThumbnailViewModel": {
                                                                        "stackColor": {
                                                                            "darkTheme": 4283054898u32,
                                                                            "lightTheme": 4292399048u32
                                                                        },
                                                                        "primaryThumbnail": {
                                                                            "thumbnailViewModel": {
                                                                                "image": {
                                                                                    "sources": [{ "url": "https://i.ytimg.com/vi/wl123456789/hqdefault.jpg", "width": 480 }]
                                                                                },
                                                                                "overlays": [{
                                                                                    "thumbnailOverlayBadgeViewModel": {
                                                                                        "thumbnailBadges": [{ "thumbnailBadgeViewModel": { "text": "33 vídeos" } }]
                                                                                    }
                                                                                }]
                                                                            }
                                                                        }
                                                                    }
                                                                },
                                                                "metadata": {
                                                                    "lockupMetadataViewModel": {
                                                                        "title": { "content": "Assistir mais tarde" },
                                                                        "metadata": {
                                                                            "contentMetadataViewModel": {
                                                                                "metadataRows": [{
                                                                                    "metadataParts": [{
                                                                                        "text": { "content": "Particular · Playlist" }
                                                                                    }]
                                                                                }]
                                                                            }
                                                                        }
                                                                    }
                                                                },
                                                                "rendererContext": {
                                                                    "commandContext": {
                                                                        "onTap": {
                                                                            "innertubeCommand": {
                                                                                "browseEndpoint": { "browseId": "VLWL" }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        },
                                                        {
                                                            "lockupViewModel": {
                                                                "contentId": "PLlilpeep001",
                                                                "contentType": "LOCKUP_CONTENT_TYPE_PLAYLIST",
                                                                "contentImage": {
                                                                    "collectionThumbnailViewModel": {
                                                                        "stackColor": {
                                                                            "darkTheme": 4281548870u32,
                                                                            "lightTheme": 4291613148u32
                                                                        },
                                                                        "primaryThumbnail": {
                                                                            "thumbnailViewModel": {
                                                                                "image": {
                                                                                    "sources": [{ "url": "https://i.ytimg.com/vi/abc11111111/hqdefault.jpg", "width": 480 }]
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                },
                                                                "metadata": {
                                                                    "lockupMetadataViewModel": {
                                                                        "title": { "content": "ALL LIL PEEP SONGS" },
                                                                        "metadata": {
                                                                            "contentMetadataViewModel": {
                                                                                "metadataRows": [{
                                                                                    "metadataParts": [{
                                                                                        "text": {
                                                                                            "content": "@LILPEEPENERGY · Playlist",
                                                                                            "commandRuns": [{
                                                                                                "onTap": {
                                                                                                    "innertubeCommand": {
                                                                                                        "browseEndpoint": { "browseId": "UCexternal123" }
                                                                                                    }
                                                                                                }
                                                                                            }]
                                                                                        }
                                                                                    }]
                                                                                }]
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    ]
                                                }
                                            }]
                                        }
                                    }]
                                }
                            }
                        }
                    }]
                }
            },
            "onResponseReceivedEndpoints": [{
                "reloadContinuationItemsCommand": {
                    "continuationItems": [{
                        "lockupViewModel": {
                            "contentId": "PLcont999",
                            "contentType": "LOCKUP_CONTENT_TYPE_PLAYLIST",
                            "metadata": {
                                "lockupMetadataViewModel": {
                                    "title": { "content": "Saved Via Endpoint" },
                                    "metadata": {
                                        "contentMetadataViewModel": {
                                            "metadataRows": [{
                                                "metadataParts": [{
                                                    "text": { "content": "@EndpointChannel · Playlist" }
                                                }]
                                            }]
                                        }
                                    }
                                }
                            },
                            "rendererContext": {
                                "commandContext": {
                                    "onTap": {
                                        "innertubeCommand": {
                                            "browseEndpoint": { "browseId": "VLPLcont999" }
                                        }
                                    }
                                }
                            }
                        }
                    }]
                }
            }]
        });

        let page = parse_playlists_feed_response(&sample);
        assert_eq!(page.cards.len(), 3);
        assert_eq!(page.cards[0].id, "WL");
        assert_eq!(page.cards[0].stack_color_dark, Some(0xFF4A_3B32));
        assert_eq!(page.cards[0].stack_color_light, Some(0xFFD8_CFC8));
        assert_eq!(page.cards[0].owner_label, "Particular");
        assert_eq!(page.cards[0].subtitle_line, "Particular · Playlist");

        assert_eq!(page.cards[1].id, "PLlilpeep001");
        assert!(page.cards[1].is_saved);
        assert_eq!(page.cards[1].owner_label, "@LILPEEPENERGY");
        assert_eq!(page.cards[1].subtitle_line, "@LILPEEPENERGY · Playlist");

        assert_eq!(page.cards[2].id, "PLcont999");
        assert_eq!(page.cards[2].title, "Saved Via Endpoint");
        assert_eq!(page.cards[2].subtitle_line, "@EndpointChannel · Playlist");

        assert_eq!(page.selected_sort, Some(PlaylistSort::AlphabeticalAZ));
        assert!(page.sort_options.iter().any(|opt| {
            opt.sort == PlaylistSort::RecentlyAdded
                && opt.action == PlaylistChipAction::Continuation("sort_recent_tok".into())
        }));
        assert!(page.sort_options.iter().any(|opt| {
            opt.sort == PlaylistSort::AlphabeticalAZ
                && opt.action == PlaylistChipAction::Continuation("sort_az_sheet_tok".into())
        }));
    }
}
