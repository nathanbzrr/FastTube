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
const INITIAL_POLL_RETRY_BACKOFF_SECS: u64 = 5;
const MAX_POLL_RETRY_BACKOFF_SECS: u64 = 60;

pub(crate) fn poll_retry_backoff(consecutive_errors: usize) -> Duration {
    // Exponential backoff: 5s -> 10s -> 20s -> 40s -> 60s (capped).
    let shift = consecutive_errors.saturating_sub(1).min(4) as u32;
    let secs = INITIAL_POLL_RETRY_BACKOFF_SECS
        .saturating_mul(1u64 << shift)
        .min(MAX_POLL_RETRY_BACKOFF_SECS);
    Duration::from_secs(secs)
}

pub(crate) fn format_poll_reconnect_status(
    error_message: &str,
    consecutive_errors: usize,
    retry_after: Duration,
) -> String {
    let secs = retry_after.as_secs().max(1);
    let trimmed = error_message.trim();
    if trimmed.is_empty() {
        format!("Reconectando chat em {secs}s (tentativa {consecutive_errors})...")
    } else {
        format!("{trimmed} — reconectando em {secs}s (tentativa {consecutive_errors})...")
    }
}

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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LiveChatMessageKind {
    #[default]
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatTextRun {
    pub text: String,
    pub emoji_url: Option<String>,
    pub is_sticker: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SuperChatTickerItem {
    pub id: String,
    pub author: String,
    pub amount_text: String,
    pub avatar_url: Option<String>,
    pub color_rgb: u32,
    pub duration_secs: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LiveChatMessage {
    pub id: String,
    pub author_name: String,
    pub author_channel_id: Option<String>,
    pub author_avatar_url: Option<String>,
    pub badges: Vec<LiveChatBadgeKind>,
    pub text: String,
    pub runs: Vec<ChatTextRun>,
    pub timestamp_text: Option<String>,
    pub video_offset_ms: Option<u64>,
    pub kind: LiveChatMessageKind,
}

#[allow(dead_code)]
pub type ChatMessage = LiveChatMessage;

#[derive(Debug, Clone, PartialEq)]
pub struct HeatmapMarker {
    pub start_ms: u64,
    pub duration_ms: u64,
    pub intensity: f32,
}

impl HeatmapMarker {
    pub fn to_overlay_point(&self) -> crate::overlay::HeatmapPoint {
        crate::overlay::HeatmapPoint {
            start_secs: self.start_ms as f64 / 1000.0,
            duration_secs: self.duration_ms as f64 / 1000.0,
            intensity: self.intensity.clamp(0.0, 1.0),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
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
    pub relative_date_text: Option<String>,
    pub description: Option<String>,
    pub modes: Vec<LiveChatMode>,
    pub initial_continuation: Option<String>,
    pub client_version: String,
    pub visitor_data: Option<String>,
    pub heights: Vec<u32>,
    pub subtitles: Vec<crate::search::SubtitleTrack>,
    pub subtitles_parsed: bool,
    pub storyboard: Option<crate::search::StoryboardSpec>,
    pub chapters: Vec<crate::overlay::OverlayChapter>,
    pub heatmap: Vec<HeatmapMarker>,
    pub channel_verified: bool,
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
    pub pinned_message: Option<LiveChatMessage>,
    pub pinned_removed: bool,
    pub ticker_items: Vec<SuperChatTickerItem>,
    pub slow_mode_secs: Option<u32>,
}

#[allow(dead_code)]
pub type ChatPollBatch = LiveChatBatch;

pub enum WatchLiveEvent {
    WatchInfo {
        session_id: u64,
        result: Box<Result<WatchLiveInfo, String>>,
    },
    WebSessionUpdated(YouTubeWebSession),
}

#[allow(clippy::large_enum_variant)]
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
    pub pinned_message: Option<LiveChatMessage>,
    pub ticker_items: Vec<SuperChatTickerItem>,
    pub slow_mode_secs: Option<u32>,
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

#[allow(dead_code)]
pub type LiveChatState = LiveWatchState;

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
        self.pinned_message = None;
        self.ticker_items.clear();
        self.slow_mode_secs = None;
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
        if batch.pinned_removed {
            self.pinned_message = None;
        }
        if let Some(pinned) = batch.pinned_message {
            self.pinned_message = Some(pinned);
        }
        if batch.slow_mode_secs.is_some() {
            self.slow_mode_secs = batch.slow_mode_secs;
        }
        for item in batch.ticker_items {
            if !item.id.is_empty()
                && let Some(existing) = self.ticker_items.iter_mut().find(|t| t.id == item.id)
            {
                *existing = item;
            } else {
                if self.ticker_items.len() >= 24 {
                    self.ticker_items.remove(0);
                }
                self.ticker_items.push(item);
            }
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

#[allow(clippy::too_many_arguments)]
fn run_live_chat_stream_loop_at<F, S>(
    base_url: &str,
    client: &reqwest::blocking::Client,
    session_id: u64,
    stream_id: u64,
    initial_continuation: String,
    client_version: String,
    visitor_data: Option<String>,
    web_session: Option<YouTubeWebSession>,
    tx: &mpsc::Sender<LiveChatStreamEvent>,
    cancel_bg: &Cancellation,
    mut on_event: F,
    mut sleep_fn: S,
) where
    F: FnMut(),
    S: FnMut(Duration, &Cancellation) -> bool,
{
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
            base_url,
            client,
            &cont,
            &active_version,
            active_visitor.as_deref(),
            maybe_session.as_mut(),
            cancel_bg,
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
                on_event();
                if !send_ok || next_cont.is_none() {
                    break;
                }
                continuation = next_cont;
                if !sleep_fn(Duration::from_millis(timeout_ms), cancel_bg) {
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
                consecutive_errors = consecutive_errors.saturating_add(1);
                let retry_after = poll_retry_backoff(consecutive_errors);
                let msg = format_poll_reconnect_status(
                    &err.into_message(),
                    consecutive_errors,
                    retry_after,
                );
                let send_ok = tx
                    .send(LiveChatStreamEvent::Batch {
                        session_id,
                        stream_id,
                        result: Err(msg),
                    })
                    .is_ok();
                on_event();
                if !send_ok {
                    break;
                }
                continuation = Some(cont);
                if !sleep_fn(retry_after, cancel_bg) {
                    break;
                }
            }
        }
    }
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

        run_live_chat_stream_loop_at(
            INNERTUBE_BASE_URL,
            &client,
            session_id,
            stream_id,
            initial_continuation,
            client_version,
            visitor_data,
            web_session,
            &tx,
            &cancel_bg,
            || ctx_clone.request_repaint(),
            sleep_cancellable,
        );
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
    let html = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
    };
    if let Some(session) = web_session {
        if let Some(ver) = extract_json_string_field(&html, "INNERTUBE_CLIENT_VERSION") {
            session.client_version = ver;
        }
        if let Some(vis) = extract_json_string_field(&html, "VISITOR_DATA") {
            session.visitor_data = Some(vis);
        }
        session.client_config_refreshed = true;
    }
    if cancel.cancelled() {
        return Err("Consulta ao vivo cancelada.".into());
    }
    parse_watch_live_info(video_id, &html)
}

pub fn parse_watch_live_info(video_id: &str, html: &str) -> Result<WatchLiveInfo, String> {
    let client_version = extract_json_string_field(html, "INNERTUBE_CLIENT_VERSION")
        .unwrap_or_else(|| FALLBACK_CLIENT_VERSION.to_owned());
    let visitor_data = extract_json_string_field(html, "VISITOR_DATA");

    let player_response = extract_assigned_json_object(html, "ytInitialPlayerResponse");
    let has_player_response = player_response.is_some();
    let mut info = build_watch_live_info_from_player_response(
        video_id,
        player_response.as_ref(),
        client_version,
        visitor_data,
    );
    drop(player_response);

    let initial_data = extract_assigned_json_object(html, "ytInitialData");
    if !has_player_response && initial_data.is_none() {
        return Err("Dados da transmiss\u{00e3}o n\u{00e3}o encontrados na p\u{00e1}gina.".into());
    }
    if let Some(data) = initial_data.as_ref() {
        merge_initial_data_into_watch_live_info(&mut info, data);
    }
    drop(initial_data);

    Ok(info)
}

fn parse_player_response_heights(player_response: &Value) -> Vec<u32> {
    let Some(streaming) = player_response.get("streamingData") else {
        return Vec::new();
    };
    let mut heights = Vec::new();
    for key in ["adaptiveFormats", "formats"] {
        let Some(list) = streaming.get(key).and_then(Value::as_array) else {
            continue;
        };
        for fmt in list {
            let is_video = fmt
                .get("mimeType")
                .and_then(Value::as_str)
                .is_some_and(|m| m.starts_with("video/"));
            if !is_video {
                continue;
            }
            if let Some(h) = fmt
                .get("height")
                .and_then(Value::as_u64)
                .filter(|&h| (1..=16_384).contains(&h))
                .and_then(|h| u32::try_from(h).ok())
            {
                heights.push(h);
            }
        }
    }
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();
    heights
}

fn normalize_vtt_subtitle_url(raw: &str) -> Option<(String, bool)> {
    let mut parsed = url::Url::parse(raw).ok()?;
    let mut has_tlang = false;
    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .filter_map(|(k, v)| {
            if k == "tlang" {
                has_tlang = true;
            }
            if k == "fmt" {
                None
            } else {
                Some((k.into_owned(), v.into_owned()))
            }
        })
        .collect();
    {
        let mut qp = parsed.query_pairs_mut();
        qp.clear();
        for (k, v) in &pairs {
            qp.append_pair(k, v);
        }
        qp.append_pair("fmt", "vtt");
    }
    let normalized = parsed.to_string();
    crate::search::is_allowed_subtitle_url(&normalized).then_some((normalized, has_tlang))
}

fn parse_player_response_subtitles(player_response: &Value) -> Vec<crate::search::SubtitleTrack> {
    let Some(caption_tracks) = player_response
        .get("captions")
        .and_then(|c| c.get("playerCaptionsTracklistRenderer"))
        .and_then(|r| r.get("captionTracks"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for track in caption_tracks {
        let Some(language) = track.get("languageCode").and_then(Value::as_str) else {
            continue;
        };
        if language.is_empty()
            || language.len() > 64
            || !language
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            continue;
        }
        let Some(base_url) = track.get("baseUrl").and_then(Value::as_str) else {
            continue;
        };
        let Some((url, translated)) = normalize_vtt_subtitle_url(base_url) else {
            continue;
        };
        let automatic = track.get("kind").and_then(Value::as_str) == Some("asr");
        candidates.push((
            crate::search::SubtitleTrack {
                language: language.to_owned(),
                automatic,
                url,
            },
            translated,
        ));
    }
    let has_native_tracks = candidates.iter().any(|(_, translated)| !translated);
    let mut tracks: Vec<crate::search::SubtitleTrack> = candidates
        .into_iter()
        .filter_map(|(track, translated)| (!has_native_tracks || !translated).then_some(track))
        .collect();
    tracks.sort_by(|left, right| {
        (left.automatic, &left.language).cmp(&(right.automatic, &right.language))
    });
    tracks.dedup_by(|a, b| a.automatic == b.automatic && a.language == b.language);
    tracks.truncate(80);
    tracks
}

fn is_valid_ytimg_https_url(parsed: &url::Url) -> bool {
    let host = parsed.host_str().unwrap_or("");
    parsed.scheme() == "https"
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && (host == "ytimg.com" || host.ends_with(".ytimg.com"))
}

fn parse_player_response_storyboard(
    player_response: &Value,
) -> Option<crate::search::StoryboardSpec> {
    let spec = player_response
        .get("storyboards")?
        .get("playerStoryboardSpecRenderer")?
        .get("spec")?
        .as_str()?
        .trim();
    if spec.is_empty() {
        return None;
    }
    let length_val = player_response.get("videoDetails")?.get("lengthSeconds")?;
    let length_secs = length_val
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| length_val.as_f64())
        .filter(|d| d.is_finite() && *d > 0.0)?;
    let mut segments = spec.split('|');
    let base_url = segments.next()?.trim();
    let parsed_base = url::Url::parse(base_url).ok()?;
    if !is_valid_ytimg_https_url(&parsed_base) {
        return None;
    }

    struct SbLevel<'a> {
        index: usize,
        width: u32,
        height: u32,
        total_count: usize,
        columns: u32,
        rows: u32,
        interval_ms: u64,
        name_pattern: &'a str,
        sigh: &'a str,
    }

    let mut levels = Vec::new();
    for (level_idx, raw_level) in segments.enumerate() {
        let parts: Vec<&str> = raw_level.split('#').collect();
        if parts.len() < 8 {
            continue;
        }
        let Ok(width) = parts[0].parse::<u32>() else {
            continue;
        };
        let Ok(height) = parts[1].parse::<u32>() else {
            continue;
        };
        let Ok(total_count) = parts[2].parse::<usize>() else {
            continue;
        };
        let Ok(columns) = parts[3].parse::<u32>() else {
            continue;
        };
        let Ok(rows) = parts[4].parse::<u32>() else {
            continue;
        };
        let Ok(interval_ms) = parts[5].parse::<u64>() else {
            continue;
        };
        let name_pattern = parts[6].trim();
        let sigh = parts[7].trim();
        if width == 0
            || height == 0
            || !(1..=20_000).contains(&total_count)
            || !(1..=20).contains(&columns)
            || !(1..=20).contains(&rows)
            || name_pattern.is_empty()
        {
            continue;
        }
        levels.push(SbLevel {
            index: level_idx,
            width,
            height,
            total_count,
            columns,
            rows,
            interval_ms,
            name_pattern,
            sigh,
        });
    }
    let chosen = levels
        .iter()
        .filter(|l| l.width <= 320)
        .max_by_key(|l| l.width)
        .or_else(|| levels.iter().max_by_key(|l| l.width))?;

    let per_sheet = (chosen.rows as usize) * (chosen.columns as usize);
    if per_sheet == 0 {
        return None;
    }
    let sheet_count = chosen.total_count.div_ceil(per_sheet).min(512);
    let frame_dur = if chosen.interval_ms > 0 {
        chosen.interval_ms as f64 / 1000.0
    } else {
        (length_secs / chosen.total_count as f64).max(0.5)
    };
    let level_idx_str = chosen.index.to_string();
    let mut fragments = Vec::with_capacity(sheet_count);
    for sheet_idx in 0..sheet_count {
        let sheet_name = chosen.name_pattern.replace("$M", &sheet_idx.to_string());
        let raw_frag = base_url
            .replace("$L", &level_idx_str)
            .replace("$N", &sheet_name);
        let mut parsed = url::Url::parse(&raw_frag).ok()?;
        if !chosen.sigh.is_empty() {
            parsed.query_pairs_mut().append_pair("sigh", chosen.sigh);
        }
        if !is_valid_ytimg_https_url(&parsed) {
            return None;
        }
        let frames_in_sheet = if sheet_idx + 1 == sheet_count {
            chosen
                .total_count
                .saturating_sub(sheet_idx * per_sheet)
                .min(per_sheet)
                .max(1)
        } else {
            per_sheet
        };
        fragments.push(crate::search::StoryboardFragment {
            url: parsed.into(),
            duration_secs: (frames_in_sheet as f64 * frame_dur).max(1.0),
        });
    }
    if fragments.is_empty() {
        return None;
    }
    Some(crate::search::StoryboardSpec {
        rows: chosen.rows,
        columns: chosen.columns,
        thumb_width: chosen.width,
        thumb_height: chosen.height,
        fragments,
    })
}

fn build_watch_live_info_from_player_response(
    video_id: &str,
    player_response: Option<&Value>,
    client_version: String,
    visitor_data: Option<String>,
) -> WatchLiveInfo {
    let video_details = player_response.and_then(|v| v.get("videoDetails"));
    let microformat = player_response
        .and_then(|v| v.get("microformat"))
        .and_then(|v| v.get("playerMicroformatRenderer"));
    let live_details = microformat.and_then(|v| v.get("liveBroadcastDetails"));

    let is_live = video_details
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
    let channel = video_details
        .and_then(|v| v.get("author"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let channel_id = video_details
        .and_then(|v| v.get("channelId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let channel_url = channel_id
        .as_deref()
        .map(|id| format!("{YOUTUBE_ORIGIN}/channel/{id}"));
    let description = video_details
        .and_then(|v| v.get("shortDescription"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let (heights, subtitles, subtitles_parsed, storyboard) = match player_response {
        Some(pr) => (
            parse_player_response_heights(pr),
            parse_player_response_subtitles(pr),
            true,
            parse_player_response_storyboard(pr),
        ),
        None => (Vec::new(), Vec::new(), false, None),
    };

    WatchLiveInfo {
        video_id: video_id.to_owned(),
        is_live,
        is_live_dvr,
        title,
        channel,
        channel_id,
        channel_url,
        channel_avatar_url: None,
        subscriber_count_text: None,
        view_count_text: None,
        concurrent_view_count: None,
        started_text: None,
        relative_date_text: None,
        description,
        modes: Vec::new(),
        initial_continuation: None,
        client_version,
        visitor_data,
        heights,
        subtitles,
        subtitles_parsed,
        storyboard,
        chapters: Vec::new(),
        heatmap: player_response
            .map(extract_heatmap_markers)
            .unwrap_or_default(),
        channel_verified: false,
    }
}

fn parse_chapter_time_str_secs(raw: &str) -> Option<f64> {
    crate::watch_extras::extract_timestamps(raw.trim())
        .into_iter()
        .next()
        .map(|ts| ts.seconds as f64)
}

fn parse_macro_markers_panel_chapters(panel: &Value) -> Vec<crate::overlay::OverlayChapter> {
    let Some(items) = panel
        .get("content")
        .and_then(|c| c.get("macroMarkersListRenderer"))
        .and_then(|r| r.get("contents"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in items {
        let Some(item) = entry.get("macroMarkersListItemRenderer") else {
            continue;
        };
        let Some(title) = item.get("title").and_then(extract_yt_text) else {
            continue;
        };
        let start_secs = item
            .get("onTap")
            .and_then(|t| t.get("watchEndpoint"))
            .and_then(|w| w.get("startTimeSeconds"))
            .and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_u64().map(|n| n as f64))
                    .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
            })
            .or_else(|| {
                item.get("timeDescription")
                    .and_then(extract_yt_text)
                    .and_then(|s| parse_chapter_time_str_secs(&s))
            });
        let Some(time_secs) = start_secs.filter(|s| s.is_finite() && *s >= 0.0) else {
            continue;
        };
        out.push(crate::overlay::OverlayChapter { title, time_secs });
    }
    out
}

fn parse_decorated_player_bar_chapters(data: &Value) -> Vec<crate::overlay::OverlayChapter> {
    let Some(markers_map) = data
        .get("playerOverlays")
        .and_then(|p| p.get("playerOverlayRenderer"))
        .and_then(|p| p.get("decoratedPlayerBarRenderer"))
        .and_then(|d| d.get("decoratedPlayerBarRenderer"))
        .and_then(|d| d.get("playerBar"))
        .and_then(|b| b.get("multiMarkersPlayerBarRenderer"))
        .and_then(|m| m.get("markersMap"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    for target_key in ["DESCRIPTION_CHAPTERS", "AUTO_CHAPTERS"] {
        let Some(entry) = markers_map
            .iter()
            .find(|item| item.get("key").and_then(Value::as_str) == Some(target_key))
        else {
            continue;
        };
        let Some(chapters) = entry
            .get("value")
            .and_then(|v| v.get("chapters"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        let mut out = Vec::new();
        for ch in chapters {
            let Some(renderer) = ch.get("chapterRenderer") else {
                continue;
            };
            let Some(title) = renderer.get("title").and_then(extract_yt_text) else {
                continue;
            };
            let millis = renderer.get("timeRangeStartMillis").and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_f64().map(|f| f.max(0.0).round() as u64))
                    .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
            });
            let Some(ms) = millis else {
                continue;
            };
            out.push(crate::overlay::OverlayChapter {
                title,
                time_secs: ms as f64 / 1000.0,
            });
        }
        if !out.is_empty() {
            return out;
        }
    }
    Vec::new()
}

fn normalize_overlay_chapters(
    mut chapters: Vec<crate::overlay::OverlayChapter>,
) -> Vec<crate::overlay::OverlayChapter> {
    chapters.retain(|ch| {
        !ch.title.trim().is_empty() && ch.time_secs.is_finite() && ch.time_secs >= 0.0
    });
    chapters.sort_by(|a, b| a.time_secs.total_cmp(&b.time_secs));
    chapters.dedup_by_key(|ch| (ch.time_secs * 1000.0).round() as u64);
    chapters
}

fn parse_initial_data_chapters(data: &Value) -> Vec<crate::overlay::OverlayChapter> {
    if let Some(panels) = data.get("engagementPanels").and_then(Value::as_array) {
        for target_panel_id in [
            "engagement-panel-macro-markers-description-chapters",
            "engagement-panel-macro-markers-auto-chapters",
        ] {
            for ep in panels {
                let Some(panel) = ep.get("engagementPanelSectionListRenderer") else {
                    continue;
                };
                if panel.get("panelIdentifier").and_then(Value::as_str) != Some(target_panel_id) {
                    continue;
                }
                let parsed = normalize_overlay_chapters(parse_macro_markers_panel_chapters(panel));
                if !parsed.is_empty() {
                    return parsed;
                }
            }
        }
    }
    normalize_overlay_chapters(parse_decorated_player_bar_chapters(data))
}

fn parse_json_u64_millis(val: &Value) -> Option<u64> {
    val.as_u64()
        .or_else(|| val.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| {
            val.as_f64()
                .filter(|f| f.is_finite() && *f >= 0.0)
                .map(|f| f.round() as u64)
        })
        .or_else(|| {
            val.as_str().and_then(|s| {
                let trimmed = s.trim();
                trimmed.parse::<u64>().ok().or_else(|| {
                    trimmed
                        .parse::<f64>()
                        .ok()
                        .filter(|f| f.is_finite() && *f >= 0.0)
                        .map(|f| f.round() as u64)
                })
            })
        })
}

fn parse_json_f32_score(val: &Value) -> Option<f32> {
    val.as_f64()
        .or_else(|| val.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
        .filter(|f| f.is_finite())
        .map(|f| (f as f32).clamp(0.0, 1.0))
}

fn parse_heatmap_marker_entry(entry: &Value) -> Option<HeatmapMarker> {
    let renderer = entry
        .get("heatMarkerRenderer")
        .or_else(|| entry.get("macroMarkersListItemRenderer"))
        .unwrap_or(entry);
    let start_ms = renderer
        .get("timeRangeStartMillis")
        .or_else(|| renderer.get("startMillis"))
        .and_then(parse_json_u64_millis)?;
    let duration_ms = renderer
        .get("markerDurationMillis")
        .or_else(|| renderer.get("durationMillis"))
        .and_then(parse_json_u64_millis)?;
    if duration_ms == 0 {
        return None;
    }
    let intensity = renderer
        .get("heatMarkerIntensityScoreNormalized")
        .or_else(|| renderer.get("intensityScoreNormalized"))
        .and_then(parse_json_f32_score)?;
    Some(HeatmapMarker {
        start_ms,
        duration_ms,
        intensity,
    })
}

fn collect_heatmap_markers_from_value(container: &Value) -> Vec<HeatmapMarker> {
    let candidate_arrays = [
        container
            .get("heatmap")
            .and_then(|h| h.get("heatmapRenderer"))
            .and_then(|r| r.get("heatMarkers")),
        container
            .get("heatmapRenderer")
            .and_then(|r| r.get("heatMarkers")),
        container.get("heatMarkers"),
        container
            .get("macroMarkersListEntity")
            .and_then(|e| e.get("markersList"))
            .and_then(|l| l.get("markers")),
        container
            .get("macroMarkersListEntity")
            .and_then(|e| e.get("markers")),
        container.get("markersList").and_then(|l| l.get("markers")),
        container.get("markers"),
    ];

    for arr in candidate_arrays.into_iter().flatten() {
        let Some(items) = arr.as_array() else {
            continue;
        };
        let mut parsed: Vec<HeatmapMarker> = items
            .iter()
            .filter_map(parse_heatmap_marker_entry)
            .collect();
        if !parsed.is_empty() {
            parsed.sort_by_key(|m| m.start_ms);
            return parsed;
        }
    }
    Vec::new()
}

pub fn extract_heatmap_markers(initial_data: &Value) -> Vec<HeatmapMarker> {
    let decorated_bar = initial_data
        .get("playerOverlays")
        .and_then(|p| p.get("playerOverlayRenderer"))
        .and_then(|p| p.get("decoratedPlayerBarRenderer"));
    let markers_map = decorated_bar
        .and_then(|d| d.get("decoratedPlayerBarRenderer").or(Some(d)))
        .and_then(|d| d.get("playerBar"))
        .and_then(|b| b.get("multiMarkersPlayerBarRenderer"))
        .and_then(|m| m.get("markersMap"))
        .and_then(Value::as_array);

    if let Some(map_entries) = markers_map {
        for entry in map_entries {
            let is_heatseeker = entry.get("key").and_then(Value::as_str).is_some_and(|k| {
                k.eq_ignore_ascii_case("HEATSEEKER") || k == "MARKER_TYPE_HEATMAP"
            });
            if !is_heatseeker {
                continue;
            }
            if let Some(value) = entry.get("value") {
                let markers = collect_heatmap_markers_from_value(value);
                if !markers.is_empty() {
                    return markers;
                }
            }
        }
    }

    let direct = collect_heatmap_markers_from_value(initial_data);
    if !direct.is_empty() {
        return direct;
    }

    if let Some(mutations) = initial_data
        .get("frameworkUpdates")
        .and_then(|f| f.get("entityBatchUpdate"))
        .and_then(|b| b.get("mutations"))
        .and_then(Value::as_array)
    {
        for mutation in mutations {
            let Some(entity) = mutation
                .get("payload")
                .and_then(|p| p.get("macroMarkersListEntity"))
            else {
                continue;
            };
            let marker_type = entity
                .get("markersList")
                .and_then(|l| l.get("markerType"))
                .and_then(Value::as_str);
            if marker_type.is_some_and(|t| {
                t != "MARKER_TYPE_HEATMAP" && !t.eq_ignore_ascii_case("HEATSEEKER")
            }) {
                continue;
            }
            let markers = collect_heatmap_markers_from_value(entity);
            if !markers.is_empty() {
                return markers;
            }
        }
    }

    Vec::new()
}

fn owner_renderer_has_verified_badge(owner: &Value) -> bool {
    let Some(badges) = owner.get("badges").and_then(Value::as_array) else {
        return false;
    };
    badges.iter().any(|badge| {
        let renderer = badge.get("metadataBadgeRenderer").unwrap_or(badge);
        renderer
            .get("style")
            .and_then(Value::as_str)
            .is_some_and(|style| {
                matches!(
                    style,
                    "BADGE_STYLE_TYPE_VERIFIED" | "BADGE_STYLE_TYPE_VERIFIED_ARTIST"
                )
            })
    })
}

pub fn extract_watch_channel_verified(initial_data: &Value) -> bool {
    if let Some(contents) = initial_data
        .get("contents")
        .and_then(|c| c.get("twoColumnWatchNextResults"))
        .and_then(|tc| tc.get("results"))
        .and_then(|r| r.get("results"))
        .and_then(|r| r.get("contents"))
        .and_then(Value::as_array)
    {
        for item in contents {
            if let Some(owner) = item
                .get("videoSecondaryInfoRenderer")
                .and_then(|s| s.get("owner"))
                .and_then(|o| o.get("videoOwnerRenderer"))
                .or_else(|| item.get("videoOwnerRenderer"))
                && owner_renderer_has_verified_badge(owner)
            {
                return true;
            }
        }
    }

    for candidate in [
        initial_data
            .get("videoSecondaryInfoRenderer")
            .and_then(|s| s.get("owner"))
            .and_then(|o| o.get("videoOwnerRenderer")),
        initial_data
            .get("owner")
            .and_then(|o| o.get("videoOwnerRenderer")),
        initial_data.get("videoOwnerRenderer"),
    ]
    .into_iter()
    .flatten()
    {
        if owner_renderer_has_verified_badge(candidate) {
            return true;
        }
    }

    false
}

fn merge_initial_data_into_watch_live_info(info: &mut WatchLiveInfo, data: &Value) {
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
                        info.is_live = true;
                    }
                    if let Some(vc_text) = vc_renderer
                        .get("viewCount")
                        .and_then(extract_yt_text)
                        .filter(|s| !s.is_empty())
                    {
                        if info.concurrent_view_count.is_none() {
                            info.concurrent_view_count = parse_digits_count(&vc_text);
                        }
                        info.view_count_text = Some(vc_text);
                    }
                }
                if let Some(date_text) = primary
                    .get("dateText")
                    .and_then(extract_yt_text)
                    .filter(|s| !s.is_empty())
                {
                    info.started_text = Some(date_text);
                }
                if let Some(rel_text) = primary
                    .get("relativeDateText")
                    .and_then(extract_yt_text)
                    .and_then(|s| crate::search::clean_published_time_text(&s))
                {
                    info.relative_date_text = Some(rel_text);
                }
            }
            if let Some(secondary) = item.get("videoSecondaryInfoRenderer")
                && let Some(owner) = secondary
                    .get("owner")
                    .and_then(|o| o.get("videoOwnerRenderer"))
            {
                if info.channel.is_none() {
                    info.channel = owner.get("title").and_then(extract_yt_text);
                }
                if info.channel_avatar_url.is_none() {
                    info.channel_avatar_url =
                        owner.get("thumbnail").and_then(extract_best_thumbnail_url);
                }
                if info.subscriber_count_text.is_none() {
                    info.subscriber_count_text = owner
                        .get("subscriberCountText")
                        .and_then(extract_yt_text)
                        .filter(|s| !s.is_empty());
                }
                if let Some(browse) = owner
                    .get("navigationEndpoint")
                    .and_then(|n| n.get("browseEndpoint"))
                {
                    if info.channel_id.is_none() {
                        info.channel_id = browse
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
                        info.channel_url = Some(if canon.starts_with("http") {
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
        info.modes = parse_live_chat_modes(chat_renderer);
        info.initial_continuation = chat_renderer
            .get("continuations")
            .and_then(Value::as_array)
            .and_then(|arr| arr.iter().find_map(extract_continuation_from_entry))
            .or_else(|| {
                info.modes
                    .iter()
                    .find(|m| m.selected)
                    .or_else(|| info.modes.first())
                    .map(|m| m.continuation.clone())
            });
    }

    info.chapters = parse_initial_data_chapters(data);
    let initial_heatmap = extract_heatmap_markers(data);
    if !initial_heatmap.is_empty() {
        info.heatmap = initial_heatmap;
    }
    if extract_watch_channel_verified(data) {
        info.channel_verified = true;
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

#[allow(dead_code)]
pub fn build_live_chat_replay_payload(
    continuation: &str,
    player_offset_ms: u64,
    client_version: &str,
    visitor_data: Option<&str>,
) -> Value {
    let mut client_ctx = json!({
        "clientName": "WEB",
        "clientVersion": client_version,
        "hl": "pt",
        "gl": "BR",
    });
    if let Some(visitor) = visitor_data.map(str::trim).filter(|s| !s.is_empty())
        && let Some(obj) = client_ctx.as_object_mut()
    {
        obj.insert("visitorData".into(), Value::String(visitor.to_owned()));
    }
    json!({
        "context": { "client": client_ctx },
        "continuation": continuation,
        "currentPlayerState": {
            "playerOffsetMs": player_offset_ms.to_string(),
        },
    })
}

#[allow(dead_code, clippy::too_many_arguments)]
pub fn fetch_live_chat_replay_page_at(
    base_url: &str,
    client: &reqwest::blocking::Client,
    continuation: &str,
    player_offset_ms: u64,
    client_version: &str,
    visitor_data: Option<&str>,
    web_session: Option<&mut YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<LiveChatBatch, InnerTubeError> {
    if cancel.cancelled() {
        return Err(InnerTubeError::Cancelled(
            "Replay do chat cancelado.".into(),
        ));
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

    let body = build_live_chat_replay_payload(
        continuation,
        player_offset_ms,
        effective_version,
        effective_visitor,
    );
    let url = format!("{base_url}/live_chat/get_live_chat_replay?prettyPrint=false");
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
        InnerTubeError::Other("Falha de conex\u{00e3}o ao carregar o replay do chat.".into())
    })?;
    if cancel.cancelled() {
        return Err(InnerTubeError::Cancelled(
            "Replay do chat cancelado.".into(),
        ));
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
            "Erro ao carregar replay do chat (HTTP {}).",
            status.as_u16()
        )));
    }
    if let Some(err) = parse_innertube_error_json(&json, status.as_u16()) {
        return Err(err);
    }
    Ok(parse_live_chat_response(&json))
}

#[allow(dead_code)]
pub fn fetch_live_chat_replay_page(
    continuation: &str,
    player_offset_ms: u64,
    client_version: &str,
    visitor_data: Option<&str>,
    web_session: Option<&mut YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<LiveChatBatch, InnerTubeError> {
    let client = http_client().map_err(InnerTubeError::Other)?;
    fetch_live_chat_replay_page_at(
        INNERTUBE_BASE_URL,
        &client,
        continuation,
        player_offset_ms,
        client_version,
        visitor_data,
        web_session,
        cancel,
    )
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

    batch.slow_mode_secs = parse_slow_mode_secs(chat);

    if let Some(actions) = chat.get("actions").and_then(Value::as_array) {
        for action in actions {
            apply_live_chat_action(action, None, &mut batch);
        }
    }

    batch
}

fn apply_live_chat_action(action: &Value, video_offset_ms: Option<u64>, batch: &mut LiveChatBatch) {
    if let Some(replay) = action.get("replayChatItemAction") {
        let offset_ms = replay
            .get("videoOffsetTimeMsec")
            .and_then(parse_json_u64_millis)
            .or(video_offset_ms);
        if let Some(inner_actions) = replay.get("actions").and_then(Value::as_array) {
            for inner in inner_actions {
                apply_live_chat_action(inner, offset_ms, batch);
            }
        }
        return;
    }

    if let Some(add) = action.get("addChatItemAction") {
        if let Some(item) = add.get("item")
            && let Some(mut msg) = parse_live_chat_item(item)
        {
            msg.video_offset_ms = video_offset_ms;
            batch.added.push(msg);
        }
    } else if let Some(replace) = action.get("replaceChatItemAction") {
        if let Some(target_id) = replace
            .get("targetItemId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            && let Some(item) = replace.get("replacementItem")
            && let Some(mut msg) = parse_live_chat_item(item)
        {
            msg.video_offset_ms = video_offset_ms;
            batch.replaced.push((target_id.to_owned(), msg));
        }
    } else if let Some(banner_cmd) = action.get("addBannerToLiveChatCommand") {
        if let Some(mut msg) = parse_banner_message(banner_cmd) {
            msg.video_offset_ms = video_offset_ms;
            batch.pinned_message = Some(msg);
            batch.pinned_removed = false;
        }
    } else if action.get("removeBannerForLiveChatCommand").is_some() {
        batch.pinned_message = None;
        batch.pinned_removed = true;
    } else if let Some(ticker_action) = action.get("addLiveChatTickerItemAction") {
        if let Some(ticker_item) = parse_ticker_item(ticker_action) {
            batch.ticker_items.push(ticker_item);
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

fn parse_banner_message(cmd: &Value) -> Option<LiveChatMessage> {
    let banner = cmd
        .pointer("/bannerRenderer/liveChatBannerRenderer")
        .or_else(|| cmd.get("liveChatBannerRenderer"))?;
    let contents = banner.get("contents")?;
    let mut msg = parse_live_chat_item(contents)?;
    if msg.id.is_empty()
        && let Some(action_id) = banner
            .get("actionId")
            .or_else(|| banner.get("targetId"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    {
        msg.id = action_id.to_owned();
    }
    Some(msg)
}

fn parse_ticker_item(action: &Value) -> Option<SuperChatTickerItem> {
    let item = action.get("item")?;
    let action_duration = action
        .get("durationSec")
        .and_then(parse_json_u64_millis)
        .and_then(|v| u32::try_from(v).ok());

    if let Some(r) = item.get("liveChatTickerPaidMessageItemRenderer") {
        let id = r.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
        let amount_text = r
            .get("amount")
            .or_else(|| r.get("purchaseAmountText"))
            .and_then(extract_yt_text)
            .unwrap_or_default();
        if amount_text.is_empty() {
            return None;
        }
        let author = r
            .pointer("/showItemEndpoint/showLiveChatItemEndpoint/renderer/liveChatPaidMessageRenderer/authorName")
            .and_then(extract_yt_text)
            .or_else(|| r.get("authorUsername").and_then(extract_yt_text))
            .or_else(|| {
                r.pointer("/authorPhoto/accessibility/accessibilityData/label")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        let avatar_url = r.get("authorPhoto").and_then(extract_best_thumbnail_url);
        let color_rgb = r
            .get("startBackgroundColor")
            .or_else(|| r.get("endBackgroundColor"))
            .and_then(Value::as_u64)
            .map(|c| (c as u32) & 0x00FF_FFFF)
            .unwrap_or(0x00B8D4);
        let duration_secs = r
            .get("durationSec")
            .and_then(parse_json_u64_millis)
            .and_then(|v| u32::try_from(v).ok())
            .or(action_duration)
            .unwrap_or(120);
        return Some(SuperChatTickerItem {
            id,
            author,
            amount_text,
            avatar_url,
            color_rgb,
            duration_secs,
        });
    }

    if let Some(r) = item.get("liveChatTickerSponsorItemRenderer") {
        let id = r.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
        let amount_text = r
            .get("detailText")
            .and_then(extract_yt_text)
            .unwrap_or_else(|| "Member".to_owned());
        let author = r
            .pointer("/showItemEndpoint/showLiveChatItemEndpoint/renderer/liveChatMembershipItemRenderer/authorName")
            .and_then(extract_yt_text)
            .or_else(|| {
                r.pointer("/sponsorPhoto/accessibility/accessibilityData/label")
                    .or_else(|| r.pointer("/authorPhoto/accessibility/accessibilityData/label"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        let avatar_url = r
            .get("sponsorPhoto")
            .or_else(|| r.get("authorPhoto"))
            .and_then(extract_best_thumbnail_url);
        let color_rgb = r
            .get("startBackgroundColor")
            .or_else(|| r.get("endBackgroundColor"))
            .and_then(Value::as_u64)
            .map(|c| (c as u32) & 0x00FF_FFFF)
            .unwrap_or(0x0F9D58);
        let duration_secs = r
            .get("durationSec")
            .and_then(parse_json_u64_millis)
            .and_then(|v| u32::try_from(v).ok())
            .or(action_duration)
            .unwrap_or(120);
        return Some(SuperChatTickerItem {
            id,
            author,
            amount_text,
            avatar_url,
            color_rgb,
            duration_secs,
        });
    }

    if let Some(r) = item.get("liveChatTickerPaidStickerItemRenderer") {
        let id = r.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
        let amount_text = r
            .pointer("/showItemEndpoint/showLiveChatItemEndpoint/renderer/liveChatPaidStickerRenderer/purchaseAmountText")
            .and_then(extract_yt_text)
            .unwrap_or_else(|| "Super Sticker".to_owned());
        let author = r
            .pointer("/showItemEndpoint/showLiveChatItemEndpoint/renderer/liveChatPaidStickerRenderer/authorName")
            .and_then(extract_yt_text)
            .unwrap_or_default();
        let avatar_url = r.get("authorPhoto").and_then(extract_best_thumbnail_url);
        let color_rgb = r
            .get("startBackgroundColor")
            .or_else(|| r.get("endBackgroundColor"))
            .and_then(Value::as_u64)
            .map(|c| (c as u32) & 0x00FF_FFFF)
            .unwrap_or(0xFFB300);
        let duration_secs = r
            .get("durationSec")
            .and_then(parse_json_u64_millis)
            .and_then(|v| u32::try_from(v).ok())
            .or(action_duration)
            .unwrap_or(120);
        return Some(SuperChatTickerItem {
            id,
            author,
            amount_text,
            avatar_url,
            color_rgb,
            duration_secs,
        });
    }

    None
}

fn parse_slow_mode_secs_from_text(text: &str) -> Option<u32> {
    let lower = text.to_lowercase();
    if !(lower.contains("slow mode") || lower.contains("modo lento") || lower.contains("slow-mode"))
    {
        return None;
    }
    let bytes = lower.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if let Ok(num) = lower[start..i].parse::<u32>()
                && num > 0
            {
                let rest = lower[i..].trim_start();
                if rest.starts_with("min") || rest.starts_with('m') && !rest.starts_with("ms") {
                    return Some(num.saturating_mul(60));
                }
                return Some(num);
            }
        } else {
            i += 1;
        }
    }
    None
}

fn find_slow_mode_secs_in_value(val: &Value) -> Option<u32> {
    match val {
        Value::Object(map) => {
            for key in [
                "slowModeTimeoutSecs",
                "slowModeDurationSec",
                "slowModeSeconds",
                "cooldownSeconds",
                "delaySeconds",
            ] {
                if let Some(secs) = map
                    .get(key)
                    .and_then(parse_json_u64_millis)
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|&n| n > 0)
                {
                    return Some(secs);
                }
            }
            if let Some(text) = extract_yt_text(val)
                && let Some(secs) = parse_slow_mode_secs_from_text(&text)
            {
                return Some(secs);
            }
            for child in map.values() {
                if let Some(found) = find_slow_mode_secs_in_value(child) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(find_slow_mode_secs_in_value),
        Value::String(s) => parse_slow_mode_secs_from_text(s),
        _ => None,
    }
}

fn parse_slow_mode_secs(chat: &Value) -> Option<u32> {
    chat.get("header")
        .and_then(find_slow_mode_secs_in_value)
        .or_else(|| {
            chat.get("actionPanel")
                .and_then(find_slow_mode_secs_in_value)
        })
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
        "playerSeekContinuationData",
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
        let runs = r.get("message").map(extract_yt_runs).unwrap_or_default();
        if text.trim().is_empty() && runs.is_empty() {
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
            runs,
            timestamp_text: r.get("timestampText").and_then(extract_yt_text),
            video_offset_ms: None,
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
        let runs = r.get("message").map(extract_yt_runs).unwrap_or_default();
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
            runs,
            timestamp_text: r.get("timestampText").and_then(extract_yt_text),
            video_offset_ms: None,
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
        let sticker_url = r.get("sticker").and_then(extract_best_thumbnail_url);
        let runs = vec![ChatTextRun {
            text: sticker_label.clone(),
            emoji_url: sticker_url,
            is_sticker: true,
        }];
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
            runs,
            timestamp_text: r.get("timestampText").and_then(extract_yt_text),
            video_offset_ms: None,
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
        let runs = r.get("message").map(extract_yt_runs).unwrap_or_default();
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
            runs,
            timestamp_text: r.get("timestampText").and_then(extract_yt_text),
            video_offset_ms: None,
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
            runs: Vec::new(),
            timestamp_text: None,
            video_offset_ms: None,
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
        let runs = r.get("message").map(extract_yt_runs).unwrap_or_default();
        return Some(LiveChatMessage {
            id,
            author_name: String::new(),
            author_channel_id: None,
            author_avatar_url: None,
            badges: Vec::new(),
            text,
            runs,
            timestamp_text: None,
            video_offset_ms: None,
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

fn is_raw_custom_emoji_id(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.contains('/')
        || (trimmed.starts_with("UC") && trimmed.len() >= 16)
}

fn format_emoji_label(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || is_raw_custom_emoji_id(trimmed) {
        return None;
    }
    if trimmed.starts_with(':') && trimmed.ends_with(':') && trimmed.len() > 2 {
        Some(trimmed.to_owned())
    } else {
        Some(format!(":{trimmed}:"))
    }
}

fn format_yt_emoji(emoji: &Value) -> Option<String> {
    let is_custom = emoji
        .get("isCustomEmoji")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let emoji_id = emoji
        .get("emojiId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if !is_custom
        && let Some(id) = emoji_id
        && !is_raw_custom_emoji_id(id)
    {
        return Some(id.to_owned());
    }

    if let Some(shortcut) = emoji
        .get("shortcuts")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .find_map(format_emoji_label)
        })
    {
        return Some(shortcut);
    }

    if let Some(term) = emoji
        .get("searchTerms")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .find_map(format_emoji_label)
        })
    {
        return Some(term);
    }

    let accessibility_label = emoji
        .get("image")
        .and_then(|img| img.get("accessibility"))
        .and_then(|acc| acc.get("accessibilityData"))
        .and_then(|data| data.get("label"))
        .and_then(Value::as_str)
        .and_then(format_emoji_label);
    if accessibility_label.is_some() {
        return accessibility_label;
    }

    if let Some(tooltip) = emoji
        .get("tooltip")
        .and_then(Value::as_str)
        .and_then(format_emoji_label)
    {
        return Some(tooltip);
    }

    if is_custom || emoji_id.is_some() {
        Some(":emoji:".to_owned())
    } else {
        None
    }
}

fn extract_yt_runs(val: &Value) -> Vec<ChatTextRun> {
    if let Some(simple) = val.get("simpleText").and_then(Value::as_str) {
        let trimmed = simple.trim();
        if !trimmed.is_empty() {
            return vec![ChatTextRun {
                text: trimmed.to_owned(),
                emoji_url: None,
                is_sticker: false,
            }];
        }
        return Vec::new();
    }
    let Some(runs) = val.get("runs").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(runs.len());
    for run in runs {
        if let Some(text) = run.get("text").and_then(Value::as_str) {
            if !text.is_empty() {
                out.push(ChatTextRun {
                    text: text.to_owned(),
                    emoji_url: None,
                    is_sticker: false,
                });
            }
        } else if let Some(emoji) = run.get("emoji") {
            let label = format_yt_emoji(emoji).unwrap_or_else(|| ":emoji:".to_owned());
            let emoji_url = emoji.get("image").and_then(extract_best_thumbnail_url);
            out.push(ChatTextRun {
                text: label,
                emoji_url,
                is_sticker: false,
            });
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
            } else if let Some(emoji) = run.get("emoji")
                && let Some(rendered) = format_yt_emoji(emoji)
            {
                out.push_str(&rendered);
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
        .and_then(|url| {
            if url.starts_with("https://") || url.starts_with("http://") {
                Some(url.to_owned())
            } else if url.starts_with("//") {
                Some(format!("https:{url}"))
            } else {
                None
            }
        })
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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

    #[test]
    fn parses_heights_subtitles_and_storyboard_from_player_response() {
        let html = r#"
        <html><head><script>
        var ytInitialPlayerResponse = {
            "videoDetails": {
                "videoId": "abc12345678",
                "title": "Test Video",
                "author": "Tester",
                "channelId": "UC1234567890123456789012",
                "lengthSeconds": "120"
            },
            "streamingData": {
                "formats": [
                    { "mimeType": "video/mp4; codecs=\"avc1\"", "height": 360 },
                    { "mimeType": "video/mp4; codecs=\"avc1\"", "height": 720 }
                ],
                "adaptiveFormats": [
                    { "mimeType": "video/webm; codecs=\"vp9\"", "height": 1080 },
                    { "mimeType": "video/webm; codecs=\"vp9\"", "height": 720 },
                    { "mimeType": "audio/webm; codecs=\"opus\"", "audioQuality": "AUDIO_QUALITY_MEDIUM" }
                ]
            },
            "captions": {
                "playerCaptionsTracklistRenderer": {
                    "captionTracks": [
                        {
                            "baseUrl": "https://www.youtube.com/api/timedtext?v=abc12345678&lang=pt&fmt=srv3",
                            "languageCode": "pt"
                        },
                        {
                            "baseUrl": "https://www.youtube.com/api/timedtext?v=abc12345678&lang=en",
                            "languageCode": "en",
                            "kind": "asr"
                        },
                        {
                            "baseUrl": "https://evil.example.com/api/timedtext?v=abc12345678&lang=fr",
                            "languageCode": "fr"
                        }
                    ]
                }
            },
            "storyboards": {
                "playerStoryboardSpecRenderer": {
                    "spec": "https://i.ytimg.com/sb/abc12345678/storyboard3_L$L/$N.jpg?sqp=123|48#27#100#10#10#0#default#S0|160#90#12#2#2#2000#M$M#S2"
                }
            }
        };
        </script></head></html>"#;

        let info = parse_watch_live_info("abc12345678", html).unwrap();
        assert_eq!(info.heights, vec![1080, 720, 360]);
        assert!(info.subtitles_parsed);
        assert_eq!(info.subtitles.len(), 2);
        assert_eq!(info.subtitles[0].language, "pt");
        assert!(!info.subtitles[0].automatic);
        assert!(info.subtitles[0].url.contains("fmt=vtt"));
        assert!(!info.subtitles[0].url.contains("fmt=srv3"));
        assert_eq!(info.subtitles[1].language, "en");
        assert!(info.subtitles[1].automatic);
        assert!(info.subtitles[1].url.contains("fmt=vtt"));

        let sb = info.storyboard.expect("storyboard should be parsed");
        assert_eq!(sb.rows, 2);
        assert_eq!(sb.columns, 2);
        assert_eq!(sb.thumb_width, 160);
        assert_eq!(sb.thumb_height, 90);
        assert_eq!(sb.fragments.len(), 3);
        assert_eq!(
            sb.fragments[0].url,
            "https://i.ytimg.com/sb/abc12345678/storyboard3_L1/M0.jpg?sqp=123&sigh=S2"
        );
        let (frag_url, _) = sb.lookup(10.0).unwrap();
        assert_eq!(frag_url, sb.fragments[1].url);
    }

    #[test]
    fn parse_watch_live_info_extracts_chapters_from_panels_and_player_bar() {
        let html_auto = r#"<html><head><script>
        var ytInitialPlayerResponse = { "videoDetails": { "videoId": "uLVuvJY_AkY" } };
        var ytInitialData = {
            "engagementPanels": [
                {
                    "engagementPanelSectionListRenderer": {
                        "panelIdentifier": "engagement-panel-macro-markers-auto-chapters",
                        "content": {
                            "macroMarkersListRenderer": {
                                "contents": [
                                    {
                                        "macroMarkersListItemRenderer": {
                                            "title": { "simpleText": "New Single from the Strokes" },
                                            "timeDescription": { "simpleText": "0:30" },
                                            "onTap": { "watchEndpoint": { "startTimeSeconds": 30 } }
                                        }
                                    },
                                    {
                                        "macroMarkersListItemRenderer": {
                                            "title": { "runs": [{"text": "One Way Trigger"}] },
                                            "timeDescription": { "simpleText": "0:47" }
                                        }
                                    }
                                ]
                            }
                        }
                    }
                }
            ]
        };
        </script></head></html>"#;
        let info = parse_watch_live_info("uLVuvJY_AkY", html_auto).unwrap();
        assert_eq!(info.chapters.len(), 2);
        assert_eq!(info.chapters[0].title, "New Single from the Strokes");
        assert_eq!(info.chapters[0].time_secs, 30.0);
        assert_eq!(info.chapters[1].title, "One Way Trigger");
        assert_eq!(info.chapters[1].time_secs, 47.0);

        let html_bar = r#"<html><head><script>
        var ytInitialPlayerResponse = { "videoDetails": { "videoId": "bar12345678" } };
        var ytInitialData = {
            "playerOverlays": {
                "playerOverlayRenderer": {
                    "decoratedPlayerBarRenderer": {
                        "decoratedPlayerBarRenderer": {
                            "playerBar": {
                                "multiMarkersPlayerBarRenderer": {
                                    "markersMap": [
                                        {
                                            "key": "DESCRIPTION_CHAPTERS",
                                            "value": {
                                                "chapters": [
                                                    {
                                                        "chapterRenderer": {
                                                            "title": { "simpleText": "Intro" },
                                                            "timeRangeStartMillis": 0
                                                        }
                                                    },
                                                    {
                                                        "chapterRenderer": {
                                                            "title": { "simpleText": "Review" },
                                                            "timeRangeStartMillis": 95000
                                                        }
                                                    }
                                                ]
                                            }
                                        }
                                    ]
                                }
                            }
                        }
                    }
                }
            }
        };
        </script></head></html>"#;
        let info_bar = parse_watch_live_info("bar12345678", html_bar).unwrap();
        assert_eq!(info_bar.chapters.len(), 2);
        assert_eq!(info_bar.chapters[0].title, "Intro");
        assert_eq!(info_bar.chapters[0].time_secs, 0.0);
        assert_eq!(info_bar.chapters[1].title, "Review");
        assert_eq!(info_bar.chapters[1].time_secs, 95.0);
    }

    #[test]
    fn extract_yt_text_resolves_custom_emojis_without_raw_channel_ids() {
        let msg = json!({
            "runs": [
                { "text": "Go " },
                { "emoji": { "emojiId": "🔥" } },
                { "text": " " },
                {
                    "emoji": {
                        "emojiId": "UC48rkTIXjRd6pnqqBkdV0Mw/abc123def456",
                        "isCustomEmoji": true,
                        "shortcuts": [":_hype:"],
                        "searchTerms": ["hype"]
                    }
                },
                { "text": " " },
                {
                    "emoji": {
                        "emojiId": "UC48rkTIXjRd6pnqqBkdV0Mw/no_shortcut",
                        "searchTerms": ["party_parrot"]
                    }
                },
                { "text": " " },
                {
                    "emoji": {
                        "emojiId": "UC48rkTIXjRd6pnqqBkdV0Mw/only_a11y",
                        "isCustomEmoji": true,
                        "image": {
                            "accessibility": {
                                "accessibilityData": { "label": "member_star" }
                            }
                        }
                    }
                },
                { "text": " " },
                {
                    "emoji": {
                        "emojiId": "UC48rkTIXjRd6pnqqBkdV0Mw/only_tooltip",
                        "tooltip": "clap"
                    }
                },
                { "text": " " },
                {
                    "emoji": {
                        "emojiId": "UC48rkTIXjRd6pnqqBkdV0Mw/bare_id_only"
                    }
                }
            ]
        });

        let rendered = extract_yt_text(&msg).expect("message text should parse");
        assert_eq!(
            rendered,
            "Go 🔥 :_hype: :party_parrot: :member_star: :clap: :emoji:"
        );
        assert!(!rendered.contains("UC48rkTIXjRd6pnqqBkdV0Mw"));
    }

    #[test]
    fn poll_retry_backoff_doubles_and_caps_at_sixty_seconds() {
        assert_eq!(poll_retry_backoff(1), Duration::from_secs(5));
        assert_eq!(poll_retry_backoff(2), Duration::from_secs(10));
        assert_eq!(poll_retry_backoff(3), Duration::from_secs(20));
        assert_eq!(poll_retry_backoff(4), Duration::from_secs(40));
        assert_eq!(poll_retry_backoff(5), Duration::from_secs(60));
        assert_eq!(poll_retry_backoff(6), Duration::from_secs(60));
        assert_eq!(poll_retry_backoff(50), Duration::from_secs(60));
    }

    #[test]
    fn live_chat_stream_recovers_after_more_than_three_consecutive_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local listener");
        let addr = listener.local_addr().expect("listener addr");

        let server = std::thread::spawn(move || {
            for attempt in 0..6usize {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                if attempt == 4 {
                    let body = json!({
                        "continuationContents": {
                            "liveChatContinuation": {
                                "continuations": [
                                    {
                                        "timedContinuationData": {
                                            "continuation": "CONT_NEXT",
                                            "timeoutMs": 1500
                                        }
                                    }
                                ],
                                "actions": [
                                    {
                                        "addChatItemAction": {
                                            "item": {
                                                "liveChatTextMessageRenderer": {
                                                    "id": "recovered-1",
                                                    "authorName": { "simpleText": "Viewer" },
                                                    "message": { "runs": [{ "text": "Back online!" }] }
                                                }
                                            }
                                        }
                                    }
                                ]
                            }
                        }
                    })
                    .to_string();
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes());
                } else {
                    let body = r#"{"error":{"message":"temporary upstream failure"}}"#;
                    let resp = format!(
                        "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes());
                }
            }
        });

        let client = http_client().expect("build http client");
        let (tx, rx) = mpsc::channel();
        let cancel = Cancellation::default();
        let mut recorded_sleeps = Vec::new();
        let mut error_sleeps = 0usize;

        run_live_chat_stream_loop_at(
            &format!("http://{addr}"),
            &client,
            1,
            1,
            "CONT_START".into(),
            FALLBACK_CLIENT_VERSION.into(),
            None,
            None,
            &tx,
            &cancel,
            || {},
            |dur, _| {
                recorded_sleeps.push(dur);
                if dur != Duration::from_millis(1500) {
                    error_sleeps += 1;
                    // Stop after verifying the post-recovery error resets backoff to 5s.
                    if error_sleeps >= 5 {
                        return false;
                    }
                }
                true
            },
        );

        let _ = server.join();
        let events: Vec<LiveChatStreamEvent> = rx.try_iter().collect();
        assert_eq!(events.len(), 6);
        // First 4 attempts fail with 5s -> 10s -> 20s -> 40s backoff (no hard stop at 3).
        for (idx, event) in events.iter().enumerate().take(4) {
            match event {
                LiveChatStreamEvent::Batch {
                    result: Err(msg), ..
                } => {
                    assert!(msg.contains("reconectando em"), "unexpected msg: {msg}");
                }
                _ => panic!("expected reconnect error event at index {idx}"),
            }
        }
        // 5th attempt succeeds and recovers chat delivery.
        match &events[4] {
            LiveChatStreamEvent::Batch {
                result: Ok(batch), ..
            } => {
                assert_eq!(batch.added.len(), 1);
                assert_eq!(batch.added[0].text, "Back online!");
            }
            _ => panic!("expected recovered Ok batch at index 4"),
        }
        // 6th attempt fails after recovery and resets backoff to 5s.
        assert!(matches!(
            &events[5],
            LiveChatStreamEvent::Batch { result: Err(_), .. }
        ));
        assert_eq!(
            recorded_sleeps,
            vec![
                Duration::from_secs(5),
                Duration::from_secs(10),
                Duration::from_secs(20),
                Duration::from_secs(40),
                Duration::from_millis(1500),
                Duration::from_secs(5),
            ]
        );
    }

    #[test]
    fn extract_heatmap_markers_parses_heat_marker_renderer_and_macro_markers_variants() {
        let initial_data_heat_renderer = json!({
            "playerOverlays": {
                "playerOverlayRenderer": {
                    "decoratedPlayerBarRenderer": {
                        "decoratedPlayerBarRenderer": {
                            "playerBar": {
                                "multiMarkersPlayerBarRenderer": {
                                    "markersMap": [
                                        {
                                            "key": "DESCRIPTION_CHAPTERS",
                                            "value": { "chapters": [] }
                                        },
                                        {
                                            "key": "HEATSEEKER",
                                            "value": {
                                                "heatmap": {
                                                    "heatmapRenderer": {
                                                        "heatMarkers": [
                                                            {
                                                                "heatMarkerRenderer": {
                                                                    "timeRangeStartMillis": 20000,
                                                                    "markerDurationMillis": 10000,
                                                                    "heatMarkerIntensityScoreNormalized": 1.35
                                                                }
                                                            },
                                                            {
                                                                "heatMarkerRenderer": {
                                                                    "timeRangeStartMillis": 0,
                                                                    "markerDurationMillis": 10000,
                                                                    "heatMarkerIntensityScoreNormalized": -0.25
                                                                }
                                                            },
                                                            {
                                                                "heatMarkerRenderer": {
                                                                    "timeRangeStartMillis": "10000",
                                                                    "markerDurationMillis": "10000",
                                                                    "heatMarkerIntensityScoreNormalized": 0.65
                                                                }
                                                            }
                                                        ]
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
        });

        let markers = extract_heatmap_markers(&initial_data_heat_renderer);
        assert_eq!(
            markers,
            vec![
                HeatmapMarker {
                    start_ms: 0,
                    duration_ms: 10_000,
                    intensity: 0.0,
                },
                HeatmapMarker {
                    start_ms: 10_000,
                    duration_ms: 10_000,
                    intensity: 0.65,
                },
                HeatmapMarker {
                    start_ms: 20_000,
                    duration_ms: 10_000,
                    intensity: 1.0,
                },
            ]
        );
        let overlay_pt = markers[1].to_overlay_point();
        assert_eq!(
            overlay_pt,
            crate::overlay::HeatmapPoint {
                start_secs: 10.0,
                duration_secs: 10.0,
                intensity: 0.65,
            }
        );

        let initial_data_entity = json!({
            "frameworkUpdates": {
                "entityBatchUpdate": {
                    "mutations": [
                        {
                            "entityKey": " heathen ",
                            "payload": {
                                "macroMarkersListEntity": {
                                    "markersList": {
                                        "markerType": "MARKER_TYPE_HEATMAP",
                                        "markers": [
                                            {
                                                "startMillis": "15000",
                                                "durationMillis": "5000",
                                                "intensityScoreNormalized": 0.92
                                            },
                                            {
                                                "startMillis": "5000",
                                                "durationMillis": "5000",
                                                "intensityScoreNormalized": 0.40
                                            }
                                        ]
                                    }
                                }
                            }
                        }
                    ]
                }
            }
        });

        let entity_markers = extract_heatmap_markers(&initial_data_entity);
        assert_eq!(
            entity_markers,
            vec![
                HeatmapMarker {
                    start_ms: 5_000,
                    duration_ms: 5_000,
                    intensity: 0.40,
                },
                HeatmapMarker {
                    start_ms: 15_000,
                    duration_ms: 5_000,
                    intensity: 0.92,
                },
            ]
        );
    }

    #[test]
    fn extract_heatmap_markers_handles_missing_and_empty_markers_gracefully() {
        assert!(extract_heatmap_markers(&json!({})).is_empty());
        assert!(
            extract_heatmap_markers(&json!({
                "playerOverlays": {
                    "playerOverlayRenderer": {
                        "decoratedPlayerBarRenderer": {
                            "decoratedPlayerBarRenderer": {
                                "playerBar": {
                                    "multiMarkersPlayerBarRenderer": {
                                        "markersMap": [
                                            {
                                                "key": "HEATSEEKER",
                                                "value": {
                                                    "heatmap": {
                                                        "heatmapRenderer": {
                                                            "heatMarkers": [
                                                                {
                                                                    "heatMarkerRenderer": {
                                                                        "timeRangeStartMillis": 0,
                                                                        "markerDurationMillis": 0,
                                                                        "heatMarkerIntensityScoreNormalized": 0.5
                                                                    }
                                                                }
                                                            ]
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
            }))
            .is_empty()
        );
    }

    #[test]
    fn extract_watch_channel_verified_detects_verified_artist_and_unverified_badges() {
        let verified_data = json!({
            "contents": {
                "twoColumnWatchNextResults": {
                    "results": {
                        "results": {
                            "contents": [
                                {
                                    "videoSecondaryInfoRenderer": {
                                        "owner": {
                                            "videoOwnerRenderer": {
                                                "badges": [
                                                    {
                                                        "metadataBadgeRenderer": {
                                                            "style": "BADGE_STYLE_TYPE_VERIFIED"
                                                        }
                                                    }
                                                ]
                                            }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        });
        assert!(extract_watch_channel_verified(&verified_data));

        let artist_data = json!({
            "videoSecondaryInfoRenderer": {
                "owner": {
                    "videoOwnerRenderer": {
                        "badges": [
                            {
                                "metadataBadgeRenderer": {
                                    "style": "BADGE_STYLE_TYPE_VERIFIED_ARTIST"
                                }
                            }
                        ]
                    }
                }
            }
        });
        assert!(extract_watch_channel_verified(&artist_data));

        let unverified_data = json!({
            "videoOwnerRenderer": {
                "badges": [
                    {
                        "metadataBadgeRenderer": {
                            "style": "BADGE_STYLE_TYPE_SIMPLE"
                        }
                    }
                ]
            }
        });
        assert!(!extract_watch_channel_verified(&unverified_data));
        assert!(!extract_watch_channel_verified(&json!({})));

        let html = r#"<html><head><script>
        var ytInitialPlayerResponse = { "videoDetails": { "videoId": "heatVer1234" } };
        var ytInitialData = {
            "contents": {
                "twoColumnWatchNextResults": {
                    "results": {
                        "results": {
                            "contents": [
                                {
                                    "videoSecondaryInfoRenderer": {
                                        "owner": {
                                            "videoOwnerRenderer": {
                                                "title": { "simpleText": "Official Artist" },
                                                "badges": [
                                                    {
                                                        "metadataBadgeRenderer": {
                                                            "style": "BADGE_STYLE_TYPE_VERIFIED_ARTIST"
                                                        }
                                                    }
                                                ]
                                            }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            },
            "playerOverlays": {
                "playerOverlayRenderer": {
                    "decoratedPlayerBarRenderer": {
                        "decoratedPlayerBarRenderer": {
                            "playerBar": {
                                "multiMarkersPlayerBarRenderer": {
                                    "markersMap": [
                                        {
                                            "key": "HEATSEEKER",
                                            "value": {
                                                "heatmap": {
                                                    "heatmapRenderer": {
                                                        "heatMarkers": [
                                                            {
                                                                "heatMarkerRenderer": {
                                                                    "timeRangeStartMillis": 0,
                                                                    "markerDurationMillis": 5000,
                                                                    "heatMarkerIntensityScoreNormalized": 0.95
                                                                }
                                                            }
                                                        ]
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
        };
        </script></head></html>"#;
        let info = parse_watch_live_info("heatVer1234", html).unwrap();
        assert!(info.channel_verified);
        assert_eq!(info.heatmap.len(), 1);
        assert_eq!(info.heatmap[0].start_ms, 0);
        assert_eq!(info.heatmap[0].duration_ms, 5000);
        assert!((info.heatmap[0].intensity - 0.95).abs() < 1e-6);
    }

    #[test]
    fn parses_live_chat_replay_pinned_banner_ticker_emotes_stickers_and_slow_mode() {
        let fixture = json!({
            "continuationContents": {
                "liveChatContinuation": {
                    "header": {
                        "liveChatHeaderRenderer": {
                            "subtitle": { "simpleText": "Modo lento ativado (15 segundos)" }
                        }
                    },
                    "continuations": [
                        { "liveChatReplayContinuationData": { "continuation": "REPLAY_CONT_2", "timeUntilLastMessageMsec": 5000 } }
                    ],
                    "actions": [
                        {
                            "replayChatItemAction": {
                                "videoOffsetTimeMsec": "42500",
                                "actions": [
                                    {
                                        "addChatItemAction": {
                                            "item": {
                                                "liveChatTextMessageRenderer": {
                                                    "id": "replay_msg_1",
                                                    "authorName": { "simpleText": "@ReplayFan" },
                                                    "message": {
                                                        "runs": [
                                                            { "text": "Hype " },
                                                            {
                                                                "emoji": {
                                                                    "emojiId": "UC1234567890123456/emote_gg",
                                                                    "isCustomEmoji": true,
                                                                    "shortcuts": [":gg:"],
                                                                    "image": {
                                                                        "thumbnails": [
                                                                            { "url": "//yt3.ggpht.com/emote_gg=w48-h48" }
                                                                        ]
                                                                    }
                                                                }
                                                            }
                                                        ]
                                                    },
                                                    "timestampText": { "simpleText": "0:42" }
                                                }
                                            }
                                        }
                                    },
                                    {
                                        "addBannerToLiveChatCommand": {
                                            "bannerRenderer": {
                                                "liveChatBannerRenderer": {
                                                    "actionId": "pin_1",
                                                    "contents": {
                                                        "liveChatTextMessageRenderer": {
                                                            "id": "pinned_msg_1",
                                                            "authorName": { "simpleText": "@Host" },
                                                            "message": { "simpleText": "Welcome to the VOD replay!" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    {
                                        "addLiveChatTickerItemAction": {
                                            "durationSec": "300",
                                            "item": {
                                                "liveChatTickerPaidMessageItemRenderer": {
                                                    "id": "ticker_sc_1",
                                                    "amount": { "simpleText": "R$ 50,00" },
                                                    "startBackgroundColor": 0xFF_E65100u64,
                                                    "authorPhoto": { "thumbnails": [{ "url": "https://yt3.ggpht.com/supporter=s64" }] },
                                                    "showItemEndpoint": {
                                                        "showLiveChatItemEndpoint": {
                                                            "renderer": {
                                                                "liveChatPaidMessageRenderer": {
                                                                    "authorName": { "simpleText": "@Supporter" }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                ]
                            }
                        },
                        {
                            "addLiveChatTickerItemAction": {
                                "item": {
                                    "liveChatTickerSponsorItemRenderer": {
                                        "id": "ticker_mem_1",
                                        "detailText": { "simpleText": "Member (6 months)" },
                                        "startBackgroundColor": 0xFF_0F9D58u64,
                                        "durationSec": 180,
                                        "sponsorPhoto": {
                                            "thumbnails": [{ "url": "https://yt3.ggpht.com/member=s64" }],
                                            "accessibility": { "accessibilityData": { "label": "@MemberOne" } }
                                        }
                                    }
                                }
                            }
                        },
                        {
                            "addChatItemAction": {
                                "item": {
                                    "liveChatPaidStickerRenderer": {
                                        "id": "sticker_1",
                                        "authorName": { "simpleText": "@StickerFan" },
                                        "purchaseAmountText": { "simpleText": "$5.00" },
                                        "backgroundColor": 0xFF_FFB300u64,
                                        "sticker": {
                                            "thumbnails": [{ "url": "https://lh3.googleusercontent.com/sticker.webp" }],
                                            "accessibility": { "accessibilityData": { "label": "Cheering pear" } }
                                        }
                                    }
                                }
                            }
                        }
                    ]
                }
            }
        });

        let batch = parse_live_chat_response(&fixture);
        assert_eq!(batch.slow_mode_secs, Some(15));
        assert_eq!(batch.next_continuation.as_deref(), Some("REPLAY_CONT_2"));
        assert_eq!(batch.added.len(), 2);

        let replay_msg = &batch.added[0];
        assert_eq!(replay_msg.id, "replay_msg_1");
        assert_eq!(replay_msg.video_offset_ms, Some(42_500));
        assert_eq!(replay_msg.text, "Hype :gg:");
        assert_eq!(replay_msg.runs.len(), 2);
        assert_eq!(replay_msg.runs[0].text, "Hype ");
        assert_eq!(replay_msg.runs[0].emoji_url, None);
        assert_eq!(replay_msg.runs[1].text, ":gg:");
        assert_eq!(
            replay_msg.runs[1].emoji_url.as_deref(),
            Some("https://yt3.ggpht.com/emote_gg=w48-h48")
        );
        assert!(!replay_msg.runs[1].is_sticker);

        let sticker_msg = &batch.added[1];
        assert_eq!(sticker_msg.id, "sticker_1");
        assert_eq!(sticker_msg.runs.len(), 1);
        assert!(sticker_msg.runs[0].is_sticker);
        assert_eq!(
            sticker_msg.runs[0].emoji_url.as_deref(),
            Some("https://lh3.googleusercontent.com/sticker.webp")
        );

        let pinned = batch.pinned_message.as_ref().expect("pinned message");
        assert_eq!(pinned.id, "pinned_msg_1");
        assert_eq!(pinned.video_offset_ms, Some(42_500));
        assert_eq!(pinned.text, "Welcome to the VOD replay!");

        assert_eq!(batch.ticker_items.len(), 2);
        assert_eq!(batch.ticker_items[0].id, "ticker_sc_1");
        assert_eq!(batch.ticker_items[0].author, "@Supporter");
        assert_eq!(batch.ticker_items[0].amount_text, "R$ 50,00");
        assert_eq!(batch.ticker_items[0].color_rgb, 0xE65100);
        assert_eq!(batch.ticker_items[0].duration_secs, 300);

        assert_eq!(batch.ticker_items[1].id, "ticker_mem_1");
        assert_eq!(batch.ticker_items[1].author, "@MemberOne");
        assert_eq!(batch.ticker_items[1].amount_text, "Member (6 months)");
        assert_eq!(batch.ticker_items[1].color_rgb, 0x0F9D58);
        assert_eq!(batch.ticker_items[1].duration_secs, 180);

        let mut state = LiveWatchState::default();
        state.apply_batch(batch);
        assert!(state.pinned_message.is_some());
        assert_eq!(state.ticker_items.len(), 2);
        assert_eq!(state.slow_mode_secs, Some(15));

        let unpin_batch = parse_live_chat_response(&json!({
            "continuationContents": {
                "liveChatContinuation": {
                    "actionPanel": {
                        "liveChatMessageInputRenderer": {
                            "slowModeMessage": { "simpleText": "Slow mode is on · 2 minutes" }
                        }
                    },
                    "actions": [{
                        "removeBannerForLiveChatCommand": { "targetActionId": "pin_1" }
                    }]
                }
            }
        }));
        assert!(unpin_batch.pinned_removed);
        assert_eq!(unpin_batch.slow_mode_secs, Some(120));

        state.apply_batch(unpin_batch);
        assert!(state.pinned_message.is_none());
        assert_eq!(state.slow_mode_secs, Some(120));

        state.reset_for_video(None, None);
        assert!(state.ticker_items.is_empty());
        assert_eq!(state.slow_mode_secs, None);
    }

    #[test]
    fn fetches_live_chat_replay_page_with_player_offset_ms() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}/youtubei/v1");

        let server = std::thread::spawn(move || {
            let (mut s1, _) = listener.accept().unwrap();
            let (h1, b1) = read_http_request(&mut s1);
            assert!(h1.contains("/youtubei/v1/live_chat/get_live_chat_replay"));
            assert!(b1.contains("\"REPLAY_CONT_0\""));
            assert!(b1.contains("\"playerOffsetMs\":\"125000\""));
            write_http_response(
                &mut s1,
                "200 OK",
                &json!({
                    "continuationContents": {
                        "liveChatContinuation": {
                            "continuations": [
                                { "liveChatReplayContinuationData": { "continuation": "REPLAY_CONT_1" } }
                            ],
                            "actions": [{
                                "replayChatItemAction": {
                                    "videoOffsetTimeMsec": "126000",
                                    "actions": [{
                                        "addChatItemAction": {
                                            "item": {
                                                "liveChatTextMessageRenderer": {
                                                    "id": "r_msg_1",
                                                    "authorName": { "simpleText": "@VodViewer" },
                                                    "message": { "runs": [{ "text": "Great moment!" }] }
                                                }
                                            }
                                        }
                                    }]
                                }
                            }]
                        }
                    }
                })
                .to_string(),
            );
        });

        let client = http_client().unwrap();
        let cancel = Cancellation::default();
        let batch = fetch_live_chat_replay_page_at(
            &base_url,
            &client,
            "REPLAY_CONT_0",
            125_000,
            FALLBACK_CLIENT_VERSION,
            None,
            None,
            &cancel,
        )
        .unwrap();
        assert_eq!(batch.next_continuation.as_deref(), Some("REPLAY_CONT_1"));
        assert_eq!(batch.added.len(), 1);
        assert_eq!(batch.added[0].id, "r_msg_1");
        assert_eq!(batch.added[0].video_offset_ms, Some(126_000));

        server.join().unwrap();
    }
}
