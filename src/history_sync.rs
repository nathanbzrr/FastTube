use crate::comment_actions::{
    FALLBACK_CLIENT_VERSION, INNERTUBE_BASE_URL, InnerTubeError, WEB_USER_AGENT, YOUTUBE_ORIGIN,
    YouTubeWebSession, http_client, parse_innertube_error_json,
};
use crate::history::{self, HistoryFilter};
use crate::process::Cancellation;
use crate::search::{
    SearchResultKind, VideoMetadata, channel_url_from_browse_endpoint, clean_pt_br_view_label,
    clean_published_time_text, has_verified_owner_badge, parse_lockup_metadata_rows,
    parse_pt_br_compact_count, select_yt_thumbnail, yt_text,
};
use eframe::egui;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Maximum pages eagerly fetched on initial history sync. Remaining pages stay
/// reachable via `next_continuation` for on-demand load-more pagination.
pub const INITIAL_HISTORY_SYNC_PAGES: usize = 3;
/// Maximum remote history items retained per filter feed in memory.
#[allow(dead_code)]
pub const MAX_REMOTE_HISTORY_FEED_ITEMS: usize = 600;
/// Maximum distinct `HistoryFilter` feeds kept warm simultaneously in memory.
#[allow(dead_code)]
pub const MAX_REMOTE_HISTORY_CACHED_FEEDS: usize = 3;
const CPN_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Bounded LRU tracker for `history.remote.feeds` so switching tabs does not
/// accumulate every category feed indefinitely in RAM.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct RemoteHistoryFeedLru {
    order: VecDeque<HistoryFilter>,
    max_feeds: usize,
}

impl Default for RemoteHistoryFeedLru {
    fn default() -> Self {
        Self::new(MAX_REMOTE_HISTORY_CACHED_FEEDS)
    }
}

#[allow(dead_code)]
impl RemoteHistoryFeedLru {
    pub fn new(max_feeds: usize) -> Self {
        Self {
            order: VecDeque::new(),
            max_feeds: max_feeds.max(1),
        }
    }

    pub fn max_feeds(&self) -> usize {
        self.max_feeds
    }

    /// Marks `filter` as most recently used and evicts the least recently used
    /// entries from `feeds` when `feeds.len() > max_feeds`.
    pub fn touch<V>(
        &mut self,
        filter: HistoryFilter,
        feeds: &mut HashMap<HistoryFilter, V>,
    ) -> Vec<HistoryFilter> {
        self.order
            .retain(|existing| *existing != filter && feeds.contains_key(existing));
        if feeds.contains_key(&filter) {
            self.order.push_back(filter);
        }
        // Track any pre-existing feeds inserted without calling `touch`.
        if self.order.len() < feeds.len() {
            let mut missing: Vec<HistoryFilter> = feeds
                .keys()
                .copied()
                .filter(|key| *key != filter && !self.order.contains(key))
                .collect();
            missing.sort_by_key(|f| *f as u8);
            for key in missing {
                self.order.push_front(key);
            }
        }
        let mut evicted = Vec::new();
        while feeds.len() > self.max_feeds {
            let victim_pos = self
                .order
                .iter()
                .position(|&candidate| {
                    candidate != filter && (self.max_feeds == 1 || candidate != HistoryFilter::All)
                })
                .or_else(|| self.order.iter().position(|&candidate| candidate != filter));
            let Some(pos) = victim_pos else {
                break;
            };
            if let Some(victim) = self.order.remove(pos)
                && feeds.remove(&victim).is_some()
            {
                evicted.push(victim);
            }
        }
        evicted
    }

    /// Evicts all feeds other than `keep_filter` (e.g. when leaving the History view).
    pub fn evict_except<V>(
        &mut self,
        keep_filter: HistoryFilter,
        feeds: &mut HashMap<HistoryFilter, V>,
    ) -> Vec<HistoryFilter> {
        let mut evicted = Vec::new();
        feeds.retain(|&filter, _| {
            if filter == keep_filter {
                true
            } else {
                evicted.push(filter);
                false
            }
        });
        self.order
            .retain(|&filter| filter == keep_filter && feeds.contains_key(&filter));
        evicted
    }

    pub fn remove(&mut self, filter: HistoryFilter) {
        self.order.retain(|&existing| existing != filter);
    }

    pub fn clear(&mut self) {
        self.order.clear();
    }
}

/// Trims trailing oldest items from `sections` so total items do not exceed
/// `max_items`, keeping `seen_ids` synchronized and dropping empty sections.
#[allow(dead_code)]
pub fn enforce_sections_item_cap(
    sections: &mut Vec<RemoteHistorySection>,
    seen_ids: &mut HashSet<String>,
    max_items: usize,
) -> usize {
    let mut kept = 0usize;
    let mut removed = 0usize;
    let mut keep_sections = Vec::with_capacity(sections.len());
    for mut section in sections.drain(..) {
        if kept >= max_items {
            for item in section.items {
                seen_ids.remove(&item.video_id);
                removed += 1;
            }
            continue;
        }
        let remaining = max_items - kept;
        if section.items.len() > remaining {
            for item in section.items.drain(remaining..) {
                seen_ids.remove(&item.video_id);
                removed += 1;
            }
        }
        if !section.items.is_empty() {
            kept += section.items.len();
            keep_sections.push(section);
        }
    }
    *sections = keep_sections;
    removed
}

