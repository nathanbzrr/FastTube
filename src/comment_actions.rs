use crate::oauth::CommentOrder;
use crate::process::Cancellation;
use crate::session;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroize;

pub(crate) const YOUTUBE_ORIGIN: &str = "https://www.youtube.com";
pub(crate) const INNERTUBE_BASE_URL: &str = "https://www.youtube.com/youtubei/v1";
pub(crate) const FALLBACK_CLIENT_VERSION: &str = "2.20260901.00.00";
pub(crate) const WEB_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const MAX_COMMENT_PAGES: usize = 12;
const MAX_REPLY_PAGES: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentRating {
    Like,
    Dislike,
}

#[derive(Clone, Default)]
pub struct YouTubeWebSession {
    cookies: BTreeMap<String, String>,
    pub client_version: String,
    pub visitor_data: Option<String>,
    pub client_config_refreshed: bool,
    pub delegated_session_id: Option<String>,
    pub api_key: Option<String>,
    pub auth_user_index: u8,
}

impl Drop for YouTubeWebSession {
    fn drop(&mut self) {
        for value in self.cookies.values_mut() {
            value.zeroize();
        }
        if let Some(visitor) = &mut self.visitor_data {
            visitor.zeroize();
        }
        if let Some(delegated) = &mut self.delegated_session_id {
            delegated.zeroize();
        }
        if let Some(api_key) = &mut self.api_key {
            api_key.zeroize();
        }
    }
}

impl fmt::Debug for YouTubeWebSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("YouTubeWebSession")
            .field("cookie_count", &self.cookies.len())
            .field("client_version", &self.client_version)
            .field("has_visitor_data", &self.visitor_data.is_some())
            .field("client_config_refreshed", &self.client_config_refreshed)
            .field(
                "has_delegated_session_id",
                &self.delegated_session_id.is_some(),
            )
            .field("has_api_key", &self.api_key.is_some())
            .field("auth_user_index", &self.auth_user_index)
            .finish()
    }
}

impl YouTubeWebSession {
    #[cfg(test)]
    pub(crate) fn test_session() -> Self {
        let mut cookies = BTreeMap::new();
        cookies.insert("SAPISID".into(), "test-sapisid".into());
        cookies.insert("SID".into(), "test-sid".into());
        cookies.insert("__Secure-1PAPISID".into(), "test-1papisid".into());
        cookies.insert("__Secure-3PAPISID".into(), "test-3papisid".into());
        Self {
            cookies,
            client_version: FALLBACK_CLIENT_VERSION.into(),
            visitor_data: Some("test-visitor".into()),
            client_config_refreshed: true,
            delegated_session_id: None,
            api_key: None,
            auth_user_index: 0,
        }
    }

    pub fn from_devtools_cookies(raw: &[Value]) -> Result<Self, String> {
        let mut cookies = BTreeMap::new();
        for cookie in raw {
            let domain = cookie.get("domain").and_then(Value::as_str).unwrap_or("");
            if !domain.is_empty()
                && !["youtube.com", ".youtube.com", "www.youtube.com"].contains(&domain)
            {
                continue;
            }
            let name = cookie
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let value = cookie.get("value").and_then(Value::as_str).unwrap_or("");
            if !valid_cookie_name(name) || !valid_cookie_value(value) {
                continue;
            }
            cookies.insert(name.to_owned(), value.to_owned());
        }
        let has_api_sid = cookies
            .get("SAPISID")
            .or_else(|| cookies.get("__Secure-3PAPISID"))
            .is_some_and(|v| !v.is_empty());
        let has_sid = cookies
            .get("SID")
            .or_else(|| cookies.get("__Secure-3PSID"))
            .is_some_and(|v| !v.is_empty());
        if !has_api_sid || !has_sid {
            return Err(
                "Sess\u{00e3}o do YouTube inv\u{00e1}lida ou expirada. Entre novamente.".into(),
            );
        }
        Ok(Self {
            cookies,
            client_version: FALLBACK_CLIENT_VERSION.into(),
            visitor_data: None,
            client_config_refreshed: false,
            delegated_session_id: None,
            api_key: None,
            auth_user_index: 0,
        })
    }

    pub fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub fn authorization_header(
        &self,
        timestamp_secs: u64,
        origin: &str,
    ) -> Result<String, String> {
        let sapisid = self
            .cookies
            .get("SAPISID")
            .or_else(|| self.cookies.get("__Secure-3PAPISID"))
            .filter(|v| !v.is_empty())
            .ok_or_else(|| "Cookie de autentica\u{00e7}\u{00e3}o do YouTube ausente.".to_owned())?;
        let mut parts = vec![format!(
            "SAPISIDHASH {}",
            sapisid_hash(timestamp_secs, sapisid, origin)
        )];
        if let Some(p1) = self
            .cookies
            .get("__Secure-1PAPISID")
            .filter(|v| !v.is_empty())
        {
            parts.push(format!(
                "SAPISID1PHASH {}",
                sapisid_hash(timestamp_secs, p1, origin)
            ));
        }
        if let Some(p3) = self
            .cookies
            .get("__Secure-3PAPISID")
            .filter(|v| !v.is_empty())
        {
            parts.push(format!(
                "SAPISID3PHASH {}",
                sapisid_hash(timestamp_secs, p3, origin)
            ));
        }
        Ok(parts.join(" "))
    }

    pub fn absorb_set_cookies(&mut self, headers: &reqwest::header::HeaderMap) {
        for value in headers.get_all(reqwest::header::SET_COOKIE) {
            let Ok(raw) = value.to_str() else {
                continue;
            };
            let pair = raw.split(';').next().unwrap_or("");
            let Some((name, val)) = pair.split_once('=') else {
                continue;
            };
            let name = name.trim();
            let val = val.trim();
            if !valid_cookie_name(name) || !valid_cookie_value(val) {
                continue;
            }
            self.cookies.insert(name.to_owned(), val.to_owned());
        }
    }

    pub fn refresh_client_config_if_needed(
        &mut self,
        client: &reqwest::blocking::Client,
        cancel: &Cancellation,
    ) {
        if self.client_config_refreshed || cancel.cancelled() {
            return;
        }
        self.client_config_refreshed = true;
        let Ok(response) = client
            .get(format!("{YOUTUBE_ORIGIN}/?hl=pt"))
            .header(reqwest::header::USER_AGENT, WEB_USER_AGENT)
            .header(reqwest::header::COOKIE, self.cookie_header())
            .send()
        else {
            return;
        };
        self.absorb_set_cookies(response.headers());
        if !response.status().is_success() {
            return;
        }
        let Ok(html) = response.text() else {
            return;
        };
        let fields = crate::innertube::parse_ytcfg_session_fields(&html);
        if let Some(version) = fields.client_version {
            self.client_version = version;
        }
        if let Some(visitor) = fields.visitor_data {
            self.visitor_data = Some(visitor);
        }
        if let Some(api_key) = fields.api_key {
            self.api_key = Some(api_key);
        }
        if fields.delegated_session_id.is_some() {
            self.delegated_session_id = fields.delegated_session_id;
        }
        if let Some(auth_idx) = fields.auth_user_index {
            self.auth_user_index = auth_idx;
        }
    }
}

fn valid_cookie_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name
            .bytes()
            .all(|b| b.is_ascii_graphic() && !matches!(b, b';' | b'=' | b',' | b'\\'))
}

fn valid_cookie_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 8192
        && !value.bytes().any(|b| matches!(b, b'\r' | b'\n' | 0))
}

