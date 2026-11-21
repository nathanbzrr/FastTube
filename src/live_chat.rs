use crate::comment_actions::{
    FALLBACK_CLIENT_VERSION, INNERTUBE_BASE_URL, InnerTubeError, WEB_USER_AGENT, YOUTUBE_ORIGIN,
    YouTubeWebSession, extract_json_string_field, http_client, parse_innertube_error_json,
    post_innertube_at,
};
use crate::process::Cancellation;
use eframe::egui;
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

const MAX_LIVE_CHAT_MESSAGES: usize = 250;
const MIN_POLL_TIMEOUT_MS: u64 = 1_000;
const MAX_POLL_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_POLL_TIMEOUT_MS: u64 = 3_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveChatMode {
    pub label: String,
    pub continuation: String,
    pub selected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveChatBadgeKind {
    Owner,
    Moderator,
    Member,
    Verified,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveChatMessageKind {
    Text,
    Paid {
        amount: String,
        header_bg: u32,
        body_bg: u32,
    },
    Membership {
        header_subtext: String,
    },
    Engagement,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveChatMessage {
    pub id: String,
    pub author_name: String,
    pub author_channel_id: Option<String>,
    pub author_avatar_url: Option<String>,
    pub badges: Vec<LiveChatBadgeKind>,
    pub text: String,
    pub timestamp_text: Option<String>,
    pub kind: LiveChatMessageKind,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WatchLiveInfo {
    pub video_id: String,
    pub is_live: bool,
    pub is_live_dvr: bool,
    pub title: Option<String>,
    pub channel: Option<String>,
    pub channel_id: Option<String>,
    pub channel_url: Option<String>,
    pub channel_avatar_url: Option<String>,
    pub subscriber_count_text: Option<String>,
    pub view_count_text: Option<String>,
    pub concurrent_view_count: Option<u64>,
    pub started_text: Option<String>,
    pub description: Option<String>,
    pub modes: Vec<LiveChatMode>,
    pub initial_continuation: Option<String>,
    pub client_version: String,
    pub visitor_data: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LiveChatBatch {
    pub added: Vec<LiveChatMessage>,
    pub replaced: Vec<(String, LiveChatMessage)>,
    pub deleted_ids: Vec<String>,
    pub deleted_author_channel_ids: Vec<String>,
    pub modes: Vec<LiveChatMode>,
    pub send_params: Option<String>,
    pub next_continuation: Option<String>,
    pub timeout_ms: u64,
    pub client_version: Option<String>,
    pub visitor_data: Option<String>,
}

pub enum WatchLiveEvent {
    WatchInfo {
        session_id: u64,
        result: Box<Result<WatchLiveInfo, String>>,
    },
    WebSessionUpdated(YouTubeWebSession),
}

pub enum LiveChatStreamEvent {
    Batch {
        session_id: u64,
        stream_id: u64,
        result: Result<LiveChatBatch, String>,
    },
    WebSessionUpdated(YouTubeWebSession),
}

pub struct LiveChatSendEvent {
    pub session_id: u64,
    pub web_session: Option<YouTubeWebSession>,
    pub auth_expired: bool,
    pub result: Result<Vec<LiveChatMessage>, String>,
}

#[derive(Default)]
pub struct LiveWatchState {
    pub session_id: u64,
    pub info_revision: u64,
    pub video_url: Option<String>,
    pub video_id: Option<String>,
    pub info: Option<WatchLiveInfo>,
    pub watch_info_loading: bool,
    pub watch_info_error: Option<String>,
    pub watch_rx: Option<Receiver<WatchLiveEvent>>,
    pub watch_cancel: Option<Cancellation>,

    pub chat_visible: bool,
    pub chat_user_toggled: bool,
    pub chat_stream_id: u64,
    pub chat_loading: bool,
    pub chat_error: Option<String>,
    pub chat_modes: Vec<LiveChatMode>,
    pub active_continuation: Option<String>,
    pub send_params: Option<String>,
    pub client_version: String,
    pub visitor_data: Option<String>,
    pub messages: VecDeque<LiveChatMessage>,
    pub seen_ids: HashSet<String>,
    pub auto_scroll: bool,
    pub filter_query: String,
    pub composer_draft: String,
    pub send_error: Option<String>,
    pub sending: bool,
    pub stream_rx: Option<Receiver<LiveChatStreamEvent>>,
    pub stream_cancel: Option<Cancellation>,
    pub send_rx: Option<Receiver<LiveChatSendEvent>>,
    pub send_cancel: Option<Cancellation>,
}

fn normalize_filter_str(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars().flat_map(char::to_lowercase) {
        let mapped = match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            'ý' | 'ÿ' => 'y',
            '\u{0300}'..='\u{036f}' => continue,
            other => other,
        };
        out.push(mapped);
    }
    out
}

pub fn message_matches_filter(msg: &LiveChatMessage, query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return true;
    }
    let needle = normalize_filter_str(trimmed);
    let role_match = match needle.as_str() {
        "@mod" | "mod" | "moderador" | "moderator" => {
            msg.badges.contains(&LiveChatBadgeKind::Moderator)
        }
        "@member" | "member" | "membro" => {
            msg.badges.contains(&LiveChatBadgeKind::Member)
                || matches!(msg.kind, LiveChatMessageKind::Membership { .. })
        }
        "@owner" | "owner" | "dono" => msg.badges.contains(&LiveChatBadgeKind::Owner),
        "@verified" | "verified" | "verificado" => {
            msg.badges.contains(&LiveChatBadgeKind::Verified)
        }
        "@superchat" | "superchat" | "paid" => {
            matches!(msg.kind, LiveChatMessageKind::Paid { .. })
        }
        _ => false,
    };
    if role_match {
        return true;
    }
    let author_norm = normalize_filter_str(&msg.author_name);
    if author_norm.contains(&needle)
        || needle
            .strip_prefix('@')
            .is_some_and(|stripped| !stripped.is_empty() && author_norm.contains(stripped))
    {
        return true;
    }
    if normalize_filter_str(&msg.text).contains(&needle) {
        return true;
    }
    match &msg.kind {
        LiveChatMessageKind::Membership { header_subtext } => {
            normalize_filter_str(header_subtext).contains(&needle)
        }
        LiveChatMessageKind::Paid { amount, .. } => normalize_filter_str(amount).contains(&needle),
        LiveChatMessageKind::Text | LiveChatMessageKind::Engagement => false,
    }
}

impl LiveWatchState {
    pub fn reset_for_video(&mut self, video_url: Option<&str>, video_id: Option<String>) {
        if let Some(cancel) = self.watch_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.stream_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.send_cancel.take() {
            cancel.cancel();
        }
        self.session_id = self.session_id.wrapping_add(1);
        self.info_revision = self.info_revision.wrapping_add(1);
        self.video_url = video_url.map(str::to_owned);
        self.video_id = video_id;
        self.info = None;
        self.watch_info_loading = false;
        self.watch_info_error = None;
        self.watch_rx = None;

        self.chat_visible = false;
        self.chat_user_toggled = false;
        self.chat_stream_id = self.chat_stream_id.wrapping_add(1);
        self.chat_loading = false;
        self.chat_error = None;
        self.chat_modes.clear();
        self.active_continuation = None;
        self.send_params = None;
        self.client_version = FALLBACK_CLIENT_VERSION.to_owned();
        self.visitor_data = None;
        self.messages.clear();
        self.seen_ids.clear();
        self.auto_scroll = true;
        self.filter_query.clear();
        self.composer_draft.clear();
        self.send_error = None;
        self.sending = false;
        self.stream_rx = None;
        self.send_rx = None;
    }

    #[allow(dead_code)]
    pub fn filtered_messages(&self) -> Vec<&LiveChatMessage> {
        self.messages
            .iter()
            .filter(|msg| message_matches_filter(msg, &self.filter_query))
            .collect()
    }

    pub fn stop_chat_stream(&mut self) {
        if let Some(cancel) = self.stream_cancel.take() {
            cancel.cancel();
        }
        self.chat_stream_id = self.chat_stream_id.wrapping_add(1);
        self.chat_loading = false;
        self.stream_rx = None;
    }

    pub fn has_chat(&self) -> bool {
        self.info
            .as_ref()
            .is_some_and(|info| info.initial_continuation.is_some() || !info.modes.is_empty())
            || self.active_continuation.is_some()
            || !self.messages.is_empty()
    }

    pub fn apply_batch(&mut self, batch: LiveChatBatch) {
        if let Some(version) = batch.client_version {
            self.client_version = version;
        }
        if let Some(visitor) = batch.visitor_data {
            self.visitor_data = Some(visitor);
        }
        if !batch.modes.is_empty() {
            self.chat_modes = batch.modes;
        }
        if batch.send_params.is_some() {
            self.send_params = batch.send_params;
        }
        if let Some(next) = batch.next_continuation {
            self.active_continuation = Some(next);
        }
        if !batch.deleted_ids.is_empty() {
            let deleted: HashSet<&str> = batch.deleted_ids.iter().map(String::as_str).collect();
            self.messages
                .retain(|msg| !deleted.contains(msg.id.as_str()));
            for id in &batch.deleted_ids {
                self.seen_ids.remove(id);
            }
        }
        if !batch.deleted_author_channel_ids.is_empty() {
            let deleted_authors: HashSet<&str> = batch
                .deleted_author_channel_ids
                .iter()
                .map(String::as_str)
                .collect();
            let mut removed_ids = Vec::new();
            self.messages.retain(|msg| {
                let drop = msg
                    .author_channel_id
                    .as_deref()
                    .is_some_and(|id| deleted_authors.contains(id));
                if drop {
                    removed_ids.push(msg.id.clone());
                }
                !drop
            });
            for id in removed_ids {
                self.seen_ids.remove(&id);
            }
        }
        for (target_id, replacement) in batch.replaced {
            if let Some(existing) = self.messages.iter_mut().find(|m| m.id == target_id) {
                if !replacement.id.is_empty() && replacement.id != target_id {
                    self.seen_ids.remove(&target_id);
                    self.seen_ids.insert(replacement.id.clone());
                }
                *existing = replacement;
            }
        }
        for msg in batch.added {
            self.push_message(msg);
        }
    }

    pub fn push_message(&mut self, msg: LiveChatMessage) {
        if !msg.id.is_empty() && !self.seen_ids.insert(msg.id.clone()) {
            return;
        }
        while self.messages.len() >= MAX_LIVE_CHAT_MESSAGES {
            if let Some(removed) = self.messages.pop_front()
                && !removed.id.is_empty()
            {
                self.seen_ids.remove(&removed.id);
            }
        }
        self.messages.push_back(msg);
    }
}

pub fn start_watch_live_fetch(
    session_id: u64,
    video_id: String,
    web_session: Option<YouTubeWebSession>,
    ctx: &egui::Context,
) -> (Receiver<WatchLiveEvent>, Cancellation) {
    let (tx, rx) = mpsc::channel();
    let cancel = Cancellation::default();
    let cancel_bg = cancel.clone();
    let ctx_clone = ctx.clone();

    std::thread::spawn(move || {
        let mut maybe_session = web_session;
        let result = fetch_watch_live_info(&video_id, maybe_session.as_mut(), &cancel_bg);
        if !cancel_bg.cancelled() {
            if let Some(session) = maybe_session {
                let _ = tx.send(WatchLiveEvent::WebSessionUpdated(session));
            }
            let _ = tx.send(WatchLiveEvent::WatchInfo {
                session_id,
                result: Box::new(result),
            });
            ctx_clone.request_repaint();
        }
    });

    (rx, cancel)
}

pub fn start_live_chat_stream(
    session_id: u64,
    stream_id: u64,
    initial_continuation: String,
    client_version: String,
    visitor_data: Option<String>,
    web_session: Option<YouTubeWebSession>,
    ctx: &egui::Context,
) -> (Receiver<LiveChatStreamEvent>, Cancellation) {
    let (tx, rx) = mpsc::channel();
    let cancel = Cancellation::default();
    let cancel_bg = cancel.clone();
    let ctx_clone = ctx.clone();

    std::thread::spawn(move || {
        let client = match http_client() {
            Ok(client) => client,
            Err(error) => {
                if !cancel_bg.cancelled() {
                    let _ = tx.send(LiveChatStreamEvent::Batch {
                        session_id,
                        stream_id,
                        result: Err(error),
                    });
                    ctx_clone.request_repaint();
                }
                return;
            }
        };

        let mut continuation = Some(initial_continuation);
        let mut active_version = client_version;
        let mut active_visitor = visitor_data;
        let mut maybe_session = web_session;
        let mut consecutive_errors = 0usize;

        while let Some(cont) = continuation.take() {
            if cancel_bg.cancelled() {
                break;
            }
            let step = fetch_live_chat_batch_at(
                INNERTUBE_BASE_URL,
                &client,
                &cont,
                &active_version,
                active_visitor.as_deref(),
                maybe_session.as_mut(),
                &cancel_bg,
            );
            if cancel_bg.cancelled() {
                break;
            }
            match step {
                Ok(batch) => {
                    consecutive_errors = 0;
                    if let Some(v) = &batch.client_version {
                        active_version = v.clone();
                    }
                    if let Some(vis) = &batch.visitor_data {
                        active_visitor = Some(vis.clone());
                    }
                    let timeout_ms = batch.timeout_ms;
                    let next_cont = batch.next_continuation.clone();
                    if let Some(sess) = &maybe_session {
                        let _ = tx.send(LiveChatStreamEvent::WebSessionUpdated(sess.clone()));
                    }
                    let send_ok = tx
                        .send(LiveChatStreamEvent::Batch {
                            session_id,
                            stream_id,
                            result: Ok(batch),
                        })
                        .is_ok();
                    ctx_clone.request_repaint();
                    if !send_ok || next_cont.is_none() {
                        break;
                    }
                    continuation = next_cont;
                    if !sleep_cancellable(Duration::from_millis(timeout_ms), &cancel_bg) {
                        break;
                    }
                }
                Err(InnerTubeError::Cancelled(_)) => break,
                Err(InnerTubeError::AuthExpired(_)) if maybe_session.is_some() => {
                    // Drop stale web session and fall back to public read-only chat polling.
                    maybe_session = None;
                    continuation = Some(cont);
                }
                Err(err) => {
                    consecutive_errors += 1;
                    let msg = err.into_message();
                    let send_ok = tx
                        .send(LiveChatStreamEvent::Batch {
                            session_id,
                            stream_id,
                            result: Err(msg),
                        })
                        .is_ok();
                    ctx_clone.request_repaint();
                    if !send_ok || consecutive_errors >= 3 {
                        break;
                    }
                    continuation = Some(cont);
                    if !sleep_cancellable(Duration::from_secs(3), &cancel_bg) {
                        break;
                    }
                }
            }
        }
    });

    (rx, cancel)
}

#[allow(clippy::too_many_arguments)]
pub fn start_send_live_chat_message(
    session_id: u64,
    cached_session: Option<YouTubeWebSession>,
    allow_interactive_login: bool,
    send_params: String,
    message_text: String,
    client_version: String,
    visitor_data: Option<String>,
    ctx: &egui::Context,
) -> (Receiver<LiveChatSendEvent>, Cancellation) {
    let (tx, rx) = mpsc::channel();
    let cancel = Cancellation::default();
    let cancel_bg = cancel.clone();
    let ctx_clone = ctx.clone();

    std::thread::spawn(move || {
        let outcome = send_live_chat_message_worker(
            session_id,
            cached_session,
            allow_interactive_login,
            &send_params,
            &message_text,
            &client_version,
            visitor_data.as_deref(),
            &cancel_bg,
        );
        if !cancel_bg.cancelled() {
            let _ = tx.send(outcome);
            ctx_clone.request_repaint();
        }
    });

    (rx, cancel)
}

#[allow(clippy::too_many_arguments)]
fn send_live_chat_message_worker(
    session_id: u64,
    cached_session: Option<YouTubeWebSession>,
    allow_interactive_login: bool,
    send_params: &str,
    message_text: &str,
    client_version: &str,
    visitor_data: Option<&str>,
    cancel: &Cancellation,
) -> LiveChatSendEvent {
    let trimmed = message_text.trim();
    if trimmed.is_empty() {
        return LiveChatSendEvent {
            session_id,
            web_session: cached_session,
            auth_expired: false,
            result: Err("Digite uma mensagem para enviar.".into()),
        };
    }

    let client = match http_client() {
        Ok(c) => c,
        Err(err) => {
            return LiveChatSendEvent {
                session_id,
                web_session: cached_session,
                auth_expired: false,
                result: Err(err),
            };
        }
    };

    let mut session = match crate::comment_actions::acquire_web_session(
        cached_session,
        allow_interactive_login,
        cancel,
    ) {
        Ok(s) => s,
        Err(err) => {
            return LiveChatSendEvent {
                session_id,
                web_session: None,
                auth_expired: false,
                result: Err(err),
            };
        }
    };

    if session.client_version.is_empty() {
        session.client_version = client_version.to_owned();
    }
    if session.visitor_data.is_none() {
        session.visitor_data = visitor_data.map(str::to_owned);
    }
    session.refresh_client_config_if_needed(&client, cancel);

    match send_live_chat_message_at(
        INNERTUBE_BASE_URL,
        &client,
        &mut session,
        send_params,
        trimmed,
        cancel,
    ) {
        Ok(msgs) => LiveChatSendEvent {
            session_id,
            web_session: Some(session),
            auth_expired: false,
            result: Ok(msgs),
        },
        Err(InnerTubeError::AuthExpired(err)) => LiveChatSendEvent {
            session_id,
            web_session: None,
            auth_expired: true,
            result: Err(err),
        },
        Err(err) => LiveChatSendEvent {
            session_id,
            web_session: Some(session),
            auth_expired: false,
            result: Err(err.into_message()),
        },
    }
}

fn sleep_cancellable(duration: Duration, cancel: &Cancellation) -> bool {
    let mut remaining = duration;
    let slice = Duration::from_millis(100);
    while !remaining.is_zero() {
        if cancel.cancelled() {
            return false;
        }
        let step = remaining.min(slice);
        std::thread::sleep(step);
        remaining = remaining.saturating_sub(step);
    }
    !cancel.cancelled()
}

pub fn fetch_watch_live_info(
    video_id: &str,
    mut web_session: Option<&mut YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<WatchLiveInfo, String> {
    use std::io::Read;
    const MAX_WATCH_HTML_BYTES: usize = 8 * 1024 * 1024;
    if cancel.cancelled() {
        return Err("Consulta ao vivo cancelada.".into());
    }
    let client = http_client()?;
    let url = format!("{YOUTUBE_ORIGIN}/watch?v={video_id}&hl=pt");
    let mut request = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, WEB_USER_AGENT)
        .header(
            reqwest::header::ACCEPT_LANGUAGE,
            "pt-BR,pt;q=0.9,en-US;q=0.8",
        );
    if let Some(session) = web_session.as_deref() {
        request = request.header(reqwest::header::COOKIE, session.cookie_header());
    }
    let response = request.send().map_err(|_| {
        "Falha de conex\u{00e3}o ao consultar dados da transmiss\u{00e3}o.".to_owned()
    })?;
    if cancel.cancelled() {
        return Err("Consulta ao vivo cancelada.".into());
    }
    if let Some(session) = &mut web_session {
        session.absorb_set_cookies(response.headers());
    }
    if !response.status().is_success() {
        return Err(format!(
            "O YouTube retornou HTTP {} ao abrir a transmiss\u{00e3}o.",
            response.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    response
        .take((MAX_WATCH_HTML_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            "N\u{00e3}o foi poss\u{00ed}vel ler a p\u{00e1}gina do v\u{00ed}deo.".to_owned()
        })?;
    if bytes.len() > MAX_WATCH_HTML_BYTES {
        return Err("A p\u{00e1}gina do v\u{00ed}deo excedeu o limite de 8 MiB.".into());
    }
    let html = String::from_utf8_lossy(&bytes).into_owned();
    if let Some(session) = web_session {
        if let Some(ver) = extract_json_string_field(&html, "INNERTUBE_CLIENT_VERSION") {
            session.client_version = ver;
        }
        if let Some(vis) = extract_json_string_field(&html, "VISITOR_DATA") {
            session.visitor_data = Some(vis);
        }
        session.client_config_refreshed = true;
    }
    parse_watch_live_info(video_id, &html)
}

pub fn parse_watch_live_info(video_id: &str, html: &str) -> Result<WatchLiveInfo, String> {
    let player_response = extract_assigned_json_object(html, "ytInitialPlayerResponse");
    let initial_data = extract_assigned_json_object(html, "ytInitialData");
    if player_response.is_none() && initial_data.is_none() {
        return Err("Dados da transmiss\u{00e3}o n\u{00e3}o encontrados na p\u{00e1}gina.".into());
    }

    let client_version = extract_json_string_field(html, "INNERTUBE_CLIENT_VERSION")
        .unwrap_or_else(|| FALLBACK_CLIENT_VERSION.to_owned());
    let visitor_data = extract_json_string_field(html, "VISITOR_DATA");

    Ok(build_watch_live_info(
        video_id,
        player_response.as_ref(),
        initial_data.as_ref(),
        client_version,
        visitor_data,
    ))
}

pub fn build_watch_live_info(
    video_id: &str,
    player_response: Option<&Value>,
    initial_data: Option<&Value>,
    client_version: String,
    visitor_data: Option<String>,
) -> WatchLiveInfo {
    let video_details = player_response.and_then(|v| v.get("videoDetails"));
    let microformat = player_response
        .and_then(|v| v.get("microformat"))
        .and_then(|v| v.get("playerMicroformatRenderer"));
    let live_details = microformat.and_then(|v| v.get("liveBroadcastDetails"));

    let mut is_live = video_details
        .and_then(|v| v.get("isLive"))
        .and_then(Value::as_bool)
        == Some(true)
        || live_details
            .and_then(|v| v.get("isLiveNow"))
            .and_then(Value::as_bool)
            == Some(true);

    let is_live_dvr = video_details
        .and_then(|v| v.get("isLiveDvrEnabled"))
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let title = video_details
        .and_then(|v| v.get("title"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let mut channel = video_details
        .and_then(|v| v.get("author"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let mut channel_id = video_details
        .and_then(|v| v.get("channelId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let mut channel_url = channel_id
        .as_deref()
        .map(|id| format!("{YOUTUBE_ORIGIN}/channel/{id}"));
    let description = video_details
        .and_then(|v| v.get("shortDescription"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let mut channel_avatar_url = None;
    let mut subscriber_count_text = None;
    let mut view_count_text = None;
    let mut concurrent_view_count = None;
    let mut started_text = None;
    let mut modes = Vec::new();
    let mut initial_continuation = None;

    if let Some(data) = initial_data {
        let two_col = data
            .get("contents")
            .and_then(|c| c.get("twoColumnWatchNextResults"));

        if let Some(contents) = two_col
            .and_then(|tc| tc.get("results"))
            .and_then(|r| r.get("results"))
            .and_then(|r| r.get("contents"))
            .and_then(Value::as_array)
        {
            for item in contents {
                if let Some(primary) = item.get("videoPrimaryInfoRenderer") {
                    if let Some(vc_renderer) = primary
                        .get("viewCount")
                        .and_then(|v| v.get("videoViewCountRenderer"))
                    {
                        if vc_renderer.get("isLive").and_then(Value::as_bool) == Some(true) {
                            is_live = true;
                        }
                        if let Some(vc_text) = vc_renderer
                            .get("viewCount")
                            .and_then(extract_yt_text)
                            .filter(|s| !s.is_empty())
                        {
                            if concurrent_view_count.is_none() {
                                concurrent_view_count = parse_digits_count(&vc_text);
                            }
                            view_count_text = Some(vc_text);
                        }
                    }
                    if let Some(date_text) = primary
                        .get("dateText")
                        .and_then(extract_yt_text)
                        .filter(|s| !s.is_empty())
                    {
                        started_text = Some(date_text);
                    }
                }
                if let Some(secondary) = item.get("videoSecondaryInfoRenderer")
                    && let Some(owner) = secondary
                        .get("owner")
                        .and_then(|o| o.get("videoOwnerRenderer"))
                {
                    if channel.is_none() {
                        channel = owner.get("title").and_then(extract_yt_text);
                    }
                    if channel_avatar_url.is_none() {
                        channel_avatar_url =
                            owner.get("thumbnail").and_then(extract_best_thumbnail_url);
                    }
                    if subscriber_count_text.is_none() {
                        subscriber_count_text = owner
                            .get("subscriberCountText")
                            .and_then(extract_yt_text)
                            .filter(|s| !s.is_empty());
                    }
                    if let Some(browse) = owner
                        .get("navigationEndpoint")
                        .and_then(|n| n.get("browseEndpoint"))
                    {
                        if channel_id.is_none() {
                            channel_id = browse
                                .get("browseId")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_owned);
                        }
                        if let Some(canon) = browse
                            .get("canonicalBaseUrl")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        {
                            channel_url = Some(if canon.starts_with("http") {
                                canon.to_owned()
                            } else {
                                format!("{YOUTUBE_ORIGIN}{canon}")
                            });
                        }
                    }
                }
            }
        }

        if let Some(chat_renderer) = two_col
            .and_then(|tc| tc.get("conversationBar"))
            .and_then(|cb| cb.get("liveChatRenderer"))
        {
            modes = parse_live_chat_modes(chat_renderer);
            initial_continuation = chat_renderer
                .get("continuations")
                .and_then(Value::as_array)
                .and_then(|arr| arr.iter().find_map(extract_continuation_from_entry))
                .or_else(|| {
                    modes
                        .iter()
                        .find(|m| m.selected)
                        .or_else(|| modes.first())
                        .map(|m| m.continuation.clone())
                });
        }
    }

    WatchLiveInfo {
        video_id: video_id.to_owned(),
        is_live,
        is_live_dvr,
        title,
        channel,
        channel_id,
        channel_url,
        channel_avatar_url,
        subscriber_count_text,
        view_count_text,
        concurrent_view_count,
        started_text,
        description,
        modes,
        initial_continuation,
        client_version,
        visitor_data,
    }
}

pub fn fetch_live_chat_batch_at(
    base_url: &str,
    client: &reqwest::blocking::Client,
    continuation: &str,
    client_version: &str,
    visitor_data: Option<&str>,
    web_session: Option<&mut YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<LiveChatBatch, InnerTubeError> {
    if cancel.cancelled() {
        return Err(InnerTubeError::Cancelled("Chat ao vivo cancelado.".into()));
    }
    let effective_version = web_session
        .as_deref()
        .map(|s| s.client_version.as_str())
        .filter(|v| !v.is_empty())
        .unwrap_or(client_version);
    let effective_visitor = web_session
        .as_deref()
        .and_then(|s| s.visitor_data.as_deref())
        .or(visitor_data);

    let mut client_ctx = json!({
        "clientName": "WEB",
        "clientVersion": effective_version,
        "hl": "pt",
        "gl": "BR",
    });
    if let Some(visitor) = effective_visitor
        && let Some(obj) = client_ctx.as_object_mut()
    {
        obj.insert("visitorData".into(), Value::String(visitor.to_owned()));
    }

    let body = json!({
        "context": { "client": client_ctx },
        "continuation": continuation,
    });
    let url = format!("{base_url}/live_chat/get_live_chat?prettyPrint=false");
    let mut request = client
        .post(&url)
        .header(reqwest::header::USER_AGENT, WEB_USER_AGENT)
        .header("Origin", YOUTUBE_ORIGIN)
        .header("X-Origin", YOUTUBE_ORIGIN)
        .header(reqwest::header::REFERER, format!("{YOUTUBE_ORIGIN}/"))
        .header("X-Youtube-Client-Name", "1")
        .header("X-Youtube-Client-Version", effective_version)
        .json(&body);
    if let Some(visitor) = effective_visitor {
        request = request.header("X-Goog-Visitor-Id", visitor);
    }
    if let Some(session) = web_session.as_deref() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if let Ok(auth) = session.authorization_header(ts, YOUTUBE_ORIGIN) {
            request = request
                .header(reqwest::header::COOKIE, session.cookie_header())
                .header(reqwest::header::AUTHORIZATION, auth)
                .header("X-Goog-AuthUser", "0");
        }
    }

    let response = request.send().map_err(|_| {
        InnerTubeError::Other("Falha de conex\u{00e3}o ao carregar o chat ao vivo.".into())
    })?;
    if cancel.cancelled() {
        return Err(InnerTubeError::Cancelled("Chat ao vivo cancelado.".into()));
    }
    if let Some(session) = web_session {
        session.absorb_set_cookies(response.headers());
    }
    let status = response.status();
    let text = response.text().unwrap_or_default();
    let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);

    if matches!(status.as_u16(), 401 | 403) {
        return Err(InnerTubeError::AuthExpired(
            "Sua sess\u{00e3}o do YouTube expirou. Entre novamente.".into(),
        ));
    }
    if !status.is_success() {
        if let Some(err) = parse_innertube_error_json(&json, status.as_u16()) {
            return Err(err);
        }
        return Err(InnerTubeError::Other(format!(
            "Erro ao atualizar o chat ao vivo (HTTP {}).",
            status.as_u16()
        )));
    }
    if let Some(err) = parse_innertube_error_json(&json, status.as_u16()) {
        return Err(err);
    }
    Ok(parse_live_chat_response(&json))
}

pub fn send_live_chat_message_at(
    base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    send_params: &str,
    message_text: &str,
    cancel: &Cancellation,
) -> Result<Vec<LiveChatMessage>, InnerTubeError> {
    let client_msg_id = format!(
        "ft-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let body = json!({
        "params": send_params,
        "clientMessageId": client_msg_id,
        "richMessage": {
            "textSegments": [{ "text": message_text }]
        }
    });
    let resp = post_innertube_at(
        base_url,
        client,
        session,
        "live_chat/send_message",
        body,
        cancel,
    )?;
    let mut out = Vec::new();
    if let Some(actions) = resp.get("actions").and_then(Value::as_array) {
        for action in actions {
            if let Some(item) = action.get("addChatItemAction").and_then(|a| a.get("item"))
                && let Some(msg) = parse_live_chat_item(item)
            {
                out.push(msg);
            }
        }
    }
    Ok(out)
}

pub fn parse_live_chat_response(json: &Value) -> LiveChatBatch {
    let Some(chat) = json
        .get("continuationContents")
        .and_then(|c| c.get("liveChatContinuation"))
    else {
        return LiveChatBatch {
            timeout_ms: DEFAULT_POLL_TIMEOUT_MS,
            ..Default::default()
        };
    };

    let mut batch = LiveChatBatch {
        timeout_ms: DEFAULT_POLL_TIMEOUT_MS,
        ..Default::default()
    };

    batch.modes = parse_live_chat_modes(chat);

    if let Some(conts) = chat.get("continuations").and_then(Value::as_array) {
        for entry in conts {
            if let Some((cont, timeout_ms)) = extract_continuation_and_timeout(entry) {
                batch.next_continuation = Some(cont);
                if let Some(ms) = timeout_ms {
                    batch.timeout_ms = ms.clamp(MIN_POLL_TIMEOUT_MS, MAX_POLL_TIMEOUT_MS);
                }
                break;
            }
        }
    }

    batch.send_params = chat
        .get("actionPanel")
        .and_then(|ap| ap.get("liveChatMessageInputRenderer"))
        .and_then(|inp| inp.get("sendButton"))
        .and_then(|sb| sb.get("buttonRenderer"))
        .and_then(|br| br.get("serviceEndpoint"))
        .and_then(|se| se.get("sendLiveChatMessageEndpoint"))
        .and_then(|ep| ep.get("params"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    if let Some(actions) = chat.get("actions").and_then(Value::as_array) {
        for action in actions {
            if let Some(add) = action.get("addChatItemAction")
                && let Some(item) = add.get("item")
                && let Some(msg) = parse_live_chat_item(item)
            {
                batch.added.push(msg);
            } else if let Some(replace) = action.get("replaceChatItemAction") {
                if let Some(target_id) = replace
                    .get("targetItemId")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    && let Some(item) = replace.get("replacementItem")
                    && let Some(msg) = parse_live_chat_item(item)
                {
                    batch.replaced.push((target_id.to_owned(), msg));
                }
            } else if let Some(remove) = action.get("removeChatItemAction") {
                if let Some(target_id) = remove
                    .get("targetItemId")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    batch.deleted_ids.push(target_id.to_owned());
                }
            } else if let Some(mark_del) = action.get("markChatItemAsDeletedAction") {
                if let Some(target_id) = mark_del
                    .get("targetItemId")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    batch.deleted_ids.push(target_id.to_owned());
                }
            } else if let Some(mark_author) = action.get("markChatItemsByAuthorAsDeletedAction")
                && let Some(ext_id) = mark_author
                    .get("externalChannelId")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            {
                batch.deleted_author_channel_ids.push(ext_id.to_owned());
            }
        }
    }

    batch
}

fn parse_live_chat_modes(chat_renderer: &Value) -> Vec<LiveChatMode> {
    let mut modes = Vec::new();
    let Some(items) = chat_renderer
        .get("header")
        .and_then(|h| h.get("liveChatHeaderRenderer"))
        .and_then(|hr| hr.get("viewSelector"))
        .and_then(|vs| vs.get("sortFilterSubMenuRenderer"))
        .and_then(|sf| sf.get("subMenuItems"))
        .and_then(Value::as_array)
    else {
        return modes;
    };
    for item in items {
        let label = item
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let continuation = item
            .get("continuation")
            .and_then(extract_continuation_from_entry);
        let selected = item
            .get("selected")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if let (Some(label), Some(continuation)) = (label, continuation) {
            modes.push(LiveChatMode {
                label,
                continuation,
                selected,
            });
        }
    }
    modes
}

fn extract_continuation_from_entry(entry: &Value) -> Option<String> {
    extract_continuation_and_timeout(entry).map(|(c, _)| c)
}

fn extract_continuation_and_timeout(entry: &Value) -> Option<(String, Option<u64>)> {
    for key in [
        "invalidationContinuationData",
        "timedContinuationData",
        "reloadContinuationData",
        "liveChatReplayContinuationData",
    ] {
        if let Some(obj) = entry.get(key)
            && let Some(cont) = obj
                .get("continuation")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        {
            let timeout = obj.get("timeoutMs").and_then(Value::as_u64);
            return Some((cont.to_owned(), timeout));
        }
    }
    None
}

pub fn parse_live_chat_item(item: &Value) -> Option<LiveChatMessage> {
    if let Some(r) = item.get("liveChatTextMessageRenderer") {
        let id = r.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
        let author_name = r
            .get("authorName")
            .and_then(extract_yt_text)
            .unwrap_or_else(|| "Usuário".to_owned());
        let text = r
            .get("message")
            .and_then(extract_yt_text)
            .unwrap_or_default();
        if text.trim().is_empty() {
            return None;
        }
        return Some(LiveChatMessage {
            id,
            author_name,
            author_channel_id: r
                .get("authorExternalChannelId")
                .and_then(Value::as_str)
                .map(str::to_owned),
            author_avatar_url: r.get("authorPhoto").and_then(extract_best_thumbnail_url),
            badges: parse_author_badges(r.get("authorBadges")),
            text,
            timestamp_text: r.get("timestampText").and_then(extract_yt_text),
            kind: LiveChatMessageKind::Text,
        });
    }

    if let Some(r) = item.get("liveChatPaidMessageRenderer") {
        let id = r.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
        let author_name = r
            .get("authorName")
            .and_then(extract_yt_text)
            .unwrap_or_else(|| "Usuário".to_owned());
        let amount = r
            .get("purchaseAmountText")
            .and_then(extract_yt_text)
            .unwrap_or_default();
        let text = r
            .get("message")
            .and_then(extract_yt_text)
            .unwrap_or_default();
        let header_bg = r
            .get("headerBackgroundColor")
            .and_then(Value::as_u64)
            .unwrap_or(0xFF_00B8D4) as u32;
        let body_bg = r
            .get("bodyBackgroundColor")
            .and_then(Value::as_u64)
            .unwrap_or(0xFF_00E5FF) as u32;
        return Some(LiveChatMessage {
            id,
            author_name,
            author_channel_id: r
                .get("authorExternalChannelId")
                .and_then(Value::as_str)
                .map(str::to_owned),
            author_avatar_url: r.get("authorPhoto").and_then(extract_best_thumbnail_url),
            badges: parse_author_badges(r.get("authorBadges")),
            text,
            timestamp_text: r.get("timestampText").and_then(extract_yt_text),
            kind: LiveChatMessageKind::Paid {
                amount,
                header_bg,
                body_bg,
            },
        });
    }

    if let Some(r) = item.get("liveChatPaidStickerRenderer") {
        let id = r.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
        let author_name = r
            .get("authorName")
            .and_then(extract_yt_text)
            .unwrap_or_else(|| "Usuário".to_owned());
        let amount = r
            .get("purchaseAmountText")
            .and_then(extract_yt_text)
            .unwrap_or_default();
        let sticker_label = r
            .get("sticker")
            .and_then(|s| s.get("accessibility"))
            .and_then(|a| a.get("accessibilityData"))
            .and_then(|d| d.get("label"))
            .and_then(Value::as_str)
            .unwrap_or("Super Sticker")
            .to_owned();
        let header_bg = r
            .get("backgroundColor")
            .and_then(Value::as_u64)
            .unwrap_or(0xFF_FFB300) as u32;
        return Some(LiveChatMessage {
            id,
            author_name,
            author_channel_id: r
                .get("authorExternalChannelId")
                .and_then(Value::as_str)
                .map(str::to_owned),
            author_avatar_url: r.get("authorPhoto").and_then(extract_best_thumbnail_url),
            badges: parse_author_badges(r.get("authorBadges")),
            text: sticker_label,
            timestamp_text: r.get("timestampText").and_then(extract_yt_text),
            kind: LiveChatMessageKind::Paid {
                amount,
                header_bg,
                body_bg: header_bg,
            },
        });
    }

    if let Some(r) = item.get("liveChatMembershipItemRenderer") {
        let id = r.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
        let author_name = r
            .get("authorName")
            .and_then(extract_yt_text)
            .unwrap_or_else(|| "Membro".to_owned());
        let header_subtext = r
            .get("headerSubtext")
            .and_then(extract_yt_text)
            .unwrap_or_else(|| "Novo membro do canal".to_owned());
        let text = r
            .get("message")
            .and_then(extract_yt_text)
            .unwrap_or_default();
        return Some(LiveChatMessage {
            id,
            author_name,
            author_channel_id: r
                .get("authorExternalChannelId")
                .and_then(Value::as_str)
                .map(str::to_owned),
            author_avatar_url: r.get("authorPhoto").and_then(extract_best_thumbnail_url),
            badges: parse_author_badges(r.get("authorBadges")),
            text,
            timestamp_text: r.get("timestampText").and_then(extract_yt_text),
            kind: LiveChatMessageKind::Membership { header_subtext },
        });
    }

    if let Some(r) = item.get("liveChatSponsorshipsGiftPurchaseAnnouncementRenderer") {
        let id = r.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
        let header = r
            .get("header")
            .and_then(|h| h.get("liveChatSponsorshipsHeaderRenderer"));
        let author_name = header
            .and_then(|h| h.get("authorName"))
            .and_then(extract_yt_text)
            .unwrap_or_else(|| "Comunidade".to_owned());
        let primary_text = header
            .and_then(|h| h.get("primaryText"))
            .and_then(extract_yt_text)
            .unwrap_or_else(|| "Presenteou assinaturas de membro".to_owned());
        return Some(LiveChatMessage {
            id,
            author_name,
            author_channel_id: r
                .get("authorExternalChannelId")
                .and_then(Value::as_str)
                .map(str::to_owned),
            author_avatar_url: header
                .and_then(|h| h.get("authorPhoto"))
                .and_then(extract_best_thumbnail_url),
            badges: parse_author_badges(header.and_then(|h| h.get("authorBadges"))),
            text: String::new(),
            timestamp_text: None,
            kind: LiveChatMessageKind::Membership {
                header_subtext: primary_text,
            },
        });
    }

    if let Some(r) = item.get("liveChatViewerEngagementMessageRenderer") {
        let id = r.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
        let text = r.get("message").and_then(extract_yt_text)?;
        if text.trim().is_empty() {
            return None;
        }
        return Some(LiveChatMessage {
            id,
            author_name: String::new(),
            author_channel_id: None,
            author_avatar_url: None,
            badges: Vec::new(),
            text,
            timestamp_text: None,
            kind: LiveChatMessageKind::Engagement,
        });
    }

    None
}

fn parse_author_badges(val: Option<&Value>) -> Vec<LiveChatBadgeKind> {
    let mut out = Vec::new();
    let Some(arr) = val.and_then(Value::as_array) else {
        return out;
    };
    for badge in arr {
        let Some(renderer) = badge.get("liveChatAuthorBadgeRenderer") else {
            continue;
        };
        if let Some(icon_type) = renderer
            .get("icon")
            .and_then(|i| i.get("iconType"))
            .and_then(Value::as_str)
        {
            match icon_type {
                "OWNER" => out.push(LiveChatBadgeKind::Owner),
                "MODERATOR" => out.push(LiveChatBadgeKind::Moderator),
                "VERIFIED" => out.push(LiveChatBadgeKind::Verified),
                _ => {}
            }
        } else if renderer.get("customThumbnail").is_some() {
            out.push(LiveChatBadgeKind::Member);
        }
    }
    out
}

fn extract_yt_text(val: &Value) -> Option<String> {
    if let Some(simple) = val.get("simpleText").and_then(Value::as_str) {
        let trimmed = simple.trim();
        return (!trimmed.is_empty()).then(|| trimmed.to_owned());
    }
    if let Some(runs) = val.get("runs").and_then(Value::as_array) {
        let mut out = String::new();
        for run in runs {
            if let Some(text) = run.get("text").and_then(Value::as_str) {
                out.push_str(text);
            } else if let Some(emoji) = run.get("emoji") {
                if let Some(emoji_id) = emoji.get("emojiId").and_then(Value::as_str)
                    && !emoji_id.is_empty()
                    && !emoji_id.starts_with("http")
                {
                    out.push_str(emoji_id);
                } else if let Some(shortcut) = emoji
                    .get("shortcuts")
                    .and_then(Value::as_array)
                    .and_then(|arr| arr.first())
                    .and_then(Value::as_str)
                {
                    out.push_str(shortcut);
                }
            }
        }
        let trimmed = out.trim();
        return (!trimmed.is_empty()).then(|| trimmed.to_owned());
    }
    None
}

fn extract_best_thumbnail_url(val: &Value) -> Option<String> {
    val.get("thumbnails")
        .and_then(Value::as_array)
        .and_then(|arr| arr.last())
        .and_then(|thumb| thumb.get("url"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
        .map(str::to_owned)
}

fn parse_digits_count(text: &str) -> Option<u64> {
    let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u64>().ok()
    }
}

pub fn extract_assigned_json_object(html: &str, var_name: &str) -> Option<Value> {
    let patterns = [
        format!("var {var_name} = "),
        format!("var {var_name}="),
        format!("{var_name} = "),
        format!("{var_name}="),
        format!("window[\"{var_name}\"] = "),
        format!("window[\"{var_name}\"]="),
        format!("window['{var_name}'] = "),
        format!("window['{var_name}']="),
    ];
    for pattern in &patterns {
        let mut search_from = 0usize;
        while let Some(rel_idx) = html[search_from..].find(pattern.as_str()) {
            let after = search_from + rel_idx + pattern.len();
            let rest = html[after..].trim_start();
            if rest.starts_with('{')
                && let Some(slice) = slice_balanced_json_object(rest)
                && let Ok(parsed) = serde_json::from_str::<Value>(slice)
            {
                return Some(parsed);
            }
            search_from = after;
        }
    }
    None
}

fn slice_balanced_json_object(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.first().copied() != Some(b'{') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, &b) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&text[..=idx]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn parses_watch_live_info_from_youtube_html_assignments() {
        let html = r#"
        <html><head><script>
        ytcfg.set({"INNERTUBE_CLIENT_VERSION":"2.20261110.01.00","VISITOR_DATA":"CgtTestVisitor"});
        var ytInitialPlayerResponse = {
            "videoDetails": {
                "videoId": "_UaGVka97no",
                "title": "GEN 1x2 BLG | Worlds 2026",
                "author": "CBLOL",
                "channelId": "UC48rkTIXjRd6pnqqBkdV0Mw",
                "isLive": true,
                "isLiveDvrEnabled": true,
                "shortDescription": "O momento mais esperado {ao vivo}!"
            },
            "microformat": {
                "playerMicroformatRenderer": {
                    "liveBroadcastDetails": { "isLiveNow": true }
                }
            }
        };
        var ytInitialData = {
            "contents": {
                "twoColumnWatchNextResults": {
                    "results": {
                        "results": {
                            "contents": [
                                {
                                    "videoPrimaryInfoRenderer": {
                                        "viewCount": {
                                            "videoViewCountRenderer": {
                                                "viewCount": { "runs": [ {"text": "63.014"}, {"text": " assistindo agora"} ] },
                                                "isLive": true
                                            }
                                        },
                                        "dateText": { "simpleText": "Transmissão iniciada há 5 horas" }
                                    }
                                },
                                {
                                    "videoSecondaryInfoRenderer": {
                                        "owner": {
                                            "videoOwnerRenderer": {
                                                "title": { "runs": [{"text": "CBLOL"}] },
                                                "thumbnail": { "thumbnails": [{"url": "https://yt3.ggpht.com/cblol-avatar"}] },
                                                "subscriberCountText": { "simpleText": "1,6 mi de inscritos" },
                                                "navigationEndpoint": {
                                                    "browseEndpoint": {
                                                        "browseId": "UC48rkTIXjRd6pnqqBkdV0Mw",
                                                        "canonicalBaseUrl": "/@CBLOL"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            ]
                        }
                    },
                    "conversationBar": {
                        "liveChatRenderer": {
                            "continuations": [
                                { "reloadContinuationData": { "continuation": "CONT_TOP" } }
                            ]
                        }
                    }
                }
            }
        };
        </script></head></html>"#;

        let info = parse_watch_live_info("_UaGVka97no", html).unwrap();
        assert!(info.is_live);
        assert!(info.is_live_dvr);
        assert_eq!(info.channel.as_deref(), Some("CBLOL"));
        assert_eq!(info.channel_id.as_deref(), Some("UC48rkTIXjRd6pnqqBkdV0Mw"));
        assert_eq!(
            info.channel_url.as_deref(),
            Some("https://www.youtube.com/@CBLOL")
        );
        assert_eq!(
            info.channel_avatar_url.as_deref(),
            Some("https://yt3.ggpht.com/cblol-avatar")
        );
        assert_eq!(
            info.subscriber_count_text.as_deref(),
            Some("1,6 mi de inscritos")
        );
        assert_eq!(
            info.view_count_text.as_deref(),
            Some("63.014 assistindo agora")
        );
        assert_eq!(info.concurrent_view_count, Some(63_014));
        assert_eq!(
            info.started_text.as_deref(),
            Some("Transmissão iniciada há 5 horas")
        );
        assert_eq!(info.initial_continuation.as_deref(), Some("CONT_TOP"));
        assert_eq!(info.client_version, "2.20261110.01.00");
        assert_eq!(info.visitor_data.as_deref(), Some("CgtTestVisitor"));
    }

    #[test]
    fn parses_live_chat_actions_modes_send_params_and_deletions() {
        let payload = json!({
            "continuationContents": {
                "liveChatContinuation": {
                    "continuations": [
                        { "timedContinuationData": { "timeoutMs": 2500, "continuation": "NEXT_CONT" } }
                    ],
                    "header": {
                        "liveChatHeaderRenderer": {
                            "viewSelector": {
                                "sortFilterSubMenuRenderer": {
                                    "subMenuItems": [
                                        {
                                            "title": "Principais mensagens",
                                            "selected": true,
                                            "continuation": { "reloadContinuationData": { "continuation": "CONT_TOP" } }
                                        },
                                        {
                                            "title": "Chat ao vivo",
                                            "selected": false,
                                            "continuation": { "reloadContinuationData": { "continuation": "CONT_ALL" } }
                                        }
                                    ]
                                }
                            }
                        }
                    },
                    "actionPanel": {
                        "liveChatMessageInputRenderer": {
                            "sendButton": {
                                "buttonRenderer": {
                                    "serviceEndpoint": {
                                        "sendLiveChatMessageEndpoint": {
                                            "params": "SEND_PARAMS_TOKEN"
                                        }
                                    }
                                }
                            }
                        }
                    },
                    "actions": [
                        {
                            "addChatItemAction": {
                                "item": {
                                    "liveChatTextMessageRenderer": {
                                        "id": "msg-1",
                                        "authorName": { "simpleText": "@HideoutDMO" },
                                        "authorExternalChannelId": "UC_user1",
                                        "authorPhoto": { "thumbnails": [{"url": "https://yt3.ggpht.com/u1"}] },
                                        "authorBadges": [
                                            { "liveChatAuthorBadgeRenderer": { "icon": { "iconType": "MODERATOR" } } }
                                        ],
                                        "message": { "runs": [
                                            { "text": "chovy amassou " },
                                            { "emoji": { "emojiId": "🔥" } }
                                        ] },
                                        "timestampText": { "simpleText": "5:12:00" }
                                    }
                                }
                            }
                        },
                        {
                            "addChatItemAction": {
                                "item": {
                                    "liveChatPaidMessageRenderer": {
                                        "id": "msg-2",
                                        "authorName": { "simpleText": "@Apoiador" },
                                        "purchaseAmountText": { "simpleText": "R$ 20,00" },
                                        "message": { "runs": [{ "text": "Bora CBLOL!" }] },
                                        "headerBackgroundColor": 4278237396u64,
                                        "bodyBackgroundColor": 4278248959u64
                                    }
                                }
                            }
                        },
                        {
                            "addChatItemAction": {
                                "item": {
                                    "liveChatViewerEngagementMessageRenderer": {
                                        "id": "msg-3",
                                        "message": { "runs": [{ "text": "Curta o chat ao vivo! Proteja sua privacidade." }] }
                                    }
                                }
                            }
                        },
                        { "removeChatItemAction": { "targetItemId": "msg-old" } },
                        { "markChatItemsByAuthorAsDeletedAction": { "externalChannelId": "UC_spam" } }
                    ]
                }
            }
        });

        let batch = parse_live_chat_response(&payload);
        assert_eq!(batch.next_continuation.as_deref(), Some("NEXT_CONT"));
        assert_eq!(batch.timeout_ms, 2500);
        assert_eq!(batch.modes.len(), 2);
        assert_eq!(batch.modes[0].label, "Principais mensagens");
        assert!(batch.modes[0].selected);
        assert_eq!(batch.modes[1].label, "Chat ao vivo");
        assert_eq!(batch.send_params.as_deref(), Some("SEND_PARAMS_TOKEN"));
        assert_eq!(batch.added.len(), 3);
        assert_eq!(batch.added[0].text, "chovy amassou 🔥");
        assert_eq!(batch.added[0].badges, vec![LiveChatBadgeKind::Moderator]);
        assert!(matches!(
            batch.added[1].kind,
            LiveChatMessageKind::Paid { ref amount, .. } if amount == "R$ 20,00"
        ));
        assert_eq!(batch.added[2].kind, LiveChatMessageKind::Engagement);
        assert_eq!(batch.deleted_ids, vec!["msg-old"]);
        assert_eq!(batch.deleted_author_channel_ids, vec!["UC_spam"]);

        let mut state = LiveWatchState::default();
        state.push_message(LiveChatMessage {
            id: "msg-old".into(),
            author_name: "a".into(),
            author_channel_id: Some("UC_a".into()),
            author_avatar_url: None,
            badges: Vec::new(),
            text: "old".into(),
            timestamp_text: None,
            kind: LiveChatMessageKind::Text,
        });
        state.push_message(LiveChatMessage {
            id: "msg-spam".into(),
            author_name: "spam".into(),
            author_channel_id: Some("UC_spam".into()),
            author_avatar_url: None,
            badges: Vec::new(),
            text: "spam".into(),
            timestamp_text: None,
            kind: LiveChatMessageKind::Text,
        });
        state.apply_batch(batch);
        assert_eq!(state.messages.len(), 3);
        assert!(!state.seen_ids.contains("msg-old"));
        assert!(!state.seen_ids.contains("msg-spam"));
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> (String, String) {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 2048];
        let header_end = loop {
            let n = stream.read(&mut tmp).unwrap();
            assert!(n > 0);
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos;
            }
        };
        let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let content_len = headers
            .lines()
            .find_map(|line| {
                let (k, v) = line.split_once(':')?;
                k.eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        let body_start = header_end + 4;
        while buf.len() < body_start + content_len {
            let n = stream.read(&mut tmp).unwrap();
            assert!(n > 0);
            buf.extend_from_slice(&tmp[..n]);
        }
        let body = String::from_utf8_lossy(&buf[body_start..body_start + content_len]).to_string();
        (headers, body)
    }

    fn write_http_response(stream: &mut std::net::TcpStream, status: &str, body: &str) {
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(resp.as_bytes()).unwrap();
    }

    #[test]
    fn fetches_live_chat_and_sends_message_via_innertube() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}/youtubei/v1");

        let server = std::thread::spawn(move || {
            let (mut s1, _) = listener.accept().unwrap();
            let (h1, b1) = read_http_request(&mut s1);
            assert!(h1.contains("/youtubei/v1/live_chat/get_live_chat"));
            assert!(b1.contains("\"CONT_0\""));
            write_http_response(
                &mut s1,
                "200 OK",
                &json!({
                    "continuationContents": {
                        "liveChatContinuation": {
                            "continuations": [
                                { "timedContinuationData": { "timeoutMs": 1500, "continuation": "CONT_1" } }
                            ],
                            "actionPanel": {
                                "liveChatMessageInputRenderer": {
                                    "sendButton": {
                                        "buttonRenderer": {
                                            "serviceEndpoint": {
                                                "sendLiveChatMessageEndpoint": { "params": "PARAM_OK" }
                                            }
                                        }
                                    }
                                }
                            },
                            "actions": [
                                {
                                    "addChatItemAction": {
                                        "item": {
                                            "liveChatTextMessageRenderer": {
                                                "id": "m1",
                                                "authorName": { "simpleText": "@Tester" },
                                                "message": { "runs": [{ "text": "olá chat" }] }
                                            }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                })
                .to_string(),
            );

            let (mut s2, _) = listener.accept().unwrap();
            let (h2, b2) = read_http_request(&mut s2);
            assert!(h2.contains("/youtubei/v1/live_chat/send_message"));
            assert!(h2.contains("SAPISIDHASH "));
            assert!(b2.contains("\"PARAM_OK\""));
            assert!(b2.contains("gg CBLOL"));
            write_http_response(
                &mut s2,
                "200 OK",
                &json!({
                    "actions": [
                        {
                            "addChatItemAction": {
                                "item": {
                                    "liveChatTextMessageRenderer": {
                                        "id": "m_self",
                                        "authorName": { "simpleText": "@Eu" },
                                        "message": { "runs": [{ "text": "gg CBLOL" }] }
                                    }
                                }
                            }
                        }
                    ]
                })
                .to_string(),
            );
        });

        let client = http_client().unwrap();
        let cancel = Cancellation::default();
        let batch = fetch_live_chat_batch_at(
            &base_url,
            &client,
            "CONT_0",
            FALLBACK_CLIENT_VERSION,
            None,
            None,
            &cancel,
        )
        .unwrap();
        assert_eq!(batch.added.len(), 1);
        assert_eq!(batch.send_params.as_deref(), Some("PARAM_OK"));

        let mut session = YouTubeWebSession::test_session();
        let sent = send_live_chat_message_at(
            &base_url,
            &client,
            &mut session,
            batch.send_params.as_deref().unwrap(),
            "gg CBLOL",
            &cancel,
        )
        .unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].id, "m_self");
        assert_eq!(sent[0].text, "gg CBLOL");

        server.join().unwrap();
    }

    #[test]
    fn live_chat_message_filtering_matches_author_and_keyword_and_resets() {
        let mut state = LiveWatchState::default();
        state.push_message(LiveChatMessage {
            id: "1".into(),
            author_name: "@Tiago".into(),
            author_channel_id: None,
            author_avatar_url: None,
            badges: vec![],
            text: "Bora time!".into(),
            timestamp_text: None,
            kind: LiveChatMessageKind::Text,
        });
        state.push_message(LiveChatMessage {
            id: "2".into(),
            author_name: "AnaMod".into(),
            author_channel_id: None,
            author_avatar_url: None,
            badges: vec![LiveChatBadgeKind::Moderator],
            text: "Respeitem as regras do chat".into(),
            timestamp_text: None,
            kind: LiveChatMessageKind::Text,
        });

        assert_eq!(state.filtered_messages().len(), 2);

        state.filter_query = "regras".into();
        let by_keyword = state.filtered_messages();
        assert_eq!(by_keyword.len(), 1);
        assert_eq!(by_keyword[0].id, "2");

        state.filter_query = "@anamod".into();
        let by_handle = state.filtered_messages();
        assert_eq!(by_handle.len(), 1);
        assert_eq!(by_handle[0].id, "2");

        state.filter_query = "TIAGO".into();
        let by_author = state.filtered_messages();
        assert_eq!(by_author.len(), 1);
        assert_eq!(by_author[0].id, "1");

        state.reset_for_video(None, None);
        assert!(state.filter_query.is_empty());
        assert!(state.messages.is_empty());
    }

    #[test]
    fn live_chat_filtering_supports_diacritics_roles_superchats_and_minified_json() {
        let mod_msg = LiveChatMessage {
            id: "m-mod".into(),
            author_name: "@José".into(),
            author_channel_id: None,
            author_avatar_url: None,
            badges: vec![LiveChatBadgeKind::Moderator],
            text: "Ótima transmissão, parabéns!".into(),
            timestamp_text: None,
            kind: LiveChatMessageKind::Text,
        };
        let member_msg = LiveChatMessage {
            id: "m-member".into(),
            author_name: "Carla".into(),
            author_channel_id: None,
            author_avatar_url: None,
            badges: vec![LiveChatBadgeKind::Member],
            text: "Modo escuro ficou ótimo".into(),
            timestamp_text: None,
            kind: LiveChatMessageKind::Membership {
                header_subtext: "Membro há 6 meses".into(),
            },
        };
        let paid_msg = LiveChatMessage {
            id: "m-paid".into(),
            author_name: "Dono".into(),
            author_channel_id: None,
            author_avatar_url: None,
            badges: vec![LiveChatBadgeKind::Owner, LiveChatBadgeKind::Verified],
            text: "Valeu!".into(),
            timestamp_text: None,
            kind: LiveChatMessageKind::Paid {
                amount: "R$ 50,00".into(),
                header_bg: 0,
                body_bg: 0,
            },
        };

        assert!(message_matches_filter(&mod_msg, "otima transmissao"));
        assert!(message_matches_filter(&mod_msg, "PARABÉNS"));
        assert!(message_matches_filter(&mod_msg, "@jose"));
        assert!(message_matches_filter(&mod_msg, "@mod"));
        assert!(message_matches_filter(&mod_msg, "moderador"));
        assert!(!message_matches_filter(&member_msg, "@mod"));
        assert!(message_matches_filter(&member_msg, "mod"));

        assert!(message_matches_filter(&member_msg, "@member"));
        assert!(message_matches_filter(&member_msg, "membro"));
        assert!(message_matches_filter(&member_msg, "ha 6 meses"));
        assert!(!message_matches_filter(&mod_msg, "@member"));

        assert!(message_matches_filter(&paid_msg, "@superchat"));
        assert!(message_matches_filter(&paid_msg, "paid"));
        assert!(message_matches_filter(&paid_msg, "r$ 50"));
        assert!(message_matches_filter(&paid_msg, "@owner"));
        assert!(message_matches_filter(&paid_msg, "@verified"));
        assert!(!message_matches_filter(&mod_msg, "@superchat"));

        let minified_html = concat!(
            "<script>var ytInitialData={\"contents\":{\"ok\":true}};",
            "window['ytInitialPlayerResponse']={\"videoDetails\":{\"isLive\":true}};</script>"
        );
        let initial_data = extract_assigned_json_object(minified_html, "ytInitialData").unwrap();
        assert_eq!(initial_data["contents"]["ok"], json!(true));
        let player_resp =
            extract_assigned_json_object(minified_html, "ytInitialPlayerResponse").unwrap();
        assert_eq!(player_resp["videoDetails"]["isLive"], json!(true));
    }
}