/// Caps an incoming page so `existing_count + page_items <= max_items`.
/// Returns `true` if the cap was reached (truncating excess items and clearing
/// `next_continuation` so callers stop paginating past the memory bound).
#[allow(dead_code)]
pub fn cap_remote_history_page(
    page: &mut RemoteHistoryPage,
    existing_count: usize,
    max_items: usize,
) -> bool {
    let budget = max_items.saturating_sub(existing_count);
    let mut kept = 0usize;
    let mut truncated = existing_count >= max_items;
    let mut keep_sections = Vec::with_capacity(page.sections.len());
    for mut section in page.sections.drain(..) {
        if kept >= budget {
            if !section.items.is_empty() {
                truncated = true;
            }
            continue;
        }
        let remaining = budget - kept;
        if section.items.len() > remaining {
            section.items.truncate(remaining);
            truncated = true;
        }
        if !section.items.is_empty() {
            kept += section.items.len();
            keep_sections.push(section);
        }
    }
    page.sections = keep_sections;
    if truncated || existing_count.saturating_add(kept) >= max_items {
        page.next_continuation = None;
        return true;
    }
    false
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryChipAction {
    BrowseParams(String),
    Continuation(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteHistoryItem {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub channel_id: Option<String>,
    pub channel_url: Option<String>,
    pub channel_verified: bool,
    pub thumbnail_url: String,
    pub duration_text: String,
    pub duration_ms: Option<u64>,
    pub position_ms: u64,
    pub view_count: Option<u64>,
    pub view_count_text: Option<String>,
    pub published_time_text: Option<String>,
    pub is_short: bool,
    pub is_music: bool,
    pub is_podcast: bool,
    pub is_live: bool,
    pub feedback_token: Option<String>,
    pub date_label: String,
}

impl RemoteHistoryItem {
    pub fn to_history_entry(
        &self,
        fallback_watched_at_ms: Option<u64>,
        authoritative_filter: HistoryFilter,
    ) -> history::Entry {
        let is_short = if authoritative_filter == HistoryFilter::Shorts || self.is_short {
            Some(true)
        } else if authoritative_filter == HistoryFilter::Videos {
            Some(false)
        } else {
            None
        };
        let is_podcast = if authoritative_filter == HistoryFilter::Podcasts || self.is_podcast {
            Some(true)
        } else {
            None
        };
        let is_music = if authoritative_filter == HistoryFilter::Music || self.is_music {
            Some(true)
        } else {
            None
        };
        let duration_ms = self.duration_ms.filter(|&d| d > 0);
        let position_ms = duration_ms
            .map(|d| self.position_ms.min(d))
            .unwrap_or(self.position_ms);
        let title = if self.title.trim().is_empty() {
            self.video_id.clone()
        } else {
            self.title.clone()
        };
        let date_label = (!self.date_label.trim().is_empty()).then(|| self.date_label.clone());
        let thumbnail_url =
            (!self.thumbnail_url.trim().is_empty()).then(|| self.thumbnail_url.clone());

        history::Entry {
            video_id: self.video_id.clone(),
            title,
            channel: self.channel.clone(),
            position_ms,
            duration_ms,
            watched_at_ms: fallback_watched_at_ms,
            is_short,
            is_podcast,
            is_music,
            date_label,
            view_count_text: self.view_count_text.clone(),
            published_time_text: self.published_time_text.clone(),
            thumbnail_url,
            channel_verified: self.channel_verified.then_some(true),
        }
    }

    pub fn to_video_metadata(&self) -> VideoMetadata {
        VideoMetadata {
            view_count: self.view_count,
            channel_id: self.channel_id.clone(),
            channel_url: self.channel_url.clone(),
            kind: if self.is_short {
                SearchResultKind::Short
            } else {
                SearchResultKind::Video
            },
            is_live: self.is_live,
            published_time_text: self.published_time_text.clone(),
            view_count_text: self.view_count_text.clone(),
            channel_verified: self.channel_verified,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteHistorySection {
    pub date_label: String,
    pub items: Vec<RemoteHistoryItem>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteHistoryActions {
    pub clear_feedback_token: Option<String>,
    pub pause_feedback_token: Option<String>,
    pub remote_paused: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteHistoryPage {
    pub sections: Vec<RemoteHistorySection>,
    pub chips: HashMap<HistoryFilter, HistoryChipAction>,
    pub actions: RemoteHistoryActions,
    pub next_continuation: Option<String>,
    pub signed_out: bool,
}

pub enum HistorySyncMessage {
    SessionUpdated(YouTubeWebSession),
    Page {
        request_id: u64,
        filter: HistoryFilter,
        page_index: usize,
        page: RemoteHistoryPage,
    },
    CategoryComplete {
        request_id: u64,
        filter: HistoryFilter,
        complete: bool,
    },
    Finished {
        request_id: u64,
        filter: HistoryFilter,
        auth_expired: bool,
        result: Result<usize, String>,
    },
}

#[derive(Clone, Debug)]
pub struct RemoteWatchTracker {
    pub video_id: String,
    pub cpn: String,
    pub watchtime_url: String,
    pub duration_secs: Option<f64>,
    pub started_at: Instant,
    pub last_ping_at: Instant,
    pub last_reported_pos_secs: f64,
}

#[derive(Clone, Debug)]
pub struct RemoteWatchInitResult {
    pub tracker: Option<RemoteWatchTracker>,
    pub detected_music: Option<bool>,
    pub detected_short: Option<bool>,
    pub duration_ms: Option<u64>,
}

pub struct RemoteWatchInitEvent {
    pub session_id: u64,
    pub video_id: String,
    pub web_session: Option<YouTubeWebSession>,
    pub auth_expired: bool,
    pub tracker: Option<RemoteWatchTracker>,
    pub detected_music: Option<bool>,
    pub detected_short: Option<bool>,
    pub duration_ms: Option<u64>,
}

pub struct RemoteHistoryFeedbackEvent {
    pub web_session: Option<YouTubeWebSession>,
    pub auth_expired: bool,
    pub result: Result<(), String>,
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

pub fn generate_cpn() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0x5A17_C0DE_1234_5678);
        for (idx, b) in bytes.iter_mut().enumerate() {
            *b = ((nanos >> ((idx * 5) % 96)) & 0xFF) as u8;
        }
    }
    bytes
        .iter()
        .map(|b| CPN_ALPHABET[(b & 63) as usize] as char)
        .collect()
}

pub fn normalize_stats_url(raw: &str) -> Option<String> {
    let mut parsed = url::Url::parse(raw.trim()).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    if !matches!(
        host.as_str(),
        "s.youtube.com" | "www.youtube.com" | "youtube.com"
    ) {
        return None;
    }
    if host == "s.youtube.com" {
        parsed.set_host(Some("www.youtube.com")).ok()?;
    }
    Some(parsed.to_string())
}

#[cfg(test)]
fn normalize_stats_url_for_base(raw: &str, innertube_base_url: &str) -> Option<String> {
    if let Some(norm) = normalize_stats_url(raw) {
        return Some(norm);
    }
    if innertube_base_url.starts_with("http://127.0.0.1") {
        let parsed = url::Url::parse(raw.trim()).ok()?;
        if parsed.scheme() == "http" && parsed.host_str() == Some("127.0.0.1") {
            return Some(parsed.to_string());
        }
    }
    None
}

#[cfg(not(test))]
fn normalize_stats_url_for_base(raw: &str, _innertube_base_url: &str) -> Option<String> {
    normalize_stats_url(raw)
}

pub fn build_playback_ping_url(
    base_url: &str,
    cpn: &str,
    client_version: &str,
    pos_secs: f64,
    duration_secs: Option<f64>,
    rt_secs: f64,
) -> Option<String> {
    let mut parsed = url::Url::parse(base_url.trim()).ok()?;
    let cmt = format!("{:.3}", pos_secs.max(0.0));
    let rt = format!("{:.3}", rt_secs.max(0.0));
    {
        let mut pairs = parsed.query_pairs_mut();
        pairs
            .append_pair("cpn", cpn)
            .append_pair("ver", "2")
            .append_pair("c", "WEB")
            .append_pair("cver", client_version)
            .append_pair("ns", "yt")
            .append_pair("el", "detailpage")
            .append_pair("cmt", &cmt)
            .append_pair("rt", &rt)
            .append_pair("rtn", &rt)
            .append_pair("volume", "100")
            .append_pair("muted", "0");
        if let Some(len) = duration_secs.filter(|d| d.is_finite() && *d > 0.0) {
            pairs.append_pair("len", &format!("{len:.3}"));
        }
    }
    Some(parsed.to_string())
}

#[allow(clippy::too_many_arguments)]
pub fn build_watchtime_ping_url(
    base_url: &str,
    cpn: &str,
    client_version: &str,
    st_secs: f64,
    et_secs: f64,
    cmt_secs: f64,
    duration_secs: Option<f64>,
    rt_secs: f64,
    paused: bool,
    is_final: bool,
) -> Option<String> {
    let mut parsed = url::Url::parse(base_url.trim()).ok()?;
    let st_val = st_secs.max(0.0);
    let et_val = et_secs.max(st_val);
    let cmt = format!("{:.3}", cmt_secs.max(0.0));
    let st = format!("{st_val:.3}");
    let et = format!("{et_val:.3}");
    let rt = format!("{:.3}", rt_secs.max(0.0));
    let state = if paused { "paused" } else { "playing" };
    let final_flag = if is_final { "1" } else { "0" };
    {
        let mut pairs = parsed.query_pairs_mut();
        pairs
            .append_pair("cpn", cpn)
            .append_pair("ver", "2")
            .append_pair("c", "WEB")
            .append_pair("cver", client_version)
            .append_pair("ns", "yt")
            .append_pair("el", "detailpage")
            .append_pair("st", &st)
            .append_pair("et", &et)
            .append_pair("cmt", &cmt)
            .append_pair("state", state)
            .append_pair("final", final_flag)
            .append_pair("rt", &rt)
            .append_pair("rtn", &rt)
            .append_pair("volume", "100")
            .append_pair("muted", "0");
        if let Some(len) = duration_secs.filter(|d| d.is_finite() && *d > 0.0) {
            pairs.append_pair("len", &format!("{len:.3}"));
        }
    }
    Some(parsed.to_string())
}

fn map_chip_label_to_filter(label: &str) -> Option<HistoryFilter> {
    match label.trim().to_lowercase().as_str() {
        "tudo" | "all" => Some(HistoryFilter::All),
        "vídeos" | "videos" => Some(HistoryFilter::Videos),
        "shorts" => Some(HistoryFilter::Shorts),
        "podcasts" | "podcast" => Some(HistoryFilter::Podcasts),
        "música" | "musica" | "music" => Some(HistoryFilter::Music),
        _ => None,
    }
}

fn extract_chip_action_from_endpoint(endpoint: Option<&Value>) -> Option<HistoryChipAction> {
    let ep = endpoint?;
    if let Some(params) = ep
        .pointer("/browseEndpoint/params")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(HistoryChipAction::BrowseParams(params.to_owned()));
    }
    if let Some(token) = ep
        .pointer("/continuationCommand/token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(HistoryChipAction::Continuation(token.to_owned()));
    }
    None
}

fn collect_chips_from_value(root: &Value, out: &mut HashMap<HistoryFilter, HistoryChipAction>) {
    match root {
        Value::Object(map) => {
            if let Some(chip) = map.get("chipCloudChipRenderer") {
                let label = yt_text(chip.get("text"));
                if let Some(filter) = map_chip_label_to_filter(&label)
                    && let Some(action) = extract_chip_action_from_endpoint(
                        chip.get("navigationEndpoint")
                            .or_else(|| chip.get("endpoint")),
                    )
                {
                    out.entry(filter).or_insert(action);
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
                    .or_else(|| chip.get("content").and_then(Value::as_str))
                    .unwrap_or("");
                if let Some(filter) = map_chip_label_to_filter(label) {
                    let ep = chip
                        .pointer("/onTap/innertubeCommand")
                        .or_else(|| chip.pointer("/tapCommand/innertubeCommand"))
                        .or_else(|| chip.pointer("/chipButtonViewModel/onTap/innertubeCommand"))
                        .or_else(|| chip.get("navigationEndpoint"));
                    if let Some(action) = extract_chip_action_from_endpoint(ep) {
                        out.entry(filter).or_insert(action);
                    }
                }
            }
            for child in map.values() {
                collect_chips_from_value(child, out);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_chips_from_value(item, out);
            }
        }
        _ => {}
    }
}

fn find_first_feedback_token(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(token) = value
                .pointer("/feedbackEndpoint/feedbackToken")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(token.to_owned());
            }
            for child in map.values() {
                if let Some(token) = find_first_feedback_token(child) {
                    return Some(token);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(find_first_feedback_token),
        _ => None,
    }
}

fn parse_browse_feed_actions(root: &Value, actions: &mut RemoteHistoryActions) {
    match root {
        Value::Object(map) => {
            if let Some(feed_actions) = map.get("browseFeedActionsRenderer")
                && let Some(contents) = feed_actions.get("contents").and_then(Value::as_array)
            {
                for entry in contents {
                    let Some(btn) = entry.get("buttonRenderer") else {
                        continue;
                    };
                    let label = yt_text(btn.get("text")).to_lowercase();
                    let icon = btn
                        .pointer("/icon/iconType")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_ascii_uppercase();
                    let token = find_first_feedback_token(btn);
                    if icon.contains("DELETE")
                        || icon.contains("TRASH")
                        || label.contains("limpar")
                        || label.contains("clear")
                    {
                        if actions.clear_feedback_token.is_none() {
                            actions.clear_feedback_token = token;
                        }
                    } else if icon.contains("PAUSE")
                        || icon.contains("PLAY")
                        || label.contains("pausar")
                        || label.contains("pause")
                        || label.contains("ativar")
                        || label.contains("turn on")
                    {
                        if actions.pause_feedback_token.is_none() {
                            actions.pause_feedback_token = token;
                        }
                        if icon.contains("PLAY")
                            || label.contains("ativar")
                            || label.contains("turn on")
                        {
                            actions.remote_paused = Some(true);
                        } else if icon.contains("PAUSE")
                            || label.contains("pausar")
                            || label.contains("pause")
                        {
                            actions.remote_paused = Some(false);
                        }
                    }
                }
            }
            for child in map.values() {
                parse_browse_feed_actions(child, actions);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                parse_browse_feed_actions(item, actions);
            }
        }
        _ => {}
    }
}

fn compute_watched_position_ms(
    percent: Option<f64>,
    start_time_secs: Option<f64>,
    duration_ms: Option<u64>,
) -> u64 {
    if let Some(start_secs) = start_time_secs.filter(|s| s.is_finite() && *s > 0.0) {
        let ms = (start_secs * 1000.0).round() as u64;
        return duration_ms.map(|dur| ms.min(dur)).unwrap_or(ms);
    }
    if let (Some(pct), Some(dur)) = (
        percent.filter(|p| p.is_finite() && *p > 0.0),
        duration_ms.filter(|&d| d > 0),
    ) {
        let clamped = pct.clamp(0.0, 100.0);
        return ((dur as f64) * (clamped / 100.0)).round() as u64;
    }
    0
}

fn extract_remove_feedback_token(obj: &Value) -> Option<String> {
    if let Some(buttons) = obj
        .pointer("/menu/menuRenderer/topLevelButtons")
        .and_then(Value::as_array)
    {
        let mut fallback_tok = None;
        for btn in buttons {
            let Some(tok) = find_first_feedback_token(btn) else {
                continue;
            };
            let icon = btn
                .pointer("/buttonRenderer/icon/iconType")
                .and_then(Value::as_str)
                .unwrap_or("");
            if icon.eq_ignore_ascii_case("CLOSE")
                || icon.eq_ignore_ascii_case("TRASH")
                || icon.eq_ignore_ascii_case("DELETE")
            {
                return Some(tok);
            }
            if fallback_tok.is_none() {
                fallback_tok = Some(tok);
            }
        }
        if fallback_tok.is_some() {
            return fallback_tok;
        }
    }
    if let Some(items) = obj
        .pointer("/menu/menuRenderer/items")
        .and_then(Value::as_array)
    {
        for item in items {
            let text = yt_text(
                item.pointer("/menuServiceItemRenderer/text")
                    .or_else(|| item.pointer("/menuNavigationItemRenderer/text")),
            )
            .to_lowercase();
            let icon = item
                .pointer("/menuServiceItemRenderer/icon/iconType")
                .and_then(Value::as_str)
                .unwrap_or("");
            if (text.contains("histórico")
                || text.contains("history")
                || text.contains("remover")
                || text.contains("remove")
                || icon.eq_ignore_ascii_case("DELETE")
                || icon.eq_ignore_ascii_case("TRASH")
                || icon.eq_ignore_ascii_case("CLOSE"))
                && let Some(tok) = find_first_feedback_token(item)
            {
                return Some(tok);
            }
        }
    }
    if let Some(menu) = obj.get("menu")
        && let Some(tok) = find_first_feedback_token(menu)
    {
        return Some(tok);
    }
    find_first_feedback_token(obj)
}

fn parse_history_video_renderer(v: &Value, date_label: &str) -> Option<RemoteHistoryItem> {
    let video_id = v
        .get("videoId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| history::is_valid_video_id(id))?
        .to_owned();
    let title = yt_text(v.get("title"));
    if title.is_empty() {
        return None;
    }
    let owner_obj = v
        .get("ownerText")
        .or_else(|| v.get("longBylineText"))
        .or_else(|| v.get("shortBylineText"));
    let channel = yt_text(owner_obj);
    let (channel_id, channel_url) = channel_url_from_browse_endpoint(
        owner_obj.and_then(|o| o.pointer("/runs/0/navigationEndpoint")),
    );
    let channel_verified = has_verified_owner_badge(v.get("ownerBadges"));
    let thumbnail_url = select_yt_thumbnail(v.pointer("/thumbnail/thumbnails"), 480);

    let mut is_short = v.pointer("/navigationEndpoint/reelWatchEndpoint").is_some();
    let mut is_live = false;
    let mut is_music = false;
    let mut is_podcast = false;
    let mut percent_watched: Option<f64> = None;

    let mut duration_text = yt_text(v.get("lengthText"));
    if let Some(overlays) = v.get("thumbnailOverlays").and_then(Value::as_array) {
        for ov in overlays {
            if let Some(pct) = ov
                .pointer("/thumbnailOverlayResumePlaybackRenderer/percentDurationWatched")
                .and_then(Value::as_f64)
            {
                percent_watched = Some(pct);
            }
            if let Some(ts) = ov.get("thumbnailOverlayTimeStatusRenderer") {
                let style = ts.get("style").and_then(Value::as_str).unwrap_or("");
                let icon = ts
                    .pointer("/icon/iconType")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let text = yt_text(ts.get("text"));
                if style.eq_ignore_ascii_case("SHORTS") {
                    is_short = true;
                } else if style.eq_ignore_ascii_case("LIVE")
                    || text.eq_ignore_ascii_case("ao vivo")
                    || text.eq_ignore_ascii_case("live")
                {
                    is_live = true;
                } else if duration_text.is_empty() && !text.is_empty() {
                    duration_text = text;
                }
                if icon.to_ascii_uppercase().contains("MUSIC")
                    || icon.to_ascii_uppercase().contains("AUDIO")
                {
                    is_music = true;
                }
            }
        }
    }
    if let Some(badges) = v.get("badges").and_then(Value::as_array) {
        for b in badges {
            let renderer = b.get("metadataBadgeRenderer").unwrap_or(b);
            let style = renderer.get("style").and_then(Value::as_str).unwrap_or("");
            let label = yt_text(renderer.get("label").or_else(|| renderer.get("text")))
                .to_ascii_lowercase();
            if style.contains("LIVE") || label == "ao vivo" || label == "live" {
                is_live = true;
            }
            if style.contains("PODCAST") || label.contains("podcast") {
                is_podcast = true;
            }
            if style.contains("MUSIC") {
                is_music = true;
            }
        }
    }

    let duration_ms = if is_live {
        None
    } else {
        crate::parse_duration_ms(&duration_text)
    };
    let start_secs = v
        .pointer("/navigationEndpoint/watchEndpoint/startTimeSeconds")
        .and_then(Value::as_f64);
    let position_ms = compute_watched_position_ms(percent_watched, start_secs, duration_ms);

    let views_raw = yt_text(
        v.get("shortViewCountText")
            .or_else(|| v.get("viewCountText")),
    );
    let view_count = parse_pt_br_compact_count(&views_raw);
    let view_count_text = clean_pt_br_view_label(&views_raw);
    let published_time_text = clean_published_time_text(&yt_text(v.get("publishedTimeText")));
    let feedback_token = extract_remove_feedback_token(v);

    Some(RemoteHistoryItem {
        video_id,
        title,
        channel,
        channel_id,
        channel_url,
        channel_verified,
        thumbnail_url,
        duration_text: if is_live {
            String::new()
        } else {
            duration_text
        },
        duration_ms,
        position_ms,
        view_count,
        view_count_text,
        published_time_text,
        is_short,
        is_music,
        is_podcast,
        is_live,
        feedback_token,
        date_label: date_label.to_owned(),
    })
}

fn parse_history_lockup_view_model(l: &Value, date_label: &str) -> Option<RemoteHistoryItem> {
    let content_type = l.get("contentType").and_then(Value::as_str).unwrap_or("");
    let content_id = l
        .get("contentId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| history::is_valid_video_id(id))?
        .to_owned();
    let is_podcast = content_type.contains("PODCAST");
    if content_type.contains("PLAYLIST") && !is_podcast {
        return None;
    }
    let title = l
        .pointer("/metadata/lockupMetadataViewModel/title/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())?
        .to_owned();
    let thumbnail_url = select_yt_thumbnail(
        l.pointer("/contentImage/thumbnailViewModel/image/sources"),
        480,
    );

    let mut duration_text = String::new();
    let mut is_live = false;
    let mut is_music = false;
    let mut is_short = l
        .pointer("/rendererContext/commandContext/onTap/innertubeCommand/reelWatchEndpoint")
        .is_some();
    let mut percent_watched: Option<f64> = None;

    if let Some(overlays) = l
        .pointer("/contentImage/thumbnailViewModel/overlays")
        .and_then(Value::as_array)
    {
        for ov in overlays {
            if let Some(pct) = ov
                .pointer("/thumbnailOverlayProgressBarViewModel/startPercent")
                .or_else(|| {
                    ov.pointer(
                        "/thumbnailBottomOverlayViewModel/progressBar/thumbnailOverlayProgressBarViewModel/startPercent",
                    )
                })
                .and_then(Value::as_f64)
            {
                percent_watched = Some(pct);
            }
            let badges = ov
                .pointer("/thumbnailOverlayBadgeViewModel/thumbnailBadges")
                .or_else(|| ov.pointer("/thumbnailBottomOverlayViewModel/badges"))
                .and_then(Value::as_array);
            if let Some(badges) = badges {
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
                    let icon_name = badge
                        .pointer("/icon/sources/0/clientResource/imageName")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if icon_name.to_ascii_uppercase().contains("MUSIC")
                        || icon_name.to_ascii_uppercase().contains("AUDIO")
                        || style.contains("MUSIC")
                    {
                        is_music = true;
                    }
                    if style.contains("SHORTS") {
                        is_short = true;
                    }
                    if style.contains("LIVE")
                        || text.eq_ignore_ascii_case("ao vivo")
                        || text.eq_ignore_ascii_case("live")
                    {
                        is_live = true;
                    } else if !text.is_empty() && duration_text.is_empty() {
                        duration_text = text.to_owned();
                    }
                }
            }
        }
    }

    let parsed_rows = parse_lockup_metadata_rows(l);
    is_live |= parsed_rows.is_live;
    let channel = parsed_rows.channel;
    let channel_id = parsed_rows.channel_id;
    let channel_url = parsed_rows.channel_url;
    let channel_verified = parsed_rows.channel_verified;
    let view_count = parsed_rows.view_count;
    let view_count_text = parsed_rows.view_count_text;
    let published_time_text = parsed_rows.published_time_text;

    let duration_ms = if is_live {
        None
    } else {
        crate::parse_duration_ms(&duration_text)
    };
    let start_secs = l
        .pointer(
            "/rendererContext/commandContext/onTap/innertubeCommand/watchEndpoint/startTimeSeconds",
        )
        .and_then(Value::as_f64);
    let position_ms = compute_watched_position_ms(percent_watched, start_secs, duration_ms);
    let feedback_token = extract_remove_feedback_token(l);

    Some(RemoteHistoryItem {
        video_id: content_id,
        title,
        channel,
        channel_id,
        channel_url,
        channel_verified,
        thumbnail_url,
        duration_text: if is_live {
            String::new()
        } else {
            duration_text
        },
        duration_ms,
        position_ms,
        view_count,
        view_count_text,
        published_time_text,
        is_short,
        is_music,
        is_podcast,
        is_live,
        feedback_token,
        date_label: date_label.to_owned(),
    })
}

fn parse_history_shorts_lockup(s: &Value, date_label: &str) -> Option<RemoteHistoryItem> {
    let watch_ep = s.pointer("/onTap/innertubeCommand/reelWatchEndpoint");
    let video_id = watch_ep
        .and_then(|ep| ep.get("videoId"))
        .and_then(Value::as_str)
        .or_else(|| {
            s.get("entityId")
                .and_then(Value::as_str)
                .and_then(|id| id.strip_prefix("shorts-shelf-item-"))
        })
        .map(str::trim)
        .filter(|id| history::is_valid_video_id(id))?
        .to_owned();
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
    let thumbnail_url = select_yt_thumbnail(
        s.pointer("/thumbnailViewModel/thumbnailViewModel/image/sources")
            .or_else(|| watch_ep.and_then(|ep| ep.pointer("/thumbnail/thumbnails")))
            .or_else(|| s.pointer("/thumbnail/sources")),
        480,
    );
    let feedback_token = extract_remove_feedback_token(s);
    Some(RemoteHistoryItem {
        video_id,
        title,
        channel: String::new(),
        channel_id: None,
        channel_url: None,
        channel_verified: false,
        thumbnail_url,
        duration_text: String::new(),
        duration_ms: None,
        position_ms: 0,
        view_count,
        view_count_text,
        published_time_text: None,
        is_short: true,
        is_music: false,
        is_podcast: false,
        is_live: false,
        feedback_token,
        date_label: date_label.to_owned(),
    })
}

fn parse_history_reel_item_renderer(r: &Value, date_label: &str) -> Option<RemoteHistoryItem> {
    let video_id = r
        .get("videoId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| history::is_valid_video_id(id))?
        .to_owned();
    let title = yt_text(r.get("headline").or_else(|| r.get("title")));
    if title.is_empty() {
        return None;
    }
    let views_raw = yt_text(r.get("viewCountText"));
    let view_count = parse_pt_br_compact_count(&views_raw);
    let view_count_text = clean_pt_br_view_label(&views_raw);
    let thumbnail_url = select_yt_thumbnail(r.pointer("/thumbnail/thumbnails"), 480);
    let feedback_token = extract_remove_feedback_token(r);
    Some(RemoteHistoryItem {
        video_id,
        title,
        channel: String::new(),
        channel_id: None,
        channel_url: None,
        channel_verified: false,
        thumbnail_url,
        duration_text: String::new(),
        duration_ms: None,
        position_ms: 0,
        view_count,
        view_count_text,
        published_time_text: None,
        is_short: true,
        is_music: false,
        is_podcast: false,
        is_live: false,
        feedback_token,
        date_label: date_label.to_owned(),
    })
}

fn collect_items_from_section_content(
    node: &Value,
    date_label: &str,
    items: &mut Vec<RemoteHistoryItem>,
    next_continuation: &mut Option<String>,
) {
    let Some(obj) = node.as_object() else {
        if let Some(arr) = node.as_array() {
            for child in arr {
                collect_items_from_section_content(child, date_label, items, next_continuation);
            }
        }
        return;
    };

    if let Some(cont) = obj.get("continuationItemRenderer") {
        if next_continuation.is_none()
            && let Some(token) = cont
                .pointer("/continuationEndpoint/continuationCommand/token")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            *next_continuation = Some(token.to_owned());
        }
        return;
    }
    if let Some(v) = obj
        .get("videoRenderer")
        .or_else(|| obj.get("gridVideoRenderer"))
        .or_else(|| obj.get("compactVideoRenderer"))
    {
        if let Some(item) = parse_history_video_renderer(v, date_label) {
            items.push(item);
        }
        return;
    }
    if let Some(l) = obj.get("lockupViewModel") {
        if let Some(item) = parse_history_lockup_view_model(l, date_label) {
            items.push(item);
        }
        return;
    }
    if let Some(s) = obj.get("shortsLockupViewModel") {
        if let Some(item) = parse_history_shorts_lockup(s, date_label) {
            items.push(item);
        }
        return;
    }
    if let Some(r) = obj.get("reelItemRenderer") {
        if let Some(item) = parse_history_reel_item_renderer(r, date_label) {
            items.push(item);
        }
        return;
    }
    if let Some(shelf) = obj.get("reelShelfRenderer") {
        if let Some(shelf_items) = shelf.get("items").and_then(Value::as_array) {
            for child in shelf_items {
                collect_items_from_section_content(child, date_label, items, next_continuation);
            }
        }
        return;
    }
    if let Some(shelf) = obj.get("gridShelfViewModel") {
        if let Some(shelf_items) = shelf.get("contents").and_then(Value::as_array) {
            for child in shelf_items {
                collect_items_from_section_content(child, date_label, items, next_continuation);
            }
        }
        return;
    }

    for child in obj.values() {
        collect_items_from_section_content(child, date_label, items, next_continuation);
    }
}

fn push_section_merged(sections: &mut Vec<RemoteHistorySection>, incoming: RemoteHistorySection) {
    if incoming.items.is_empty() {
        return;
    }
    if let Some(last) = sections.last_mut()
        && last.date_label == incoming.date_label
    {
        last.items.extend(incoming.items);
    } else {
        sections.push(incoming);
    }
}

fn process_section_list_items(
    list: &[Value],
    fallback_date_label: &str,
    sections: &mut Vec<RemoteHistorySection>,
    next_continuation: &mut Option<String>,
) {
    for entry in list {
        if let Some(cont) = entry.get("continuationItemRenderer") {
            if next_continuation.is_none()
                && let Some(token) = cont
                    .pointer("/continuationEndpoint/continuationCommand/token")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            {
                *next_continuation = Some(token.to_owned());
            }
            continue;
        }
        if let Some(sec) = entry.get("itemSectionRenderer") {
            let header_label = yt_text(sec.pointer("/header/itemSectionHeaderRenderer/title"));
            let date_label = if !header_label.is_empty() {
                header_label
            } else {
                sections
                    .last()
                    .map(|s| s.date_label.clone())
                    .filter(|l| !l.is_empty())
                    .unwrap_or_else(|| fallback_date_label.to_owned())
            };
            let mut items = Vec::new();
            if let Some(contents) = sec.get("contents").and_then(Value::as_array) {
                for item in contents {
                    collect_items_from_section_content(
                        item,
                        &date_label,
                        &mut items,
                        next_continuation,
                    );
                }
            }
            push_section_merged(sections, RemoteHistorySection { date_label, items });
            continue;
        }
        let date_label = sections
            .last()
            .map(|s| s.date_label.clone())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| fallback_date_label.to_owned());
        let mut items = Vec::new();
        collect_items_from_section_content(entry, &date_label, &mut items, next_continuation);
        push_section_merged(sections, RemoteHistorySection { date_label, items });
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

pub fn parse_history_response(json: &Value, fallback_date_label: &str) -> RemoteHistoryPage {
    let mut page = RemoteHistoryPage::default();
    collect_chips_from_value(json, &mut page.chips);
    parse_browse_feed_actions(json, &mut page.actions);

    if let Some(tabs) = json
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for tab in tabs {
            if let Some(contents) = tab
                .pointer("/tabRenderer/content/sectionListRenderer/contents")
                .and_then(Value::as_array)
            {
                process_section_list_items(
                    contents,
                    fallback_date_label,
                    &mut page.sections,
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
                .and_then(Value::as_array)
            {
                process_section_list_items(
                    items,
                    fallback_date_label,
                    &mut page.sections,
                    &mut page.next_continuation,
                );
            }
        }
    }

    let has_any_items = page.sections.iter().any(|s| !s.items.is_empty());
    if !has_any_items
        && page.actions.clear_feedback_token.is_none()
        && page.actions.pause_feedback_token.is_none()
        && contains_signed_out_promo(json)
    {
        page.signed_out = true;
    }

    page
}

#[allow(clippy::too_many_arguments)]
pub fn fetch_history_page_at(
    innertube_base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    browse_params: Option<&str>,
    continuation: Option<&str>,
    fallback_date_label: &str,
    hl: &str,
    cancel: &Cancellation,
) -> Result<RemoteHistoryPage, InnerTubeError> {
    let body = if let Some(token) = continuation {
        json!({ "continuation": token })
    } else if let Some(params) = browse_params {
        json!({ "browseId": "FEhistory", "params": params })
    } else {
        json!({ "browseId": "FEhistory" })
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
    let page = parse_history_response(&json, fallback_date_label);
    if page.signed_out {
        let _ = crate::innertube::invalidate_web_session();
        return Err(InnerTubeError::AuthExpired(
            "Sua sessão do YouTube expirou. Entre novamente.".into(),
        ));
    }
    Ok(page)
}

pub fn perform_history_feedback_at(
    innertube_base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    feedback_token: &str,
    hl: &str,
    cancel: &Cancellation,
) -> Result<(), InnerTubeError> {
    let json = post_innertube_with_locale_at(
        innertube_base_url,
        client,
        session,
        "feedback",
        json!({ "feedbackTokens": [feedback_token] }),
        hl,
        cancel,
    )?;
    if let Some(responses) = json.get("feedbackResponses").and_then(Value::as_array) {
        for resp in responses {
            if resp.get("isProcessed").and_then(Value::as_bool) == Some(false) {
                return Err(InnerTubeError::Other(
                    "O YouTube não conseguiu processar esta alteração no histórico.".into(),
                ));
            }
        }
    }
    Ok(())
}

pub fn init_remote_watch_at(
    innertube_base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    video_id: &str,
    start_secs: f64,
    cancel: &Cancellation,
) -> Result<RemoteWatchInitResult, InnerTubeError> {
    if !history::is_valid_video_id(video_id) {
        return Err(InnerTubeError::Other("ID de vídeo inválido.".into()));
    }
    let cpn = generate_cpn();
    let body = json!({
        "videoId": video_id,
        "cpn": cpn,
        "playbackContext": {
            "contentPlaybackContext": {
                "html5Preference": "HTML5_PREF_WANTS"
            }
        }
    });
    let json = post_innertube_with_locale_at(
        innertube_base_url,
        client,
        session,
        "player",
        body,
        "pt",
        cancel,
    )?;

    let duration_secs = json
        .pointer("/videoDetails/lengthSeconds")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| v.as_f64())
        })
        .filter(|d| d.is_finite() && *d > 0.0);
    let duration_ms = duration_secs.map(|d| (d * 1000.0).round() as u64);

    let detected_music = json
        .pointer("/microformat/playerMicroformatRenderer/category")
        .and_then(Value::as_str)
        .map(|cat| cat.eq_ignore_ascii_case("Music"));
    let detected_short = json
        .pointer("/microformat/playerMicroformatRenderer/canonicalUrl")
        .and_then(Value::as_str)
        .map(|u| u.contains("/shorts/"));

    let playback_base = json
        .pointer("/playbackTracking/videostatsPlaybackUrl/baseUrl")
        .and_then(Value::as_str)
        .and_then(|u| normalize_stats_url_for_base(u, innertube_base_url));
    let watchtime_base = json
        .pointer("/playbackTracking/videostatsWatchtimeUrl/baseUrl")
        .and_then(Value::as_str)
        .and_then(|u| normalize_stats_url_for_base(u, innertube_base_url));

    let (Some(playback_url), Some(watchtime_url)) = (playback_base, watchtime_base) else {
        return Ok(RemoteWatchInitResult {
            tracker: None,
            detected_music,
            detected_short,
            duration_ms,
        });
    };

    let now = Instant::now();
    let referer = format!("{YOUTUBE_ORIGIN}/watch?v={video_id}");
    if let Some(full_playback_url) = build_playback_ping_url(
        &playback_url,
        &cpn,
        &session.client_version,
        start_secs,
        duration_secs,
        0.0,
    ) && let Ok(resp) = client
        .get(&full_playback_url)
        .header(reqwest::header::USER_AGENT, WEB_USER_AGENT)
        .header(reqwest::header::COOKIE, session.cookie_header())
        .header(reqwest::header::REFERER, &referer)
        .header("Origin", YOUTUBE_ORIGIN)
        .send()
    {
        session.absorb_set_cookies(resp.headers());
    }

    if cancel.cancelled() {
        return Err(InnerTubeError::Cancelled("Operação cancelada.".into()));
    }

    if let Some(initial_watchtime_url) = build_watchtime_ping_url(
        &watchtime_url,
        &cpn,
        &session.client_version,
        start_secs,
        start_secs,
        start_secs,
        duration_secs,
        0.1,
        false,
        false,
    ) && let Ok(resp) = client
        .get(&initial_watchtime_url)
        .header(reqwest::header::USER_AGENT, WEB_USER_AGENT)
        .header(reqwest::header::COOKIE, session.cookie_header())
        .header(reqwest::header::REFERER, &referer)
        .header("Origin", YOUTUBE_ORIGIN)
        .send()
    {
        session.absorb_set_cookies(resp.headers());
    }

    Ok(RemoteWatchInitResult {
        tracker: Some(RemoteWatchTracker {
            video_id: video_id.to_owned(),
            cpn,
            watchtime_url,
            duration_secs,
            started_at: now,
            last_ping_at: now,
            last_reported_pos_secs: start_secs.max(0.0),
        }),
        detected_music,
        detected_short,
        duration_ms,
    })
}

/// Fetches up to `max_pages` initial pages of `HistoryFilter::All`, preserving
/// `next_continuation` on the last emitted page for on-demand load-more pagination
/// and `chips` from page 0 for on-demand category tab loading.
#[allow(clippy::too_many_arguments)]
fn fetch_initial_history_pages_at<F>(
    base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    request_id: u64,
    first_page: RemoteHistoryPage,
    max_pages: usize,
    hl: &str,
    cancel: &Cancellation,
    tx: &mpsc::Sender<HistorySyncMessage>,
    mut on_page_sent: F,
) -> usize
where
    F: FnMut(),
{
    let capped_pages = max_pages.max(1);
    let mut all_cont = first_page.next_continuation.clone();
    let mut seen_all_tokens: HashSet<String> = HashSet::new();
    if let Some(tok) = &all_cont {
        seen_all_tokens.insert(tok.clone());
    }
    let mut last_date_label = first_page
        .sections
        .last()
        .map(|s| s.date_label.clone())
        .unwrap_or_default();
    let mut total_items: usize = first_page.sections.iter().map(|s| s.items.len()).sum();

    let _ = tx.send(HistorySyncMessage::Page {
        request_id,
        filter: HistoryFilter::All,
        page_index: 0,
        page: first_page,
    });
    on_page_sent();

    for page_index in 1..capped_pages {
        if cancel.cancelled() || total_items >= history::MAX_ENTRIES {
            break;
        }
        let Some(token) = all_cont.take() else {
            break;
        };
        match fetch_history_page_at(
            base_url,
            client,
            session,
            None,
            Some(&token),
            &last_date_label,
            hl,
            cancel,
        ) {
            Ok(page) => {
                if let Some(last) = page.sections.last()
                    && !last.date_label.is_empty()
                {
                    last_date_label = last.date_label.clone();
                }
                total_items += page.sections.iter().map(|s| s.items.len()).sum::<usize>();
                all_cont = page
                    .next_continuation
                    .clone()
                    .filter(|t| seen_all_tokens.insert(t.clone()));
                let _ = tx.send(HistorySyncMessage::Page {
                    request_id,
                    filter: HistoryFilter::All,
                    page_index,
                    page,
                });
                on_page_sent();
            }
            Err(_) => break,
        }
    }

    total_items
}

pub fn start_full_history_sync(
    request_id: u64,
    cached_session: Option<YouTubeWebSession>,
    allow_interactive: bool,
    hl: String,
    ctx: &egui::Context,
) -> (Receiver<HistorySyncMessage>, Cancellation) {
    let (tx, rx) = mpsc::channel();
    let cancel = Cancellation::default();
    let cancel_bg = cancel.clone();
    let ctx_bg = ctx.clone();

    std::thread::spawn(move || {
        let client = match http_client() {
            Ok(c) => c,
            Err(err) => {
                let _ = tx.send(HistorySyncMessage::Finished {
                    request_id,
                    filter: HistoryFilter::All,
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
                    || err.contains("expirada");
                let _ = tx.send(HistorySyncMessage::Finished {
                    request_id,
                    filter: HistoryFilter::All,
                    auth_expired,
                    result: Err(err),
                });
                ctx_bg.request_repaint();
                return;
            }
        };
        session.refresh_client_config_if_needed(&client, &cancel_bg);
        persist_web_session_dpapi(&session);
        let _ = tx.send(HistorySyncMessage::SessionUpdated(session.clone()));

        let first_page = match fetch_history_page_at(
            INNERTUBE_BASE_URL,
            &client,
            &mut session,
            None,
            None,
            "",
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
                        let _ = tx.send(HistorySyncMessage::SessionUpdated(session.clone()));
                        match fetch_history_page_at(
                            INNERTUBE_BASE_URL,
                            &client,
                            &mut session,
                            None,
                            None,
                            "",
                            &hl,
                            &cancel_bg,
                        ) {
                            Ok(page) => page,
                            Err(err) => {
                                let auth_expired = matches!(err, InnerTubeError::AuthExpired(_));
                                let _ = tx.send(HistorySyncMessage::Finished {
                                    request_id,
                                    filter: HistoryFilter::All,
                                    auth_expired,
                                    result: Err(err.into_message()),
                                });
                                ctx_bg.request_repaint();
                                return;
                            }
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(HistorySyncMessage::Finished {
                            request_id,
                            filter: HistoryFilter::All,
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
                let _ = tx.send(HistorySyncMessage::Finished {
                    request_id,
                    filter: HistoryFilter::All,
                    auth_expired,
                    result: Err(err.into_message()),
                });
                ctx_bg.request_repaint();
                return;
            }
        };

        let total_items = fetch_initial_history_pages_at(
            INNERTUBE_BASE_URL,
            &client,
            &mut session,
            request_id,
            first_page,
            INITIAL_HISTORY_SYNC_PAGES,
            &hl,
            &cancel_bg,
            &tx,
            || ctx_bg.request_repaint(),
        );

        if !cancel_bg.cancelled() {
            let _ = tx.send(HistorySyncMessage::Finished {
                request_id,
                filter: HistoryFilter::All,
                auth_expired: false,
                result: Ok(total_items),
            });
            ctx_bg.request_repaint();
        }
    });

    (rx, cancel)
}

#[allow(clippy::too_many_arguments)]
pub fn start_history_chip_page_fetch(
    request_id: u64,
    filter: HistoryFilter,
    action: HistoryChipAction,
    fallback_date_label: String,
    page_index: usize,
    cached_session: Option<YouTubeWebSession>,
    hl: String,
    ctx: &egui::Context,
) -> (Receiver<HistorySyncMessage>, Cancellation) {
    let (tx, rx) = mpsc::channel();
    let cancel = Cancellation::default();
    let cancel_bg = cancel.clone();
    let ctx_bg = ctx.clone();

    std::thread::spawn(move || {
        let client = match http_client() {
            Ok(c) => c,
            Err(err) => {
                let _ = tx.send(HistorySyncMessage::Finished {
                    request_id,
                    filter,
                    auth_expired: false,
                    result: Err(err),
                });
                ctx_bg.request_repaint();
                return;
            }
        };
        let mut session = match acquire_web_session(cached_session, false, &cancel_bg) {
            Ok(s) => s,
            Err(err) => {
                let _ = tx.send(HistorySyncMessage::Finished {
                    request_id,
                    filter,
                    auth_expired: true,
                    result: Err(err),
                });
                ctx_bg.request_repaint();
                return;
            }
        };
        session.refresh_client_config_if_needed(&client, &cancel_bg);
        persist_web_session_dpapi(&session);
        let _ = tx.send(HistorySyncMessage::SessionUpdated(session.clone()));

        let (params, cont) = match &action {
            HistoryChipAction::BrowseParams(p) => (Some(p.as_str()), None),
            HistoryChipAction::Continuation(c) => (None, Some(c.as_str())),
        };
        match fetch_history_page_at(
            INNERTUBE_BASE_URL,
            &client,
            &mut session,
            params,
            cont,
            &fallback_date_label,
            &hl,
            &cancel_bg,
        ) {
            Ok(page) => {
                let count: usize = page.sections.iter().map(|s| s.items.len()).sum();
                let complete = page.next_continuation.is_none();
                let _ = tx.send(HistorySyncMessage::Page {
                    request_id,
                    filter,
                    page_index,
                    page,
                });
                if matches!(
                    filter,
                    HistoryFilter::Shorts | HistoryFilter::Podcasts | HistoryFilter::Music
                ) {
                    let _ = tx.send(HistorySyncMessage::CategoryComplete {
                        request_id,
                        filter,
                        complete,
                    });
                }
                let _ = tx.send(HistorySyncMessage::Finished {
                    request_id,
                    filter,
                    auth_expired: false,
                    result: Ok(count),
                });
            }
            Err(err) => {
                let auth_expired = matches!(err, InnerTubeError::AuthExpired(_));
                if auth_expired {
                    let _ = crate::innertube::invalidate_web_session();
                }
                let _ = tx.send(HistorySyncMessage::Finished {
                    request_id,
                    filter,
                    auth_expired,
                    result: Err(err.into_message()),
                });
            }
        }
        ctx_bg.request_repaint();
    });

    (rx, cancel)
}

pub fn start_remote_watch_init(
    session_id: u64,
    video_id: String,
    start_secs: f64,
    cached_session: Option<YouTubeWebSession>,
    ctx: &egui::Context,
) -> (Receiver<RemoteWatchInitEvent>, Cancellation) {
    let (tx, rx) = mpsc::channel();
    let cancel = Cancellation::default();
    let cancel_bg = cancel.clone();
    let ctx_bg = ctx.clone();

    std::thread::spawn(move || {
        let client = match http_client() {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut session = match acquire_web_session(cached_session, false, &cancel_bg) {
            Ok(s) => s,
            Err(_) => return,
        };
        session.refresh_client_config_if_needed(&client, &cancel_bg);
        persist_web_session_dpapi(&session);
        match init_remote_watch_at(
            INNERTUBE_BASE_URL,
            &client,
            &mut session,
            &video_id,
            start_secs,
            &cancel_bg,
        ) {
            Ok(init) => {
                persist_web_session_dpapi(&session);
                if !cancel_bg.cancelled() {
                    let _ = tx.send(RemoteWatchInitEvent {
                        session_id,
                        video_id,
                        web_session: Some(session),
                        auth_expired: false,
                        tracker: init.tracker,
                        detected_music: init.detected_music,
                        detected_short: init.detected_short,
                        duration_ms: init.duration_ms,
                    });
                    ctx_bg.request_repaint();
                }
            }
            Err(InnerTubeError::AuthExpired(_)) => {
                let _ = crate::innertube::invalidate_web_session();
                if !cancel_bg.cancelled() {
                    let _ = tx.send(RemoteWatchInitEvent {
                        session_id,
                        video_id,
                        web_session: None,
                        auth_expired: true,
                        tracker: None,
                        detected_music: None,
                        detected_short: None,
                        duration_ms: None,
                    });
                    ctx_bg.request_repaint();
                }
            }
            Err(_) => {}
        }
    });

    (rx, cancel)
}

pub fn send_watchtime_heartbeat_async(
    session: YouTubeWebSession,
    tracker: RemoteWatchTracker,
    current_pos_secs: f64,
    paused: bool,
    is_final: bool,
) {
    let rt_secs = tracker.started_at.elapsed().as_secs_f64();
    let Some(url) = build_watchtime_ping_url(
        &tracker.watchtime_url,
        &tracker.cpn,
        &session.client_version,
        tracker.last_reported_pos_secs,
        current_pos_secs,
        current_pos_secs,
        tracker.duration_secs,
        rt_secs,
        paused,
        is_final,
    ) else {
        return;
    };
    let referer = format!("{YOUTUBE_ORIGIN}/watch?v={}", tracker.video_id);
    let cookie_header = session.cookie_header();
    std::thread::spawn(move || {
        let _ = crate::http_client::no_redirect()
            .get(&url)
            .timeout(Duration::from_secs(6))
            .header(reqwest::header::USER_AGENT, WEB_USER_AGENT)
            .header(reqwest::header::COOKIE, cookie_header)
            .header(reqwest::header::REFERER, referer)
            .header("Origin", YOUTUBE_ORIGIN)
            .send();
    });
}

pub fn start_history_feedback(
    feedback_token: String,
    cached_session: Option<YouTubeWebSession>,
    hl: String,
    ctx: &egui::Context,
) -> (Receiver<RemoteHistoryFeedbackEvent>, Cancellation) {
    let (tx, rx) = mpsc::channel();
    let cancel = Cancellation::default();
    let cancel_bg = cancel.clone();
    let ctx_bg = ctx.clone();

    std::thread::spawn(move || {
        let client = match http_client() {
            Ok(c) => c,
            Err(err) => {
                let _ = tx.send(RemoteHistoryFeedbackEvent {
                    web_session: cached_session,
                    auth_expired: false,
                    result: Err(err),
                });
                ctx_bg.request_repaint();
                return;
            }
        };
        let mut session = match acquire_web_session(cached_session, false, &cancel_bg) {
            Ok(s) => s,
            Err(err) => {
                let _ = tx.send(RemoteHistoryFeedbackEvent {
                    web_session: None,
                    auth_expired: true,
                    result: Err(err),
                });
                ctx_bg.request_repaint();
                return;
            }
        };
        session.refresh_client_config_if_needed(&client, &cancel_bg);
        persist_web_session_dpapi(&session);
        match perform_history_feedback_at(
            INNERTUBE_BASE_URL,
            &client,
            &mut session,
            &feedback_token,
            &hl,
            &cancel_bg,
        ) {
            Ok(()) => {
                persist_web_session_dpapi(&session);
                let _ = tx.send(RemoteHistoryFeedbackEvent {
                    web_session: Some(session),
                    auth_expired: false,
                    result: Ok(()),
                });
            }
            Err(InnerTubeError::AuthExpired(err)) => {
                let _ = crate::innertube::invalidate_web_session();
                let _ = tx.send(RemoteHistoryFeedbackEvent {
                    web_session: None,
                    auth_expired: true,
                    result: Err(err),
                });
            }
            Err(err) => {
                let _ = tx.send(RemoteHistoryFeedbackEvent {
                    web_session: Some(session),
                    auth_expired: false,
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
    fn parses_fe_history_chips_sections_renderers_and_actions() {
        let sample: Value = serde_json::from_str(
            r#"{
            "header": {
                "feedFilterChipBarRenderer": {
                    "contents": [
                        {
                            "chipCloudChipRenderer": {
                                "text": { "simpleText": "Tudo" },
                                "navigationEndpoint": {
                                    "browseEndpoint": { "browseId": "FEhistory", "params": "p_all" }
                                }
                            }
                        },
                        {
                            "chipCloudChipRenderer": {
                                "text": { "simpleText": "Podcasts" },
                                "navigationEndpoint": {
                                    "continuationCommand": { "token": "tok_podcasts" }
                                }
                            }
                        }
                    ]
                }
            },
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "header": {
                                        "chipBarViewModel": {
                                            "chips": [
                                                {
                                                    "chipViewModel": {
                                                        "text": { "content": "Música" },
                                                        "onTap": {
                                                            "innertubeCommand": {
                                                                "browseEndpoint": { "browseId": "FEhistory", "params": "p_music" }
                                                            }
                                                        }
                                                    }
                                                },
                                                {
                                                    "chipViewModel": {
                                                        "text": { "content": "Shorts" },
                                                        "onTap": {
                                                            "innertubeCommand": {
                                                                "browseEndpoint": { "browseId": "FEhistory", "params": "p_shorts" }
                                                            }
                                                        }
                                                    }
                                                }
                                            ]
                                        }
                                    },
                                    "contents": [
                                        {
                                            "itemSectionRenderer": {
                                                "header": { "itemSectionHeaderRenderer": { "title": { "simpleText": "Ontem" } } },
                                                "contents": [
                                                    {
                                                        "videoRenderer": {
                                                            "videoId": "dQw4w9WgXcQ",
                                                            "title": { "runs": [{ "text": "The Strokes - One Way Trigger" }] },
                                                            "ownerText": { "runs": [{ "text": "Steely Dang", "navigationEndpoint": { "browseEndpoint": { "browseId": "UCuAXFkgsw1L7xaCfnd5JJOw" } } }] },
                                                            "ownerBadges": [{ "metadataBadgeRenderer": { "style": "BADGE_STYLE_TYPE_VERIFIED" } }],
                                                            "lengthText": { "simpleText": "5:42" },
                                                            "shortViewCountText": { "simpleText": "7 mil visualizações" },
                                                            "thumbnail": { "thumbnails": [{ "url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg", "width": 480 }] },
                                                            "thumbnailOverlays": [
                                                                { "thumbnailOverlayResumePlaybackRenderer": { "percentDurationWatched": 50 } },
                                                                { "thumbnailOverlayTimeStatusRenderer": { "icon": { "iconType": "MUSIC_NOTE" }, "text": { "simpleText": "5:42" } } }
                                                            ],
                                                            "menu": {
                                                                "menuRenderer": {
                                                                    "topLevelButtons": [{
                                                                        "buttonRenderer": {
                                                                            "icon": { "iconType": "CLOSE" },
                                                                            "serviceEndpoint": { "feedbackEndpoint": { "feedbackToken": "fb_remove_1" } }
                                                                        }
                                                                    }]
                                                                }
                                                            }
                                                        }
                                                    },
                                                    {
                                                        "reelShelfRenderer": {
                                                            "items": [{
                                                                "shortsLockupViewModel": {
                                                                    "entityId": "shorts-shelf-item-M7lc1UVf-VE",
                                                                    "overlayMetadata": {
                                                                        "primaryText": { "content": "Most Dangerous Job" },
                                                                        "secondaryText": { "content": "23 mi de visualizações" }
                                                                    },
                                                                    "onTap": { "innertubeCommand": { "reelWatchEndpoint": { "videoId": "M7lc1UVf-VE" } } }
                                                                }
                                                            }]
                                                        }
                                                    },
                                                    {
                                                        "lockupViewModel": {
                                                            "contentId": "aqz-KE-bpKQ",
                                                            "contentType": "LOCKUP_CONTENT_TYPE_PODCAST",
                                                            "metadata": {
                                                                "lockupMetadataViewModel": {
                                                                    "title": { "content": "Flow #673" },
                                                                    "metadata": {
                                                                        "contentMetadataViewModel": {
                                                                            "metadataRows": [{
                                                                                "metadataParts": [
                                                                                    { "text": { "content": "Flow Podcast", "attachmentRuns": [{}] } },
                                                                                    { "text": { "content": "383 mil visualizações" } }
                                                                                ]
                                                                            }]
                                                                        }
                                                                    }
                                                                }
                                                            },
                                                            "contentImage": {
                                                                "thumbnailViewModel": {
                                                                    "image": { "sources": [{ "url": "https://i.ytimg.com/vi/aqz-KE-bpKQ/hqdefault.jpg", "width": 480 }] },
                                                                    "overlays": [
                                                                        { "thumbnailOverlayProgressBarViewModel": { "startPercent": 25 } },
                                                                        { "thumbnailOverlayBadgeViewModel": { "thumbnailBadges": [{ "thumbnailBadgeViewModel": { "text": "26:30" } }] } }
                                                                    ]
                                                                }
                                                            },
                                                            "menu": { "feedbackEndpoint": { "feedbackToken": "fb_remove_2" } }
                                                        }
                                                    }
                                                ]
                                            }
                                        },
                                        {
                                            "continuationItemRenderer": {
                                                "continuationEndpoint": { "continuationCommand": { "token": "next_page_tok" } }
                                            }
                                        }
                                    ]
                                }
                            }
                        }
                    }],
                    "secondaryContents": {
                        "browseFeedActionsRenderer": {
                            "contents": [
                                {
                                    "buttonRenderer": {
                                        "text": { "simpleText": "Limpar todo o histórico de exibição" },
                                        "icon": { "iconType": "DELETE" },
                                        "navigationEndpoint": {
                                            "confirmDialogEndpoint": {
                                                "content": {
                                                    "confirmDialogRenderer": {
                                                        "confirmButton": {
                                                            "buttonRenderer": {
                                                                "serviceEndpoint": { "feedbackEndpoint": { "feedbackToken": "fb_clear_all" } }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                },
                                {
                                    "buttonRenderer": {
                                        "text": { "simpleText": "Pausar o histórico de visualizações" },
                                        "icon": { "iconType": "PAUSE" },
                                        "serviceEndpoint": { "feedbackEndpoint": { "feedbackToken": "fb_pause" } }
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

        let parsed = parse_history_response(&sample, "");
        assert!(!parsed.signed_out);
        assert_eq!(parsed.next_continuation.as_deref(), Some("next_page_tok"));
        assert_eq!(
            parsed.chips.get(&HistoryFilter::All),
            Some(&HistoryChipAction::BrowseParams("p_all".into()))
        );
        assert_eq!(
            parsed.chips.get(&HistoryFilter::Podcasts),
            Some(&HistoryChipAction::Continuation("tok_podcasts".into()))
        );
        assert_eq!(
            parsed.chips.get(&HistoryFilter::Music),
            Some(&HistoryChipAction::BrowseParams("p_music".into()))
        );
        assert_eq!(
            parsed.chips.get(&HistoryFilter::Shorts),
            Some(&HistoryChipAction::BrowseParams("p_shorts".into()))
        );
        assert_eq!(
            parsed.actions.clear_feedback_token.as_deref(),
            Some("fb_clear_all")
        );
        assert_eq!(
            parsed.actions.pause_feedback_token.as_deref(),
            Some("fb_pause")
        );
        assert_eq!(parsed.actions.remote_paused, Some(false));

        assert_eq!(parsed.sections.len(), 1);
        assert_eq!(parsed.sections[0].date_label, "Ontem");
        let items = &parsed.sections[0].items;
        assert_eq!(items.len(), 3);

        assert_eq!(items[0].video_id, "dQw4w9WgXcQ");
        assert!(items[0].is_music);
        assert!(items[0].channel_verified);
        assert_eq!(items[0].duration_ms, Some(342_000));
        assert_eq!(items[0].position_ms, 171_000);
        assert_eq!(items[0].feedback_token.as_deref(), Some("fb_remove_1"));

        assert_eq!(items[1].video_id, "M7lc1UVf-VE");
        assert!(items[1].is_short);

        assert_eq!(items[2].video_id, "aqz-KE-bpKQ");
        assert!(items[2].is_podcast);
        assert!(items[2].channel_verified);
        assert_eq!(items[2].duration_ms, Some(1_590_000));
        assert_eq!(items[2].position_ms, 397_500);
        assert_eq!(items[2].feedback_token.as_deref(), Some("fb_remove_2"));
    }

    #[test]
    fn builds_stats_urls_and_cpn_safely() {
        let cpn = generate_cpn();
        assert_eq!(cpn.len(), 16);
        assert!(
            cpn.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );

        assert_eq!(
            normalize_stats_url("https://s.youtube.com/api/stats/watchtime?docid=abc"),
            Some("https://www.youtube.com/api/stats/watchtime?docid=abc".into())
        );
        assert_eq!(
            normalize_stats_url("https://evil.example/api/stats/watchtime"),
            None
        );

        let playback = build_playback_ping_url(
            "https://www.youtube.com/api/stats/playback?docid=dQw4w9WgXcQ",
            &cpn,
            "2.20260901.00.00",
            12.5,
            Some(213.0),
            0.0,
        )
        .unwrap();
        assert!(playback.contains(&format!("cpn={cpn}")));
        assert!(playback.contains("cmt=12.500"));
        assert!(playback.contains("len=213.000"));
        assert!(playback.contains("ns=yt"));

        let watchtime = build_watchtime_ping_url(
            "https://www.youtube.com/api/stats/watchtime?docid=dQw4w9WgXcQ",
            &cpn,
            "2.20260901.00.00",
            12.5,
            27.5,
            27.5,
            Some(213.0),
            15.0,
            false,
            true,
        )
        .unwrap();
        assert!(watchtime.contains("st=12.500"));
        assert!(watchtime.contains("et=27.500"));
        assert!(watchtime.contains("cmt=27.500"));
        assert!(watchtime.contains("state=playing"));
        assert!(watchtime.contains("final=1"));
    }

    #[test]
    fn mock_http_verifies_browse_player_watchtime_and_feedback_requests() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let server = std::thread::spawn(move || {
            let mut recorded_requests = Vec::new();
            for _ in 0..5 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let header_end;
                loop {
                    let n = stream.read(&mut tmp).unwrap();
                    if n == 0 {
                        header_end = buf.len();
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = pos + 4;
                        break;
                    }
                }
                let headers_str = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let content_len = headers_str
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("content-length")
                            .then(|| v.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                while buf.len() < header_end + content_len {
                    let n = stream.read(&mut tmp).unwrap();
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                let body_str = String::from_utf8_lossy(&buf[header_end..]).to_string();
                let first_line = headers_str.lines().next().unwrap_or("").to_owned();
                let lower_headers = headers_str.to_ascii_lowercase();
                assert!(lower_headers.contains("cookie:"));
                let response_body = if first_line.starts_with("POST /browse") {
                    assert!(lower_headers.contains("authorization: sapisidhash "));
                    json!({
                        "contents": {
                            "twoColumnBrowseResultsRenderer": {
                                "tabs": [{
                                    "tabRenderer": {
                                        "content": {
                                            "sectionListRenderer": {
                                                "contents": [{
                                                    "itemSectionRenderer": {
                                                        "header": { "itemSectionHeaderRenderer": { "title": { "simpleText": "Hoje" } } },
                                                        "contents": [{
                                                            "videoRenderer": {
                                                                "videoId": "dQw4w9WgXcQ",
                                                                "title": { "simpleText": "Mock History Video" },
                                                                "ownerText": { "runs": [{ "text": "Channel" }] },
                                                                "lengthText": { "simpleText": "3:33" }
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
                    })
                    .to_string()
                } else if first_line.starts_with("POST /player") {
                    assert!(lower_headers.contains("authorization: sapisidhash "));
                    json!({
                        "videoDetails": { "lengthSeconds": "213" },
                        "microformat": { "playerMicroformatRenderer": { "category": "Music" } },
                        "playbackTracking": {
                            "videostatsPlaybackUrl": { "baseUrl": format!("http://127.0.0.1:{port}/api/stats/playback?docid=dQw4w9WgXcQ") },
                            "videostatsWatchtimeUrl": { "baseUrl": format!("http://127.0.0.1:{port}/api/stats/watchtime?docid=dQw4w9WgXcQ") }
                        }
                    })
                    .to_string()
                } else if first_line.starts_with("POST /feedback") {
                    assert!(body_str.contains("fb_tok_1"));
                    json!({ "feedbackResponses": [{ "isProcessed": true }] }).to_string()
                } else {
                    String::new()
                };
                recorded_requests.push(first_line);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response_body.len(),
                    response_body
                )
                .unwrap();
            }
            recorded_requests
        });

        let client = http_client().unwrap();
        let mut session = YouTubeWebSession::test_session();
        let cancel = Cancellation::default();

        let page = fetch_history_page_at(
            &base_url,
            &client,
            &mut session,
            None,
            None,
            "",
            "pt",
            &cancel,
        )
        .unwrap();
        assert_eq!(page.sections.len(), 1);
        assert_eq!(page.sections[0].items[0].video_id, "dQw4w9WgXcQ");

        let init = init_remote_watch_at(
            &base_url,
            &client,
            &mut session,
            "dQw4w9WgXcQ",
            5.0,
            &cancel,
        )
        .unwrap();
        assert!(init.tracker.is_some());
        assert_eq!(init.detected_music, Some(true));
        assert_eq!(init.duration_ms, Some(213_000));

        perform_history_feedback_at(&base_url, &client, &mut session, "fb_tok_1", "pt", &cancel)
            .unwrap();

        let reqs = server.join().unwrap();
        assert!(reqs.iter().any(|r| r.starts_with("POST /browse")));
        assert!(reqs.iter().any(|r| r.starts_with("POST /player")));
        assert!(
            reqs.iter()
                .any(|r| r.starts_with("GET /api/stats/playback"))
        );
        assert!(
            reqs.iter()
                .any(|r| r.starts_with("GET /api/stats/watchtime"))
        );
        assert!(reqs.iter().any(|r| r.starts_with("POST /feedback")));
    }

    #[test]
    fn initial_history_sync_caps_pages_and_preserves_continuation_token() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        // Serve up to INITIAL_HISTORY_SYNC_PAGES responses; each returns a unique
        // video and a next continuation token for subsequent on-demand load-more.
        let server = std::thread::spawn(move || {
            let mut request_count = 0usize;
            for page_idx in 0..INITIAL_HISTORY_SYNC_PAGES {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let header_end;
                loop {
                    let n = stream.read(&mut tmp).unwrap();
                    if n == 0 {
                        header_end = buf.len();
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = pos + 4;
                        break;
                    }
                }
                let headers_str = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let content_len = headers_str
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("content-length")
                            .then(|| v.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                while buf.len() < header_end + content_len {
                    let n = stream.read(&mut tmp).unwrap();
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                request_count += 1;
                let vid = format!("histpage{:03}", page_idx);
                let next_tok = format!("cont_token_{}", page_idx + 1);
                let response_body = json!({
                    "header": {
                        "feedFilterChipBarRenderer": {
                            "contents": [{
                                "chipCloudChipRenderer": {
                                    "text": { "simpleText": "Shorts" },
                                    "navigationEndpoint": {
                                        "browseEndpoint": { "browseId": "FEhistory", "params": "p_shorts" }
                                    }
                                }
                            }]
                        }
                    },
                    "contents": {
                        "twoColumnBrowseResultsRenderer": {
                            "tabs": [{
                                "tabRenderer": {
                                    "content": {
                                        "sectionListRenderer": {
                                            "contents": [
                                                {
                                                    "itemSectionRenderer": {
                                                        "header": { "itemSectionHeaderRenderer": { "title": { "simpleText": "Hoje" } } },
                                                        "contents": [{
                                                            "videoRenderer": {
                                                                "videoId": vid,
                                                                "title": { "simpleText": format!("Page {page_idx} Video") },
                                                                "ownerText": { "runs": [{ "text": "Channel" }] },
                                                                "lengthText": { "simpleText": "2:00" }
                                                            }
                                                        }]
                                                    }
                                                },
                                                {
                                                    "continuationItemRenderer": {
                                                        "continuationEndpoint": {
                                                            "continuationCommand": { "token": next_tok }
                                                        }
                                                    }
                                                }
                                            ]
                                        }
                                    }
                                }
                            }]
                        }
                    }
                })
                .to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response_body.len(),
                    response_body
                )
                .unwrap();
            }
            request_count
        });

        let client = http_client().unwrap();
        let mut session = YouTubeWebSession::test_session();
        let cancel = Cancellation::default();
        let (tx, rx) = mpsc::channel();

        let first_page = fetch_history_page_at(
            &base_url,
            &client,
            &mut session,
            None,
            None,
            "",
            "en",
            &cancel,
        )
        .unwrap();

        let total = fetch_initial_history_pages_at(
            &base_url,
            &client,
            &mut session,
            42,
            first_page,
            INITIAL_HISTORY_SYNC_PAGES,
            "en",
            &cancel,
            &tx,
            || {},
        );
        drop(tx);

        let pages: Vec<(HistoryFilter, usize, RemoteHistoryPage)> = rx
            .into_iter()
            .filter_map(|msg| match msg {
                HistorySyncMessage::Page {
                    filter,
                    page_index,
                    page,
                    ..
                } => Some((filter, page_index, page)),
                _ => None,
            })
            .collect();

        assert_eq!(server.join().unwrap(), INITIAL_HISTORY_SYNC_PAGES);
        assert_eq!(pages.len(), INITIAL_HISTORY_SYNC_PAGES);
        assert_eq!(total, INITIAL_HISTORY_SYNC_PAGES);
        // Page 0 preserves category chips for on-demand tab clicks; no eager category fetch ran.
        assert!(
            pages
                .iter()
                .all(|(filter, _, _)| *filter == HistoryFilter::All)
        );
        assert!(pages[0].2.chips.contains_key(&HistoryFilter::Shorts));
        // Final emitted page preserves next_continuation for on-demand load-more pagination.
        assert_eq!(
            pages.last().unwrap().2.next_continuation.as_deref(),
            Some("cont_token_3")
        );
    }

    #[test]
    fn remote_history_feed_lru_and_item_cap_bound_memory() {
        let mut lru = RemoteHistoryFeedLru::new(2);
        let mut feeds: HashMap<HistoryFilter, usize> = HashMap::new();

        feeds.insert(HistoryFilter::All, 1);
        assert!(lru.touch(HistoryFilter::All, &mut feeds).is_empty());

        feeds.insert(HistoryFilter::Shorts, 2);
        assert!(lru.touch(HistoryFilter::Shorts, &mut feeds).is_empty());

        // Adding a third filter evicts least-recently-used non-All filter (Shorts).
        feeds.insert(HistoryFilter::Music, 3);
        let evicted = lru.touch(HistoryFilter::Music, &mut feeds);
        assert_eq!(evicted, vec![HistoryFilter::Shorts]);
        assert_eq!(feeds.len(), 2);
        assert!(feeds.contains_key(&HistoryFilter::All));
        assert!(feeds.contains_key(&HistoryFilter::Music));

        let mut sections = vec![
            RemoteHistorySection {
                date_label: "Today".into(),
                items: (0..3)
                    .map(|i| RemoteHistoryItem {
                        video_id: format!("vid0000000{i}"),
                        ..Default::default()
                    })
                    .collect(),
            },
            RemoteHistorySection {
                date_label: "Yesterday".into(),
                items: (3..6)
                    .map(|i| RemoteHistoryItem {
                        video_id: format!("vid0000000{i}"),
                        ..Default::default()
                    })
                    .collect(),
            },
        ];
        let mut seen: HashSet<String> = (0..6).map(|i| format!("vid0000000{i}")).collect();
        let trimmed = enforce_sections_item_cap(&mut sections, &mut seen, 4);
        assert_eq!(trimmed, 2);
        assert_eq!(seen.len(), 4);
        assert!(seen.contains("vid00000000"));
        assert!(seen.contains("vid00000003"));
        assert!(!seen.contains("vid00000004"));
        assert!(!seen.contains("vid00000005"));
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[1].items.len(), 1);

        let mut page = RemoteHistoryPage {
            sections: vec![RemoteHistorySection {
                date_label: "Older".into(),
                items: (6..9)
                    .map(|i| RemoteHistoryItem {
                        video_id: format!("vid0000000{i}"),
                        ..Default::default()
                    })
                    .collect(),
            }],
            next_continuation: Some("more_tok".into()),
            ..Default::default()
        };
        assert!(cap_remote_history_page(&mut page, 4, 5));
        assert_eq!(page.sections[0].items.len(), 1);
        assert!(page.next_continuation.is_none());
    }

    #[test]
    fn dpapi_web_session_cache_reused_for_history_sync_and_invalidated_on_auth_failure() {
        use std::cell::Cell;

        let temp_dir = std::env::temp_dir().join(format!(
            "fasttube-history-dpapi-test-{}-{}",
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
                    "value": "sapisid_dpapi_history_token",
                    "domain": ".youtube.com",
                    "expires": (now_unix + 86_400) as f64
                }),
                json!({
                    "name": "SID",
                    "value": "sid_dpapi_history_token",
                    "domain": ".youtube.com",
                    "expires": (now_unix + 86_400) as f64
                }),
            ]
        };

        // 1. First call misses DPAPI cache -> calls cookie extractor and persists DPAPI file.
        let first =
            acquire_web_session_with_dpapi_cache_at(None, Some(&session_path), now_unix, || {
                extractor_calls.set(extractor_calls.get() + 1);
                Ok(sample_cookies())
            })
            .expect("initial history session extraction should succeed");
        assert_eq!(extractor_calls.get(), 1);
        assert!(session_path.exists());

        // Verify SAPISIDHASH authorization header matches crate::innertube::sapisidhash_header.
        let auth_hdr =
            build_innertube_authorization_header(&first, YOUTUBE_ORIGIN, now_unix).unwrap();
        assert_eq!(
            auth_hdr,
            crate::innertube::sapisidhash_header(
                "sapisid_dpapi_history_token",
                YOUTUBE_ORIGIN,
                now_unix,
            )
        );

        // 2. Subsequent call hits DPAPI cache -> zero headless browser launch latency!
        let cached = acquire_web_session_with_dpapi_cache_at(
            None,
            Some(&session_path),
            now_unix + 30,
            || {
                panic!("headless Chromium must not launch when valid DPAPI session is cached");
            },
        )
        .expect("cached DPAPI session should load");
        assert_eq!(extractor_calls.get(), 1);
        assert_eq!(cached.cookie_header(), first.cookie_header());

        // 3. Invalidate persisted DPAPI session on 401/403 auth failure.
        crate::innertube::invalidate_web_session_at(&session_path).unwrap();
        assert!(!session_path.exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn history_parsers_extract_published_time_and_nbsp_play_arrow_views() {
        let vr = serde_json::json!({
            "videoId": "dQw4w9WgXcQ",
            "title": { "simpleText": "Classic History Video" },
            "shortBylineText": { "runs": [{ "text": "Creator" }] },
            "lengthText": { "simpleText": "3:33" },
            "shortViewCountText": { "simpleText": "105\u{00a0}mil visualizações" },
            "publishedTimeText": { "simpleText": "há\u{00a0}7\u{00a0}meses" }
        });
        let parsed_vr = parse_history_video_renderer(&vr, "Hoje").expect("videoRenderer parsed");
        assert_eq!(parsed_vr.published_time_text.as_deref(), Some("há 7 meses"));
        assert_eq!(
            parsed_vr.to_video_metadata().published_time_text.as_deref(),
            Some("há 7 meses")
        );

        let lockup = serde_json::json!({
            "contentId": "AbxnXTCqdBc",
            "contentType": "LOCKUP_CONTENT_TYPE_VIDEO",
            "metadata": {
                "lockupMetadataViewModel": {
                    "title": { "content": "Modern Lockup History" },
                    "metadata": {
                        "contentMetadataViewModel": {
                            "metadataRows": [
                                {
                                    "metadataParts": [{
                                        "text": { "content": "XM360" }
                                    }]
                                },
                                {
                                    "metadataParts": [
                                        {
                                            "text": { "content": "11\u{00a0}mil" },
                                            "accessibilityLabel": "11 mil visualizações",
                                            "leadingIcon": { "name": "PLAY_ARROW_OUTLINED" }
                                        },
                                        {
                                            "text": { "content": "há\u{00a0}2\u{00a0}h" },
                                            "accessibilityLabel": "há 2 horas"
                                        }
                                    ]
                                }
                            ]
                        }
                    }
                }
            }
        });
        let parsed_lockup =
            parse_history_lockup_view_model(&lockup, "Hoje").expect("lockup parsed");
        assert_eq!(parsed_lockup.channel, "XM360");
        assert_eq!(parsed_lockup.view_count, Some(11_000));
        assert_eq!(parsed_lockup.view_count_text.as_deref(), Some("11 mil"));
        assert_eq!(parsed_lockup.published_time_text.as_deref(), Some("há 2 h"));
    }
}