pub(crate) fn extract_json_string_field(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = text.find(&needle)? + needle.len();
    let rest = &text[start..];
    let end = rest.find('"')?;
    let val = &rest[..end];
    (!val.is_empty() && val.len() <= 512).then(|| val.to_owned())
}

pub fn sapisid_hash(timestamp_secs: u64, sapisid: &str, origin: &str) -> String {
    let input = format!("{timestamp_secs} {sapisid} {origin}");
    let digest = sha1_hex(input.as_bytes());
    format!("{timestamp_secs}_{digest}")
}

/// Pure-Rust RFC 3174 SHA-1 digest formatted as lowercase hexadecimal.
pub fn sha1_hex(data: &[u8]) -> String {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xEFCD_AB89;
    let mut h2: u32 = 0x98BA_DCFE;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xC3D2_E1F0;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for &chunk in padded.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, &word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(word);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    format!("{h0:08x}{h1:08x}{h2:08x}{h3:08x}{h4:08x}")
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct CommentActionEntry {
    pub like_token: Option<String>,
    pub unlike_token: Option<String>,
    pub dislike_token: Option<String>,
    pub undislike_token: Option<String>,
    pub replies_token: Option<String>,
    pub like_state: Option<CommentRating>,
}

impl CommentActionEntry {
    fn merge_from(&mut self, other: Self) {
        if other.like_token.is_some() {
            self.like_token = other.like_token;
        }
        if other.unlike_token.is_some() {
            self.unlike_token = other.unlike_token;
        }
        if other.dislike_token.is_some() {
            self.dislike_token = other.dislike_token;
        }
        if other.undislike_token.is_some() {
            self.undislike_token = other.undislike_token;
        }
        if other.replies_token.is_some() {
            self.replies_token = other.replies_token;
        }
        if other.like_state.is_some() {
            self.like_state = other.like_state;
        }
    }

    pub fn action_for(
        &self,
        current: Option<CommentRating>,
        target: Option<CommentRating>,
    ) -> Option<&str> {
        match target {
            Some(CommentRating::Like) => self.like_token.as_deref(),
            Some(CommentRating::Dislike) => self.dislike_token.as_deref(),
            None => match current {
                Some(CommentRating::Like) => self.unlike_token.as_deref(),
                Some(CommentRating::Dislike) => self.undislike_token.as_deref(),
                None => self
                    .unlike_token
                    .as_deref()
                    .or(self.undislike_token.as_deref()),
            },
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct VideoCommentTokens {
    pub video_id: String,
    pub order: CommentOrder,
    pub started: bool,
    pub next_page_token: Option<String>,
    pub top_sort_token: Option<String>,
    pub newest_sort_token: Option<String>,
    pub entries: HashMap<String, CommentActionEntry>,
    pub reply_next_tokens: HashMap<String, Option<String>>,
}

impl VideoCommentTokens {
    pub fn for_video(video_id: &str, order: CommentOrder) -> Self {
        Self {
            video_id: video_id.to_owned(),
            order,
            ..Self::default()
        }
    }

    pub fn merge_from(&mut self, other: Self) {
        if self.video_id != other.video_id || self.order != other.order {
            *self = other;
            return;
        }
        self.started |= other.started;
        if other.next_page_token.is_some() {
            self.next_page_token = other.next_page_token;
        }
        if other.top_sort_token.is_some() {
            self.top_sort_token = other.top_sort_token;
        }
        if other.newest_sort_token.is_some() {
            self.newest_sort_token = other.newest_sort_token;
        }
        for (id, entry) in other.entries {
            self.entries.entry(id).or_default().merge_from(entry);
        }
        for (parent_id, token) in other.reply_next_tokens {
            self.reply_next_tokens.insert(parent_id, token);
        }
    }

    pub fn collected_states(&self) -> HashMap<String, CommentRating> {
        self.entries
            .iter()
            .filter_map(|(id, entry)| entry.like_state.map(|state| (id.clone(), state)))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InnerTubeError {
    AuthExpired(String),
    Cancelled(String),
    Other(String),
}

impl InnerTubeError {
    pub fn into_message(self) -> String {
        match self {
            Self::AuthExpired(msg) | Self::Cancelled(msg) | Self::Other(msg) => msg,
        }
    }
}

pub(crate) fn http_client() -> Result<reqwest::blocking::Client, String> {
    Ok(crate::http_client::no_redirect().clone())
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn post_innertube_at(
    base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    endpoint: &str,
    body: Value,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    post_innertube_with_locale_at(base_url, client, session, endpoint, body, "pt", cancel)
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
        return Err(InnerTubeError::Cancelled(
            "Opera\u{00e7}\u{00e3}o cancelada.".into(),
        ));
    }
    let ts = unix_timestamp_secs();
    let auth = session
        .authorization_header(ts, YOUTUBE_ORIGIN)
        .map_err(InnerTubeError::AuthExpired)?;
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
        .map_err(|_| InnerTubeError::Other("Falha de conex\u{00e3}o com o YouTube.".into()))?;
    if cancel.cancelled() {
        return Err(InnerTubeError::Cancelled(
            "Opera\u{00e7}\u{00e3}o cancelada.".into(),
        ));
    }
    session.absorb_set_cookies(response.headers());
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
            "O YouTube recusou a a\u{00e7}\u{00e3}o (HTTP {}).",
            status.as_u16()
        )));
    }
    if json.is_null() && !text.trim().is_empty() {
        return Err(InnerTubeError::Other(
            "Resposta inv\u{00e1}lida do YouTube.".into(),
        ));
    }
    if let Some(err) = parse_innertube_error_json(&json, status.as_u16()) {
        return Err(err);
    }
    Ok(json)
}

pub(crate) fn parse_innertube_error_json(json: &Value, http_status: u16) -> Option<InnerTubeError> {
    let error = json.get("error")?;
    let code = error
        .get("code")
        .and_then(Value::as_u64)
        .unwrap_or(http_status as u64);
    let status = error.get("status").and_then(Value::as_str).unwrap_or("");
    if matches!(code, 401 | 403) || status == "UNAUTHENTICATED" || status == "PERMISSION_DENIED" {
        return Some(InnerTubeError::AuthExpired(
            "Sua sess\u{00e3}o do YouTube expirou ou n\u{00e3}o est\u{00e1} autenticada.".into(),
        ));
    }
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .filter(|m| !m.trim().is_empty());
    Some(InnerTubeError::Other(match message {
        Some(msg) => format!("O YouTube recusou a a\u{00e7}\u{00e3}o: {msg}"),
        None => format!("O YouTube recusou a a\u{00e7}\u{00e3}o (HTTP {code})."),
    }))
}

pub fn validate_perform_comment_action_response(json: &Value) -> Result<(), InnerTubeError> {
    if let Some(err) = parse_innertube_error_json(json, 200) {
        return Err(err);
    }
    if let Some(results) = json.get("actionResults").and_then(Value::as_array) {
        for item in results {
            if let Some(status) = item.get("status").and_then(Value::as_str)
                && status != "STATUS_SUCCEEDED"
            {
                return Err(InnerTubeError::Other(
                    "O YouTube n\u{00e3}o aceitou a avalia\u{00e7}\u{00e3}o deste coment\u{00e1}rio.".into(),
                ));
            }
        }
    }
    Ok(())
}

pub fn perform_comment_action_at(
    base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    action_token: &str,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let response = post_innertube_at(
        base_url,
        client,
        session,
        "comment/perform_comment_action",
        json!({ "actions": [action_token] }),
        cancel,
    )?;
    validate_perform_comment_action_response(&response)?;
    Ok(response)
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommentReportReason {
    Spam,
    Harassment,
    HateSpeech,
    Misinformation,
    UnwantedCommercial,
    Other,
}

impl CommentReportReason {
    #[allow(dead_code)]
    pub const ALL: [Self; 6] = [
        Self::Spam,
        Self::Harassment,
        Self::HateSpeech,
        Self::Misinformation,
        Self::UnwantedCommercial,
        Self::Other,
    ];

    #[allow(dead_code)]
    pub fn label(self, pt: bool) -> &'static str {
        match self {
            Self::Spam => {
                if pt {
                    "Spam ou conteúdo enganoso"
                } else {
                    "Spam or misleading"
                }
            }
            Self::Harassment => {
                if pt {
                    "Assédio ou bullying"
                } else {
                    "Harassment or bullying"
                }
            }
            Self::HateSpeech => {
                if pt {
                    "Discurso de ódio"
                } else {
                    "Hate speech"
                }
            }
            Self::Misinformation => {
                if pt {
                    "Desinformação"
                } else {
                    "Misinformation"
                }
            }
            Self::UnwantedCommercial => {
                if pt {
                    "Conteúdo comercial indesejado"
                } else {
                    "Unwanted commercial content"
                }
            }
            Self::Other => {
                if pt {
                    "Outro motivo"
                } else {
                    "Other"
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn innertube_reason_code(self) -> &'static str {
        match self {
            Self::Spam => "SPAM_OR_MISLEADING",
            Self::Harassment => "HARASSMENT_OR_BULLYING",
            Self::HateSpeech => "HATE_SPEECH",
            Self::Misinformation => "MISINFORMATION",
            Self::UnwantedCommercial => "UNWANTED_COMMERCIAL_CONTENT",
            Self::Other => "OTHER",
        }
    }
}

/// Builds a `comment/update_comment` payload for editing the user's own comment.
#[allow(dead_code)]
pub fn build_edit_comment_payload(
    update_comment_params: &str,
    new_text: &str,
) -> Result<Value, InnerTubeError> {
    let params = update_comment_params.trim();
    let text = new_text.trim();
    if params.is_empty() || params.len() > 4096 {
        return Err(InnerTubeError::Other(
            "Token de edição do comentário ausente ou inválido.".into(),
        ));
    }
    if text.is_empty() || text.chars().count() > 10_000 {
        return Err(InnerTubeError::Other(
            "O texto do comentário não pode estar vazio.".into(),
        ));
    }
    Ok(json!({
        "updateCommentParams": params,
        "commentText": text,
    }))
}

#[allow(dead_code)]
pub fn edit_comment_at(
    base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    update_comment_params: &str,
    new_text: &str,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let payload = build_edit_comment_payload(update_comment_params, new_text)?;
    let response = post_innertube_at(
        base_url,
        client,
        session,
        "comment/update_comment",
        payload,
        cancel,
    )?;
    validate_perform_comment_action_response(&response)?;
    Ok(response)
}

#[allow(dead_code)]
pub fn edit_comment(
    session: &mut YouTubeWebSession,
    update_comment_params: &str,
    new_text: &str,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let client = http_client().map_err(InnerTubeError::Other)?;
    session.refresh_client_config_if_needed(&client, cancel);
    edit_comment_at(
        INNERTUBE_BASE_URL,
        &client,
        session,
        update_comment_params,
        new_text,
        cancel,
    )
}

/// Builds a `comment/perform_comment_action` payload for deleting the user's own comment.
#[allow(dead_code)]
pub fn build_delete_comment_payload(delete_action_token: &str) -> Result<Value, InnerTubeError> {
    let token = delete_action_token.trim();
    if token.is_empty() || token.len() > 4096 {
        return Err(InnerTubeError::Other(
            "Token de exclusão do comentário ausente ou inválido.".into(),
        ));
    }
    Ok(json!({ "actions": [token] }))
}

#[allow(dead_code)]
pub fn delete_comment_at(
    base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    delete_action_token: &str,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let payload = build_delete_comment_payload(delete_action_token)?;
    let response = post_innertube_at(
        base_url,
        client,
        session,
        "comment/perform_comment_action",
        payload,
        cancel,
    )?;
    validate_perform_comment_action_response(&response)?;
    Ok(response)
}

#[allow(dead_code)]
pub fn delete_comment(
    session: &mut YouTubeWebSession,
    delete_action_token: &str,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let client = http_client().map_err(InnerTubeError::Other)?;
    session.refresh_client_config_if_needed(&client, cancel);
    delete_comment_at(
        INNERTUBE_BASE_URL,
        &client,
        session,
        delete_action_token,
        cancel,
    )
}

fn is_valid_comment_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Deletes the user's own comment via YouTube Data API v3 (`DELETE /comments?id=...`) using OAuth.
#[allow(dead_code)]
pub fn delete_comment_oauth_at(
    api_base_url: &str,
    client: &reqwest::blocking::Client,
    access_token: &str,
    comment_id: &str,
    cancel: &Cancellation,
) -> Result<(), InnerTubeError> {
    if cancel.cancelled() {
        return Err(InnerTubeError::Cancelled("Operação cancelada.".into()));
    }
    let cid = comment_id.trim();
    if !is_valid_comment_id(cid) {
        return Err(InnerTubeError::Other("ID de comentário inválido.".into()));
    }
    let token = access_token.trim();
    if token.is_empty() {
        return Err(InnerTubeError::AuthExpired(
            "Token OAuth ausente. Entre novamente.".into(),
        ));
    }
    let url = format!("{}/comments?id={cid}", api_base_url.trim_end_matches('/'));
    let response = client
        .delete(&url)
        .bearer_auth(token)
        .send()
        .map_err(|_| InnerTubeError::Other("Falha de conexão ao excluir comentário.".into()))?;
    if cancel.cancelled() {
        return Err(InnerTubeError::Cancelled("Operação cancelada.".into()));
    }
    let status = response.status();
    if matches!(status.as_u16(), 401 | 403) {
        return Err(InnerTubeError::AuthExpired(
            "Sua sessão do YouTube expirou ou não tem permissão para excluir comentários.".into(),
        ));
    }
    if !status.is_success() {
        return Err(InnerTubeError::Other(format!(
            "Não foi possível excluir o comentário (HTTP {}).",
            status.as_u16()
        )));
    }
    Ok(())
}

#[allow(dead_code)]
pub fn delete_comment_oauth(
    access_token: &str,
    comment_id: &str,
    cancel: &Cancellation,
) -> Result<(), InnerTubeError> {
    let client = http_client().map_err(InnerTubeError::Other)?;
    delete_comment_oauth_at(
        "https://www.googleapis.com/youtube/v3",
        &client,
        access_token,
        comment_id,
        cancel,
    )
}

/// Builds a `flag/flag` payload for reporting a comment with a reason and optional details.
#[allow(dead_code)]
pub fn build_report_comment_payload(
    flag_action: &str,
    reason: CommentReportReason,
    details: Option<&str>,
) -> Result<Value, InnerTubeError> {
    let action = flag_action.trim();
    if action.is_empty() || action.len() > 4096 {
        return Err(InnerTubeError::Other(
            "Token de denúncia do comentário ausente ou inválido.".into(),
        ));
    }
    let mut payload = json!({
        "action": action,
        "reason": reason.innertube_reason_code(),
    });
    if let Some(extra) = details.map(str::trim).filter(|s| !s.is_empty()) {
        let bounded: String = extra.chars().take(1000).collect();
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("userComment".into(), Value::String(bounded));
        }
    }
    Ok(payload)
}

#[allow(dead_code)]
pub fn report_comment_at(
    base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    flag_action: &str,
    reason: CommentReportReason,
    details: Option<&str>,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let payload = build_report_comment_payload(flag_action, reason, details)?;
    let response = post_innertube_at(base_url, client, session, "flag/flag", payload, cancel)?;
    validate_perform_comment_action_response(&response)?;
    Ok(response)
}

#[allow(dead_code)]
pub fn report_comment(
    session: &mut YouTubeWebSession,
    flag_action: &str,
    reason: CommentReportReason,
    details: Option<&str>,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let client = http_client().map_err(InnerTubeError::Other)?;
    session.refresh_client_config_if_needed(&client, cancel);
    report_comment_at(
        INNERTUBE_BASE_URL,
        &client,
        session,
        flag_action,
        reason,
        details,
        cancel,
    )
}

fn walk_values<'a>(root: &'a Value, visitor: &mut impl FnMut(&'a Value)) {
    visitor(root);
    match root {
        Value::Object(map) => {
            for val in map.values() {
                walk_values(val, visitor);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_values(item, visitor);
            }
        }
        _ => {}
    }
}

fn walk_named_objects<'a>(root: &'a Value, visitor: &mut impl FnMut(&'a str, &'a Value)) {
    match root {
        Value::Object(map) => {
            for (k, v) in map {
                if v.is_object() {
                    visitor(k.as_str(), v);
                }
                walk_named_objects(v, visitor);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_named_objects(item, visitor);
            }
        }
        _ => {}
    }
}

fn find_first_string_key<'a>(root: &'a Value, target_key: &str) -> Option<&'a str> {
    let mut found = None;
    walk_values(root, &mut |val| {
        if found.is_some() {
            return;
        }
        if let Some(s) = val
            .get(target_key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            found = Some(s);
        }
    });
    found
}

pub fn parse_watch_comments_continuation(watch_json: &Value) -> Option<String> {
    let mut found = None;
    walk_values(watch_json, &mut |val| {
        if found.is_some() {
            return;
        }
        if val.get("sectionIdentifier").and_then(Value::as_str) == Some("comment-item-section") {
            found = find_first_string_key(val, "token").map(str::to_owned);
        }
    });
    found
}

fn extract_sort_tokens(page_json: &Value) -> (Option<String>, Option<String>) {
    let mut top = None;
    let mut newest = None;
    walk_named_objects(page_json, &mut |key, obj| {
        if key != "sortFilterSubMenuRenderer" {
            return;
        }
        let Some(items) = obj.get("subMenuItems").and_then(Value::as_array) else {
            return;
        };
        if let Some(first) = items.first()
            && top.is_none()
        {
            top = first
                .pointer("/serviceEndpoint/continuationCommand/token")
                .and_then(Value::as_str)
                .or_else(|| find_first_string_key(first, "token"))
                .map(str::to_owned);
        }
        if let Some(second) = items.get(1)
            && newest.is_none()
        {
            newest = second
                .pointer("/serviceEndpoint/continuationCommand/token")
                .and_then(Value::as_str)
                .or_else(|| find_first_string_key(second, "token"))
                .map(str::to_owned);
        }
    });
    (top, newest)
}

fn extract_action_token(command_container: Option<&Value>) -> Option<String> {
    let container = command_container?;
    container
        .pointer("/innertubeCommand/performCommentActionEndpoint/action")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            container
                .pointer("/performCommentActionEndpoint/action")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        })
        .or_else(|| find_first_string_key(container, "action"))
        .map(str::to_owned)
}

fn parse_like_state_str(raw: Option<&str>) -> Option<CommentRating> {
    match raw? {
        "TOOLBAR_LIKE_STATE_LIKED" => Some(CommentRating::Like),
        "TOOLBAR_LIKE_STATE_DISLIKED" => Some(CommentRating::Dislike),
        _ => None,
    }
}

fn extract_innermost_comment_vm(obj: &Value) -> Option<&Value> {
    if obj.get("commentKey").is_some() {
        return Some(obj);
    }
    let mut found = None;
    walk_named_objects(obj, &mut |key, val| {
        if found.is_none() && key == "commentViewModel" && val.get("commentKey").is_some() {
            found = Some(val);
        }
    });
    found
}

fn parse_viewmodel_entry(
    vm: &Value,
    entities: &HashMap<&str, &Value>,
    replies_token: Option<String>,
) -> Option<(String, CommentActionEntry)> {
    let comment_key = vm.get("commentKey").and_then(Value::as_str).unwrap_or("");
    let comment_entity = entities
        .get(comment_key)
        .and_then(|p| p.get("commentEntityPayload").or(Some(*p)));
    let comment_id = comment_entity
        .and_then(|e| e.pointer("/properties/commentId"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            vm.get("commentId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        })?
        .to_owned();

    let toolbar_surface_key = vm
        .get("toolbarSurfaceKey")
        .and_then(Value::as_str)
        .unwrap_or("");
    let toolbar = entities.get(toolbar_surface_key).and_then(|p| {
        p.get("engagementToolbarSurfaceEntityPayload").or_else(|| {
            (p.get("likeCommand").is_some() || p.get("dislikeCommand").is_some()).then_some(*p)
        })
    });

    let toolbar_state_key = vm
        .get("toolbarStateKey")
        .and_then(Value::as_str)
        .unwrap_or("");
    let state_payload = entities
        .get(toolbar_state_key)
        .and_then(|p| p.get("engagementToolbarStateEntityPayload").or(Some(*p)));

    let entry = CommentActionEntry {
        like_token: extract_action_token(toolbar.and_then(|t| t.get("likeCommand"))),
        unlike_token: extract_action_token(toolbar.and_then(|t| t.get("unlikeCommand"))),
        dislike_token: extract_action_token(toolbar.and_then(|t| t.get("dislikeCommand"))),
        undislike_token: extract_action_token(toolbar.and_then(|t| t.get("undislikeCommand"))),
        replies_token,
        like_state: parse_like_state_str(
            state_payload
                .and_then(|s| s.get("likeState"))
                .and_then(Value::as_str),
        ),
    };
    Some((comment_id, entry))
}

fn parse_legacy_comment_renderer(
    renderer: &Value,
    replies_token: Option<String>,
) -> Option<(String, CommentActionEntry)> {
    let comment_id = renderer
        .get("commentId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?
        .to_owned();
    let buttons = renderer.pointer("/actionButtons/commentActionButtonsRenderer")?;
    let like_btn = buttons.pointer("/likeButton/toggleButtonRenderer");
    let dislike_btn = buttons.pointer("/dislikeButton/toggleButtonRenderer");
    let liked = like_btn
        .and_then(|b| b.get("isToggled"))
        .and_then(Value::as_bool)
        == Some(true);
    let disliked = dislike_btn
        .and_then(|b| b.get("isToggled"))
        .and_then(Value::as_bool)
        == Some(true);
    let like_state = if liked {
        Some(CommentRating::Like)
    } else if disliked {
        Some(CommentRating::Dislike)
    } else {
        None
    };
    let entry = CommentActionEntry {
        like_token: extract_action_token(like_btn.and_then(|b| b.get("defaultServiceEndpoint"))),
        unlike_token: extract_action_token(like_btn.and_then(|b| b.get("toggledServiceEndpoint"))),
        dislike_token: extract_action_token(
            dislike_btn.and_then(|b| b.get("defaultServiceEndpoint")),
        ),
        undislike_token: extract_action_token(
            dislike_btn.and_then(|b| b.get("toggledServiceEndpoint")),
        ),
        replies_token,
        like_state,
    };
    Some((comment_id, entry))
}

#[derive(Default, Debug)]
pub struct ParsedCommentsPage {
    pub entries: HashMap<String, CommentActionEntry>,
    pub next_token: Option<String>,
    pub top_sort_token: Option<String>,
    pub newest_sort_token: Option<String>,
}

pub fn parse_innertube_comments_page(page_json: &Value) -> ParsedCommentsPage {
    let mut entities: HashMap<&str, &Value> = HashMap::new();
    walk_values(page_json, &mut |val| {
        if let Some(key) = val
            .get("entityKey")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            && let Some(payload) = val.get("payload")
        {
            entities.insert(key, payload);
        }
    });

    let mut entries: HashMap<String, CommentActionEntry> = HashMap::new();
    let mut next_token: Option<String> = None;
    let (top_sort_token, newest_sort_token) = extract_sort_tokens(page_json);

    walk_named_objects(page_json, &mut |key, obj| {
        if key != "reloadContinuationItemsCommand" && key != "appendContinuationItemsAction" {
            return;
        }
        let Some(items) = obj.get("continuationItems").and_then(Value::as_array) else {
            return;
        };
        let has_comment_items = items.iter().any(|item| {
            item.get("commentThreadRenderer").is_some()
                || item.get("commentViewModel").is_some()
                || item.get("commentRenderer").is_some()
        });
        for item in items {
            if let Some(thread) = item.get("commentThreadRenderer") {
                let replies_token = thread
                    .get("replies")
                    .and_then(|r| find_first_string_key(r, "token"))
                    .map(str::to_owned);
                if let Some(vm) = extract_innermost_comment_vm(thread)
                    && let Some((id, entry)) =
                        parse_viewmodel_entry(vm, &entities, replies_token.clone())
                {
                    entries.entry(id).or_default().merge_from(entry);
                } else if let Some(renderer) = thread.pointer("/comment/commentRenderer")
                    && let Some((id, entry)) =
                        parse_legacy_comment_renderer(renderer, replies_token)
                {
                    entries.entry(id).or_default().merge_from(entry);
                }
            } else if let Some(vm_wrap) = item.get("commentViewModel") {
                if let Some(vm) = extract_innermost_comment_vm(vm_wrap)
                    && let Some((id, entry)) = parse_viewmodel_entry(vm, &entities, None)
                {
                    entries.entry(id).or_default().merge_from(entry);
                }
            } else if let Some(renderer) = item.get("commentRenderer")
                && let Some((id, entry)) = parse_legacy_comment_renderer(renderer, None)
            {
                entries.entry(id).or_default().merge_from(entry);
            }
        }
        if has_comment_items
            && let Some(cont) = items
                .last()
                .and_then(|last| last.get("continuationItemRenderer"))
        {
            next_token = find_first_string_key(cont, "token").map(str::to_owned);
        }
    });

    ParsedCommentsPage {
        entries,
        next_token,
        top_sort_token,
        newest_sort_token,
    }
}

fn load_page_into_cache_at(
    base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    cache: &mut VideoCommentTokens,
    continuation: &str,
    reply_parent_id: Option<&str>,
    cancel: &Cancellation,
) -> Result<(), InnerTubeError> {
    let page = post_innertube_at(
        base_url,
        client,
        session,
        "next",
        json!({ "continuation": continuation }),
        cancel,
    )?;
    let parsed = parse_innertube_comments_page(&page);
    if parsed.top_sort_token.is_some() {
        cache.top_sort_token = parsed.top_sort_token;
    }
    if parsed.newest_sort_token.is_some() {
        cache.newest_sort_token = parsed.newest_sort_token;
    }
    for (id, entry) in parsed.entries {
        cache.entries.entry(id).or_default().merge_from(entry);
    }
    if let Some(parent_id) = reply_parent_id {
        cache
            .reply_next_tokens
            .insert(parent_id.to_owned(), parsed.next_token);
    } else {
        cache.next_page_token = parsed.next_token;
    }
    Ok(())
}

fn start_video_pages_at(
    base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    cache: &mut VideoCommentTokens,
    cancel: &Cancellation,
) -> Result<(), InnerTubeError> {
    cache.started = true;
    cache.next_page_token = None;
    let watch = post_innertube_at(
        base_url,
        client,
        session,
        "next",
        json!({ "videoId": cache.video_id }),
        cancel,
    )?;
    let Some(start_token) = parse_watch_comments_continuation(&watch) else {
        return Ok(());
    };
    load_page_into_cache_at(base_url, client, session, cache, &start_token, None, cancel)?;
    if cache.order == CommentOrder::Time
        && let Some(newest_token) = cache.newest_sort_token.clone()
    {
        load_page_into_cache_at(
            base_url,
            client,
            session,
            cache,
            &newest_token,
            None,
            cancel,
        )?;
    }
    Ok(())
}

pub fn sync_first_page_at(
    base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    video_id: &str,
    order: CommentOrder,
    cancel: &Cancellation,
) -> Result<VideoCommentTokens, InnerTubeError> {
    session.refresh_client_config_if_needed(client, cancel);
    let mut cache = VideoCommentTokens::for_video(video_id, order);
    start_video_pages_at(base_url, client, session, &mut cache, cancel)?;
    Ok(cache)
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_and_perform_comment_action_at(
    base_url: &str,
    client: &reqwest::blocking::Client,
    session: &mut YouTubeWebSession,
    cache: &mut VideoCommentTokens,
    comment_id: &str,
    parent_comment_id: Option<&str>,
    current: Option<CommentRating>,
    target: Option<CommentRating>,
    cancel: &Cancellation,
) -> Result<Option<CommentRating>, InnerTubeError> {
    session.refresh_client_config_if_needed(client, cancel);

    if let Some(action) = cache
        .entries
        .get(comment_id)
        .and_then(|entry| entry.action_for(current, target))
        .map(str::to_owned)
    {
        perform_comment_action_at(base_url, client, session, &action, cancel)?;
        return Ok(target);
    }

    let inferred_parent = parent_comment_id.or_else(|| comment_id.split_once('.').map(|(p, _)| p));

    if !cache.started {
        start_video_pages_at(base_url, client, session, cache, cancel)?;
    }

    if let Some(action) = cache
        .entries
        .get(comment_id)
        .and_then(|entry| entry.action_for(current, target))
        .map(str::to_owned)
    {
        perform_comment_action_at(base_url, client, session, &action, cancel)?;
        return Ok(target);
    }

    if let Some(parent_id) = inferred_parent {
        let mut pages = 0;
        while !cache.entries.contains_key(parent_id) && pages < MAX_COMMENT_PAGES {
            let Some(next) = cache.next_page_token.clone() else {
                break;
            };
            load_page_into_cache_at(base_url, client, session, cache, &next, None, cancel)?;
            pages += 1;
        }
        if let Some(initial_replies) = cache
            .entries
            .get(parent_id)
            .and_then(|entry| entry.replies_token.clone())
        {
            let mut reply_token = if cache.reply_next_tokens.contains_key(parent_id) {
                cache.reply_next_tokens.get(parent_id).cloned().flatten()
            } else {
                Some(initial_replies)
            };
            let mut reply_pages = 0;
            while cache
                .entries
                .get(comment_id)
                .and_then(|e| e.action_for(current, target))
                .is_none()
                && reply_pages < MAX_REPLY_PAGES
            {
                let Some(token) = reply_token.take() else {
                    break;
                };
                load_page_into_cache_at(
                    base_url,
                    client,
                    session,
                    cache,
                    &token,
                    Some(parent_id),
                    cancel,
                )?;
                reply_token = cache.reply_next_tokens.get(parent_id).cloned().flatten();
                reply_pages += 1;
            }
        }
    } else {
        let mut pages = 0;
        while cache
            .entries
            .get(comment_id)
            .and_then(|e| e.action_for(current, target))
            .is_none()
            && pages < MAX_COMMENT_PAGES
        {
            let Some(next) = cache.next_page_token.clone() else {
                break;
            };
            load_page_into_cache_at(base_url, client, session, cache, &next, None, cancel)?;
            pages += 1;
        }
    }

    // If the user just posted a comment or reply in this session, page 1 may be stale; resync once.
    if cache
        .entries
        .get(comment_id)
        .and_then(|e| e.action_for(current, target))
        .is_none()
    {
        if let Some(parent_id) = inferred_parent
            && let Some(replies_token) = cache
                .entries
                .get(parent_id)
                .and_then(|e| e.replies_token.clone())
        {
            load_page_into_cache_at(
                base_url,
                client,
                session,
                cache,
                &replies_token,
                Some(parent_id),
                cancel,
            )?;
        } else {
            let mut fresh = VideoCommentTokens::for_video(&cache.video_id, CommentOrder::Time);
            start_video_pages_at(base_url, client, session, &mut fresh, cancel)?;
            for (id, entry) in fresh.entries {
                cache.entries.entry(id).or_default().merge_from(entry);
            }
        }
    }

    let action = cache
        .entries
        .get(comment_id)
        .and_then(|entry| entry.action_for(current, target))
        .map(str::to_owned)
        .ok_or_else(|| {
            InnerTubeError::Other(
                "N\u{00e3}o foi poss\u{00ed}vel localizar este coment\u{00e1}rio no YouTube para avaliar."
                    .into(),
            )
        })?;

    perform_comment_action_at(base_url, client, session, &action, cancel)?;
    Ok(target)
}

fn should_retry_interactive(error: &str) -> bool {
    error.contains("Entre na sua conta") || error.contains("sess\u{00e3}o expirou")
}

pub fn acquire_web_session(
    cached: Option<YouTubeWebSession>,
    allow_interactive: bool,
    cancel: &Cancellation,
) -> Result<YouTubeWebSession, String> {
    if let Some(session) = cached {
        return Ok(session);
    }
    match session::youtube_cookies(false, cancel) {
        Ok(cookies) => YouTubeWebSession::from_devtools_cookies(&cookies),
        Err(err) if allow_interactive && should_retry_interactive(&err) => {
            let cookies = session::youtube_cookies(true, cancel)?;
            YouTubeWebSession::from_devtools_cookies(&cookies)
        }
        Err(err) => Err(err),
    }
}

pub struct RemoteCommentRatingOutcome {
    pub web_session: Option<YouTubeWebSession>,
    pub cache: VideoCommentTokens,
    pub result: Result<Option<CommentRating>, String>,
}

pub fn sync_comment_states_remote(
    cached_session: Option<YouTubeWebSession>,
    video_id: &str,
    order: CommentOrder,
    cancel: &Cancellation,
) -> (
    Option<YouTubeWebSession>,
    bool,
    Result<VideoCommentTokens, String>,
) {
    let client = match http_client() {
        Ok(c) => c,
        Err(e) => return (cached_session, false, Err(e)),
    };
    let mut session = match acquire_web_session(cached_session, false, cancel) {
        Ok(s) => s,
        Err(e) => return (None, false, Err(e)),
    };
    match sync_first_page_at(
        INNERTUBE_BASE_URL,
        &client,
        &mut session,
        video_id,
        order,
        cancel,
    ) {
        Ok(tokens) => (Some(session), false, Ok(tokens)),
        Err(InnerTubeError::AuthExpired(msg)) => (None, true, Err(msg)),
        Err(err) => (Some(session), false, Err(err.into_message())),
    }
}

pub fn apply_comment_rating_remote(
    cached_session: Option<YouTubeWebSession>,
    mut cache: VideoCommentTokens,
    comment_id: &str,
    parent_comment_id: Option<&str>,
    current: Option<CommentRating>,
    target: Option<CommentRating>,
    cancel: &Cancellation,
) -> RemoteCommentRatingOutcome {
    let client = match http_client() {
        Ok(c) => c,
        Err(e) => {
            return RemoteCommentRatingOutcome {
                web_session: cached_session,
                cache,
                result: Err(e),
            };
        }
    };
    let had_cached = cached_session.is_some();
    let mut session = match acquire_web_session(cached_session, true, cancel) {
        Ok(s) => s,
        Err(e) => {
            return RemoteCommentRatingOutcome {
                web_session: None,
                cache,
                result: Err(e),
            };
        }
    };

    match resolve_and_perform_comment_action_at(
        INNERTUBE_BASE_URL,
        &client,
        &mut session,
        &mut cache,
        comment_id,
        parent_comment_id,
        current,
        target,
        cancel,
    ) {
        Ok(applied) => RemoteCommentRatingOutcome {
            web_session: Some(session),
            cache,
            result: Ok(applied),
        },
        Err(InnerTubeError::AuthExpired(_)) if had_cached && !cancel.cancelled() => {
            // Re-acquire fresh cookies (headless first, then interactive login) and retry once.
            match acquire_web_session(None, true, cancel) {
                Ok(mut refreshed) => {
                    let mut fresh_cache =
                        VideoCommentTokens::for_video(&cache.video_id, cache.order);
                    let retried = resolve_and_perform_comment_action_at(
                        INNERTUBE_BASE_URL,
                        &client,
                        &mut refreshed,
                        &mut fresh_cache,
                        comment_id,
                        parent_comment_id,
                        current,
                        target,
                        cancel,
                    );
                    cache.merge_from(fresh_cache);
                    match retried {
                        Ok(applied) => RemoteCommentRatingOutcome {
                            web_session: Some(refreshed),
                            cache,
                            result: Ok(applied),
                        },
                        Err(InnerTubeError::AuthExpired(msg)) => RemoteCommentRatingOutcome {
                            web_session: None,
                            cache,
                            result: Err(msg),
                        },
                        Err(err) => RemoteCommentRatingOutcome {
                            web_session: Some(refreshed),
                            cache,
                            result: Err(err.into_message()),
                        },
                    }
                }
                Err(err) => RemoteCommentRatingOutcome {
                    web_session: None,
                    cache,
                    result: Err(err),
                },
            }
        }
        Err(InnerTubeError::AuthExpired(msg)) => RemoteCommentRatingOutcome {
            web_session: None,
            cache,
            result: Err(msg),
        },
        Err(err) => RemoteCommentRatingOutcome {
            web_session: Some(session),
            cache,
            result: Err(err.into_message()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn sha1_matches_rfc_3174_vectors() {
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha1_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn sapisid_hash_builds_expected_authorization_headers() {
        let mut session = YouTubeWebSession::test_session();
        let header = session
            .authorization_header(1_700_000_000, YOUTUBE_ORIGIN)
            .unwrap();
        let expected_p0 = sapisid_hash(1_700_000_000, "test-sapisid", YOUTUBE_ORIGIN);
        let expected_p1 = sapisid_hash(1_700_000_000, "test-1papisid", YOUTUBE_ORIGIN);
        let expected_p3 = sapisid_hash(1_700_000_000, "test-3papisid", YOUTUBE_ORIGIN);
        assert_eq!(
            header,
            format!(
                "SAPISIDHASH {expected_p0} SAPISID1PHASH {expected_p1} SAPISID3PHASH {expected_p3}"
            )
        );

        let mut resp_headers = reqwest::header::HeaderMap::new();
        resp_headers.append(
            reqwest::header::SET_COOKIE,
            "__Secure-1PSIDTS=rotated-token; Path=/; Secure"
                .parse()
                .unwrap(),
        );
        session.absorb_set_cookies(&resp_headers);
        assert!(
            session
                .cookie_header()
                .contains("__Secure-1PSIDTS=rotated-token")
        );
    }

    #[test]
    fn from_devtools_cookies_validates_required_tokens() {
        let missing = YouTubeWebSession::from_devtools_cookies(&[
            json!({"name": "SAPISID", "value": "abc", "domain": ".youtube.com"}),
        ]);
        assert!(missing.is_err());

        let valid = YouTubeWebSession::from_devtools_cookies(&[
            json!({"name": "SAPISID", "value": "abc", "domain": ".youtube.com"}),
            json!({"name": "__Secure-3PSID", "value": "xyz", "domain": ".youtube.com"}),
            json!({"name": "OTHER", "value": "ignored", "domain": ".example.com"}),
        ])
        .unwrap();
        assert!(valid.cookie_header().contains("SAPISID=abc"));
        assert!(!valid.cookie_header().contains("ignored"));
    }

    #[test]
    fn parses_entity_toolbar_tokens_like_state_and_replies_continuation() {
        let page = json!({
            "onResponseReceivedEndpoints": [
                {
                    "reloadContinuationItemsCommand": {
                        "continuationItems": [
                            {
                                "commentsHeaderRenderer": {
                                    "sortMenu": {
                                        "sortFilterSubMenuRenderer": {
                                            "subMenuItems": [
                                                { "serviceEndpoint": { "continuationCommand": { "token": "TOP_SORT" } } },
                                                { "serviceEndpoint": { "continuationCommand": { "token": "NEW_SORT" } } }
                                            ]
                                        }
                                    }
                                }
                            },
                            {
                                "commentThreadRenderer": {
                                    "commentViewModel": {
                                        "commentViewModel": {
                                            "commentId": "c1",
                                            "commentKey": "ck1",
                                            "toolbarSurfaceKey": "ts1",
                                            "toolbarStateKey": "tst1"
                                        }
                                    },
                                    "replies": {
                                        "commentRepliesRenderer": {
                                            "contents": [
                                                {
                                                    "continuationItemRenderer": {
                                                        "continuationEndpoint": {
                                                            "continuationCommand": { "token": "REPLIES_C1" }
                                                        }
                                                    }
                                                }
                                            ]
                                        }
                                    }
                                }
                            },
                            {
                                "continuationItemRenderer": {
                                    "continuationEndpoint": {
                                        "continuationCommand": { "token": "NEXT_COMMENTS" }
                                    }
                                }
                            }
                        ]
                    }
                }
            ],
            "frameworkUpdates": {
                "entityBatchUpdate": {
                    "mutations": [
                        {
                            "entityKey": "ck1",
                            "payload": {
                                "commentEntityPayload": {
                                    "properties": { "commentId": "c1" }
                                }
                            }
                        },
                        {
                            "entityKey": "ts1",
                            "payload": {
                                "engagementToolbarSurfaceEntityPayload": {
                                    "likeCommand": { "innertubeCommand": { "performCommentActionEndpoint": { "action": "ACT_LIKE" } } },
                                    "unlikeCommand": { "innertubeCommand": { "performCommentActionEndpoint": { "action": "ACT_UNLIKE" } } },
                                    "dislikeCommand": { "innertubeCommand": { "performCommentActionEndpoint": { "action": "ACT_DISLIKE" } } },
                                    "undislikeCommand": { "innertubeCommand": { "performCommentActionEndpoint": { "action": "ACT_UNDISLIKE" } } }
                                }
                            }
                        },
                        {
                            "entityKey": "tst1",
                            "payload": {
                                "engagementToolbarStateEntityPayload": {
                                    "likeState": "TOOLBAR_LIKE_STATE_LIKED"
                                }
                            }
                        }
                    ]
                }
            }
        });

        let parsed = parse_innertube_comments_page(&page);
        assert_eq!(parsed.top_sort_token.as_deref(), Some("TOP_SORT"));
        assert_eq!(parsed.newest_sort_token.as_deref(), Some("NEW_SORT"));
        assert_eq!(parsed.next_token.as_deref(), Some("NEXT_COMMENTS"));
        let entry = parsed.entries.get("c1").unwrap();
        assert_eq!(entry.like_token.as_deref(), Some("ACT_LIKE"));
        assert_eq!(entry.unlike_token.as_deref(), Some("ACT_UNLIKE"));
        assert_eq!(entry.dislike_token.as_deref(), Some("ACT_DISLIKE"));
        assert_eq!(entry.undislike_token.as_deref(), Some("ACT_UNDISLIKE"));
        assert_eq!(entry.replies_token.as_deref(), Some("REPLIES_C1"));
        assert_eq!(entry.like_state, Some(CommentRating::Like));
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
    fn resolves_and_performs_remote_comment_like_and_dislike_via_innertube() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}/youtubei/v1");

        let server = std::thread::spawn(move || {
            // 1) next {"videoId": "vid1"}
            let (mut s1, _) = listener.accept().unwrap();
            let (h1, b1) = read_http_request(&mut s1);
            assert!(h1.contains("/youtubei/v1/next"));
            assert!(h1.contains("SAPISIDHASH "));
            assert!(b1.contains("\"vid1\""));
            write_http_response(
                &mut s1,
                "200 OK",
                &json!({
                    "contents": {
                        "sectionIdentifier": "comment-item-section",
                        "continuationEndpoint": { "continuationCommand": { "token": "START_TOK" } }
                    }
                })
                .to_string(),
            );

            // 2) next {"continuation": "START_TOK"}
            let (mut s2, _) = listener.accept().unwrap();
            let (_h2, b2) = read_http_request(&mut s2);
            assert!(b2.contains("START_TOK"));
            write_http_response(
                &mut s2,
                "200 OK",
                &json!({
                    "onResponseReceivedEndpoints": [{
                        "reloadContinuationItemsCommand": {
                            "continuationItems": [{
                                "commentThreadRenderer": {
                                    "commentViewModel": {
                                        "commentId": "c1",
                                        "commentKey": "ck1",
                                        "toolbarSurfaceKey": "ts1",
                                        "toolbarStateKey": "tst1"
                                    }
                                }
                            }]
                        }
                    }],
                    "frameworkUpdates": {
                        "entityBatchUpdate": {
                            "mutations": [
                                { "entityKey": "ck1", "payload": { "commentEntityPayload": { "properties": { "commentId": "c1" } } } },
                                { "entityKey": "ts1", "payload": { "engagementToolbarSurfaceEntityPayload": {
                                    "likeCommand": { "innertubeCommand": { "performCommentActionEndpoint": { "action": "TOKEN_LIKE_C1" } } },
                                    "unlikeCommand": { "innertubeCommand": { "performCommentActionEndpoint": { "action": "TOKEN_UNLIKE_C1" } } },
                                    "dislikeCommand": { "innertubeCommand": { "performCommentActionEndpoint": { "action": "TOKEN_DISLIKE_C1" } } },
                                    "undislikeCommand": { "innertubeCommand": { "performCommentActionEndpoint": { "action": "TOKEN_UNDISLIKE_C1" } } }
                                } } },
                                { "entityKey": "tst1", "payload": { "engagementToolbarStateEntityPayload": { "likeState": "TOOLBAR_LIKE_STATE_INDIFFERENT" } } }
                            ]
                        }
                    }
                })
                .to_string(),
            );

            // 3) perform_comment_action -> TOKEN_LIKE_C1
            let (mut s3, _) = listener.accept().unwrap();
            let (h3, b3) = read_http_request(&mut s3);
            assert!(h3.contains("/youtubei/v1/comment/perform_comment_action"));
            assert!(b3.contains("TOKEN_LIKE_C1"));
            write_http_response(
                &mut s3,
                "200 OK",
                &json!({ "actionResults": [{ "status": "STATUS_SUCCEEDED" }] }).to_string(),
            );

            // 4) perform_comment_action -> TOKEN_DISLIKE_C1 (cached!)
            let (mut s4, _) = listener.accept().unwrap();
            let (h4, b4) = read_http_request(&mut s4);
            assert!(h4.contains("/youtubei/v1/comment/perform_comment_action"));
            assert!(b4.contains("TOKEN_DISLIKE_C1"));
            write_http_response(
                &mut s4,
                "200 OK",
                &json!({ "actionResults": [{ "status": "STATUS_SUCCEEDED" }] }).to_string(),
            );

            // 5) perform_comment_action -> TOKEN_UNDISLIKE_C1 fails with STATUS_FAILED
            let (mut s5, _) = listener.accept().unwrap();
            let (_h5, b5) = read_http_request(&mut s5);
            assert!(b5.contains("TOKEN_UNDISLIKE_C1"));
            write_http_response(
                &mut s5,
                "200 OK",
                &json!({ "actionResults": [{ "status": "STATUS_FAILED" }] }).to_string(),
            );
        });

        let client = http_client().unwrap();
        let mut session = YouTubeWebSession::test_session();
        let mut cache = VideoCommentTokens::for_video("vid1", CommentOrder::Relevance);
        let cancel = Cancellation::default();

        let r1 = resolve_and_perform_comment_action_at(
            &base_url,
            &client,
            &mut session,
            &mut cache,
            "c1",
            None,
            None,
            Some(CommentRating::Like),
            &cancel,
        )
        .unwrap();
        assert_eq!(r1, Some(CommentRating::Like));

        let r2 = resolve_and_perform_comment_action_at(
            &base_url,
            &client,
            &mut session,
            &mut cache,
            "c1",
            None,
            Some(CommentRating::Like),
            Some(CommentRating::Dislike),
            &cancel,
        )
        .unwrap();
        assert_eq!(r2, Some(CommentRating::Dislike));

        let r3 = resolve_and_perform_comment_action_at(
            &base_url,
            &client,
            &mut session,
            &mut cache,
            "c1",
            None,
            Some(CommentRating::Dislike),
            None,
            &cancel,
        );
        assert!(r3.is_err());

        server.join().unwrap();
    }

    #[test]
    fn comment_edit_delete_and_report_payload_builders_validate_contracts() {
        let edit = build_edit_comment_payload("UPD_PARAMS_1", "Updated comment text").unwrap();
        assert_eq!(edit["updateCommentParams"], "UPD_PARAMS_1");
        assert_eq!(edit["commentText"], "Updated comment text");
        assert!(build_edit_comment_payload("   ", "text").is_err());
        assert!(build_edit_comment_payload("UPD_PARAMS_1", "   ").is_err());

        let del = build_delete_comment_payload("DEL_ACTION_1").unwrap();
        assert_eq!(del["actions"][0], "DEL_ACTION_1");
        assert!(build_delete_comment_payload("").is_err());

        let report_basic =
            build_report_comment_payload("FLAG_ACT_1", CommentReportReason::Spam, None).unwrap();
        assert_eq!(report_basic["action"], "FLAG_ACT_1");
        assert_eq!(report_basic["reason"], "SPAM_OR_MISLEADING");
        assert!(report_basic.get("userComment").is_none());

        let report_details = build_report_comment_payload(
            "FLAG_ACT_2",
            CommentReportReason::Harassment,
            Some("Repeated personal attacks"),
        )
        .unwrap();
        assert_eq!(report_details["reason"], "HARASSMENT_OR_BULLYING");
        assert_eq!(report_details["userComment"], "Repeated personal attacks");
        assert!(build_report_comment_payload(" ", CommentReportReason::Other, None).is_err());

        assert_eq!(CommentReportReason::ALL.len(), 6);
        assert_eq!(
            CommentReportReason::HateSpeech.label(true),
            "Discurso de ódio"
        );
        assert_eq!(CommentReportReason::HateSpeech.label(false), "Hate speech");
    }

    #[test]
    fn edit_delete_oauth_delete_and_report_comment_roundtrip_over_http() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}/youtubei/v1");
        let oauth_base = format!("http://127.0.0.1:{port}/youtube/v3");

        let server = std::thread::spawn(move || {
            // 1) comment/update_comment
            let (mut s1, _) = listener.accept().unwrap();
            let (h1, b1) = read_http_request(&mut s1);
            assert!(h1.contains("/youtubei/v1/comment/update_comment"));
            assert!(b1.contains("UPD_TOK"));
            assert!(b1.contains("Edited body"));
            write_http_response(
                &mut s1,
                "200 OK",
                &json!({ "actionResults": [{ "status": "STATUS_SUCCEEDED" }] }).to_string(),
            );

            // 2) comment/perform_comment_action (delete via web session)
            let (mut s2, _) = listener.accept().unwrap();
            let (h2, b2) = read_http_request(&mut s2);
            assert!(h2.contains("/youtubei/v1/comment/perform_comment_action"));
            assert!(b2.contains("DEL_TOK"));
            write_http_response(
                &mut s2,
                "200 OK",
                &json!({ "actionResults": [{ "status": "STATUS_SUCCEEDED" }] }).to_string(),
            );

            // 3) DELETE /youtube/v3/comments?id=UgxOwn123 (delete via OAuth)
            let (mut s3, _) = listener.accept().unwrap();
            let (h3, _b3) = read_http_request(&mut s3);
            assert!(h3.starts_with("DELETE /youtube/v3/comments?id=UgxOwn123 "));
            assert!(h3.contains("Bearer oauth_tok_test"));
            write_http_response(&mut s3, "204 No Content", "");

            // 4) flag/flag (report comment)
            let (mut s4, _) = listener.accept().unwrap();
            let (h4, b4) = read_http_request(&mut s4);
            assert!(h4.contains("/youtubei/v1/flag/flag"));
            assert!(b4.contains("FLAG_TOK"));
            assert!(b4.contains("SPAM_OR_MISLEADING"));
            assert!(b4.contains("Bot link"));
            write_http_response(
                &mut s4,
                "200 OK",
                &json!({ "actionResults": [{ "status": "STATUS_SUCCEEDED" }] }).to_string(),
            );
        });

        let client = http_client().unwrap();
        let mut session = YouTubeWebSession::test_session();
        let cancel = Cancellation::default();

        edit_comment_at(
            &base_url,
            &client,
            &mut session,
            "UPD_TOK",
            "Edited body",
            &cancel,
        )
        .unwrap();
        delete_comment_at(&base_url, &client, &mut session, "DEL_TOK", &cancel).unwrap();
        delete_comment_oauth_at(&oauth_base, &client, "oauth_tok_test", "UgxOwn123", &cancel)
            .unwrap();
        report_comment_at(
            &base_url,
            &client,
            &mut session,
            "FLAG_TOK",
            CommentReportReason::Spam,
            Some("Bot link"),
            &cancel,
        )
        .unwrap();

        server.join().unwrap();
    }
}
