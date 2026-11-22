//! Unified YouTube InnerTube (`youtubei/v1`) client, DPAPI-persisted web session,
//! feed/search/channel/playlist/watch parsers, and zero-quota authenticated mutations.
//!
//! Security & persistence invariants:
//! - Persisted web sessions (`web-session.dpapi`) are encrypted with Windows DPAPI,
//!   zeroized on drop, never included in backup envelopes, and never printed in `Debug`
//!   or error strings.
//! - Corrupt session or SWR cache files are left untouched on disk and surface errors.
//! - All HTTP requests reuse `crate::http_client::shared()` (pooled HTTP/2 + gzip/brotli)
//!   and honor `crate::process::Cancellation`.

use crate::history::{is_valid_video_id, write_atomic};
use crate::i18n::UiLanguage;
use crate::playlist_sync::is_valid_playlist_id;
use crate::process::Cancellation;
use crate::search::{
    ChannelTarget, SearchFilters as LegacySearchFilters, SearchResult, SearchResultKind, Video,
    VideoMetadata, channel_url_from_browse_endpoint, clean_pt_br_view_label,
    clean_published_time_text, format_duration_seconds, has_verified_owner_badge,
    is_valid_channel_handle, is_valid_channel_id, normalize_youtube_asset_url,
    parse_channel_target, parse_lockup_metadata_rows, parse_pt_br_compact_count,
    parse_video_info_views_and_date, select_yt_thumbnail, yt_text,
};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zeroize::Zeroize;

pub const YOUTUBE_ORIGIN: &str = "https://www.youtube.com";
pub const INNERTUBE_BASE_URL: &str = "https://www.youtube.com/youtubei/v1";
pub const DEFAULT_CLIENT_NAME: &str = "WEB";
pub const DEFAULT_CLIENT_VERSION: &str = "2.20260901.00.00";
pub const WEB_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
pub const WEB_SESSION_FILENAME: &str = "web-session.dpapi";
#[allow(dead_code)]
pub const HOME_CACHE_FILENAME: &str = "home-cache.json";
#[allow(dead_code)]
pub const DEFAULT_HOME_CACHE_TTL_SECS: u64 = 15 * 60;
pub const MAX_HOME_CACHE_ITEMS: usize = 300;
pub const DEFAULT_CHANNEL_CACHE_CAPACITY: usize = 16;
pub const DEFAULT_CHANNEL_CACHE_TTL: Duration = Duration::from_secs(10 * 60);

const WEB_SESSION_FORMAT_VERSION: u32 = 1;
const HOME_CACHE_FORMAT_VERSION: u32 = 1;
const MAX_WEB_SESSION_BYTES: usize = 128 * 1024;
const MAX_HOME_CACHE_BYTES: usize = 4 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_SESSION_TTL_SECS: u64 = 30 * 24 * 60 * 60;
#[allow(dead_code)]
const CLOCK_SKEW_GUARD_SECS: u64 = 300;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Sanitized error type for InnerTube requests and local persistence.
/// Never carries raw URLs, cookies, tokens, or filesystem paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InnerTubeError {
    AuthExpired(String),
    Cancelled(String),
    Http { status: u16, message: String },
    Network(String),
    InvalidResponse(String),
    InvalidInput(String),
    Storage(String),
}

impl InnerTubeError {
    pub fn is_auth_expired(&self) -> bool {
        matches!(self, Self::AuthExpired(_))
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled(_))
    }

    pub fn into_message(self) -> String {
        match self {
            Self::AuthExpired(msg)
            | Self::Cancelled(msg)
            | Self::Http { message: msg, .. }
            | Self::Network(msg)
            | Self::InvalidResponse(msg)
            | Self::InvalidInput(msg)
            | Self::Storage(msg) => msg,
        }
    }
}

impl fmt::Display for InnerTubeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthExpired(msg)
            | Self::Cancelled(msg)
            | Self::Http { message: msg, .. }
            | Self::Network(msg)
            | Self::InvalidResponse(msg)
            | Self::InvalidInput(msg)
            | Self::Storage(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for InnerTubeError {}

// ---------------------------------------------------------------------------
// R4.1 — Pure-Rust SHA-1, SAPISIDHASH, InnerTubeContext, YouTubeWebSession
// ---------------------------------------------------------------------------

/// Pure-Rust RFC 3174 SHA-1 digest formatted as lowercase hexadecimal.
pub fn sha1_hex(data: &[u8]) -> String {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xEFCD_AB89;
    let mut h2: u32 = 0x98BA_DCFE;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xC3D2_E1F0;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
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

/// Computes the `<timestamp>_<sha1>` value used inside `SAPISIDHASH` headers.
pub fn sapisidhash_value(sapisid: &str, origin: &str, timestamp_secs: u64) -> String {
    let input = format!("{timestamp_secs} {sapisid} {origin}");
    let digest = sha1_hex(input.as_bytes());
    format!("{timestamp_secs}_{digest}")
}

/// Builds a `SAPISIDHASH <timestamp>_<sha1>` `Authorization` header value.
pub fn sapisidhash_header(sapisid: &str, origin: &str, timestamp_secs: u64) -> String {
    format!(
        "SAPISIDHASH {}",
        sapisidhash_value(sapisid, origin, timestamp_secs)
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InnerTubeContext {
    pub hl: String,
    pub gl: String,
    pub client_name: String,
    pub client_version: String,
    pub visitor_data: Option<String>,
    pub api_key: Option<String>,
    pub restricted_mode: bool,
}

impl Default for InnerTubeContext {
    fn default() -> Self {
        Self::from_language(UiLanguage::default(), None)
    }
}

impl InnerTubeContext {
    /// Derives `hl` and `gl` from the active `UiLanguage` plus an optional 2-letter
    /// ISO-3166 country override (e.g. `"US"`, `"BR"`, `"GB"`).
    pub fn from_language(language: UiLanguage, country_override: Option<&str>) -> Self {
        let gl = country_override
            .map(str::trim)
            .filter(|c| c.len() == 2 && c.bytes().all(|b| b.is_ascii_alphabetic()))
            .map(|c| c.to_ascii_uppercase())
            .unwrap_or_else(|| language.region_code().to_owned());
        Self {
            hl: language.code().to_owned(),
            gl,
            client_name: DEFAULT_CLIENT_NAME.to_owned(),
            client_version: DEFAULT_CLIENT_VERSION.to_owned(),
            visitor_data: None,
            api_key: None,
            restricted_mode: false,
        }
    }

    #[allow(dead_code)]
    pub fn with_visitor_data(mut self, visitor_data: Option<String>) -> Self {
        self.visitor_data = visitor_data.filter(|v| !v.trim().is_empty());
        self
    }

    #[allow(dead_code)]
    pub fn with_api_key(mut self, api_key: Option<String>) -> Self {
        self.api_key = api_key.filter(|k| valid_api_key(k));
        self
    }

    #[allow(dead_code)]
    pub fn with_client_version(mut self, client_version: impl Into<String>) -> Self {
        let ver = client_version.into();
        if !ver.trim().is_empty() {
            self.client_version = ver;
        }
        self
    }

    #[allow(dead_code)]
    pub fn with_restricted_mode(mut self, enabled: bool) -> Self {
        self.restricted_mode = enabled;
        self
    }

    pub fn accept_language_header(&self) -> String {
        if self.hl.eq_ignore_ascii_case("pt-BR") || self.hl.eq_ignore_ascii_case("pt") {
            "pt-BR,pt;q=0.9,en-US;q=0.8,en;q=0.7".to_owned()
        } else {
            format!("{},en-US;q=0.9,en;q=0.8", self.hl)
        }
    }

    pub fn to_client_json(&self) -> Value {
        let mut client = json!({
            "clientName": self.client_name,
            "clientVersion": self.client_version,
            "hl": self.hl,
            "gl": self.gl,
        });
        if let Some(visitor) = self
            .visitor_data
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            && let Some(obj) = client.as_object_mut()
        {
            obj.insert("visitorData".into(), Value::String(visitor.to_owned()));
        }
        client
    }

    pub fn to_context_json(&self) -> Value {
        if self.restricted_mode {
            json!({
                "client": self.to_client_json(),
                "user": { "enableSafetyMode": true },
            })
        } else {
            json!({ "client": self.to_client_json() })
        }
    }
}

/// Ensures the YouTube `PREF` cookie enables Restricted Mode (`f2=8000000`) when requested.
pub fn apply_restricted_mode_pref_cookie(cookie_header: &str, restricted_mode: bool) -> String {
    if !restricted_mode {
        return cookie_header.to_owned();
    }
    const RESTRICTED_F2_BIT: u64 = 0x0800_0000;
    let trimmed = cookie_header.trim();
    if trimmed.is_empty() {
        return "PREF=f2=8000000".to_owned();
    }
    let mut parts: Vec<String> = Vec::new();
    let mut found_pref = false;
    for segment in trimmed.split(';') {
        let seg = segment.trim();
        if seg.is_empty() {
            continue;
        }
        if let Some((name, val)) = seg.split_once('=')
            && name.trim() == "PREF"
        {
            found_pref = true;
            let mut params: Vec<String> = Vec::new();
            let mut found_f2 = false;
            for param in val.trim().split('&') {
                let p = param.trim();
                if p.is_empty() {
                    continue;
                }
                if let Some((pk, pv)) = p.split_once('=')
                    && pk.trim() == "f2"
                {
                    found_f2 = true;
                    let merged = u64::from_str_radix(pv.trim(), 16)
                        .map(|n| n | RESTRICTED_F2_BIT)
                        .unwrap_or(RESTRICTED_F2_BIT);
                    params.push(format!("f2={merged:x}"));
                } else {
                    params.push(p.to_owned());
                }
            }
            if !found_f2 {
                params.push("f2=8000000".to_owned());
            }
            parts.push(format!("PREF={}", params.join("&")));
        } else {
            parts.push(seg.to_owned());
        }
    }
    if !found_pref {
        parts.push("PREF=f2=8000000".to_owned());
    }
    parts.join("; ")
}

/// Authenticated YouTube web cookie session persisted via Windows DPAPI.
/// All secret fields are zeroized on drop and redacted from `Debug`.
#[derive(Clone, Serialize, Deserialize)]
pub struct YouTubeWebSession {
    pub cookies: BTreeMap<String, String>,
    pub sapisid: Option<String>,
    pub visitor_data: Option<String>,
    #[serde(default = "default_client_version_owned")]
    pub client_version: String,
    pub saved_at_unix: u64,
    pub expires_at_unix: Option<u64>,
    #[serde(default)]
    pub auth_user_index: u8,
    #[serde(default)]
    pub delegated_session_id: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
}

fn default_client_version_owned() -> String {
    DEFAULT_CLIENT_VERSION.to_owned()
}

impl Default for YouTubeWebSession {
    fn default() -> Self {
        Self {
            cookies: BTreeMap::new(),
            sapisid: None,
            visitor_data: None,
            client_version: DEFAULT_CLIENT_VERSION.to_owned(),
            saved_at_unix: 0,
            expires_at_unix: None,
            auth_user_index: 0,
            delegated_session_id: None,
            api_key: None,
        }
    }
}

impl Drop for YouTubeWebSession {
    fn drop(&mut self) {
        for value in self.cookies.values_mut() {
            value.zeroize();
        }
        if let Some(sapisid) = &mut self.sapisid {
            sapisid.zeroize();
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
            .field("has_sapisid", &self.effective_sapisid().is_some())
            .field("has_visitor_data", &self.visitor_data.is_some())
            .field("client_version", &self.client_version)
            .field("saved_at_unix", &self.saved_at_unix)
            .field("expires_at_unix", &self.expires_at_unix)
            .field("auth_user_index", &self.auth_user_index)
            .field(
                "has_delegated_session_id",
                &self.delegated_session_id.is_some(),
            )
            .field("has_api_key", &self.api_key.is_some())
            .finish()
    }
}

impl YouTubeWebSession {
    pub fn new(
        raw_cookies: BTreeMap<String, String>,
        visitor_data: Option<String>,
        saved_at_unix: u64,
        expires_at_unix: Option<u64>,
        auth_user_index: u8,
    ) -> Result<Self, InnerTubeError> {
        let mut cookies = BTreeMap::new();
        for (name, value) in raw_cookies {
            let trimmed_name = name.trim();
            if valid_cookie_name(trimmed_name) && valid_cookie_value(&value) {
                cookies.insert(trimmed_name.to_owned(), value);
            }
        }
        let sapisid = cookies
            .get("SAPISID")
            .or_else(|| cookies.get("__Secure-3PAPISID"))
            .cloned();
        let session = Self {
            cookies,
            sapisid,
            visitor_data: visitor_data.filter(|v| !v.trim().is_empty()),
            client_version: DEFAULT_CLIENT_VERSION.to_owned(),
            saved_at_unix,
            expires_at_unix,
            auth_user_index,
            delegated_session_id: None,
            api_key: None,
        };
        if !session.has_auth_cookies() {
            return Err(InnerTubeError::AuthExpired(
                "YouTube web session is missing required authentication cookies.".into(),
            ));
        }
        Ok(session)
    }

    #[allow(dead_code)]
    pub fn needs_config_refresh(&self) -> bool {
        self.client_version == DEFAULT_CLIENT_VERSION
            || self.visitor_data.is_none()
            || self.api_key.is_none()
    }

    pub fn from_devtools_cookies(raw: &[Value], now_unix: u64) -> Result<Self, InnerTubeError> {
        let mut cookies = BTreeMap::new();
        let mut earliest_auth_expiry: Option<u64> = None;

        for cookie in raw {
            let domain = cookie.get("domain").and_then(Value::as_str).unwrap_or("");
            if !domain.is_empty()
                && !matches!(domain, "youtube.com" | ".youtube.com" | "www.youtube.com")
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
            if matches!(
                name,
                "SAPISID" | "SID" | "__Secure-3PAPISID" | "__Secure-3PSID"
            ) && let Some(exp) = cookie.get("expires").and_then(Value::as_f64)
                && exp.is_finite()
                && exp > 0.0
            {
                let exp_u64 = exp as u64;
                earliest_auth_expiry = Some(
                    earliest_auth_expiry
                        .map(|prev| prev.min(exp_u64))
                        .unwrap_or(exp_u64),
                );
            }
            cookies.insert(name.to_owned(), value.to_owned());
        }

        let expires_at = earliest_auth_expiry
            .unwrap_or_else(|| now_unix.saturating_add(DEFAULT_SESSION_TTL_SECS));
        Self::new(cookies, None, now_unix, Some(expires_at), 0)
    }

    pub fn effective_sapisid(&self) -> Option<&str> {
        self.sapisid
            .as_deref()
            .filter(|v| !v.is_empty())
            .or_else(|| self.cookies.get("SAPISID").map(String::as_str))
            .or_else(|| self.cookies.get("__Secure-3PAPISID").map(String::as_str))
            .filter(|v| !v.is_empty())
    }

    pub fn has_auth_cookies(&self) -> bool {
        let has_sapisid = self.effective_sapisid().is_some();
        let has_sid = self
            .cookies
            .get("SID")
            .or_else(|| self.cookies.get("__Secure-3PSID"))
            .is_some_and(|v| !v.is_empty());
        has_sapisid && has_sid
    }

    pub fn is_expired(&self, now_unix: u64) -> bool {
        self.expires_at_unix
            .is_some_and(|expires_at| now_unix >= expires_at)
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
        origin: &str,
        timestamp_secs: u64,
    ) -> Result<String, InnerTubeError> {
        let sapisid = self.effective_sapisid().ok_or_else(|| {
            InnerTubeError::AuthExpired("YouTube SAPISID authentication cookie is missing.".into())
        })?;
        let mut parts = vec![sapisidhash_header(sapisid, origin, timestamp_secs)];
        if let Some(p1) = self
            .cookies
            .get("__Secure-1PAPISID")
            .filter(|v| !v.is_empty())
        {
            parts.push(format!(
                "SAPISID1PHASH {}",
                sapisidhash_value(p1, origin, timestamp_secs)
            ));
        }
        if let Some(p3) = self
            .cookies
            .get("__Secure-3PAPISID")
            .filter(|v| !v.is_empty())
        {
            parts.push(format!(
                "SAPISID3PHASH {}",
                sapisidhash_value(p3, origin, timestamp_secs)
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
            if name == "SAPISID" {
                self.sapisid = Some(val.to_owned());
            }
            self.cookies.insert(name.to_owned(), val.to_owned());
        }
    }
}

#[derive(Serialize, Deserialize)]
struct PersistedWebSessionEnvelope {
    version: u32,
    session: YouTubeWebSession,
}

/// Returns `%LOCALAPPDATA%/FastTube/web-session.dpapi`.
pub fn web_session_path() -> Result<PathBuf, InnerTubeError> {
    crate::settings::settings_path()
        .map(|p| p.with_file_name(WEB_SESSION_FILENAME))
        .map_err(|_| InnerTubeError::Storage("Could not locate FastTube data directory.".into()))
}

/// Encrypts `session` with Windows DPAPI and persists it atomically to `path`.
/// Plaintext JSON bytes are zeroized immediately after encryption.
pub fn save_web_session_to(path: &Path, session: &YouTubeWebSession) -> Result<(), InnerTubeError> {
    if !session.has_auth_cookies() {
        return Err(InnerTubeError::InvalidInput(
            "Cannot persist a web session without authentication cookies.".into(),
        ));
    }
    let envelope = PersistedWebSessionEnvelope {
        version: WEB_SESSION_FORMAT_VERSION,
        session: session.clone(),
    };
    let mut plain = serde_json::to_vec(&envelope)
        .map_err(|_| InnerTubeError::Storage("Could not encode web session.".into()))?;
    if plain.len() > MAX_WEB_SESSION_BYTES {
        plain.zeroize();
        return Err(InnerTubeError::Storage(
            "Encoded web session exceeds size limit.".into(),
        ));
    }
    let encrypted_res = dpapi_protect(&plain);
    plain.zeroize();
    let encrypted = encrypted_res?;
    if encrypted.len() > MAX_WEB_SESSION_BYTES {
        return Err(InnerTubeError::Storage(
            "Encrypted web session exceeds size limit.".into(),
        ));
    }
    write_atomic(path, &encrypted)
        .map_err(|_| InnerTubeError::Storage("Could not persist encrypted web session.".into()))
}

pub fn save_web_session(session: &YouTubeWebSession) -> Result<(), InnerTubeError> {
    let path = web_session_path()?;
    save_web_session_to(&path, session)
}

/// Loads and decrypts a DPAPI-protected web session from `path`.
/// - Missing file returns `Ok(None)`.
/// - Expired session returns `Ok(None)`.
/// - Corrupt or tampered files remain untouched on disk and return `Err`.
pub fn load_web_session_from(
    path: &Path,
    now_unix: u64,
) -> Result<Option<YouTubeWebSession>, InnerTubeError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(InnerTubeError::Storage(
                "Could not read local web session file.".into(),
            ));
        }
    };
    let mut encrypted = Vec::new();
    file.take((MAX_WEB_SESSION_BYTES + 1) as u64)
        .read_to_end(&mut encrypted)
        .map_err(|_| InnerTubeError::Storage("Could not read local web session file.".into()))?;
    if encrypted.is_empty() || encrypted.len() > MAX_WEB_SESSION_BYTES {
        return Err(InnerTubeError::Storage(
            "Persisted web session file is corrupt or oversized.".into(),
        ));
    }
    let mut plain = dpapi_unprotect(&encrypted)?;
    let parsed: Result<PersistedWebSessionEnvelope, _> = serde_json::from_slice(&plain);
    plain.zeroize();
    let envelope = parsed.map_err(|_| {
        InnerTubeError::Storage("Persisted web session payload is malformed.".into())
    })?;
    if envelope.version != WEB_SESSION_FORMAT_VERSION || !envelope.session.has_auth_cookies() {
        return Err(InnerTubeError::Storage(
            "Persisted web session payload has invalid schema or missing cookies.".into(),
        ));
    }
    if envelope.session.is_expired(now_unix) {
        return Ok(None);
    }
    Ok(Some(envelope.session))
}

pub fn load_web_session() -> Result<Option<YouTubeWebSession>, InnerTubeError> {
    let path = web_session_path()?;
    load_web_session_from(&path, unix_now_secs())
}

/// Removes a persisted DPAPI web session file (e.g. after 401/403 auth failure).
pub fn invalidate_web_session_at(path: &Path) -> Result<(), InnerTubeError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(InnerTubeError::Storage(
            "Could not remove persisted web session.".into(),
        )),
    }
}

pub fn invalidate_web_session() -> Result<(), InnerTubeError> {
    let path = web_session_path()?;
    invalidate_web_session_at(&path)
}

#[cfg(windows)]
fn dpapi_protect(bytes: &[u8]) -> Result<Vec<u8>, InnerTubeError> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };

    let cb_data = u32::try_from(bytes.len())
        .map_err(|_| InnerTubeError::Storage("Web session payload exceeds DPAPI limit.".into()))?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: cb_data,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            null_mut(),
            null_mut(),
            null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return Err(InnerTubeError::Storage(
            "Windows DPAPI failed to protect web session.".into(),
        ));
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as *mut _);
    }
    Ok(result)
}

#[cfg(not(windows))]
fn dpapi_protect(_: &[u8]) -> Result<Vec<u8>, InnerTubeError> {
    Err(InnerTubeError::Storage(
        "DPAPI web session encryption is only available on Windows.".into(),
    ))
}

#[cfg(windows)]
fn dpapi_unprotect(bytes: &[u8]) -> Result<Vec<u8>, InnerTubeError> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };

    let cb_data = u32::try_from(bytes.len())
        .map_err(|_| InnerTubeError::Storage("Web session payload exceeds DPAPI limit.".into()))?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: cb_data,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return Err(InnerTubeError::Storage(
            "Windows DPAPI failed to decrypt web session.".into(),
        ));
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
        LocalFree(output.pbData as *mut _);
    }
    Ok(result)
}

#[cfg(not(windows))]
fn dpapi_unprotect(_: &[u8]) -> Result<Vec<u8>, InnerTubeError> {
    Err(InnerTubeError::Storage(
        "DPAPI web session decryption is only available on Windows.".into(),
    ))
}

// ---------------------------------------------------------------------------
// Generic InnerTube Transport & HTML Initial Data Extractor
// ---------------------------------------------------------------------------

/// Sends a POST request to `https://www.youtube.com/youtubei/v1/<endpoint>`
/// using the shared pooled HTTP/2 client with `Cancellation` and default timeout.
/// On HTTP 401/403 when authenticated, invalidates the DPAPI persisted session.
pub fn post_innertube(
    endpoint: &str,
    payload: Value,
    ctx: &InnerTubeContext,
    session: Option<&mut YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let invalidate_path = web_session_path().ok();
    post_innertube_at(
        INNERTUBE_BASE_URL,
        endpoint,
        payload,
        ctx,
        session,
        DEFAULT_REQUEST_TIMEOUT,
        invalidate_path.as_deref(),
        cancel,
    )
}

/// Same as `post_innertube`, with an explicit per-request timeout.
#[allow(dead_code)]
pub fn post_innertube_with_timeout(
    endpoint: &str,
    payload: Value,
    ctx: &InnerTubeContext,
    session: Option<&mut YouTubeWebSession>,
    timeout: Duration,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let invalidate_path = web_session_path().ok();
    post_innertube_at(
        INNERTUBE_BASE_URL,
        endpoint,
        payload,
        ctx,
        session,
        timeout,
        invalidate_path.as_deref(),
        cancel,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn post_innertube_at(
    base_url: &str,
    endpoint: &str,
    mut payload: Value,
    ctx: &InnerTubeContext,
    session: Option<&mut YouTubeWebSession>,
    timeout: Duration,
    invalidate_session_path: Option<&Path>,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    if cancel.cancelled() {
        return Err(InnerTubeError::Cancelled("Request cancelled.".into()));
    }
    let clean_endpoint = endpoint.trim().trim_start_matches('/');
    if clean_endpoint.is_empty()
        || clean_endpoint.contains("..")
        || clean_endpoint.contains('?')
        || clean_endpoint.contains('#')
    {
        return Err(InnerTubeError::InvalidInput(
            "Invalid InnerTube endpoint path.".into(),
        ));
    }

    let mut effective_ctx = ctx.clone();
    if let Some(sess) = session.as_deref() {
        if !sess.client_version.trim().is_empty()
            && effective_ctx.client_version == DEFAULT_CLIENT_VERSION
        {
            effective_ctx.client_version = sess.client_version.clone();
        }
        if effective_ctx.visitor_data.is_none() {
            effective_ctx.visitor_data = sess.visitor_data.clone();
        }
        if effective_ctx.api_key.is_none() {
            effective_ctx.api_key = sess
                .api_key
                .as_deref()
                .map(str::trim)
                .filter(|k| valid_api_key(k))
                .map(str::to_owned);
        }
    }

    let mut context_json = effective_ctx.to_context_json();
    if let Some(sess) = session.as_deref()
        && let Some(ctx_obj) = context_json.as_object_mut()
    {
        let user_entry = ctx_obj
            .entry("user")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if !user_entry.is_object() {
            *user_entry = Value::Object(serde_json::Map::new());
        }
        if let Some(user_obj) = user_entry.as_object_mut() {
            user_obj
                .entry("lockedSafetyMode")
                .or_insert(Value::Bool(false));
            if let Some(page_id) = sess
                .delegated_session_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
            {
                user_obj.insert("onBehalfOfUser".into(), Value::String(page_id.to_owned()));
            }
        }
    }

    if let Some(obj) = payload.as_object_mut() {
        obj.insert("context".into(), context_json);
    } else {
        return Err(InnerTubeError::InvalidInput(
            "InnerTube request payload must be a JSON object.".into(),
        ));
    }

    let url = match effective_ctx
        .api_key
        .as_deref()
        .filter(|k| valid_api_key(k))
    {
        Some(key) => format!("{base_url}/{clean_endpoint}?key={key}&prettyPrint=false"),
        None => format!("{base_url}/{clean_endpoint}?prettyPrint=false"),
    };

    let mut req = crate::http_client::shared()
        .post(&url)
        .timeout(timeout)
        .header(reqwest::header::USER_AGENT, WEB_USER_AGENT)
        .header(
            reqwest::header::ACCEPT_LANGUAGE,
            effective_ctx.accept_language_header(),
        )
        .header("Origin", YOUTUBE_ORIGIN)
        .header("X-Origin", YOUTUBE_ORIGIN)
        .header(reqwest::header::REFERER, format!("{YOUTUBE_ORIGIN}/"))
        .header("X-Youtube-Client-Name", "1")
        .header("X-Youtube-Client-Version", &effective_ctx.client_version);

    if let Some(visitor) = effective_ctx.visitor_data.as_deref() {
        req = req.header("X-Goog-Visitor-Id", visitor);
    }

    let is_authenticated = session.is_some();
    if let Some(sess) = session.as_deref() {
        let auth = sess.authorization_header(YOUTUBE_ORIGIN, unix_now_secs())?;
        let cookie_header =
            apply_restricted_mode_pref_cookie(&sess.cookie_header(), effective_ctx.restricted_mode);
        req = req
            .header(reqwest::header::COOKIE, cookie_header)
            .header(reqwest::header::AUTHORIZATION, auth)
            .header("X-Goog-AuthUser", sess.auth_user_index.to_string())
            .header("X-Youtube-Bootstrap-Logged-In", "true");
        if let Some(page_id) = sess
            .delegated_session_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            req = req.header("X-Goog-PageId", page_id);
        }
    } else if effective_ctx.restricted_mode {
        req = req.header(
            reqwest::header::COOKIE,
            apply_restricted_mode_pref_cookie("", true),
        );
    }

    let response = req
        .json(&payload)
        .send()
        .map_err(|_| InnerTubeError::Network("Failed to connect to YouTube InnerTube.".into()))?;

    if cancel.cancelled() {
        return Err(InnerTubeError::Cancelled("Request cancelled.".into()));
    }

    if let Some(sess) = session {
        sess.absorb_set_cookies(response.headers());
    }

    let status = response.status();
    let mut raw_bytes = Vec::new();
    response
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut raw_bytes)
        .map_err(|_| InnerTubeError::InvalidResponse("Failed to read YouTube response.".into()))?;
    if raw_bytes.len() > MAX_RESPONSE_BYTES {
        return Err(InnerTubeError::InvalidResponse(
            "YouTube response exceeded size limit.".into(),
        ));
    }

    let json: Value = serde_json::from_slice(&raw_bytes).unwrap_or(Value::Null);

    if matches!(status.as_u16(), 401 | 403) {
        if is_authenticated && let Some(path) = invalidate_session_path {
            let _ = invalidate_web_session_at(path);
        }
        return Err(InnerTubeError::AuthExpired(
            "YouTube web session expired or was rejected.".into(),
        ));
    }

    if let Some(err) = parse_innertube_error(&json, status.as_u16()) {
        if err.is_auth_expired()
            && is_authenticated
            && let Some(path) = invalidate_session_path
        {
            let _ = invalidate_web_session_at(path);
        }
        return Err(err);
    }

    if !status.is_success() {
        return Err(InnerTubeError::Http {
            status: status.as_u16(),
            message: format!("YouTube rejected the request (HTTP {}).", status.as_u16()),
        });
    }

    if json.is_null() && !raw_bytes.iter().all(|b| b.is_ascii_whitespace()) {
        return Err(InnerTubeError::InvalidResponse(
            "YouTube returned non-JSON response.".into(),
        ));
    }

    Ok(json)
}

fn parse_innertube_error(json: &Value, http_status: u16) -> Option<InnerTubeError> {
    let error = json.get("error")?;
    let code = error
        .get("code")
        .and_then(Value::as_u64)
        .unwrap_or(u64::from(http_status));
    let status_str = error.get("status").and_then(Value::as_str).unwrap_or("");
    if matches!(code, 401 | 403)
        || status_str == "UNAUTHENTICATED"
        || status_str == "PERMISSION_DENIED"
    {
        return Some(InnerTubeError::AuthExpired(
            "YouTube web session expired or lacks permission.".into(),
        ));
    }
    let safe_status = if !status_str.is_empty()
        && status_str.len() <= 64
        && status_str
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    {
        format!("YouTube rejected the request (HTTP {code}, {status_str}).")
    } else {
        format!("YouTube rejected the request (HTTP {code}).")
    };
    Some(InnerTubeError::Http {
        status: u16::try_from(code).unwrap_or(http_status),
        message: safe_status,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct YtCfgSessionFields {
    pub client_version: Option<String>,
    pub visitor_data: Option<String>,
    pub api_key: Option<String>,
    pub delegated_session_id: Option<String>,
    pub auth_user_index: Option<u8>,
}

fn extract_ytcfg_string_field(html: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":");
    for (idx, _) in html.match_indices(&needle) {
        let rest = html[idx + needle.len()..].trim_start();
        if let Some(after_quote) = rest.strip_prefix('"')
            && let Some(end) = after_quote.find('"')
        {
            let val = after_quote[..end].trim();
            if !val.is_empty() && val.len() <= 512 && !val.contains('\\') {
                return Some(val.to_owned());
            }
        }
    }
    None
}

fn extract_ytcfg_u8_field(html: &str, key: &str) -> Option<u8> {
    let needle = format!("\"{key}\":");
    for (idx, _) in html.match_indices(&needle) {
        let rest = html[idx + needle.len()..].trim_start();
        if let Some(after_quote) = rest.strip_prefix('"') {
            if let Some(end) = after_quote.find('"')
                && let Ok(parsed) = after_quote[..end].trim().parse::<u8>()
            {
                return Some(parsed);
            }
        } else {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty()
                && let Ok(parsed) = digits.parse::<u8>()
            {
                return Some(parsed);
            }
        }
    }
    None
}

/// Extracts `ytcfg` session parameters from a YouTube web HTML response.
pub fn parse_ytcfg_session_fields(html: &str) -> YtCfgSessionFields {
    let client_version = extract_ytcfg_string_field(html, "INNERTUBE_CLIENT_VERSION");
    let visitor_data = extract_ytcfg_string_field(html, "VISITOR_DATA");
    let api_key =
        extract_ytcfg_string_field(html, "INNERTUBE_API_KEY").filter(|k| valid_api_key(k));
    let delegated_session_id =
        extract_ytcfg_string_field(html, "DELEGATED_SESSION_ID").filter(|id| {
            !id.eq_ignore_ascii_case("null")
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        });
    let auth_user_index = extract_ytcfg_u8_field(html, "SESSION_INDEX");
    YtCfgSessionFields {
        client_version,
        visitor_data,
        api_key,
        delegated_session_id,
        auth_user_index,
    }
}

/// Refreshes live `ytcfg` session parameters (`client_version`, `visitor_data`, `api_key`,
/// `delegated_session_id`, `auth_user_index`) from `https://www.youtube.com/`.
pub fn refresh_web_session_config(
    session: &mut YouTubeWebSession,
    ctx: &InnerTubeContext,
    cancel: &Cancellation,
) -> bool {
    if cancel.cancelled() {
        return false;
    }
    let url = format!("{YOUTUBE_ORIGIN}/?hl={}", ctx.hl);
    let Ok(response) = crate::http_client::shared()
        .get(&url)
        .timeout(DEFAULT_REQUEST_TIMEOUT)
        .header(reqwest::header::USER_AGENT, WEB_USER_AGENT)
        .header(
            reqwest::header::ACCEPT_LANGUAGE,
            ctx.accept_language_header(),
        )
        .header(
            reqwest::header::COOKIE,
            apply_restricted_mode_pref_cookie(&session.cookie_header(), ctx.restricted_mode),
        )
        .send()
    else {
        return false;
    };
    if cancel.cancelled() {
        return false;
    }
    session.absorb_set_cookies(response.headers());
    if !response.status().is_success() {
        return false;
    }
    let mut html = String::new();
    if response
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_string(&mut html)
        .is_err()
        || html.is_empty()
    {
        return false;
    }
    let fields = parse_ytcfg_session_fields(&html);
    let has_client_version = fields.client_version.is_some();
    let mut updated = false;
    if let Some(version) = fields.client_version {
        session.client_version = version;
        updated = true;
    }
    if let Some(visitor) = fields.visitor_data {
        session.visitor_data = Some(visitor);
        updated = true;
    }
    if let Some(api_key) = fields.api_key {
        session.api_key = Some(api_key);
        updated = true;
    }
    if has_client_version || fields.delegated_session_id.is_some() {
        session.delegated_session_id = fields.delegated_session_id;
        updated = true;
    }
    if let Some(auth_idx) = fields.auth_user_index {
        session.auth_user_index = auth_idx;
        updated = true;
    }
    updated
}

/// Extracts the `ytInitialData` JSON object embedded inside a YouTube HTML page.
#[allow(dead_code)]
pub fn extract_yt_initial_data(html: &str) -> Option<Value> {
    for marker in [
        "var ytInitialData = ",
        "window[\"ytInitialData\"] = ",
        "window['ytInitialData'] = ",
        "ytInitialData = ",
    ] {
        if let Some(pos) = html.find(marker) {
            let trimmed = html[pos + marker.len()..].trim_start();
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

// ---------------------------------------------------------------------------
// R4.2 — Algorithmic Home (`FEwhat_to_watch`) + `home-cache.json` SWR Cache
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeFeedChip {
    pub title: String,
    pub token: Option<String>,
    pub selected: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InnerTubeHomePage {
    pub items: Vec<Video>,
    pub metadata: Vec<VideoMetadata>,
    pub chips: Vec<HomeFeedChip>,
    pub continuation: Option<String>,
    pub visitor_data: Option<String>,
}

/// Fetches YouTube's algorithmic Home feed (`FEwhat_to_watch`) or a continuation page.
pub fn fetch_home_feed(
    ctx: &InnerTubeContext,
    session: Option<&mut YouTubeWebSession>,
    continuation: Option<&str>,
    cancel: &Cancellation,
) -> Result<InnerTubeHomePage, InnerTubeError> {
    let payload = match continuation.map(str::trim).filter(|t| !t.is_empty()) {
        Some(token) => json!({ "continuation": token }),
        None => json!({ "browseId": "FEwhat_to_watch" }),
    };
    let json = post_innertube("browse", payload, ctx, session, cancel)?;
    Ok(parse_home_browse_response(&json))
}

/// Parses an InnerTube `browse` response for `FEwhat_to_watch` (initial or continuation).
pub fn parse_home_browse_response(json: &Value) -> InnerTubeHomePage {
    let mut page = InnerTubeHomePage {
        visitor_data: extract_response_visitor_data(json),
        ..Default::default()
    };
    let mut seen_urls = HashSet::new();
    let mut seen_chips = HashSet::new();

    collect_home_chips(json, &mut page.chips, &mut seen_chips);

    if let Some(tabs) = json
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for tab in tabs {
            if let Some(content) = tab.pointer("/tabRenderer/content") {
                collect_feed_items_and_continuation(
                    content,
                    &mut page.items,
                    &mut page.metadata,
                    &mut seen_urls,
                    &mut page.continuation,
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
                collect_feed_items_and_continuation(
                    items,
                    &mut page.items,
                    &mut page.metadata,
                    &mut seen_urls,
                    &mut page.continuation,
                );
            }
        }
    }

    page
}

/// Disk-backed Stale-While-Revalidate cache for Home feed (`home-cache.json`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HomeSwrCache {
    pub items: Vec<Video>,
    pub metadata: Vec<VideoMetadata>,
    pub chips: Vec<HomeFeedChip>,
    pub continuation: Option<String>,
    pub visitor_data: Option<String>,
    pub saved_at_unix: u64,
    pub language: UiLanguage,
}

impl HomeSwrCache {
    pub fn from_page(page: &InnerTubeHomePage, language: UiLanguage, saved_at_unix: u64) -> Self {
        let count = page
            .items
            .len()
            .min(page.metadata.len())
            .min(MAX_HOME_CACHE_ITEMS);
        Self {
            items: page.items[..count].to_vec(),
            metadata: page.metadata[..count].to_vec(),
            chips: page.chips.clone(),
            continuation: page.continuation.clone(),
            visitor_data: page.visitor_data.clone(),
            saved_at_unix,
            language,
        }
    }

    #[allow(dead_code)]
    pub fn into_page(self) -> InnerTubeHomePage {
        InnerTubeHomePage {
            items: self.items,
            metadata: self.metadata,
            chips: self.chips,
            continuation: self.continuation,
            visitor_data: self.visitor_data,
        }
    }

    #[allow(dead_code)]
    pub fn is_stale(&self, now_unix: u64, ttl_secs: u64) -> bool {
        if self.saved_at_unix > now_unix.saturating_add(CLOCK_SKEW_GUARD_SECS) {
            return true;
        }
        now_unix.saturating_sub(self.saved_at_unix) >= ttl_secs
    }

    pub fn matches_language(&self, language: UiLanguage) -> bool {
        self.language == language
    }

    #[allow(dead_code)]
    pub fn home_cache_path() -> Result<PathBuf, InnerTubeError> {
        crate::settings::settings_path()
            .map(|p| p.with_file_name(HOME_CACHE_FILENAME))
            .map_err(|_| {
                InnerTubeError::Storage("Could not locate FastTube data directory.".into())
            })
    }

    /// Loads `home-cache.json` from `path`.
    /// Missing files return `Ok(None)`. Corrupt files remain untouched and return `Err`.
    pub fn load_from(path: &Path) -> Result<Option<Self>, InnerTubeError> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(InnerTubeError::Storage(
                    "Could not read local Home cache file.".into(),
                ));
            }
        };
        let mut bytes = Vec::new();
        file.take((MAX_HOME_CACHE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| InnerTubeError::Storage("Could not read local Home cache file.".into()))?;
        if bytes.is_empty() || bytes.len() > MAX_HOME_CACHE_BYTES {
            return Err(InnerTubeError::Storage(
                "Local Home cache file is empty or oversized.".into(),
            ));
        }
        let disk: DiskHomeSwrCache = serde_json::from_slice(&bytes)
            .map_err(|_| InnerTubeError::Storage("Local Home cache JSON is corrupt.".into()))?;
        if disk.version != HOME_CACHE_FORMAT_VERSION || disk.entries.len() > MAX_HOME_CACHE_ITEMS {
            return Err(InnerTubeError::Storage(
                "Local Home cache has unsupported version or exceeds item cap.".into(),
            ));
        }

        let mut items = Vec::with_capacity(disk.entries.len());
        let mut metadata = Vec::with_capacity(disk.entries.len());
        for entry in disk.entries {
            items.push(entry.video);
            metadata.push(entry.metadata.into_video_metadata());
        }

        Ok(Some(Self {
            items,
            metadata,
            chips: disk.chips,
            continuation: disk.continuation,
            visitor_data: disk.visitor_data,
            saved_at_unix: disk.saved_at_unix,
            language: disk.language,
        }))
    }

    #[allow(dead_code)]
    pub fn load() -> Result<Option<Self>, InnerTubeError> {
        let path = Self::home_cache_path()?;
        Self::load_from(&path)
    }

    /// Atomically persists the Home SWR cache (capped at `MAX_HOME_CACHE_ITEMS = 300`).
    /// If an existing file at `path` is corrupt, refuses to overwrite it and returns an error.
    pub fn save_to(&self, path: &Path) -> Result<(), InnerTubeError> {
        if path.exists() {
            // Preserve corrupt on-disk files rather than silently clobbering them.
            let _ = Self::load_from(path)?;
        }
        let count = self
            .items
            .len()
            .min(self.metadata.len())
            .min(MAX_HOME_CACHE_ITEMS);
        let mut entries = Vec::with_capacity(count);
        for (video, meta) in self.items[..count]
            .iter()
            .zip(self.metadata[..count].iter())
        {
            entries.push(DiskHomeEntry {
                video: video.clone(),
                metadata: DiskVideoMetadata::from_video_metadata(meta),
            });
        }
        let disk = DiskHomeSwrCache {
            version: HOME_CACHE_FORMAT_VERSION,
            saved_at_unix: self.saved_at_unix,
            language: self.language,
            continuation: self.continuation.clone(),
            visitor_data: self.visitor_data.clone(),
            chips: self.chips.clone(),
            entries,
        };
        let bytes = serde_json::to_vec(&disk)
            .map_err(|_| InnerTubeError::Storage("Could not encode Home cache JSON.".into()))?;
        if bytes.len() > MAX_HOME_CACHE_BYTES {
            return Err(InnerTubeError::Storage(
                "Encoded Home cache exceeds size limit.".into(),
            ));
        }
        write_atomic(path, &bytes)
            .map_err(|_| InnerTubeError::Storage("Could not write Home cache atomically.".into()))
    }

    #[allow(dead_code)]
    pub fn save(&self) -> Result<(), InnerTubeError> {
        let path = Self::home_cache_path()?;
        self.save_to(&path)
    }
}

#[derive(Serialize, Deserialize)]
struct DiskHomeSwrCache {
    version: u32,
    saved_at_unix: u64,
    language: UiLanguage,
    continuation: Option<String>,
    visitor_data: Option<String>,
    #[serde(default)]
    chips: Vec<HomeFeedChip>,
    entries: Vec<DiskHomeEntry>,
}

#[derive(Serialize, Deserialize)]
struct DiskHomeEntry {
    video: Video,
    metadata: DiskVideoMetadata,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DiskResultKind {
    Video,
    Short,
    Channel,
    Playlist,
    Movie,
}

impl From<SearchResultKind> for DiskResultKind {
    fn from(kind: SearchResultKind) -> Self {
        match kind {
            SearchResultKind::Video => Self::Video,
            SearchResultKind::Short => Self::Short,
            SearchResultKind::Channel => Self::Channel,
            SearchResultKind::Playlist => Self::Playlist,
            SearchResultKind::Movie => Self::Movie,
        }
    }
}

impl From<DiskResultKind> for SearchResultKind {
    fn from(kind: DiskResultKind) -> Self {
        match kind {
            DiskResultKind::Video => Self::Video,
            DiskResultKind::Short => Self::Short,
            DiskResultKind::Channel => Self::Channel,
            DiskResultKind::Playlist => Self::Playlist,
            DiskResultKind::Movie => Self::Movie,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct DiskVideoMetadata {
    view_count: Option<u64>,
    upload_date: Option<String>,
    description: Option<String>,
    channel_id: Option<String>,
    channel_url: Option<String>,
    kind: DiskResultKind,
    is_live: bool,
    published_time_text: Option<String>,
    view_count_text: Option<String>,
    channel_avatar_url: Option<String>,
    channel_verified: bool,
    #[serde(default)]
    badges: Vec<String>,
    playlist_count: Option<String>,
    subscriber_count_text: Option<String>,
}

impl DiskVideoMetadata {
    fn from_video_metadata(m: &VideoMetadata) -> Self {
        Self {
            view_count: m.view_count,
            upload_date: m.upload_date.clone(),
            description: m.description.clone(),
            channel_id: m.channel_id.clone(),
            channel_url: m.channel_url.clone(),
            kind: m.kind.into(),
            is_live: m.is_live,
            published_time_text: m.published_time_text.clone(),
            view_count_text: m.view_count_text.clone(),
            channel_avatar_url: m.channel_avatar_url.clone(),
            channel_verified: m.channel_verified,
            badges: m.badges.clone(),
            playlist_count: m.playlist_count.clone(),
            subscriber_count_text: m.subscriber_count_text.clone(),
        }
    }

    fn into_video_metadata(self) -> VideoMetadata {
        VideoMetadata {
            view_count: self.view_count,
            upload_date: self.upload_date,
            description: self.description,
            channel_id: self.channel_id,
            channel_url: self.channel_url,
            kind: self.kind.into(),
            is_live: self.is_live,
            published_time_text: self.published_time_text,
            view_count_text: self.view_count_text,
            channel_avatar_url: self.channel_avatar_url,
            channel_verified: self.channel_verified,
            badges: self.badges,
            playlist_count: self.playlist_count,
            subscriber_count_text: self.subscriber_count_text,
            upcoming_start_unix: None,
        }
    }
}

// ---------------------------------------------------------------------------
// R4.3 — Instant Subscriptions (`FEsubscriptions`)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InnerTubeSubscriptionsPage {
    pub items: Vec<Video>,
    pub metadata: Vec<VideoMetadata>,
    pub continuation: Option<String>,
    pub signed_out: bool,
}

/// Fetches the authenticated Subscriptions feed (`FEsubscriptions`) or a continuation page.
pub fn fetch_subscriptions_feed(
    ctx: &InnerTubeContext,
    session: &mut YouTubeWebSession,
    continuation: Option<&str>,
    cancel: &Cancellation,
) -> Result<InnerTubeSubscriptionsPage, InnerTubeError> {
    let payload = match continuation.map(str::trim).filter(|t| !t.is_empty()) {
        Some(token) => json!({ "continuation": token }),
        None => json!({ "browseId": "FEsubscriptions" }),
    };
    let json = post_innertube("browse", payload, ctx, Some(session), cancel)?;
    let page = parse_subscriptions_browse_response(&json);
    if page.signed_out {
        return Err(InnerTubeError::AuthExpired(
            "Subscriptions feed requires an active YouTube session.".into(),
        ));
    }
    Ok(page)
}

/// Parses an InnerTube `browse` response for `FEsubscriptions` (initial or continuation).
pub fn parse_subscriptions_browse_response(json: &Value) -> InnerTubeSubscriptionsPage {
    let mut page = InnerTubeSubscriptionsPage::default();
    let mut seen_urls = HashSet::new();

    if let Some(tabs) = json
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for tab in tabs {
            if let Some(content) = tab.pointer("/tabRenderer/content") {
                collect_feed_items_and_continuation(
                    content,
                    &mut page.items,
                    &mut page.metadata,
                    &mut seen_urls,
                    &mut page.continuation,
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
                collect_feed_items_and_continuation(
                    items,
                    &mut page.items,
                    &mut page.metadata,
                    &mut seen_urls,
                    &mut page.continuation,
                );
            }
        }
    }

    if page.items.is_empty() && contains_signed_out_promo(json) {
        page.signed_out = true;
    }

    page
}

// ---------------------------------------------------------------------------
// R4.4 — Search Pagination, Spelling Correction & Multi-Select Filters
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SearchUploadDate {
    #[default]
    Any,
    LastHour,
    Today,
    ThisWeek,
    ThisMonth,
    ThisYear,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SearchResultType {
    #[default]
    Any,
    Video,
    Channel,
    Playlist,
    Movie,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SearchDuration {
    #[default]
    Any,
    Under4Min,
    Between4And20Min,
    Over20Min,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SearchFeature {
    Live,
    FourK,
    Hd,
    Subtitles,
    CreativeCommons,
    ThreeSixty,
    Vr180,
    ThreeD,
    Hdr,
    Location,
    Purchased,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SearchSortBy {
    #[default]
    Relevance,
    UploadDate,
    ViewCount,
    Rating,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchFilters {
    pub upload_date: SearchUploadDate,
    pub result_type: SearchResultType,
    pub duration: SearchDuration,
    pub features: Vec<SearchFeature>,
    pub sort_by: SearchSortBy,
}

impl SearchFilters {
    #[allow(dead_code)]
    pub fn is_default(&self) -> bool {
        self.upload_date == SearchUploadDate::Any
            && self.result_type == SearchResultType::Any
            && self.duration == SearchDuration::Any
            && self.features.is_empty()
            && self.sort_by == SearchSortBy::Relevance
    }

    #[allow(dead_code)]
    pub fn has_feature(&self, feature: SearchFeature) -> bool {
        self.features.contains(&feature)
    }

    #[allow(dead_code)]
    pub fn toggle_feature(&mut self, feature: SearchFeature) {
        if let Some(pos) = self.features.iter().position(|f| *f == feature) {
            self.features.remove(pos);
        } else {
            self.features.push(feature);
        }
    }
}

impl From<LegacySearchFilters> for SearchFilters {
    fn from(legacy: LegacySearchFilters) -> Self {
        legacy.to_innertube()
    }
}

/// Encodes `SearchFilters` into YouTube's canonical protobuf-encoded base64 `sp` parameter.
pub fn encode_search_sp(filters: &SearchFilters) -> Option<String> {
    let mut msg = Vec::new();
    match filters.sort_by {
        SearchSortBy::Relevance => {}
        SearchSortBy::Rating => push_proto_field_varint(&mut msg, 1, 1),
        SearchSortBy::UploadDate => push_proto_field_varint(&mut msg, 1, 2),
        SearchSortBy::ViewCount => push_proto_field_varint(&mut msg, 1, 3),
    }

    let mut sub = Vec::new();
    match filters.upload_date {
        SearchUploadDate::Any => {}
        SearchUploadDate::LastHour => push_proto_field_varint(&mut sub, 1, 1),
        SearchUploadDate::Today => push_proto_field_varint(&mut sub, 1, 2),
        SearchUploadDate::ThisWeek => push_proto_field_varint(&mut sub, 1, 3),
        SearchUploadDate::ThisMonth => push_proto_field_varint(&mut sub, 1, 4),
        SearchUploadDate::ThisYear => push_proto_field_varint(&mut sub, 1, 5),
    }
    match filters.result_type {
        SearchResultType::Any => {}
        SearchResultType::Video => push_proto_field_varint(&mut sub, 2, 1),
        SearchResultType::Channel => push_proto_field_varint(&mut sub, 2, 2),
        SearchResultType::Playlist => push_proto_field_varint(&mut sub, 2, 3),
        SearchResultType::Movie => push_proto_field_varint(&mut sub, 2, 4),
    }
    match filters.duration {
        SearchDuration::Any => {}
        SearchDuration::Under4Min => push_proto_field_varint(&mut sub, 3, 1),
        SearchDuration::Over20Min => push_proto_field_varint(&mut sub, 3, 2),
        SearchDuration::Between4And20Min => push_proto_field_varint(&mut sub, 3, 3),
    }

    // Emit feature bits in deterministic ascending protobuf field-number order.
    let feature_field = |feature: SearchFeature| -> u32 {
        match feature {
            SearchFeature::Hd => 4,
            SearchFeature::Subtitles => 5,
            SearchFeature::CreativeCommons => 6,
            SearchFeature::ThreeD => 7,
            SearchFeature::Live => 8,
            SearchFeature::Purchased => 9,
            SearchFeature::FourK => 14,
            SearchFeature::ThreeSixty => 15,
            SearchFeature::Location => 23,
            SearchFeature::Hdr => 25,
            SearchFeature::Vr180 => 26,
        }
    };
    let mut fields: Vec<u32> = filters
        .features
        .iter()
        .copied()
        .map(feature_field)
        .collect();
    fields.sort_unstable();
    fields.dedup();
    for field in fields {
        push_proto_field_varint(&mut sub, field, 1);
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchSpellingCorrection {
    pub corrected_query: String,
    pub original_query: Option<String>,
    pub is_auto_corrected: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InnerTubeSearchPage {
    pub items: Vec<Video>,
    pub metadata: Vec<VideoMetadata>,
    pub continuation: Option<String>,
    pub spelling: Option<SearchSpellingCorrection>,
    pub estimated_results: Option<u64>,
}

/// Fetches a page of search results via `youtubei/v1/search` (initial or continuation).
pub fn fetch_search_page(
    query: &str,
    filters: &SearchFilters,
    continuation: Option<&str>,
    ctx: &InnerTubeContext,
    session: Option<&mut YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<InnerTubeSearchPage, InnerTubeError> {
    let payload = if let Some(token) = continuation.map(str::trim).filter(|t| !t.is_empty()) {
        json!({ "continuation": token })
    } else {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(InnerTubeError::InvalidInput(
                "Search query cannot be empty.".into(),
            ));
        }
        let mut body = json!({ "query": trimmed });
        if let Some(sp) = encode_search_sp(filters)
            && let Some(obj) = body.as_object_mut()
        {
            obj.insert("params".into(), Value::String(sp));
        }
        body
    };
    let json = post_innertube("search", payload, ctx, session, cancel)?;
    Ok(parse_search_response(&json))
}

/// Parses an InnerTube `search` response (initial results or continuation page).
pub fn parse_search_response(json: &Value) -> InnerTubeSearchPage {
    let mut page = InnerTubeSearchPage {
        estimated_results: json.get("estimatedResults").and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
        }),
        ..Default::default()
    };
    let mut seen_urls = HashSet::new();

    if let Some(primary) = json.pointer("/contents/twoColumnSearchResultsRenderer/primaryContents")
    {
        collect_search_spelling(primary, &mut page.spelling);
        collect_feed_items_and_continuation(
            primary,
            &mut page.items,
            &mut page.metadata,
            &mut seen_urls,
            &mut page.continuation,
        );
    }

    for key in ["onResponseReceivedCommands", "onResponseReceivedActions"] {
        if let Some(actions) = json.get(key).and_then(Value::as_array) {
            for action in actions {
                if let Some(items) = action
                    .pointer("/appendContinuationItemsAction/continuationItems")
                    .or_else(|| action.pointer("/reloadContinuationItemsCommand/continuationItems"))
                {
                    collect_search_spelling(items, &mut page.spelling);
                    collect_feed_items_and_continuation(
                        items,
                        &mut page.items,
                        &mut page.metadata,
                        &mut seen_urls,
                        &mut page.continuation,
                    );
                }
            }
        }
    }

    page
}

fn collect_search_spelling(node: &Value, out: &mut Option<SearchSpellingCorrection>) {
    if out.is_some() {
        return;
    }
    match node {
        Value::Object(map) => {
            if let Some(showing) = map.get("showingResultsForRenderer") {
                let corrected = non_empty_yt_text(showing.get("correctedQuery")).or_else(|| {
                    showing
                        .pointer("/correctedQueryEndpoint/searchEndpoint/query")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                });
                if let Some(corrected_query) = corrected {
                    let original_query =
                        non_empty_yt_text(showing.get("originalQuery")).or_else(|| {
                            showing
                                .pointer("/originalQueryEndpoint/searchEndpoint/query")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_owned)
                        });
                    *out = Some(SearchSpellingCorrection {
                        corrected_query,
                        original_query,
                        is_auto_corrected: true,
                    });
                    return;
                }
            }
            if let Some(dym) = map.get("didYouMeanRenderer") {
                let corrected = non_empty_yt_text(dym.get("correctedQuery")).or_else(|| {
                    dym.pointer("/correctedQueryEndpoint/searchEndpoint/query")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                });
                if let Some(corrected_query) = corrected {
                    *out = Some(SearchSpellingCorrection {
                        corrected_query,
                        original_query: None,
                        is_auto_corrected: false,
                    });
                    return;
                }
            }
            for child in map.values() {
                collect_search_spelling(child, out);
                if out.is_some() {
                    return;
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_search_spelling(item, out);
                if out.is_some() {
                    return;
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// R4.5 — Channel Pagination, Dynamic Tabs, Channel ID LRU Cache & VL<id> Playlists
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ChannelTabKind {
    Home,
    Videos,
    Shorts,
    Live,
    Posts,
    Playlists,
    Podcasts,
    Releases,
    Store,
    Courses,
    Search,
    Other(String),
}

impl ChannelTabKind {
    pub fn classify(url: &str, title: &str) -> Self {
        let lower_url = url.trim().to_ascii_lowercase();
        let path = lower_url.split(['?', '#']).next().unwrap_or("");
        if path.ends_with("/featured") || path.ends_with("/home") {
            return Self::Home;
        }
        if path.ends_with("/videos") {
            return Self::Videos;
        }
        if path.ends_with("/shorts") {
            return Self::Shorts;
        }
        if path.ends_with("/streams") || path.ends_with("/live") {
            return Self::Live;
        }
        if path.ends_with("/posts") || path.ends_with("/community") {
            return Self::Posts;
        }
        if path.ends_with("/playlists") {
            return Self::Playlists;
        }
        if path.ends_with("/podcasts") {
            return Self::Podcasts;
        }
        if path.ends_with("/releases") {
            return Self::Releases;
        }
        if path.ends_with("/store") {
            return Self::Store;
        }
        if path.ends_with("/courses") {
            return Self::Courses;
        }
        if path.ends_with("/search") {
            return Self::Search;
        }

        match title.trim().to_lowercase().as_str() {
            "home" | "início" | "inicio" => Self::Home,
            "videos" | "vídeos" => Self::Videos,
            "shorts" => Self::Shorts,
            "live" | "ao vivo" | "streams" => Self::Live,
            "posts" | "community" | "postagens" | "comunidade" => Self::Posts,
            "playlists" => Self::Playlists,
            "podcasts" => Self::Podcasts,
            "releases" | "lançamentos" | "lancamentos" => Self::Releases,
            "store" | "loja" => Self::Store,
            "courses" | "cursos" => Self::Courses,
            "search" | "pesquisar" => Self::Search,
            _ => Self::Other(title.trim().to_owned()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelTabEndpoint {
    pub title: String,
    pub browse_id: String,
    pub params: Option<String>,
    pub selected: bool,
    pub kind: ChannelTabKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelSortChip {
    pub title: String,
    pub continuation: String,
    pub selected: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChannelPollChoice {
    pub text: String,
    pub vote_ratio: Option<f32>,
    pub is_correct: Option<bool>,
    pub explanation: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChannelCommunityPost {
    pub id: String,
    pub author: String,
    pub author_avatar: Option<String>,
    pub published_text: String,
    pub content_text: String,
    pub image_url: Option<String>,
    pub like_count_text: Option<String>,
    pub comment_count_text: Option<String>,
    pub poll_choices: Vec<ChannelPollChoice>,
    pub is_quiz: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InnerTubeChannelPage {
    pub channel_id: Option<String>,
    pub title: Option<String>,
    pub handle: Option<String>,
    pub subscriber_count_text: Option<String>,
    pub video_count_text: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub verified: bool,
    pub official_artist: bool,
    pub tabs: Vec<ChannelTabEndpoint>,
    pub sort_chips: Vec<ChannelSortChip>,
    pub items: Vec<Video>,
    pub metadata: Vec<VideoMetadata>,
    pub continuation: Option<String>,
    pub community_posts: Vec<ChannelCommunityPost>,
}

/// Fetches a channel tab or continuation page via `youtubei/v1/browse` with no 100-item cap.
pub fn fetch_channel_browse(
    browse_id: Option<&str>,
    params: Option<&str>,
    continuation: Option<&str>,
    ctx: &InnerTubeContext,
    session: Option<&mut YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<InnerTubeChannelPage, InnerTubeError> {
    let payload = if let Some(token) = continuation.map(str::trim).filter(|t| !t.is_empty()) {
        json!({ "continuation": token })
    } else {
        let bid = browse_id
            .map(str::trim)
            .filter(|id| is_valid_channel_id(id))
            .ok_or_else(|| InnerTubeError::InvalidInput("Invalid channel browseId.".into()))?;
        let mut body = json!({ "browseId": bid });
        if let Some(p) = params.map(str::trim).filter(|s| !s.is_empty())
            && let Some(obj) = body.as_object_mut()
        {
            obj.insert("params".into(), Value::String(p.to_owned()));
        }
        body
    };
    let json = post_innertube("browse", payload, ctx, session, cancel)?;
    Ok(parse_channel_browse_response(&json))
}

/// Parses an InnerTube channel `browse` response (any tab or continuation page).
pub fn parse_channel_browse_response(json: &Value) -> InnerTubeChannelPage {
    let mut page = InnerTubeChannelPage::default();

    let meta = json.pointer("/metadata/channelMetadataRenderer");
    let page_header = json.pointer("/header/pageHeaderRenderer");
    let header_vm = page_header.and_then(|h| h.pointer("/content/pageHeaderViewModel"));
    let c4_header = json.pointer("/header/c4TabbedHeaderRenderer");

    page.channel_id = meta
        .and_then(|m| m.get("externalId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_channel_id(id))
        .map(str::to_owned)
        .or_else(|| {
            c4_header
                .and_then(|c| c.get("channelId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| is_valid_channel_id(id))
                .map(str::to_owned)
        });

    page.title = page_header
        .and_then(|h| h.get("pageTitle"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            header_vm
                .and_then(|vm| vm.pointer("/title/dynamicTextViewModel/text/content"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        })
        .or_else(|| {
            meta.and_then(|m| m.get("title"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        })
        .or_else(|| {
            c4_header
                .and_then(|c| c.get("title"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        });

    page.handle = meta
        .and_then(|m| m.get("vanityChannelUrl"))
        .and_then(Value::as_str)
        .and_then(parse_channel_target)
        .and_then(|target| match target {
            ChannelTarget::Handle(h) => Some(h),
            ChannelTarget::ChannelId(_) => None,
        });

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
                    page.handle = Some(text.to_owned());
                } else {
                    let lower = text.to_lowercase();
                    if lower.contains("subscriber") || lower.contains("inscrito") {
                        page.subscriber_count_text = Some(text.to_owned());
                    } else if lower.contains("video") || lower.contains("vídeo") {
                        page.video_count_text = Some(text.to_owned());
                    }
                }
            }
        }
    }

    if let Some(runs) = header_vm
        .and_then(|vm| vm.pointer("/title/dynamicTextViewModel/text/attachmentRuns"))
        .and_then(Value::as_array)
        && !runs.is_empty()
    {
        let mut is_artist = false;
        for run in runs {
            let icon = run
                .pointer("/element/type/imageType/image/sources/0/clientResource/imageName")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_uppercase();
            if icon.contains("MUSIC") || icon.contains("ARTIST") {
                is_artist = true;
            }
        }
        page.official_artist = is_artist;
        page.verified = true;
    } else if let Some(badges) = c4_header.and_then(|c| c.get("badges")) {
        page.verified = has_verified_owner_badge(Some(badges));
        if let Some(arr) = badges.as_array() {
            page.official_artist = arr.iter().any(|b| {
                b.pointer("/metadataBadgeRenderer/icon/iconType")
                    .and_then(Value::as_str)
                    .is_some_and(|icon| icon.contains("OFFICIAL_ARTIST"))
            });
        }
    }

    page.avatar_url = header_vm
        .map(|vm| {
            select_yt_thumbnail(
                vm.pointer("/image/decoratedAvatarViewModel/avatar/avatarViewModel/image/sources"),
                176,
            )
        })
        .filter(|u| !u.is_empty())
        .or_else(|| {
            meta.map(|m| select_yt_thumbnail(m.pointer("/avatar/thumbnails"), 176))
                .filter(|u| !u.is_empty())
        });

    page.banner_url = header_vm
        .map(|vm| {
            select_yt_thumbnail(
                vm.pointer("/banner/imageBannerViewModel/image/sources"),
                1707,
            )
        })
        .filter(|u| !u.is_empty());

    let mut seen_urls = HashSet::new();
    let mut seen_sorts = HashSet::new();

    if let Some(tabs) = json
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for entry in tabs {
            if let Some(tab) = entry.get("tabRenderer") {
                let title = tab
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("")
                    .to_owned();
                let selected = tab
                    .get("selected")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let url = tab
                    .pointer("/endpoint/commandMetadata/webCommandMetadata/url")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let browse_id = tab
                    .pointer("/endpoint/browseEndpoint/browseId")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .or_else(|| page.channel_id.clone());
                let params = tab
                    .pointer("/endpoint/browseEndpoint/params")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);

                if let Some(bid) = browse_id
                    && !title.is_empty()
                {
                    if page.channel_id.is_none() && is_valid_channel_id(&bid) {
                        page.channel_id = Some(bid.clone());
                    }
                    page.tabs.push(ChannelTabEndpoint {
                        kind: ChannelTabKind::classify(url, &title),
                        title,
                        browse_id: bid,
                        params,
                        selected,
                    });
                }

                if let Some(content) = tab.get("content") {
                    collect_channel_sort_chips(content, &mut page.sort_chips, &mut seen_sorts);
                    collect_feed_items_and_continuation(
                        content,
                        &mut page.items,
                        &mut page.metadata,
                        &mut seen_urls,
                        &mut page.continuation,
                    );
                }
            } else if let Some(exp) = entry.get("expandableTabRenderer") {
                let title = exp
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Search")
                    .to_owned();
                let browse_id = exp
                    .pointer("/endpoint/browseEndpoint/browseId")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .or_else(|| page.channel_id.clone());
                if let Some(bid) = browse_id {
                    page.tabs.push(ChannelTabEndpoint {
                        title,
                        browse_id: bid,
                        params: exp
                            .pointer("/endpoint/browseEndpoint/params")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned),
                        selected: exp
                            .get("selected")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        kind: ChannelTabKind::Search,
                    });
                }
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
                collect_channel_sort_chips(items, &mut page.sort_chips, &mut seen_sorts);
                collect_feed_items_and_continuation(
                    items,
                    &mut page.items,
                    &mut page.metadata,
                    &mut seen_urls,
                    &mut page.continuation,
                );
            }
        }
    }

    // Backfill channel owner defaults onto parsed items from channel tabs.
    let canonical_url = page
        .channel_id
        .as_deref()
        .map(|id| format!("{YOUTUBE_ORIGIN}/channel/{id}"));
    for (video, meta) in page.items.iter_mut().zip(page.metadata.iter_mut()) {
        if video.channel.is_empty()
            && let Some(title) = &page.title
        {
            video.channel = title.clone();
        }
        if meta.channel_id.is_none() {
            meta.channel_id = page.channel_id.clone();
        }
        if meta.channel_url.is_none() {
            meta.channel_url = canonical_url.clone();
        }
        if page.verified {
            meta.channel_verified = true;
        }
    }

    page.community_posts = parse_channel_community_posts(json);

    page
}

/// Canonical `UC...` channel cache key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ChannelCacheKey(String);

impl ChannelCacheKey {
    pub fn from_channel_id(id: &str) -> Option<Self> {
        let trimmed = id.trim();
        is_valid_channel_id(trimmed).then(|| Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Normalizes a `@handle` or channel URL into a lowercase alias lookup token.
    pub fn normalize_alias(input: &str) -> Option<String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return None;
        }
        if is_valid_channel_handle(trimmed) {
            return Some(trimmed.to_ascii_lowercase());
        }
        match parse_channel_target(trimmed)? {
            ChannelTarget::Handle(h) => Some(h.to_ascii_lowercase()),
            ChannelTarget::ChannelId(_) => None,
        }
    }
}

#[derive(Clone, Debug)]
struct ChannelCacheEntry<V> {
    value: V,
    inserted_at: Instant,
}

/// Bounded LRU cache keyed strictly by canonical `UC...` channel ID, with
/// `@handle` -> `UC...` alias resolution so handle vs ID lookups share one slot.
#[derive(Clone, Debug)]
pub struct ChannelIdCache<V> {
    capacity: usize,
    ttl: Duration,
    aliases: HashMap<String, ChannelCacheKey>,
    entries: HashMap<ChannelCacheKey, ChannelCacheEntry<V>>,
    lru_order: VecDeque<ChannelCacheKey>,
}

impl<V> Default for ChannelIdCache<V> {
    fn default() -> Self {
        Self::new(DEFAULT_CHANNEL_CACHE_CAPACITY, DEFAULT_CHANNEL_CACHE_TTL)
    }
}

impl<V> ChannelIdCache<V> {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            capacity: capacity.max(1),
            ttl,
            aliases: HashMap::new(),
            entries: HashMap::new(),
            lru_order: VecDeque::new(),
        }
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.aliases.clear();
        self.entries.clear();
        self.lru_order.clear();
    }

    /// Registers a `@handle` or handle URL alias pointing to a canonical `UC...` ID.
    #[allow(dead_code)]
    pub fn register_alias(&mut self, handle_or_url: &str, channel_id: &str) -> bool {
        let Some(key) = ChannelCacheKey::from_channel_id(channel_id) else {
            return false;
        };
        let Some(alias) = ChannelCacheKey::normalize_alias(handle_or_url) else {
            return false;
        };
        self.aliases.insert(alias, key);
        true
    }

    /// Resolves any channel identifier (`UC...`, `@handle`, `/channel/UC...`, `/@handle`)
    /// to its canonical `ChannelCacheKey`.
    pub fn resolve_key(&self, input: &str) -> Option<ChannelCacheKey> {
        let trimmed = input.trim();
        if let Some(key) = ChannelCacheKey::from_channel_id(trimmed) {
            return Some(key);
        }
        if let Some(target) = parse_channel_target(trimmed) {
            match target {
                ChannelTarget::ChannelId(id) => return ChannelCacheKey::from_channel_id(&id),
                ChannelTarget::Handle(h) => {
                    return self.aliases.get(&h.to_ascii_lowercase()).cloned();
                }
            }
        }
        ChannelCacheKey::normalize_alias(trimmed)
            .and_then(|alias| self.aliases.get(&alias).cloned())
    }

    /// Returns `true` when two channel references resolve to the same canonical `UC...` ID.
    #[allow(dead_code)]
    pub fn same_channel(&self, a: &str, b: &str) -> bool {
        match (self.resolve_key(a), self.resolve_key(b)) {
            (Some(ka), Some(kb)) => ka == kb,
            _ => a.trim().eq_ignore_ascii_case(b.trim()),
        }
    }

    pub fn insert(
        &mut self,
        channel_id: &str,
        aliases: &[&str],
        value: V,
        now: Instant,
    ) -> Option<ChannelCacheKey> {
        let key = ChannelCacheKey::from_channel_id(channel_id)?;
        self.evict_expired(now);
        for alias in aliases {
            if let Some(norm) = ChannelCacheKey::normalize_alias(alias) {
                self.aliases.insert(norm, key.clone());
            }
        }
        self.entries.insert(
            key.clone(),
            ChannelCacheEntry {
                value,
                inserted_at: now,
            },
        );
        self.touch_lru(&key);
        self.evict_overflow();
        Some(key)
    }

    pub fn get(&mut self, input: &str, now: Instant) -> Option<&V> {
        self.evict_expired(now);
        let key = self.resolve_key(input)?;
        if !self.entries.contains_key(&key) {
            return None;
        }
        self.touch_lru(&key);
        self.entries.get(&key).map(|entry| &entry.value)
    }

    pub fn get_mut(&mut self, input: &str, now: Instant) -> Option<&mut V> {
        self.evict_expired(now);
        let key = self.resolve_key(input)?;
        if !self.entries.contains_key(&key) {
            return None;
        }
        self.touch_lru(&key);
        self.entries.get_mut(&key).map(|entry| &mut entry.value)
    }

    #[allow(dead_code)]
    pub fn invalidate(&mut self, input: &str) -> bool {
        let Some(key) = self.resolve_key(input) else {
            return false;
        };
        let removed = self.entries.remove(&key).is_some();
        self.lru_order.retain(|k| k != &key);
        self.aliases.retain(|_, target| target != &key);
        removed
    }

    fn touch_lru(&mut self, key: &ChannelCacheKey) {
        self.lru_order.retain(|existing| existing != key);
        self.lru_order.push_back(key.clone());
    }

    fn evict_expired(&mut self, now: Instant) {
        let ttl = self.ttl;
        let mut expired = Vec::new();
        self.entries.retain(|key, entry| {
            let fresh = now.saturating_duration_since(entry.inserted_at) < ttl;
            if !fresh {
                expired.push(key.clone());
            }
            fresh
        });
        if !expired.is_empty() {
            self.lru_order.retain(|k| !expired.contains(k));
            self.aliases.retain(|_, target| !expired.contains(target));
        }
    }

    fn evict_overflow(&mut self) {
        while self.entries.len() > self.capacity {
            let Some(victim) = self.lru_order.pop_front() else {
                break;
            };
            self.entries.remove(&victim);
            self.aliases.retain(|_, target| target != &victim);
        }
    }
}

/// Playlist detail page returned by InnerTube `browse` (`VL<id>`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InnerTubePlaylistDetail {
    pub playlist_id: String,
    pub title: String,
    pub owner: String,
    pub description: Option<String>,
    pub total_videos: Option<u64>,
    pub thumbnail_url: Option<String>,
    pub items: Vec<Video>,
    pub metadata: Vec<VideoMetadata>,
    pub set_video_ids: Vec<Option<String>>,
    pub continuation: Option<String>,
    pub signed_out: bool,
}

/// Normalizes a playlist ID into its `VL<id>` InnerTube `browseId`.
#[allow(dead_code)]
pub fn playlist_browse_id(playlist_id: &str) -> Result<String, InnerTubeError> {
    let trimmed = playlist_id.trim();
    let stripped = trimmed.strip_prefix("VL").unwrap_or(trimmed);
    if !is_valid_playlist_id(stripped) {
        return Err(InnerTubeError::InvalidInput("Invalid playlist ID.".into()));
    }
    Ok(format!("VL{stripped}"))
}

/// Fetches playlist detail items via InnerTube `browseId = "VL<id>"` with continuations.
/// Works unauthenticated for public playlists and authenticated for private/`WL`/`LL` playlists.
pub fn fetch_playlist_detail(
    playlist_id: &str,
    continuation: Option<&str>,
    ctx: &InnerTubeContext,
    session: Option<&mut YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<InnerTubePlaylistDetail, InnerTubeError> {
    let normalized_id = playlist_id
        .trim()
        .strip_prefix("VL")
        .unwrap_or(playlist_id.trim());
    if !is_valid_playlist_id(normalized_id) {
        return Err(InnerTubeError::InvalidInput("Invalid playlist ID.".into()));
    }
    let payload = match continuation.map(str::trim).filter(|t| !t.is_empty()) {
        Some(token) => json!({ "continuation": token }),
        None => json!({ "browseId": format!("VL{normalized_id}") }),
    };
    let json = post_innertube("browse", payload, ctx, session, cancel)?;
    let detail = parse_playlist_browse_response(&json, normalized_id);
    if detail.signed_out {
        return Err(InnerTubeError::AuthExpired(
            "Playlist requires an active YouTube session.".into(),
        ));
    }
    Ok(detail)
}

/// Parses an InnerTube `browse` response for `VL<id>` (initial page or continuation).
pub fn parse_playlist_browse_response(json: &Value, playlist_id: &str) -> InnerTubePlaylistDetail {
    let clean_id = playlist_id
        .trim()
        .strip_prefix("VL")
        .unwrap_or(playlist_id.trim())
        .to_owned();
    let mut detail = InnerTubePlaylistDetail {
        playlist_id: clean_id,
        ..Default::default()
    };

    if let Some(vm) = find_object_by_key(json, "pageHeaderViewModel") {
        let title = vm
            .pointer("/title/dynamicTextViewModel/text/content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| yt_text(vm.pointer("/title/dynamicTextViewModel/text")));
        if !title.is_empty() {
            detail.title = title;
        }
        detail.description = vm
            .pointer("/description/descriptionPreviewViewModel/description/content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        detail.thumbnail_url = Some(select_yt_thumbnail(
            vm.pointer("/heroImage/contentPreviewImageViewModel/image/sources"),
            480,
        ))
        .filter(|u| !u.is_empty());

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
                    if lower.contains("video") || lower.contains("vídeo") {
                        if detail.total_videos.is_none() {
                            detail.total_videos = extract_grouped_u64(&text);
                        }
                    } else if detail.owner.is_empty()
                        && !lower.contains("view")
                        && !lower.contains("visualiza")
                        && !lower.starts_with("updated")
                        && !lower.starts_with("atualiz")
                    {
                        let cleaned = text
                            .strip_prefix("by ")
                            .or_else(|| text.strip_prefix("por "))
                            .unwrap_or(&text)
                            .trim();
                        if !cleaned.is_empty() {
                            detail.owner = cleaned.to_owned();
                        }
                    }
                }
            }
        }
    }

    if let Some(hdr) = find_object_by_key(json, "playlistHeaderRenderer") {
        if detail.title.is_empty() {
            detail.title = yt_text(hdr.get("title"));
        }
        if detail.owner.is_empty() {
            detail.owner = yt_text(hdr.get("ownerText"));
        }
        if detail.description.is_none() {
            detail.description = non_empty_yt_text(hdr.get("descriptionText"));
        }
        if detail.total_videos.is_none() {
            let num_text = yt_text(
                hdr.get("numVideosText")
                    .or_else(|| hdr.pointer("/byline/0/playlistBylineRenderer/text")),
            );
            detail.total_videos = extract_grouped_u64(&num_text);
        }
        if detail.thumbnail_url.is_none() {
            detail.thumbnail_url = Some(select_yt_thumbnail(
                hdr.pointer(
                    "/playlistHeaderBanner/heroPlaylistThumbnailRenderer/thumbnail/thumbnails",
                ),
                480,
            ))
            .filter(|u| !u.is_empty());
        }
    }

    let mut seen_keys = HashSet::new();
    if let Some(tabs) = json
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for tab in tabs {
            if let Some(content) = tab.pointer("/tabRenderer/content") {
                collect_playlist_videos(
                    content,
                    &mut detail.items,
                    &mut detail.metadata,
                    &mut detail.set_video_ids,
                    &mut seen_keys,
                    &mut detail.continuation,
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
                collect_playlist_videos(
                    items,
                    &mut detail.items,
                    &mut detail.metadata,
                    &mut detail.set_video_ids,
                    &mut seen_keys,
                    &mut detail.continuation,
                );
            }
        }
    }

    if detail.items.is_empty() && detail.title.is_empty() && contains_signed_out_promo(json) {
        detail.signed_out = true;
    }

    detail
}

// ---------------------------------------------------------------------------
// R4.6 — Watch Page `next`, Public Comments, Heatmap & Zero-Quota Mutations
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeatmapBucket {
    pub start_ms: u64,
    pub duration_ms: u64,
    pub intensity: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelatedFilterChip {
    pub title: String,
    pub continuation: Option<String>,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DescriptionRun {
    pub text: String,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InnerTubeComment {
    pub id: String,
    pub author: String,
    pub author_avatar: Option<String>,
    pub text: String,
    pub published_text: String,
    pub like_count_text: Option<String>,
    pub is_pinned: bool,
    pub is_hearted: bool,
    pub is_owner: bool,
    pub is_member: bool,
    pub reply_count: u64,
    pub replies_continuation: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InnerTubeCommentsPage {
    pub comments: Vec<InnerTubeComment>,
    pub continuation: Option<String>,
    pub top_sort_continuation: Option<String>,
    pub newest_sort_continuation: Option<String>,
    pub total_count_text: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InnerTubeWatchNext {
    pub related_videos: Vec<Video>,
    pub related_metadata: Vec<VideoMetadata>,
    pub related_chips: Vec<RelatedFilterChip>,
    pub related_continuation: Option<String>,
    pub comments_continuation: Option<String>,
    pub heatmap: Vec<HeatmapBucket>,
    pub channel_id: Option<String>,
    pub channel_url: Option<String>,
    pub channel_title: Option<String>,
    pub channel_avatar_url: Option<String>,
    pub channel_verified: bool,
    pub like_count_text: Option<String>,
    pub view_count_text: Option<String>,
    pub published_date_text: Option<String>,
    pub subscriber_count_text: Option<String>,
    pub description_runs: Vec<DescriptionRun>,
}

/// Fetches Watch page `next` data (related videos, filter chips, `HEATSEEKER` heatmap,
/// channel/like metadata, and public comments continuation). Works 100% OAuth-free.
pub fn fetch_watch_next(
    video_id: &str,
    continuation: Option<&str>,
    ctx: &InnerTubeContext,
    session: Option<&mut YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<InnerTubeWatchNext, InnerTubeError> {
    let payload = if let Some(token) = continuation.map(str::trim).filter(|t| !t.is_empty()) {
        json!({ "continuation": token })
    } else {
        let vid = video_id.trim();
        if !is_valid_video_id(vid) {
            return Err(InnerTubeError::InvalidInput(
                "Invalid YouTube video ID.".into(),
            ));
        }
        json!({ "videoId": vid })
    };
    let json = post_innertube("next", payload, ctx, session, cancel)?;
    Ok(parse_watch_next_response(&json))
}

/// Parses an InnerTube `youtubei/v1/next` response for a Watch page (or related continuation).
pub fn parse_watch_next_response(json: &Value) -> InnerTubeWatchNext {
    let mut out = InnerTubeWatchNext::default();

    // 1. Primary watch column: likes, channel owner, verified badge, description runs, comments continuation.
    if let Some(primary_contents) = json
        .pointer("/contents/twoColumnWatchNextResults/results/results/contents")
        .and_then(Value::as_array)
    {
        for entry in primary_contents {
            if let Some(primary_info) = entry.get("videoPrimaryInfoRenderer") {
                if out.like_count_text.is_none() {
                    out.like_count_text = extract_watch_like_count(primary_info);
                }
                if out.view_count_text.is_none() {
                    let vc_renderer = primary_info.pointer("/viewCount/videoViewCountRenderer");
                    let raw_views = vc_renderer
                        .and_then(|vc| {
                            non_empty_yt_text(vc.get("shortViewCount"))
                                .or_else(|| non_empty_yt_text(vc.get("viewCount")))
                        })
                        .or_else(|| non_empty_yt_text(primary_info.get("viewCount")));
                    out.view_count_text = raw_views
                        .as_deref()
                        .and_then(clean_pt_br_view_label)
                        .or(raw_views);
                }
                if out.published_date_text.is_none() {
                    out.published_date_text =
                        non_empty_yt_text(primary_info.get("relativeDateText"))
                            .or_else(|| non_empty_yt_text(primary_info.get("dateText")))
                            .and_then(|s| clean_published_time_text(&s));
                }
            }
            if let Some(secondary_info) = entry.get("videoSecondaryInfoRenderer") {
                if let Some(owner) = secondary_info.pointer("/owner/videoOwnerRenderer") {
                    if out.channel_id.is_none() || out.channel_url.is_none() {
                        let (cid, curl) = channel_url_from_browse_endpoint(
                            owner
                                .get("navigationEndpoint")
                                .or_else(|| owner.pointer("/title/runs/0/navigationEndpoint")),
                        );
                        if out.channel_id.is_none() {
                            out.channel_id = cid;
                        }
                        if out.channel_url.is_none() {
                            out.channel_url = curl;
                        }
                    }
                    if out.channel_title.is_none() {
                        out.channel_title = non_empty_yt_text(owner.get("title"));
                    }
                    if out.channel_avatar_url.is_none() {
                        let av = select_yt_thumbnail(owner.pointer("/thumbnail/thumbnails"), 68);
                        if !av.is_empty() {
                            out.channel_avatar_url = Some(av);
                        }
                    }
                    if !out.channel_verified {
                        out.channel_verified = has_verified_owner_badge(owner.get("badges"));
                    }
                    if out.subscriber_count_text.is_none() {
                        out.subscriber_count_text =
                            non_empty_yt_text(owner.get("subscriberCountText"));
                    }
                }
                if out.description_runs.is_empty() {
                    out.description_runs = extract_description_runs(secondary_info);
                }
            }
            if let Some(section) = entry.get("itemSectionRenderer")
                && out.comments_continuation.is_none()
                && is_comments_section(section)
            {
                out.comments_continuation = find_first_continuation_token(section);
            }
        }
    }

    if out.comments_continuation.is_none()
        && let Some(panels) = json.get("engagementPanels").and_then(Value::as_array)
    {
        for panel in panels {
            let panel_id = panel
                .pointer("/engagementPanelSectionListRenderer/panelIdentifier")
                .and_then(Value::as_str)
                .unwrap_or("");
            if panel_id == "engagement-panel-comments-section" {
                out.comments_continuation = find_first_continuation_token(panel);
                if out.comments_continuation.is_some() {
                    break;
                }
            }
        }
    }

    // 2. Secondary column: related filter chips, related videos, and related continuation.
    let mut seen_related = HashSet::new();
    let mut seen_chips = HashSet::new();
    if let Some(secondary) =
        json.pointer("/contents/twoColumnWatchNextResults/secondaryResults/secondaryResults")
    {
        collect_related_chips(secondary, &mut out.related_chips, &mut seen_chips);
        collect_feed_items_and_continuation(
            secondary,
            &mut out.related_videos,
            &mut out.related_metadata,
            &mut seen_related,
            &mut out.related_continuation,
        );
    }

    for key in ["onResponseReceivedEndpoints", "onResponseReceivedActions"] {
        if let Some(actions) = json.get(key).and_then(Value::as_array) {
            for action in actions {
                if let Some(items) = action
                    .pointer("/appendContinuationItemsAction/continuationItems")
                    .or_else(|| action.pointer("/reloadContinuationItemsCommand/continuationItems"))
                {
                    collect_related_chips(items, &mut out.related_chips, &mut seen_chips);
                    collect_feed_items_and_continuation(
                        items,
                        &mut out.related_videos,
                        &mut out.related_metadata,
                        &mut seen_related,
                        &mut out.related_continuation,
                    );
                }
            }
        }
    }

    // 3. Most-replayed heatmap (`HEATSEEKER` in `markersMap`).
    out.heatmap = parse_heatseeker_heatmap(json);

    out
}

/// Fetches public comments or replies from `youtubei/v1/next` using a continuation token.
/// Works 100% OAuth-free for public videos.
pub fn fetch_comments_next(
    continuation: &str,
    ctx: &InnerTubeContext,
    session: Option<&mut YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<InnerTubeCommentsPage, InnerTubeError> {
    let token = continuation.trim();
    if token.is_empty() {
        return Err(InnerTubeError::InvalidInput(
            "Comments continuation token cannot be empty.".into(),
        ));
    }
    let json = post_innertube(
        "next",
        json!({ "continuation": token }),
        ctx,
        session,
        cancel,
    )?;
    Ok(parse_comments_response(&json))
}

/// Parses an InnerTube `next` comments/replies continuation response (supports both
/// modern `commentViewModel` + `frameworkUpdates` entities and legacy `commentRenderer`).
pub fn parse_comments_response(json: &Value) -> InnerTubeCommentsPage {
    let mut entities: HashMap<&str, &Value> = HashMap::new();
    collect_entity_mutations(json, &mut entities);

    let mut page = InnerTubeCommentsPage::default();
    let mut seen_ids = HashSet::new();

    extract_comments_header_meta(json, &mut page);

    for key in ["onResponseReceivedEndpoints", "onResponseReceivedActions"] {
        let Some(actions) = json.get(key).and_then(Value::as_array) else {
            continue;
        };
        for action in actions {
            let items = action
                .pointer("/reloadContinuationItemsCommand/continuationItems")
                .or_else(|| action.pointer("/appendContinuationItemsAction/continuationItems"))
                .and_then(Value::as_array);
            let Some(items) = items else {
                continue;
            };
            let has_comments = items.iter().any(|item| {
                item.get("commentThreadRenderer").is_some()
                    || item.get("commentViewModel").is_some()
                    || item.get("commentRenderer").is_some()
            });
            for item in items {
                if let Some(thread) = item.get("commentThreadRenderer") {
                    if let Some(comment) = parse_comment_thread_renderer(thread, &entities)
                        && seen_ids.insert(comment.id.clone())
                    {
                        page.comments.push(comment);
                    }
                } else if let Some(vm_wrap) = item.get("commentViewModel") {
                    if let Some(vm) = extract_innermost_comment_vm(vm_wrap)
                        && let Some(comment) = parse_comment_view_model(vm, &entities, false, None)
                        && seen_ids.insert(comment.id.clone())
                    {
                        page.comments.push(comment);
                    }
                } else if let Some(renderer) = item.get("commentRenderer")
                    && let Some(comment) = parse_legacy_comment_renderer(renderer, false, None)
                    && seen_ids.insert(comment.id.clone())
                {
                    page.comments.push(comment);
                }
            }
            if has_comments
                && page.continuation.is_none()
                && let Some(last) = items.last()
                && let Some(cont) = last.get("continuationItemRenderer")
            {
                page.continuation = extract_continuation_item_token(cont);
            }
        }
    }

    page
}

// ---------------------------------------------------------------------------
// R4.6 — Zero-Quota InnerTube Mutations
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoRating {
    Like,
    Dislike,
    Remove,
}

impl VideoRating {
    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Like => "like/like",
            Self::Dislike => "like/dislike",
            Self::Remove => "like/removelike",
        }
    }
}

/// Builds a `browse/edit_playlist` `ACTION_ADD_VIDEO` payload (works for `WL` and user playlists).
pub fn build_playlist_add_video_payload(
    playlist_id: &str,
    video_id: &str,
) -> Result<Value, InnerTubeError> {
    let pid = playlist_id.trim();
    let vid = video_id.trim();
    if !is_valid_playlist_id(pid) {
        return Err(InnerTubeError::InvalidInput("Invalid playlist ID.".into()));
    }
    if !is_valid_video_id(vid) {
        return Err(InnerTubeError::InvalidInput("Invalid video ID.".into()));
    }
    Ok(json!({
        "playlistId": pid,
        "actions": [{
            "addedVideoId": vid,
            "action": "ACTION_ADD_VIDEO",
        }],
    }))
}

pub fn mutate_playlist_add_video(
    playlist_id: &str,
    video_id: &str,
    ctx: &InnerTubeContext,
    session: &mut YouTubeWebSession,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let payload = build_playlist_add_video_payload(playlist_id, video_id)?;
    let resp = post_innertube("browse/edit_playlist", payload, ctx, Some(session), cancel)?;
    check_mutation_status(&resp, "Could not add video to playlist.")?;
    Ok(resp)
}

/// Builds a `browse/edit_playlist` remove payload (`ACTION_REMOVE_VIDEO` when `set_video_id`
/// is present, or `ACTION_REMOVE_VIDEO_BY_VIDEO_ID` otherwise).
#[allow(dead_code)]
pub fn build_playlist_remove_video_payload(
    playlist_id: &str,
    set_video_id: Option<&str>,
    video_id: &str,
) -> Result<Value, InnerTubeError> {
    let pid = playlist_id.trim();
    let vid = video_id.trim();
    if !is_valid_playlist_id(pid) {
        return Err(InnerTubeError::InvalidInput("Invalid playlist ID.".into()));
    }
    if !is_valid_video_id(vid) {
        return Err(InnerTubeError::InvalidInput("Invalid video ID.".into()));
    }
    let action = match set_video_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(svid) => json!({
            "action": "ACTION_REMOVE_VIDEO",
            "setVideoId": svid,
        }),
        None => json!({
            "action": "ACTION_REMOVE_VIDEO_BY_VIDEO_ID",
            "removedVideoId": vid,
        }),
    };
    Ok(json!({
        "playlistId": pid,
        "actions": [action],
    }))
}

#[allow(dead_code)]
pub fn mutate_playlist_remove_video(
    playlist_id: &str,
    set_video_id: Option<&str>,
    video_id: &str,
    ctx: &InnerTubeContext,
    session: &mut YouTubeWebSession,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let payload = build_playlist_remove_video_payload(playlist_id, set_video_id, video_id)?;
    let resp = post_innertube("browse/edit_playlist", payload, ctx, Some(session), cancel)?;
    check_mutation_status(&resp, "Could not remove video from playlist.")?;
    Ok(resp)
}

/// Builds the endpoint and payload for `like/like`, `like/dislike`, or `like/removelike`.
pub fn build_rate_video_payload(
    video_id: &str,
    rating: VideoRating,
) -> Result<(&'static str, Value), InnerTubeError> {
    let vid = video_id.trim();
    if !is_valid_video_id(vid) {
        return Err(InnerTubeError::InvalidInput("Invalid video ID.".into()));
    }
    Ok((rating.endpoint(), json!({ "target": { "videoId": vid } })))
}

pub fn mutate_video_rating(
    video_id: &str,
    rating: VideoRating,
    ctx: &InnerTubeContext,
    session: &mut YouTubeWebSession,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let (endpoint, payload) = build_rate_video_payload(video_id, rating)?;
    post_innertube(endpoint, payload, ctx, Some(session), cancel)
}

/// Builds the endpoint and payload for `subscription/subscribe` or `subscription/unsubscribe`.
pub fn build_subscription_payload(
    channel_id: &str,
    subscribe: bool,
) -> Result<(&'static str, Value), InnerTubeError> {
    let cid = channel_id.trim();
    if !is_valid_channel_id(cid) {
        return Err(InnerTubeError::InvalidInput("Invalid channel ID.".into()));
    }
    let endpoint = if subscribe {
        "subscription/subscribe"
    } else {
        "subscription/unsubscribe"
    };
    Ok((endpoint, json!({ "channelIds": [cid] })))
}

pub fn mutate_subscription(
    channel_id: &str,
    subscribe: bool,
    ctx: &InnerTubeContext,
    session: &mut YouTubeWebSession,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let (endpoint, payload) = build_subscription_payload(channel_id, subscribe)?;
    post_innertube(endpoint, payload, ctx, Some(session), cancel)
}

/// Builds a `feedback` payload (e.g. Not Interested / Don't recommend channel).
pub fn build_feedback_payload(feedback_token: &str) -> Result<Value, InnerTubeError> {
    let token = feedback_token.trim();
    if token.is_empty() || token.len() > 4096 {
        return Err(InnerTubeError::InvalidInput(
            "Feedback token is empty or invalid.".into(),
        ));
    }
    Ok(json!({
        "feedbackTokens": [token],
        "isFeedbackTokenUnencrypted": false,
        "shouldMerge": false,
    }))
}

pub fn mutate_feedback(
    feedback_token: &str,
    ctx: &InnerTubeContext,
    session: &mut YouTubeWebSession,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let payload = build_feedback_payload(feedback_token)?;
    post_innertube("feedback", payload, ctx, Some(session), cancel)
}

fn check_mutation_status(resp: &Value, error_msg: &str) -> Result<(), InnerTubeError> {
    if let Some(status) = resp.get("status").and_then(Value::as_str)
        && status != "STATUS_SUCCEEDED"
    {
        return Err(InnerTubeError::InvalidResponse(error_msg.to_owned()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// R5.4 — Playlist Rename / Privacy / Delete / Reorder Mutations
// ---------------------------------------------------------------------------

/// Builds a `browse/edit_playlist` `ACTION_SET_PLAYLIST_NAME` payload.
#[allow(dead_code)]
pub fn build_playlist_rename_payload(
    playlist_id: &str,
    new_title: &str,
) -> Result<Value, InnerTubeError> {
    let pid = playlist_id.trim();
    let title = new_title.trim();
    if !is_valid_playlist_id(pid) {
        return Err(InnerTubeError::InvalidInput("Invalid playlist ID.".into()));
    }
    if title.is_empty() || title.chars().count() > 150 {
        return Err(InnerTubeError::InvalidInput(
            "Playlist title must be between 1 and 150 characters.".into(),
        ));
    }
    Ok(json!({
        "playlistId": pid,
        "actions": [{
            "action": "ACTION_SET_PLAYLIST_NAME",
            "playlistName": title,
        }],
    }))
}

#[allow(dead_code)]
pub fn mutate_playlist_rename(
    playlist_id: &str,
    new_title: &str,
    ctx: &InnerTubeContext,
    session: &mut YouTubeWebSession,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let payload = build_playlist_rename_payload(playlist_id, new_title)?;
    let resp = post_innertube("browse/edit_playlist", payload, ctx, Some(session), cancel)?;
    check_mutation_status(&resp, "Could not rename playlist.")?;
    Ok(resp)
}

/// Builds a `browse/edit_playlist` `ACTION_SET_PLAYLIST_PRIVACY` payload
/// (`"PUBLIC"`, `"UNLISTED"`, or `"PRIVATE"`).
#[allow(dead_code)]
pub fn build_playlist_privacy_payload(
    playlist_id: &str,
    privacy: &str,
) -> Result<Value, InnerTubeError> {
    let pid = playlist_id.trim();
    if !is_valid_playlist_id(pid) {
        return Err(InnerTubeError::InvalidInput("Invalid playlist ID.".into()));
    }
    let normalized = match privacy.trim().to_ascii_uppercase().as_str() {
        "PUBLIC" => "PUBLIC",
        "UNLISTED" => "UNLISTED",
        "PRIVATE" => "PRIVATE",
        _ => {
            return Err(InnerTubeError::InvalidInput(
                "Playlist privacy must be PUBLIC, UNLISTED, or PRIVATE.".into(),
            ));
        }
    };
    Ok(json!({
        "playlistId": pid,
        "actions": [{
            "action": "ACTION_SET_PLAYLIST_PRIVACY",
            "playlistPrivacy": normalized,
        }],
    }))
}

#[allow(dead_code)]
pub fn mutate_playlist_privacy(
    playlist_id: &str,
    privacy: &str,
    ctx: &InnerTubeContext,
    session: &mut YouTubeWebSession,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let payload = build_playlist_privacy_payload(playlist_id, privacy)?;
    let resp = post_innertube("browse/edit_playlist", payload, ctx, Some(session), cancel)?;
    check_mutation_status(&resp, "Could not update playlist privacy.")?;
    Ok(resp)
}

/// Builds a `playlist/delete` payload (rejects system playlists `WL`/`LL`).
#[allow(dead_code)]
pub fn build_playlist_delete_payload(playlist_id: &str) -> Result<Value, InnerTubeError> {
    let pid = playlist_id.trim();
    if !is_valid_playlist_id(pid) {
        return Err(InnerTubeError::InvalidInput("Invalid playlist ID.".into()));
    }
    if matches!(pid, "WL" | "LL" | "LM") {
        return Err(InnerTubeError::InvalidInput(
            "Built-in YouTube playlists cannot be deleted.".into(),
        ));
    }
    Ok(json!({ "playlistId": pid }))
}

#[allow(dead_code)]
pub fn mutate_playlist_delete(
    playlist_id: &str,
    ctx: &InnerTubeContext,
    session: &mut YouTubeWebSession,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let payload = build_playlist_delete_payload(playlist_id)?;
    let resp = post_innertube("playlist/delete", payload, ctx, Some(session), cancel)?;
    check_mutation_status(&resp, "Could not delete playlist.")?;
    Ok(resp)
}

/// Builds a `browse/edit_playlist` `ACTION_MOVE_VIDEO_AFTER` payload for drag-to-reorder.
/// When `predecessor_set_video_id` is `None`, moves the item to the top of the playlist.
#[allow(dead_code)]
pub fn build_playlist_move_video_payload(
    playlist_id: &str,
    set_video_id: &str,
    predecessor_set_video_id: Option<&str>,
) -> Result<Value, InnerTubeError> {
    let pid = playlist_id.trim();
    let svid = set_video_id.trim();
    if !is_valid_playlist_id(pid) {
        return Err(InnerTubeError::InvalidInput("Invalid playlist ID.".into()));
    }
    if svid.is_empty() || svid.len() > 256 {
        return Err(InnerTubeError::InvalidInput(
            "Playlist item setVideoId is empty or invalid.".into(),
        ));
    }
    let mut action = json!({
        "action": "ACTION_MOVE_VIDEO_AFTER",
        "setVideoId": svid,
    });
    if let Some(pred) = predecessor_set_video_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if pred.len() > 256 || pred == svid {
            return Err(InnerTubeError::InvalidInput(
                "Invalid predecessor setVideoId for playlist reorder.".into(),
            ));
        }
        if let Some(obj) = action.as_object_mut() {
            obj.insert(
                "movedSetVideoIdPredecessor".into(),
                Value::String(pred.to_owned()),
            );
        }
    }
    Ok(json!({
        "playlistId": pid,
        "actions": [action],
    }))
}

#[allow(dead_code)]
pub fn mutate_playlist_move_video(
    playlist_id: &str,
    set_video_id: &str,
    predecessor_set_video_id: Option<&str>,
    ctx: &InnerTubeContext,
    session: &mut YouTubeWebSession,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let payload =
        build_playlist_move_video_payload(playlist_id, set_video_id, predecessor_set_video_id)?;
    let resp = post_innertube("browse/edit_playlist", payload, ctx, Some(session), cancel)?;
    check_mutation_status(&resp, "Could not reorder playlist video.")?;
    Ok(resp)
}

// ---------------------------------------------------------------------------
// R5.1 — Notifications Inbox & Channel Notification Preferences
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InnerTubeNotification {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub thumbnail_url: Option<String>,
    pub channel_avatar_url: Option<String>,
    pub video_url: Option<String>,
    pub published_text: String,
    pub is_read: bool,
    pub mark_read_token: Option<String>,
    pub section_title: Option<String>,
    pub channel_url: Option<String>,
    pub hide_feedback_token: Option<String>,
    pub hide_interaction_token: Option<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InnerTubeNotificationsMenu {
    pub items: Vec<InnerTubeNotification>,
    pub unread_count: usize,
    pub continuation: Option<String>,
}

#[allow(dead_code)]
pub fn build_notifications_menu_payload_with_continuation(continuation: Option<&str>) -> Value {
    match continuation.map(str::trim).filter(|s| !s.is_empty()) {
        Some(token) => json!({
            "notificationsMenuRequestType": "NOTIFICATIONS_MENU_REQUEST_TYPE_INBOX",
            "ctoken": token,
        }),
        None => json!({
            "notificationsMenuRequestType": "NOTIFICATIONS_MENU_REQUEST_TYPE_INBOX",
        }),
    }
}

#[allow(dead_code)]
pub fn build_notifications_menu_payload() -> Value {
    build_notifications_menu_payload_with_continuation(None)
}

fn extract_notification_target_url(endpoint: &Value) -> (Option<&str>, Option<String>) {
    let video_id = endpoint
        .pointer("/watchEndpoint/videoId")
        .or_else(|| endpoint.pointer("/reelWatchEndpoint/videoId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_video_id(id));

    let url = if let Some(vid) = endpoint
        .pointer("/watchEndpoint/videoId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_video_id(id))
    {
        Some(format!("{YOUTUBE_ORIGIN}/watch?v={vid}"))
    } else if let Some(vid) = endpoint
        .pointer("/reelWatchEndpoint/videoId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_video_id(id))
    {
        Some(format!("{YOUTUBE_ORIGIN}/shorts/{vid}"))
    } else if let (_, Some(browse_url)) = channel_url_from_browse_endpoint(Some(endpoint)) {
        Some(browse_url)
    } else {
        endpoint
            .pointer("/commandMetadata/webCommandMetadata/url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .map(|u| {
                if u.starts_with("http://") || u.starts_with("https://") {
                    u.to_owned()
                } else if u.starts_with('/') {
                    format!("{YOUTUBE_ORIGIN}{u}")
                } else {
                    format!("{YOUTUBE_ORIGIN}/{u}")
                }
            })
    };

    (video_id, url)
}

fn extract_notification_hide_tokens(r: &Value) -> (Option<String>, Option<String>) {
    let mut hide_feedback_token = None;
    let mut hide_interaction_token = None;
    let Some(menu_items) = r
        .pointer("/contextualMenu/menuRenderer/items")
        .or_else(|| r.pointer("/menu/menuRenderer/items"))
        .and_then(Value::as_array)
    else {
        return (None, None);
    };
    let mut extract_from_endpoint = |endpoint: &Value| {
        if hide_feedback_token.is_none()
            && let Some(tok) = endpoint
                .pointer("/feedbackEndpoint/feedbackToken")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            hide_feedback_token = Some(tok.to_owned());
        }
        if hide_interaction_token.is_none()
            && let Some(tok) = endpoint
                .pointer("/recordNotificationInteractionsEndpoint/serializedInteractionsRequest")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            hide_interaction_token = Some(tok.to_owned());
        }
    };
    for entry in menu_items {
        let item = entry.get("menuServiceItemRenderer").unwrap_or(entry);
        let Some(service_endpoint) = item.get("serviceEndpoint") else {
            continue;
        };
        extract_from_endpoint(service_endpoint);
        if let Some(commands) = service_endpoint
            .pointer("/commandExecutorCommand/commands")
            .and_then(Value::as_array)
        {
            for cmd in commands {
                extract_from_endpoint(cmd);
            }
        }
    }
    (hide_feedback_token, hide_interaction_token)
}

fn resolve_notification_thumbnails(
    video_thumb_raw: String,
    channel_thumb_explicit: String,
    legacy_thumb: String,
) -> (Option<String>, Option<String>) {
    if !video_thumb_raw.is_empty() {
        let avatar = if !channel_thumb_explicit.is_empty() {
            Some(channel_thumb_explicit)
        } else if !legacy_thumb.is_empty() {
            Some(legacy_thumb)
        } else {
            None
        };
        (Some(video_thumb_raw), avatar)
    } else if !channel_thumb_explicit.is_empty() {
        (
            (!legacy_thumb.is_empty()).then_some(legacy_thumb),
            Some(channel_thumb_explicit),
        )
    } else {
        (None, (!legacy_thumb.is_empty()).then_some(legacy_thumb))
    }
}

fn parse_notification_renderer(
    r: &Value,
    section_title: Option<&str>,
) -> Option<InnerTubeNotification> {
    let title = non_empty_yt_text(r.get("shortMessage"))
        .or_else(|| non_empty_yt_text(r.get("text")))
        .or_else(|| non_empty_yt_text(r.get("title")))
        .unwrap_or_default();
    let nav_endpoint = r.get("navigationEndpoint").unwrap_or(&Value::Null);
    let (video_id, video_url) = extract_notification_target_url(nav_endpoint);
    let id = r
        .get("notificationId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| video_id.map(str::to_owned))?;
    if title.is_empty() {
        return None;
    }

    let video_thumb_raw = select_yt_thumbnail(
        r.pointer("/videoThumbnail/thumbnails")
            .or_else(|| r.pointer("/videoThumbnail/image/thumbnails")),
        320,
    );
    let channel_thumb_explicit = select_yt_thumbnail(
        r.pointer("/channelThumbnail/thumbnails")
            .or_else(|| r.pointer("/channelThumbnail/image/thumbnails")),
        68,
    );
    let legacy_thumb = select_yt_thumbnail(
        r.pointer("/thumbnail/thumbnails")
            .or_else(|| r.pointer("/thumbnail/image/thumbnails")),
        176,
    );
    let (thumbnail_url, channel_avatar_url) =
        resolve_notification_thumbnails(video_thumb_raw, channel_thumb_explicit, legacy_thumb);

    let channel = non_empty_yt_text(r.get("channelName"))
        .or_else(|| non_empty_yt_text(r.get("bylineText")))
        .or_else(|| {
            r.pointer("/channelThumbnail/accessibility/accessibilityData/label")
                .or_else(|| r.pointer("/thumbnail/accessibility/accessibilityData/label"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_default();

    let channel_url = channel_url_from_browse_endpoint(r.get("channelEndpoint"))
        .1
        .or_else(|| channel_url_from_browse_endpoint(Some(nav_endpoint)).1)
        .or_else(|| {
            r.pointer("/shortMessage/runs")
                .and_then(Value::as_array)
                .and_then(|runs| {
                    runs.iter().find_map(|run| {
                        channel_url_from_browse_endpoint(run.get("navigationEndpoint")).1
                    })
                })
        });

    let published_text = non_empty_yt_text(r.get("sentTimeText"))
        .or_else(|| non_empty_yt_text(r.get("timeStatusText")))
        .or_else(|| non_empty_yt_text(r.get("publishedTimeText")))
        .unwrap_or_default();
    let is_read = r.get("read").and_then(Value::as_bool).unwrap_or(false);
    let mark_read_token = r
        .pointer("/recordClickEndpoint/recordNotificationInteractionsEndpoint/serializedInteractionsRequest")
        .or_else(|| {
            r.pointer("/markAsReadEndpoint/recordNotificationInteractionsEndpoint/serializedInteractionsRequest")
        })
        .or_else(|| {
            r.pointer("/navigationEndpoint/recordNotificationInteractionsEndpoint/serializedInteractionsRequest")
        })
        .or_else(|| r.get("serializedInteractionsRequest"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let (hide_feedback_token, hide_interaction_token) = extract_notification_hide_tokens(r);

    Some(InnerTubeNotification {
        id,
        title,
        channel,
        thumbnail_url,
        channel_avatar_url,
        video_url,
        published_text,
        is_read,
        mark_read_token,
        section_title: section_title
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        channel_url,
        hide_feedback_token,
        hide_interaction_token,
    })
}

fn parse_notification_view_model(
    vm: &Value,
    section_title: Option<&str>,
) -> Option<InnerTubeNotification> {
    let title = non_empty_yt_text(vm.get("shortMessage"))
        .or_else(|| non_empty_yt_text(vm.get("text")))
        .or_else(|| non_empty_yt_text(vm.get("title")))
        .unwrap_or_default();
    let nav_endpoint = vm
        .pointer("/rendererContext/commandContext/onTap/innertubeCommand")
        .or_else(|| vm.pointer("/onTap/innertubeCommand"))
        .or_else(|| vm.get("navigationEndpoint"))
        .unwrap_or(&Value::Null);
    let (video_id, video_url) = extract_notification_target_url(nav_endpoint);
    let id = vm
        .get("notificationId")
        .or_else(|| vm.get("entityKey"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| video_id.map(str::to_owned))?;
    if title.is_empty() {
        return None;
    }

    let video_thumb_raw = select_yt_thumbnail(
        vm.pointer("/videoThumbnail/image/sources")
            .or_else(|| vm.pointer("/videoThumbnail/thumbnails"))
            .or_else(|| vm.pointer("/thumbnail/sources")),
        320,
    );
    let channel_thumb_explicit = select_yt_thumbnail(
        vm.pointer("/avatar/avatarViewModel/image/sources")
            .or_else(|| vm.pointer("/channelThumbnail/image/sources"))
            .or_else(|| vm.pointer("/channelThumbnail/thumbnails")),
        68,
    );
    let legacy_thumb = select_yt_thumbnail(vm.pointer("/thumbnail/thumbnails"), 176);
    let (thumbnail_url, channel_avatar_url) =
        resolve_notification_thumbnails(video_thumb_raw, channel_thumb_explicit, legacy_thumb);

    let channel = non_empty_yt_text(vm.get("channelName"))
        .or_else(|| non_empty_yt_text(vm.get("bylineText")))
        .unwrap_or_default();
    let published_text = non_empty_yt_text(vm.get("sentTimeText"))
        .or_else(|| non_empty_yt_text(vm.get("timeStatusText")))
        .or_else(|| non_empty_yt_text(vm.get("publishedTimeText")))
        .unwrap_or_default();
    let is_read = vm
        .get("read")
        .or_else(|| vm.get("isRead"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mark_read_token = nav_endpoint
        .pointer("/recordNotificationInteractionsEndpoint/serializedInteractionsRequest")
        .or_else(|| {
            vm.pointer("/recordClickEndpoint/recordNotificationInteractionsEndpoint/serializedInteractionsRequest")
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let (hide_feedback_token, hide_interaction_token) = extract_notification_hide_tokens(vm);

    Some(InnerTubeNotification {
        id,
        title,
        channel,
        thumbnail_url,
        channel_avatar_url,
        video_url,
        published_text,
        is_read,
        mark_read_token,
        section_title: section_title
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        channel_url: channel_url_from_browse_endpoint(Some(nav_endpoint)).1,
        hide_feedback_token,
        hide_interaction_token,
    })
}

fn extract_notification_continuation_token(node: &Value) -> Option<String> {
    node.pointer("/continuationEndpoint/getNotificationMenuEndpoint/ctoken")
        .or_else(|| node.pointer("/continuationEndpoint/continuationCommand/token"))
        .or_else(|| node.pointer("/continuationEndpoint/nextContinuationData/continuation"))
        .or_else(|| {
            node.pointer("/button/buttonRenderer/command/getNotificationMenuEndpoint/ctoken")
        })
        .or_else(|| node.pointer("/button/buttonRenderer/command/continuationCommand/token"))
        .or_else(|| node.pointer("/nextContinuationData/continuation"))
        .or_else(|| node.pointer("/reloadContinuationData/continuation"))
        .or_else(|| node.pointer("/getNotificationMenuEndpoint/ctoken"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| extract_continuation_item_token(node))
}

fn collect_notifications(
    node: &Value,
    section_title: Option<&str>,
    out: &mut Vec<InnerTubeNotification>,
    seen_ids: &mut HashSet<String>,
    continuation: &mut Option<String>,
) {
    match node {
        Value::Object(map) => {
            if let Some(renderer) = map.get("notificationRenderer") {
                if let Some(item) = parse_notification_renderer(renderer, section_title)
                    && seen_ids.insert(item.id.clone())
                {
                    out.push(item);
                }
                return;
            }
            if let Some(vm) = map.get("notificationViewModel") {
                if let Some(item) = parse_notification_view_model(vm, section_title)
                    && seen_ids.insert(item.id.clone())
                {
                    out.push(item);
                }
                return;
            }
            if let Some(cont) = map.get("continuationItemRenderer") {
                if continuation.is_none() {
                    *continuation = extract_notification_continuation_token(cont);
                }
                return;
            }
            if let Some(section) = map.get("multiPageMenuNotificationSectionRenderer") {
                let title_owned = non_empty_yt_text(section.get("notificationSectionTitle"))
                    .or_else(|| section_title.map(str::to_owned));
                let active_title = title_owned.as_deref();
                if let Some(items) = section.get("items") {
                    collect_notifications(items, active_title, out, seen_ids, continuation);
                }
                if continuation.is_none()
                    && let Some(conts) = section.get("continuations").and_then(Value::as_array)
                {
                    for c in conts {
                        if let Some(tok) = extract_notification_continuation_token(c) {
                            *continuation = Some(tok);
                            break;
                        }
                    }
                }
                return;
            }
            if let Some(section_cont) = map.get("multiPageMenuNotificationSectionContinuation") {
                if let Some(items) = section_cont.get("items") {
                    collect_notifications(items, section_title, out, seen_ids, continuation);
                }
                if continuation.is_none()
                    && let Some(conts) = section_cont.get("continuations").and_then(Value::as_array)
                {
                    for c in conts {
                        if let Some(tok) = extract_notification_continuation_token(c) {
                            *continuation = Some(tok);
                            break;
                        }
                    }
                }
                return;
            }
            if continuation.is_none()
                && map.contains_key("nextContinuationData")
                && let Some(tok) = extract_notification_continuation_token(node)
            {
                *continuation = Some(tok);
            }
            for child in map.values() {
                collect_notifications(child, section_title, out, seen_ids, continuation);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_notifications(item, section_title, out, seen_ids, continuation);
            }
        }
        _ => {}
    }
}

fn find_unseen_notification_count(node: &Value) -> Option<usize> {
    match node {
        Value::Object(map) => {
            if let Some(count) = map
                .get("unseenCount")
                .or_else(|| map.get("notificationCount"))
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
            {
                return Some(count);
            }
            for child in map.values() {
                if let Some(found) = find_unseen_notification_count(child) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(find_unseen_notification_count),
        _ => None,
    }
}

#[allow(dead_code)]
pub fn parse_notifications_menu_response(json: &Value) -> InnerTubeNotificationsMenu {
    let mut items = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut continuation = None;
    collect_notifications(json, None, &mut items, &mut seen_ids, &mut continuation);
    let unread_count = if items.is_empty() {
        find_unseen_notification_count(json).unwrap_or(0)
    } else {
        items.iter().filter(|n| !n.is_read).count()
    };
    InnerTubeNotificationsMenu {
        items,
        unread_count,
        continuation,
    }
}

#[allow(dead_code)]
pub fn fetch_notifications_menu_at(
    base_url: &str,
    ctx: &InnerTubeContext,
    session: &mut YouTubeWebSession,
    continuation: Option<&str>,
    invalidate_session_path: Option<&Path>,
    cancel: &Cancellation,
) -> Result<InnerTubeNotificationsMenu, InnerTubeError> {
    let json = post_innertube_at(
        base_url,
        "notification/get_notification_menu",
        build_notifications_menu_payload_with_continuation(continuation),
        ctx,
        Some(session),
        DEFAULT_REQUEST_TIMEOUT,
        invalidate_session_path,
        cancel,
    )?;
    let menu = parse_notifications_menu_response(&json);
    if menu.items.is_empty() && contains_sign_in_endpoint(&json) {
        if let Some(path) = invalidate_session_path {
            let _ = invalidate_web_session_at(path);
        }
        return Err(InnerTubeError::AuthExpired(
            "YouTube web session is signed out or expired.".into(),
        ));
    }
    if continuation.is_none()
        && menu.items.is_empty()
        && let Some(next_tok) = menu.continuation.clone()
    {
        let next_json = post_innertube_at(
            base_url,
            "notification/get_notification_menu",
            build_notifications_menu_payload_with_continuation(Some(&next_tok)),
            ctx,
            Some(session),
            DEFAULT_REQUEST_TIMEOUT,
            invalidate_session_path,
            cancel,
        )?;
        let mut next_menu = parse_notifications_menu_response(&next_json);
        if next_menu.items.is_empty() && contains_sign_in_endpoint(&next_json) {
            if let Some(path) = invalidate_session_path {
                let _ = invalidate_web_session_at(path);
            }
            return Err(InnerTubeError::AuthExpired(
                "YouTube web session is signed out or expired.".into(),
            ));
        }
        if next_menu.unread_count == 0 {
            next_menu.unread_count = menu.unread_count;
        }
        return Ok(next_menu);
    }
    Ok(menu)
}

#[allow(dead_code)]
pub fn fetch_notifications_menu(
    ctx: &InnerTubeContext,
    session: &mut YouTubeWebSession,
    continuation: Option<&str>,
    cancel: &Cancellation,
) -> Result<InnerTubeNotificationsMenu, InnerTubeError> {
    let invalidate_path = web_session_path().ok();
    fetch_notifications_menu_at(
        INNERTUBE_BASE_URL,
        ctx,
        session,
        continuation,
        invalidate_path.as_deref(),
        cancel,
    )
}

#[allow(dead_code)]
pub fn build_mark_notifications_read_payload(
    serialized_interactions_token: &str,
) -> Result<Value, InnerTubeError> {
    let token = serialized_interactions_token.trim();
    if token.is_empty() || token.len() > 4096 {
        return Err(InnerTubeError::InvalidInput(
            "Notification interaction token is empty or invalid.".into(),
        ));
    }
    Ok(json!({
        "serializedRecordNotificationInteractionsRequest": token,
    }))
}

#[allow(dead_code)]
pub fn mutate_mark_notifications_read(
    serialized_interactions_token: &str,
    ctx: &InnerTubeContext,
    session: &mut YouTubeWebSession,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let payload = build_mark_notifications_read_payload(serialized_interactions_token)?;
    post_innertube(
        "notification/record_interactions",
        payload,
        ctx,
        Some(session),
        cancel,
    )
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum InnerTubeNotifyLevel {
    All,
    #[default]
    Personalized,
    None,
}

impl InnerTubeNotifyLevel {
    #[allow(dead_code)]
    pub fn label(self, pt: bool) -> &'static str {
        match self {
            Self::All => {
                if pt {
                    "Todas"
                } else {
                    "All"
                }
            }
            Self::Personalized => {
                if pt {
                    "Personalizadas"
                } else {
                    "Personalized"
                }
            }
            Self::None => {
                if pt {
                    "Nenhuma"
                } else {
                    "None"
                }
            }
        }
    }
}

#[allow(dead_code)]
pub fn build_channel_notify_payload(params: &str) -> Result<Value, InnerTubeError> {
    let trimmed = params.trim();
    if trimmed.is_empty() || trimmed.len() > 4096 {
        return Err(InnerTubeError::InvalidInput(
            "Channel notification preference params are empty or invalid.".into(),
        ));
    }
    Ok(json!({ "params": trimmed }))
}

#[allow(dead_code)]
pub fn mutate_channel_notification_preference(
    params: &str,
    ctx: &InnerTubeContext,
    session: &mut YouTubeWebSession,
    cancel: &Cancellation,
) -> Result<Value, InnerTubeError> {
    let payload = build_channel_notify_payload(params)?;
    post_innertube(
        "notification/modify_channel_notification_preference",
        payload,
        ctx,
        Some(session),
        cancel,
    )
}

// ---------------------------------------------------------------------------
// R5.1 — Shorts Infinite Sequence (`FEshorts` / `reel/reel_watch_sequence`)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShortsSequencePage {
    pub items: Vec<Video>,
    pub metadata: Vec<VideoMetadata>,
    pub continuation: Option<String>,
}

#[allow(dead_code)]
pub fn build_shorts_browse_payload(continuation: Option<&str>) -> Value {
    match continuation.map(str::trim).filter(|t| !t.is_empty()) {
        Some(token) => json!({ "continuation": token }),
        None => json!({ "browseId": "FEshorts" }),
    }
}

#[allow(dead_code)]
pub fn build_reel_watch_sequence_payload(
    continuation: Option<&str>,
    sequence_params: Option<&str>,
) -> Value {
    if let Some(token) = continuation.map(str::trim).filter(|t| !t.is_empty()) {
        json!({ "continuation": token })
    } else if let Some(params) = sequence_params.map(str::trim).filter(|p| !p.is_empty()) {
        json!({ "sequenceParams": params })
    } else {
        json!({ "inputType": "REEL_WATCH_INPUT_TYPE_SEEDLESS" })
    }
}

fn parse_reel_watch_endpoint_entry(entry: &Value) -> Option<SearchResult> {
    let endpoint = entry
        .pointer("/command/reelWatchEndpoint")
        .or_else(|| entry.get("reelWatchEndpoint"))?;
    let video_id = endpoint
        .get("videoId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_video_id(id))?;
    let header = endpoint.pointer(
        "/overlay/reelPlayerOverlayRenderer/reelPlayerHeaderSupportedRenderers/reelPlayerHeaderRenderer",
    );
    let title = header
        .and_then(|h| non_empty_yt_text(h.get("reelTitleText")))
        .or_else(|| non_empty_yt_text(entry.get("title")))
        .or_else(|| non_empty_yt_text(endpoint.get("title")))
        .unwrap_or_else(|| format!("Short {video_id}"));
    let channel = header
        .and_then(|h| non_empty_yt_text(h.get("channelTitleText")))
        .unwrap_or_default();
    let (channel_id, channel_url) =
        channel_url_from_browse_endpoint(header.and_then(|h| h.get("channelNavigationEndpoint")));
    let avatar_raw = header
        .map(|h| select_yt_thumbnail(h.pointer("/channelThumbnail/thumbnails"), 68))
        .unwrap_or_default();
    let thumbnail = select_yt_thumbnail(endpoint.pointer("/thumbnail/thumbnails"), 480);

    Some(SearchResult {
        video: Video {
            title,
            channel,
            url: format!("{YOUTUBE_ORIGIN}/shorts/{video_id}"),
            thumbnail,
            duration: "0:59".to_owned(),
        },
        metadata: VideoMetadata {
            channel_id,
            channel_url,
            kind: SearchResultKind::Short,
            channel_avatar_url: (!avatar_raw.is_empty()).then_some(avatar_raw),
            ..Default::default()
        },
    })
}

/// Parses either a `browse` (`FEshorts`) response or a `reel/reel_watch_sequence` response.
#[allow(dead_code)]
pub fn parse_shorts_sequence_response(json: &Value) -> ShortsSequencePage {
    let mut page = ShortsSequencePage::default();
    let mut seen_urls = HashSet::new();

    if let Some(entries) = json.get("entries").and_then(Value::as_array) {
        for entry in entries {
            if let Some(res) = parse_reel_watch_endpoint_entry(entry) {
                push_search_result(res, &mut page.items, &mut page.metadata, &mut seen_urls);
            }
        }
    }

    collect_feed_items_and_continuation(
        json,
        &mut page.items,
        &mut page.metadata,
        &mut seen_urls,
        &mut page.continuation,
    );

    if page.continuation.is_none() {
        page.continuation = json
            .get("continuation")
            .and_then(Value::as_str)
            .or_else(|| {
                json.pointer("/continuationEndpoint/continuationCommand/token")
                    .and_then(Value::as_str)
            })
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| find_first_continuation_token(json));
    }

    page
}

#[allow(dead_code)]
pub fn fetch_reel_watch_sequence(
    continuation: Option<&str>,
    sequence_params: Option<&str>,
    ctx: &InnerTubeContext,
    session: Option<&mut YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<ShortsSequencePage, InnerTubeError> {
    let payload = build_reel_watch_sequence_payload(continuation, sequence_params);
    let json = post_innertube("reel/reel_watch_sequence", payload, ctx, session, cancel)?;
    Ok(parse_shorts_sequence_response(&json))
}

#[allow(dead_code)]
pub fn fetch_shorts_sequence(
    continuation: Option<&str>,
    ctx: &InnerTubeContext,
    mut session: Option<&mut YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<ShortsSequencePage, InnerTubeError> {
    let browse_payload = build_shorts_browse_payload(continuation);
    let browse_json = post_innertube(
        "browse",
        browse_payload,
        ctx,
        session.as_deref_mut(),
        cancel,
    )?;
    let page = parse_shorts_sequence_response(&browse_json);
    if !page.items.is_empty() || cancel.cancelled() {
        return Ok(page);
    }
    fetch_reel_watch_sequence(continuation, None, ctx, session, cancel)
}

// ---------------------------------------------------------------------------
// R5.1 — Explore / Charts Hub (`FEexplore` / `FEtrending`)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ExploreCategory {
    #[default]
    Trending,
    Music,
    Gaming,
    News,
    Sports,
    Podcasts,
}

impl ExploreCategory {
    #[allow(dead_code)]
    pub const ALL: [Self; 6] = [
        Self::Trending,
        Self::Music,
        Self::Gaming,
        Self::News,
        Self::Sports,
        Self::Podcasts,
    ];

    #[allow(dead_code)]
    pub fn label(self, pt: bool) -> &'static str {
        match self {
            Self::Trending => {
                if pt {
                    "Em alta"
                } else {
                    "Trending"
                }
            }
            Self::Music => {
                if pt {
                    "M\u{00fa}sica"
                } else {
                    "Music"
                }
            }
            Self::Gaming => {
                if pt {
                    "Jogos"
                } else {
                    "Gaming"
                }
            }
            Self::News => {
                if pt {
                    "Not\u{00ed}cias"
                } else {
                    "News"
                }
            }
            Self::Sports => {
                if pt {
                    "Esportes"
                } else {
                    "Sports"
                }
            }
            Self::Podcasts => "Podcasts",
        }
    }

    /// Returns the canonical InnerTube `(browseId, Option<params>)` pair for this category hub.
    #[allow(dead_code)]
    pub fn browse_id_and_params(self) -> (&'static str, Option<&'static str>) {
        match self {
            Self::Trending => ("FEtrending", None),
            Self::Music => ("FEtrending", Some("4gINGgt5dG1hX2NoYXJ0cw%3D%3D")),
            Self::Gaming => (
                "FEtrending",
                Some("4gIcGhpnYW1pbmdfY29ycHVzX21vc3RfcG9wdWxhcg%3D%3D"),
            ),
            Self::News => ("UCYfdidRxbB8Qhf0Nx7ioOYw", None),
            Self::Sports => ("UCEgdi0XIXXZ-qJOFPf4JSKw", None),
            Self::Podcasts => ("FEpodcasts", None),
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExploreHubPage {
    pub category: ExploreCategory,
    pub items: Vec<Video>,
    pub metadata: Vec<VideoMetadata>,
    pub continuation: Option<String>,
}

#[allow(dead_code)]
pub fn parse_explore_response(category: ExploreCategory, json: &Value) -> ExploreHubPage {
    let mut page = ExploreHubPage {
        category,
        ..Default::default()
    };
    let mut seen_urls = HashSet::new();

    if let Some(tabs) = json
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for entry in tabs {
            if let Some(content) = entry.pointer("/tabRenderer/content") {
                collect_feed_items_and_continuation(
                    content,
                    &mut page.items,
                    &mut page.metadata,
                    &mut seen_urls,
                    &mut page.continuation,
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
                collect_feed_items_and_continuation(
                    items,
                    &mut page.items,
                    &mut page.metadata,
                    &mut seen_urls,
                    &mut page.continuation,
                );
            }
        }
    }

    if page.items.is_empty() {
        collect_feed_items_and_continuation(
            json,
            &mut page.items,
            &mut page.metadata,
            &mut seen_urls,
            &mut page.continuation,
        );
    }

    page
}

#[allow(dead_code)]
pub fn fetch_explore_hub(
    category: ExploreCategory,
    continuation: Option<&str>,
    ctx: &InnerTubeContext,
    session: Option<&mut YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<ExploreHubPage, InnerTubeError> {
    let payload = if let Some(token) = continuation.map(str::trim).filter(|t| !t.is_empty()) {
        json!({ "continuation": token })
    } else {
        let (browse_id, params) = category.browse_id_and_params();
        let mut body = json!({ "browseId": browse_id });
        if let Some(p) = params
            && let Some(obj) = body.as_object_mut()
        {
            obj.insert("params".into(), Value::String(p.to_owned()));
        }
        body
    };
    let json = post_innertube("browse", payload, ctx, session, cancel)?;
    Ok(parse_explore_response(category, &json))
}

// ---------------------------------------------------------------------------
// R5.1 — Channel Community Posts (`backstagePostRenderer`) & Upcoming Streams
// ---------------------------------------------------------------------------

/// Parses `upcomingEventData.startTime` (Unix timestamp in seconds) from a video renderer.
#[allow(dead_code)]
pub fn parse_upcoming_start_unix(renderer: &Value) -> Option<i64> {
    let raw = renderer
        .pointer("/upcomingEventData/startTime")
        .or_else(|| renderer.get("startTime"))?;
    let ts = raw
        .as_i64()
        .or_else(|| raw.as_u64().and_then(|n| i64::try_from(n).ok()))
        .or_else(|| raw.as_str().and_then(|s| s.trim().parse::<i64>().ok()))?;
    (ts > 0).then_some(ts)
}

fn parse_poll_percentage_ratio(val: Option<&Value>) -> Option<f32> {
    let v = val?;
    if let Some(f) = v.as_f64().filter(|f| f.is_finite()) {
        return Some((f as f32).clamp(0.0, 1.0));
    }
    let text = v
        .as_str()
        .map(str::to_owned)
        .or_else(|| non_empty_yt_text(Some(v)))?;
    let cleaned = text.trim().trim_end_matches('%').trim().replace(',', ".");
    let pct = cleaned.parse::<f32>().ok()?;
    pct.is_finite().then(|| (pct / 100.0).clamp(0.0, 1.0))
}

fn parse_backstage_post_renderer(p: &Value) -> Option<ChannelCommunityPost> {
    let id = p
        .get("postId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_owned();
    let author = yt_text(p.get("authorText"));
    let avatar_raw = select_yt_thumbnail(p.pointer("/authorThumbnail/thumbnails"), 68);
    let author_avatar = (!avatar_raw.is_empty()).then_some(avatar_raw);
    let published_text = yt_text(p.get("publishedTimeText"));
    let content_text = yt_text(p.get("contentText"));

    let attachment = p.get("backstageAttachment");
    let img_raw = attachment
        .map(|att| {
            let single =
                select_yt_thumbnail(att.pointer("/backstageImageRenderer/image/thumbnails"), 640);
            if !single.is_empty() {
                return single;
            }
            let multi = select_yt_thumbnail(
                att.pointer(
                    "/postMultiImageRenderer/images/0/backstageImageRenderer/image/thumbnails",
                ),
                640,
            );
            if !multi.is_empty() {
                return multi;
            }
            select_yt_thumbnail(att.pointer("/videoRenderer/thumbnail/thumbnails"), 640)
        })
        .unwrap_or_default();
    let image_url = (!img_raw.is_empty()).then_some(img_raw);

    let like_count_text = non_empty_yt_text(p.get("voteCount")).or_else(|| {
        non_empty_yt_text(
            p.pointer("/actionButtons/commentActionButtonsRenderer/likeButton/toggleButtonRenderer/defaultText"),
        )
    });
    let comment_count_text = non_empty_yt_text(
        p.pointer("/actionButtons/commentActionButtonsRenderer/replyButton/buttonRenderer/text"),
    );

    let poll_node =
        attachment.and_then(|att| att.get("pollRenderer").or_else(|| att.get("quizRenderer")));
    let mut is_quiz = attachment.and_then(|att| att.get("quizRenderer")).is_some()
        || poll_node
            .and_then(|poll| poll.get("type"))
            .and_then(Value::as_str)
            == Some("POLL_TYPE_QUIZ");
    let mut poll_choices = Vec::new();
    if let Some(choices) = poll_node
        .and_then(|poll| poll.get("choices"))
        .and_then(Value::as_array)
    {
        for choice in choices {
            let text = yt_text(choice.get("text"));
            if text.is_empty() {
                continue;
            }
            let vote_ratio = parse_poll_percentage_ratio(
                choice
                    .get("voteRatio")
                    .or_else(|| choice.get("voteRatioIfSelected")),
            )
            .or_else(|| {
                parse_poll_percentage_ratio(
                    choice
                        .get("votePercentage")
                        .or_else(|| choice.get("votePercentageIfSelected")),
                )
            });
            let is_correct = choice
                .get("isCorrect")
                .or_else(|| choice.get("correct"))
                .and_then(Value::as_bool);
            let explanation = non_empty_yt_text(choice.get("explanation"));
            if is_correct.is_some() || explanation.is_some() {
                is_quiz = true;
            }
            poll_choices.push(ChannelPollChoice {
                text,
                vote_ratio,
                is_correct,
                explanation,
            });
        }
    }

    Some(ChannelCommunityPost {
        id,
        author,
        author_avatar,
        published_text,
        content_text,
        image_url,
        like_count_text,
        comment_count_text,
        poll_choices,
        is_quiz,
    })
}

fn collect_community_posts(
    node: &Value,
    out: &mut Vec<ChannelCommunityPost>,
    seen_ids: &mut HashSet<String>,
) {
    match node {
        Value::Object(map) => {
            if let Some(post) = map
                .get("backstagePostRenderer")
                .or_else(|| map.get("postRenderer"))
            {
                if let Some(parsed) = parse_backstage_post_renderer(post)
                    && seen_ids.insert(parsed.id.clone())
                {
                    out.push(parsed);
                }
                return;
            }
            for child in map.values() {
                collect_community_posts(child, out, seen_ids);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_community_posts(item, out, seen_ids);
            }
        }
        _ => {}
    }
}

/// Extracts channel community posts (`backstagePostRenderer` / `postRenderer`), including
/// images, polls, and quizzes from a channel browse or continuation JSON payload.
pub fn parse_channel_community_posts(json: &Value) -> Vec<ChannelCommunityPost> {
    let mut posts = Vec::new();
    let mut seen_ids = HashSet::new();
    collect_community_posts(json, &mut posts, &mut seen_ids);
    posts
}

// ---------------------------------------------------------------------------
// Internal Parsers & Helpers
// ---------------------------------------------------------------------------

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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

fn valid_api_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn sanitize_https_url(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 {
        return None;
    }
    let normalized = normalize_youtube_asset_url(trimmed);
    let parsed = url::Url::parse(&normalized).ok()?;
    (matches!(parsed.scheme(), "https" | "http") && parsed.host_str().is_some())
        .then_some(normalized)
}

fn non_empty_yt_text(value: Option<&Value>) -> Option<String> {
    let text = yt_text(value);
    (!text.is_empty()).then_some(text)
}

fn extract_grouped_u64(raw: &str) -> Option<u64> {
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
    (!digits.is_empty())
        .then(|| digits.parse::<u64>().ok())
        .flatten()
}

fn extract_response_visitor_data(json: &Value) -> Option<String> {
    json.pointer("/responseContext/visitorData")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn find_object_by_key<'a>(root: &'a Value, target_key: &str) -> Option<&'a Value> {
    match root {
        Value::Object(map) => {
            if let Some(found) = map.get(target_key)
                && found.is_object()
            {
                return Some(found);
            }
            for child in map.values() {
                if let Some(found) = find_object_by_key(child, target_key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr
            .iter()
            .find_map(|item| find_object_by_key(item, target_key)),
        _ => None,
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
                .filter(|s| !s.is_empty())
            {
                return Some(token.to_owned());
            }
            if let Some(token) = map
                .get("nextContinuationData")
                .and_then(|c| c.get("continuation"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
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
        Value::Array(arr) => arr.iter().find_map(find_first_continuation_token),
        _ => None,
    }
}

fn extract_continuation_item_token(cont: &Value) -> Option<String> {
    cont.pointer("/continuationEndpoint/continuationCommand/token")
        .or_else(|| cont.pointer("/button/buttonRenderer/command/continuationCommand/token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| find_first_continuation_token(cont))
}

fn contains_signed_out_promo(node: &Value) -> bool {
    match node {
        Value::Object(map) => {
            if map.contains_key("backgroundPromoRenderer") || map.contains_key("signInEndpoint") {
                return true;
            }
            map.values().any(contains_signed_out_promo)
        }
        Value::Array(arr) => arr.iter().any(contains_signed_out_promo),
        _ => false,
    }
}

fn contains_sign_in_endpoint(node: &Value) -> bool {
    match node {
        Value::Object(map) => {
            if map.contains_key("signInEndpoint") {
                return true;
            }
            map.values().any(contains_sign_in_endpoint)
        }
        Value::Array(arr) => arr.iter().any(contains_sign_in_endpoint),
        _ => false,
    }
}

fn extract_chip_token(chip: &Value) -> Option<String> {
    chip.pointer("/navigationEndpoint/continuationCommand/token")
        .or_else(|| chip.pointer("/navigationEndpoint/reloadContinuationData/continuation"))
        .or_else(|| chip.pointer("/navigationEndpoint/browseEndpoint/params"))
        .or_else(|| chip.pointer("/onTap/innertubeCommand/continuationCommand/token"))
        .or_else(|| chip.pointer("/onTap/innertubeCommand/browseEndpoint/params"))
        .or_else(|| {
            chip.pointer("/chipButtonViewModel/onTap/innertubeCommand/continuationCommand/token")
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn collect_home_chips(node: &Value, out: &mut Vec<HomeFeedChip>, seen: &mut HashSet<String>) {
    match node {
        Value::Object(map) => {
            if let Some(bar) = map.get("feedFilterChipBarRenderer")
                && let Some(contents) = bar.get("contents").and_then(Value::as_array)
            {
                for entry in contents {
                    if let Some(chip) = entry.get("chipCloudChipRenderer") {
                        let title = yt_text(chip.get("text"));
                        if !title.is_empty() && seen.insert(title.clone()) {
                            out.push(HomeFeedChip {
                                title,
                                token: extract_chip_token(chip),
                                selected: chip
                                    .get("isSelected")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false),
                            });
                        }
                    } else if let Some(vm) = entry.get("chipViewModel") {
                        let title = vm
                            .pointer("/text/content")
                            .or_else(|| vm.get("text"))
                            .or_else(|| vm.pointer("/chipButtonViewModel/title"))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .unwrap_or("")
                            .to_owned();
                        if !title.is_empty() && seen.insert(title.clone()) {
                            out.push(HomeFeedChip {
                                title,
                                token: extract_chip_token(vm),
                                selected: vm
                                    .get("selected")
                                    .or_else(|| vm.get("isSelected"))
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false),
                            });
                        }
                    }
                }
            }
            for child in map.values() {
                collect_home_chips(child, out, seen);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_home_chips(item, out, seen);
            }
        }
        _ => {}
    }
}

fn collect_channel_sort_chips(
    node: &Value,
    out: &mut Vec<ChannelSortChip>,
    seen: &mut HashSet<String>,
) {
    match node {
        Value::Object(map) => {
            if let Some(bar) = map.get("feedFilterChipBarRenderer")
                && let Some(contents) = bar.get("contents").and_then(Value::as_array)
            {
                for entry in contents {
                    if let Some(chip) = entry.get("chipCloudChipRenderer") {
                        let title = yt_text(chip.get("text"));
                        let token = extract_chip_token(chip);
                        if !title.is_empty()
                            && let Some(continuation) = token
                            && seen.insert(title.clone())
                        {
                            out.push(ChannelSortChip {
                                title,
                                continuation,
                                selected: chip
                                    .get("isSelected")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false),
                            });
                        }
                    }
                }
            }
            if let Some(bar) = map.get("chipBarViewModel")
                && let Some(chips) = bar.get("chips").and_then(Value::as_array)
            {
                for entry in chips {
                    let vm = entry.get("chipViewModel").unwrap_or(entry);
                    let title = vm
                        .pointer("/text/content")
                        .or_else(|| vm.get("text"))
                        .or_else(|| vm.pointer("/chipButtonViewModel/title"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or("")
                        .to_owned();
                    let token =
                        extract_chip_token(vm).or_else(|| find_first_continuation_token(vm));
                    if !title.is_empty()
                        && let Some(continuation) = token
                        && seen.insert(title.clone())
                    {
                        out.push(ChannelSortChip {
                            title,
                            continuation,
                            selected: vm
                                .get("selected")
                                .or_else(|| vm.get("isSelected"))
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                        });
                    }
                }
            }
            for child in map.values() {
                collect_channel_sort_chips(child, out, seen);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_channel_sort_chips(item, out, seen);
            }
        }
        _ => {}
    }
}

fn collect_related_chips(
    node: &Value,
    out: &mut Vec<RelatedFilterChip>,
    seen: &mut HashSet<String>,
) {
    match node {
        Value::Object(map) => {
            if let Some(cloud) = map.get("chipCloudRenderer")
                && let Some(chips) = cloud.get("chips").and_then(Value::as_array)
            {
                for entry in chips {
                    if let Some(chip) = entry.get("chipCloudChipRenderer") {
                        let title = yt_text(chip.get("text"));
                        if !title.is_empty() && seen.insert(title.clone()) {
                            out.push(RelatedFilterChip {
                                title,
                                continuation: extract_chip_token(chip),
                                selected: chip
                                    .get("isSelected")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false),
                            });
                        }
                    } else if let Some(vm) = entry.get("chipViewModel") {
                        let title = vm
                            .pointer("/text/content")
                            .or_else(|| vm.get("text"))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .unwrap_or("")
                            .to_owned();
                        if !title.is_empty() && seen.insert(title.clone()) {
                            out.push(RelatedFilterChip {
                                title,
                                continuation: extract_chip_token(vm),
                                selected: vm
                                    .get("selected")
                                    .or_else(|| vm.get("isSelected"))
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false),
                            });
                        }
                    }
                }
            }
            for child in map.values() {
                collect_related_chips(child, out, seen);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_related_chips(item, out, seen);
            }
        }
        _ => {}
    }
}

fn push_search_result(
    res: SearchResult,
    videos: &mut Vec<Video>,
    metadata: &mut Vec<VideoMetadata>,
    seen_urls: &mut HashSet<String>,
) {
    if seen_urls.insert(res.video.url.clone()) {
        videos.push(res.video);
        metadata.push(res.metadata);
    }
}

fn collect_feed_items_and_continuation(
    node: &Value,
    videos: &mut Vec<Video>,
    metadata: &mut Vec<VideoMetadata>,
    seen_urls: &mut HashSet<String>,
    continuation: &mut Option<String>,
) {
    let Some(obj) = node.as_object() else {
        if let Some(arr) = node.as_array() {
            for item in arr {
                collect_feed_items_and_continuation(
                    item,
                    videos,
                    metadata,
                    seen_urls,
                    continuation,
                );
            }
        }
        return;
    };

    if obj.contains_key("adSlotRenderer")
        || obj.contains_key("promotedSparklesWebRenderer")
        || obj.contains_key("inFeedAdLayoutRenderer")
    {
        return;
    }

    if let Some(cont) = obj.get("continuationItemRenderer") {
        if continuation.is_none() {
            *continuation = extract_continuation_item_token(cont);
        }
        return;
    }

    if let Some(v) = obj
        .get("videoRenderer")
        .or_else(|| obj.get("gridVideoRenderer"))
        .or_else(|| obj.get("compactVideoRenderer"))
    {
        if let Some(res) = parse_video_renderer(v) {
            push_search_result(res, videos, metadata, seen_urls);
        }
        return;
    }

    if let Some(r) = obj.get("reelItemRenderer") {
        if let Some(res) = parse_reel_item_renderer(r) {
            push_search_result(res, videos, metadata, seen_urls);
        }
        return;
    }

    if let Some(s) = obj.get("shortsLockupViewModel") {
        if let Some(res) = parse_shorts_lockup_vm(s) {
            push_search_result(res, videos, metadata, seen_urls);
        }
        return;
    }

    if let Some(l) = obj.get("lockupViewModel") {
        if let Some(res) = parse_lockup_video_vm(l).or_else(|| parse_lockup_playlist_vm(l)) {
            push_search_result(res, videos, metadata, seen_urls);
        }
        return;
    }

    if let Some(c) = obj
        .get("channelRenderer")
        .or_else(|| obj.get("gridChannelRenderer"))
    {
        if let Some(res) = parse_channel_renderer(c) {
            push_search_result(res, videos, metadata, seen_urls);
        }
        return;
    }

    if let Some(p) = obj
        .get("playlistRenderer")
        .or_else(|| obj.get("gridPlaylistRenderer"))
        .or_else(|| obj.get("compactPlaylistRenderer"))
        .or_else(|| obj.get("compactRadioRenderer"))
    {
        if let Some(res) = parse_playlist_renderer(p) {
            push_search_result(res, videos, metadata, seen_urls);
        }
        return;
    }

    if let Some(m) = obj.get("movieRenderer") {
        if let Some(res) = parse_movie_renderer(m) {
            push_search_result(res, videos, metadata, seen_urls);
        }
        return;
    }

    for (key, child) in obj {
        if key == "secondaryContents" {
            continue;
        }
        collect_feed_items_and_continuation(child, videos, metadata, seen_urls, continuation);
    }
}

fn parse_video_renderer(v: &Value) -> Option<SearchResult> {
    let video_id = v
        .get("videoId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_video_id(id))?;
    let title = yt_text(v.get("title"));
    if title.is_empty() {
        return None;
    }
    let nav_url = v
        .pointer("/navigationEndpoint/commandMetadata/webCommandMetadata/url")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut is_short = nav_url.starts_with("/shorts/")
        || v.pointer("/navigationEndpoint/reelWatchEndpoint").is_some();
    let mut duration = yt_text(v.get("lengthText"));
    let thumbnail = select_yt_thumbnail(v.pointer("/thumbnail/thumbnails"), 480);

    let owner_obj = v
        .get("ownerText")
        .or_else(|| v.get("longBylineText"))
        .or_else(|| v.get("shortBylineText"));
    let channel = yt_text(owner_obj);
    let (channel_id, channel_url) = channel_url_from_browse_endpoint(
        v.pointer(
            "/channelThumbnailSupportedRenderers/channelThumbnailWithLinkRenderer/navigationEndpoint",
        )
        .or_else(|| owner_obj.and_then(|o| o.pointer("/runs/0/navigationEndpoint"))),
    );
    let avatar_url = select_yt_thumbnail(
        v.pointer(
            "/channelThumbnailSupportedRenderers/channelThumbnailWithLinkRenderer/thumbnail/thumbnails",
        )
        .or_else(|| v.pointer("/channelThumbnail/thumbnails")),
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
            if style.contains("LIVE_NOW")
                || icon == "LIVE"
                || label.eq_ignore_ascii_case("live")
                || label.eq_ignore_ascii_case("ao vivo")
            {
                is_live = true;
            } else if !label.is_empty() && !badges.iter().any(|b: &String| b == label) {
                badges.push(label.to_owned());
            }
        }
    }

    if let Some(overlays) = v.get("thumbnailOverlays").and_then(Value::as_array) {
        for ov in overlays {
            if let Some(ts) = ov.get("thumbnailOverlayTimeStatusRenderer") {
                let style = ts.get("style").and_then(Value::as_str).unwrap_or("");
                let text = yt_text(ts.get("text"));
                if style.eq_ignore_ascii_case("SHORTS") {
                    is_short = true;
                } else if style.eq_ignore_ascii_case("LIVE")
                    || text.eq_ignore_ascii_case("live")
                    || text.eq_ignore_ascii_case("ao vivo")
                {
                    is_live = true;
                } else if duration.is_empty() && !text.is_empty() {
                    duration = text;
                }
            }
        }
    }

    let short_views = yt_text(v.get("shortViewCountText"));
    let full_views = yt_text(v.get("viewCountText"));
    let view_count =
        parse_pt_br_compact_count(&full_views).or_else(|| parse_pt_br_compact_count(&short_views));
    let view_count_text =
        clean_pt_br_view_label(&short_views).or_else(|| clean_pt_br_view_label(&full_views));
    let mut published_time_text =
        non_empty_yt_text(v.get("publishedTimeText")).and_then(|s| clean_published_time_text(&s));
    let upcoming_start_unix = parse_upcoming_start_unix(v);
    if upcoming_start_unix.is_some() {
        if !badges.iter().any(|b| {
            b.eq_ignore_ascii_case("upcoming")
                || b.eq_ignore_ascii_case("premiere")
                || b.eq_ignore_ascii_case("estreia")
        }) {
            badges.push("Upcoming".to_owned());
        }
        if published_time_text.is_none() {
            published_time_text =
                non_empty_yt_text(v.pointer("/upcomingEventData/upcomingEventText"))
                    .and_then(|s| clean_published_time_text(&s));
        }
    }
    let description = v
        .pointer("/detailedMetadataSnippets/0/snippetText")
        .or_else(|| v.get("descriptionSnippet"))
        .and_then(|val| non_empty_yt_text(Some(val)));

    let url = if is_short {
        format!("{YOUTUBE_ORIGIN}/shorts/{video_id}")
    } else {
        format!("{YOUTUBE_ORIGIN}/watch?v={video_id}")
    };

    Some(SearchResult {
        video: Video {
            title,
            channel,
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
            upcoming_start_unix,
        },
    })
}

fn parse_reel_item_renderer(r: &Value) -> Option<SearchResult> {
    let video_id = r
        .get("videoId")
        .or_else(|| r.pointer("/navigationEndpoint/reelWatchEndpoint/videoId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_video_id(id))?;
    let title =
        non_empty_yt_text(r.get("headline")).or_else(|| non_empty_yt_text(r.get("title")))?;
    let thumbnail = select_yt_thumbnail(r.pointer("/thumbnail/thumbnails"), 480);
    let views_raw = yt_text(r.get("viewCountText"));
    let view_count = parse_pt_br_compact_count(&views_raw);
    let view_count_text = clean_pt_br_view_label(&views_raw);

    Some(SearchResult {
        video: Video {
            title,
            channel: String::new(),
            url: format!("{YOUTUBE_ORIGIN}/shorts/{video_id}"),
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

fn parse_shorts_lockup_vm(s: &Value) -> Option<SearchResult> {
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
    if !is_valid_video_id(video_id) {
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
                .filter(|t| !t.is_empty())
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
            url: format!("{YOUTUBE_ORIGIN}/shorts/{video_id}"),
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

pub(crate) fn parse_lockup_video_vm(l: &Value) -> Option<SearchResult> {
    let content_type = l.get("contentType").and_then(Value::as_str).unwrap_or("");
    let content_id = l
        .get("contentId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_video_id(id))?;
    if !content_type.is_empty() && content_type != "LOCKUP_CONTENT_TYPE_VIDEO" {
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
    let nav_url = l
        .pointer("/rendererContext/commandContext/onTap/innertubeCommand/commandMetadata/webCommandMetadata/url")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut is_short = nav_url.starts_with("/shorts/")
        || l.pointer("/rendererContext/commandContext/onTap/innertubeCommand/reelWatchEndpoint")
            .is_some();
    if let Some(overlays) = l
        .pointer("/contentImage/thumbnailViewModel/overlays")
        .and_then(Value::as_array)
    {
        for ov in overlays {
            if let Some(badges) = ov
                .pointer("/thumbnailOverlayBadgeViewModel/thumbnailBadges")
                .and_then(Value::as_array)
            {
                for entry in badges {
                    let badge = entry.get("thumbnailBadgeViewModel").unwrap_or(entry);
                    let style = badge
                        .get("badgeStyle")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let text = badge
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or("");
                    if style.contains("SHORTS") || text.eq_ignore_ascii_case("shorts") {
                        is_short = true;
                    } else if style.contains("LIVE")
                        || text.eq_ignore_ascii_case("live")
                        || text.eq_ignore_ascii_case("ao vivo")
                    {
                        is_live = true;
                    } else if !text.is_empty() && duration.is_empty() {
                        duration = text.to_owned();
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

    let avatar_url = select_yt_thumbnail(
        l.pointer(
            "/metadata/lockupMetadataViewModel/image/decoratedAvatarViewModel/avatar/avatarViewModel/image/sources",
        ),
        68,
    );
    let channel_avatar_url = (!avatar_url.is_empty()).then_some(avatar_url);

    let url = if is_short {
        format!("{YOUTUBE_ORIGIN}/shorts/{content_id}")
    } else {
        format!("{YOUTUBE_ORIGIN}/watch?v={content_id}")
    };

    Some(SearchResult {
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
            channel_avatar_url,
            channel_verified,
            ..Default::default()
        },
    })
}

pub(crate) fn extract_lockup_playlist_privacy_badges(l: &Value) -> Vec<String> {
    let mut badges = Vec::new();
    if let Some(rows) = l
        .pointer("/metadata/lockupMetadataViewModel/metadata/contentMetadataViewModel/metadataRows")
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
                for segment in text.split(['·', '•', '|']) {
                    let seg = segment.trim();
                    let lower = seg.to_lowercase();
                    if matches!(
                        lower.as_str(),
                        "private"
                            | "privado"
                            | "privada"
                            | "particular"
                            | "unlisted"
                            | "não listado"
                            | "não listada"
                            | "nao listado"
                            | "nao listada"
                            | "public"
                            | "público"
                            | "pública"
                            | "publico"
                            | "publica"
                    ) && !badges.iter().any(|b: &String| b.eq_ignore_ascii_case(seg))
                    {
                        badges.push(seg.to_owned());
                    }
                }
            }
        }
    }
    badges
}

fn parse_lockup_playlist_vm(l: &Value) -> Option<SearchResult> {
    let content_type = l.get("contentType").and_then(Value::as_str).unwrap_or("");
    let content_id = l
        .get("contentId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_playlist_id(id))?;
    if content_type != "LOCKUP_CONTENT_TYPE_PLAYLIST"
        && content_type != "LOCKUP_CONTENT_TYPE_PODCAST"
        && content_type != "LOCKUP_CONTENT_TYPE_ALBUM"
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
        .pointer(
            "/contentImage/collectionThumbnailViewModel/primaryThumbnail/thumbnailViewModel/overlays",
        )
        .or_else(|| l.pointer("/contentImage/thumbnailViewModel/overlays"))
        .and_then(Value::as_array)
        .and_then(|ovs| {
            ovs.iter().find_map(|ov| {
                ov.pointer(
                    "/thumbnailOverlayBadgeViewModel/thumbnailBadges/0/thumbnailBadgeViewModel/text",
                )
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
            })
        });
    let thumbnail = select_yt_thumbnail(
        l.pointer(
            "/contentImage/collectionThumbnailViewModel/primaryThumbnail/thumbnailViewModel/image/sources",
        )
        .or_else(|| l.pointer("/contentImage/thumbnailViewModel/image/sources")),
        480,
    );
    let badges = extract_lockup_playlist_privacy_badges(l);

    Some(SearchResult {
        video: Video {
            title,
            channel,
            url: format!("{YOUTUBE_ORIGIN}/playlist?list={content_id}"),
            thumbnail,
            duration: String::new(),
        },
        metadata: VideoMetadata {
            channel_id,
            channel_url,
            kind: SearchResultKind::Playlist,
            badges,
            playlist_count,
            ..Default::default()
        },
    })
}

fn parse_channel_renderer(c: &Value) -> Option<SearchResult> {
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
        resolved_url.unwrap_or_else(|| format!("{YOUTUBE_ORIGIN}/channel/{channel_id}"));
    let avatar = select_yt_thumbnail(c.pointer("/thumbnail/thumbnails"), 176);
    let sub_text = non_empty_yt_text(c.get("subscriberCountText"));
    let description = non_empty_yt_text(c.get("descriptionSnippet"));
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
            subscriber_count_text: sub_text,
            ..Default::default()
        },
    })
}

fn parse_playlist_renderer(p: &Value) -> Option<SearchResult> {
    let playlist_id = p
        .get("playlistId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_playlist_id(id))?;
    let title = yt_text(p.get("title"));
    if title.is_empty() {
        return None;
    }
    let channel = yt_text(p.get("shortBylineText").or_else(|| p.get("longBylineText")));
    let (channel_id, channel_url) = channel_url_from_browse_endpoint(
        p.pointer("/shortBylineText/runs/0/navigationEndpoint")
            .or_else(|| p.pointer("/longBylineText/runs/0/navigationEndpoint")),
    );
    let thumbnail = select_yt_thumbnail(
        p.pointer("/thumbnails/0/thumbnails")
            .or_else(|| p.pointer("/thumbnail/thumbnails")),
        480,
    );
    let playlist_count = non_empty_yt_text(p.get("videoCountText")).or_else(|| {
        p.get("videoCount")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_owned)
    });

    Some(SearchResult {
        video: Video {
            title,
            channel,
            url: format!("{YOUTUBE_ORIGIN}/playlist?list={playlist_id}"),
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

fn parse_movie_renderer(m: &Value) -> Option<SearchResult> {
    let video_id = m
        .get("videoId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_video_id(id))?;
    let title = yt_text(m.get("title"));
    if title.is_empty() {
        return None;
    }
    let channel = yt_text(m.get("longBylineText").or_else(|| m.get("shortBylineText")));
    let thumbnail = select_yt_thumbnail(m.pointer("/thumbnail/thumbnails"), 480);
    let duration = yt_text(m.get("lengthText"));
    let description = non_empty_yt_text(m.get("descriptionSnippet"));

    Some(SearchResult {
        video: Video {
            title,
            channel,
            url: format!("{YOUTUBE_ORIGIN}/watch?v={video_id}"),
            thumbnail,
            duration,
        },
        metadata: VideoMetadata {
            description,
            kind: SearchResultKind::Movie,
            ..Default::default()
        },
    })
}

fn collect_playlist_videos(
    node: &Value,
    videos: &mut Vec<Video>,
    metadata: &mut Vec<VideoMetadata>,
    set_video_ids: &mut Vec<Option<String>>,
    seen_keys: &mut HashSet<String>,
    continuation: &mut Option<String>,
) {
    let Some(obj) = node.as_object() else {
        if let Some(arr) = node.as_array() {
            for item in arr {
                collect_playlist_videos(
                    item,
                    videos,
                    metadata,
                    set_video_ids,
                    seen_keys,
                    continuation,
                );
            }
        }
        return;
    };

    if let Some(cont) = obj.get("continuationItemRenderer") {
        if continuation.is_none() {
            *continuation = extract_continuation_item_token(cont);
        }
        return;
    }

    if let Some(v) = obj.get("playlistVideoRenderer") {
        if v.get("isPlayable").and_then(Value::as_bool) == Some(false) {
            return;
        }
        let Some(video_id) = v
            .get("videoId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| is_valid_video_id(id))
        else {
            return;
        };
        let title = yt_text(v.get("title"));
        if title.is_empty()
            || matches!(
                title.as_str(),
                "[Deleted video]" | "[Private video]" | "[Vídeo excluído]" | "[Vídeo privado]"
            )
        {
            return;
        }
        let set_video_id = v
            .get("setVideoId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let dedup_key = set_video_id.clone().unwrap_or_else(|| video_id.to_owned());
        if !seen_keys.insert(dedup_key) {
            return;
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
                    s.as_u64()
                        .or_else(|| s.as_str().and_then(|t| t.parse::<u64>().ok()))
                })
                .filter(|&s| s > 0)
        {
            duration = format_duration_seconds(secs);
        }
        if let Some(overlays) = v.get("thumbnailOverlays").and_then(Value::as_array) {
            for ov in overlays {
                if let Some(ts) = ov.get("thumbnailOverlayTimeStatusRenderer") {
                    let style = ts.get("style").and_then(Value::as_str).unwrap_or("");
                    let text = yt_text(ts.get("text"));
                    if style.eq_ignore_ascii_case("SHORTS") {
                        is_short = true;
                    } else if style.eq_ignore_ascii_case("LIVE")
                        || text.eq_ignore_ascii_case("live")
                        || text.eq_ignore_ascii_case("ao vivo")
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
        let published_time_text = video_info_date.or_else(|| {
            non_empty_yt_text(v.get("publishedTimeText"))
                .and_then(|s| clean_published_time_text(&s))
        });
        let url = if is_short {
            format!("{YOUTUBE_ORIGIN}/shorts/{video_id}")
        } else {
            format!("{YOUTUBE_ORIGIN}/watch?v={video_id}")
        };

        videos.push(Video {
            title,
            channel,
            url,
            thumbnail,
            duration: if is_live { String::new() } else { duration },
        });
        metadata.push(VideoMetadata {
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
        });
        set_video_ids.push(set_video_id);
        return;
    }

    if let Some(l) = obj.get("lockupViewModel") {
        if let Some(res) = parse_lockup_video_vm(l) {
            let dedup_key = crate::history::youtube_video_id(&res.video.url)
                .unwrap_or_else(|| res.video.url.clone());
            if seen_keys.insert(dedup_key) {
                videos.push(res.video);
                metadata.push(res.metadata);
                set_video_ids.push(None);
            }
        }
        return;
    }

    for child in obj.values() {
        collect_playlist_videos(
            child,
            videos,
            metadata,
            set_video_ids,
            seen_keys,
            continuation,
        );
    }
}

fn is_comments_section(section: &Value) -> bool {
    section
        .get("sectionIdentifier")
        .and_then(Value::as_str)
        .is_some_and(|id| id == "comment-item-section")
        || section
            .get("targetId")
            .and_then(Value::as_str)
            .is_some_and(|id| id == "comments-section")
}

fn extract_watch_like_count(primary_info: &Value) -> Option<String> {
    let buttons = primary_info
        .pointer("/videoActions/menuRenderer/topLevelButtons")
        .and_then(Value::as_array)?;
    for button in buttons {
        if let Some(title) = button
            .pointer(
                "/segmentedLikeDislikeButtonViewModel/likeButtonViewModel/likeButtonViewModel/toggleButtonViewModel/toggleButtonViewModel/defaultButtonViewModel/buttonViewModel/title",
            )
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| s.chars().any(|c| c.is_ascii_digit()))
        {
            return Some(title.to_owned());
        }
        if let Some(like_btn) =
            button.pointer("/segmentedLikeDislikeButtonRenderer/likeButton/toggleButtonRenderer")
        {
            let text = yt_text(like_btn.get("defaultText"));
            if text.chars().any(|c| c.is_ascii_digit()) {
                return Some(text);
            }
        }
        if let Some(toggle) = button.get("toggleButtonRenderer") {
            let icon = toggle
                .pointer("/defaultIcon/iconType")
                .and_then(Value::as_str)
                .unwrap_or("");
            if icon == "LIKE" {
                let text = yt_text(toggle.get("defaultText"));
                if text.chars().any(|c| c.is_ascii_digit()) {
                    return Some(text);
                }
            }
        }
    }
    None
}

fn extract_endpoint_url(endpoint: Option<&Value>) -> Option<String> {
    let ep = endpoint?;
    if let Some(url) = ep
        .pointer("/urlEndpoint/url")
        .or_else(|| ep.pointer("/commandMetadata/webCommandMetadata/url"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if url.starts_with('/') {
            return Some(format!("{YOUTUBE_ORIGIN}{url}"));
        }
        return Some(url.to_owned());
    }
    None
}

fn extract_description_runs(secondary_info: &Value) -> Vec<DescriptionRun> {
    if let Some(runs) = secondary_info
        .pointer("/description/runs")
        .and_then(Value::as_array)
    {
        let mut out = Vec::with_capacity(runs.len());
        for run in runs {
            let Some(text) = run.get("text").and_then(Value::as_str) else {
                continue;
            };
            if text.is_empty() {
                continue;
            }
            out.push(DescriptionRun {
                text: text.to_owned(),
                url: extract_endpoint_url(run.get("navigationEndpoint")),
            });
        }
        if !out.is_empty() {
            return out;
        }
    }

    if let Some(attr) = secondary_info.get("attributedDescription")
        && let Some(content) = attr.get("content").and_then(Value::as_str)
        && !content.is_empty()
    {
        let Some(cmd_runs) = attr.get("commandRuns").and_then(Value::as_array) else {
            return vec![DescriptionRun {
                text: content.to_owned(),
                url: None,
            }];
        };

        // Map UTF-16 code-unit offsets (used by YouTube commandRuns) to UTF-8 byte offsets.
        let mut utf16_to_byte: Vec<(usize, usize)> = Vec::new();
        let mut utf16_pos = 0usize;
        for (byte_idx, ch) in content.char_indices() {
            utf16_to_byte.push((utf16_pos, byte_idx));
            utf16_pos += ch.len_utf16();
        }
        utf16_to_byte.push((utf16_pos, content.len()));
        let map_utf16 = |target: usize| -> Option<usize> {
            utf16_to_byte
                .binary_search_by_key(&target, |(u16_idx, _)| *u16_idx)
                .ok()
                .map(|idx| utf16_to_byte[idx].1)
        };

        let mut spans: Vec<(usize, usize, String)> = Vec::new();
        for run in cmd_runs {
            let start_u16 = run.get("startIndex").and_then(Value::as_u64);
            let len_u16 = run.get("length").and_then(Value::as_u64);
            let url = extract_endpoint_url(run.pointer("/onTap/innertubeCommand"));
            if let (Some(start), Some(len), Some(url)) = (start_u16, len_u16, url)
                && len > 0
                && let (Some(byte_start), Some(byte_end)) =
                    (map_utf16(start as usize), map_utf16((start + len) as usize))
                && byte_start < byte_end
            {
                spans.push((byte_start, byte_end, url));
            }
        }
        spans.sort_by_key(|(start, _, _)| *start);

        let mut out = Vec::new();
        let mut cursor = 0usize;
        for (start, end, url) in spans {
            if start < cursor {
                continue;
            }
            if start > cursor {
                out.push(DescriptionRun {
                    text: content[cursor..start].to_owned(),
                    url: None,
                });
            }
            out.push(DescriptionRun {
                text: content[start..end].to_owned(),
                url: Some(url),
            });
            cursor = end;
        }
        if cursor < content.len() {
            out.push(DescriptionRun {
                text: content[cursor..].to_owned(),
                url: None,
            });
        }
        if !out.is_empty() {
            return out;
        }
    }

    Vec::new()
}

fn parse_u64_flexible(value: Option<&Value>) -> Option<u64> {
    let val = value?;
    val.as_u64()
        .or_else(|| val.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
        .or_else(|| {
            val.as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .map(|n| n.round() as u64)
        })
}

fn parse_f64_flexible(value: Option<&Value>) -> Option<f64> {
    let val = value?;
    val.as_f64()
        .or_else(|| val.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
        .filter(|n| n.is_finite())
}

fn find_markers_map(root: &Value) -> Option<&Vec<Value>> {
    if let Some(arr) = root
        .pointer("/playerOverlays/playerOverlayRenderer/decoratedPlayerBarRenderer/decoratedPlayerBarRenderer/playerBar/multiMarkersPlayerBarRenderer/markersMap")
        .and_then(Value::as_array)
    {
        return Some(arr);
    }
    find_object_by_key(root, "multiMarkersPlayerBarRenderer")
        .and_then(|m| m.get("markersMap"))
        .and_then(Value::as_array)
}

fn parse_heatseeker_heatmap(json: &Value) -> Vec<HeatmapBucket> {
    let Some(markers_map) = find_markers_map(json) else {
        return Vec::new();
    };
    let Some(entry) = markers_map
        .iter()
        .find(|item| item.get("key").and_then(Value::as_str) == Some("HEATSEEKER"))
    else {
        return Vec::new();
    };

    let raw_markers = entry
        .pointer("/value/heatmap/heatmapRenderer/heatMarkers")
        .or_else(|| entry.pointer("/value/heatMarkers"))
        .or_else(|| entry.pointer("/value/markers"))
        .and_then(Value::as_array);
    let Some(raw_markers) = raw_markers else {
        return Vec::new();
    };

    let mut parsed: Vec<(u64, u64, f64)> = Vec::with_capacity(raw_markers.len());
    let mut max_intensity = 0.0_f64;
    for item in raw_markers {
        let marker = item.get("heatMarkerRenderer").unwrap_or(item);
        let Some(start_ms) = parse_u64_flexible(marker.get("timeRangeStartMillis")) else {
            continue;
        };
        let duration_ms = parse_u64_flexible(marker.get("markerDurationMillis")).unwrap_or(0);
        let raw_score = parse_f64_flexible(
            marker
                .get("heatMarkerIntensityScoreNormalized")
                .or_else(|| marker.get("intensityScoreNormalized")),
        )
        .unwrap_or(0.0)
        .max(0.0);
        if raw_score > max_intensity {
            max_intensity = raw_score;
        }
        parsed.push((start_ms, duration_ms, raw_score));
    }

    let divisor = if max_intensity > 1.0 {
        max_intensity
    } else {
        1.0
    };
    parsed
        .into_iter()
        .map(|(start_ms, duration_ms, score)| HeatmapBucket {
            start_ms,
            duration_ms,
            intensity: ((score / divisor) as f32).clamp(0.0, 1.0),
        })
        .collect()
}

fn collect_entity_mutations<'a>(node: &'a Value, entities: &mut HashMap<&'a str, &'a Value>) {
    match node {
        Value::Object(map) => {
            if let Some(key) = map
                .get("entityKey")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                && let Some(payload) = map.get("payload")
            {
                entities.insert(key, payload);
            }
            for child in map.values() {
                collect_entity_mutations(child, entities);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_entity_mutations(item, entities);
            }
        }
        _ => {}
    }
}

fn extract_comments_header_meta(node: &Value, page: &mut InnerTubeCommentsPage) {
    match node {
        Value::Object(map) => {
            if let Some(header) = map.get("commentsHeaderRenderer") {
                if page.total_count_text.is_none() {
                    page.total_count_text = non_empty_yt_text(
                        header.get("countText").or_else(|| header.get("titleText")),
                    );
                }
                if let Some(items) = header
                    .pointer("/sortMenu/sortFilterSubMenuRenderer/subMenuItems")
                    .and_then(Value::as_array)
                {
                    if page.top_sort_continuation.is_none()
                        && let Some(first) = items.first()
                    {
                        page.top_sort_continuation = first
                            .pointer("/serviceEndpoint/continuationCommand/token")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .or_else(|| find_first_continuation_token(first));
                    }
                    if page.newest_sort_continuation.is_none()
                        && let Some(second) = items.get(1)
                    {
                        page.newest_sort_continuation = second
                            .pointer("/serviceEndpoint/continuationCommand/token")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .or_else(|| find_first_continuation_token(second));
                    }
                }
            }
            for child in map.values() {
                extract_comments_header_meta(child, page);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                extract_comments_header_meta(item, page);
            }
        }
        _ => {}
    }
}

fn extract_innermost_comment_vm(obj: &Value) -> Option<&Value> {
    if obj.get("commentKey").is_some() {
        return Some(obj);
    }
    let mut current = find_object_by_key(obj, "commentViewModel")?;
    while let Some(inner) = current.get("commentViewModel") {
        current = inner;
    }
    current.get("commentKey").is_some().then_some(current)
}

fn parse_comment_thread_renderer(
    thread: &Value,
    entities: &HashMap<&str, &Value>,
) -> Option<InnerTubeComment> {
    let replies_continuation = thread
        .get("replies")
        .and_then(find_first_continuation_token);
    let replies_fallback_count = thread
        .pointer("/replies/commentRepliesRenderer/viewReplies/buttonRenderer/text")
        .map(|t| yt_text(Some(t)))
        .and_then(|s| parse_pt_br_compact_count(&s))
        .unwrap_or(0);
    let thread_pinned = thread.get("pinnedText").is_some();

    let mut comment = if let Some(vm) = extract_innermost_comment_vm(thread) {
        parse_comment_view_model(vm, entities, thread_pinned, replies_continuation)?
    } else {
        let renderer = thread.pointer("/comment/commentRenderer")?;
        parse_legacy_comment_renderer(renderer, thread_pinned, replies_continuation)?
    };

    if comment.reply_count == 0 && replies_fallback_count > 0 {
        comment.reply_count = replies_fallback_count;
    }
    Some(comment)
}

fn parse_comment_view_model(
    vm: &Value,
    entities: &HashMap<&str, &Value>,
    thread_pinned: bool,
    replies_continuation: Option<String>,
) -> Option<InnerTubeComment> {
    let comment_key = vm.get("commentKey").and_then(Value::as_str).unwrap_or("");
    let entity = entities
        .get(comment_key)
        .and_then(|p| p.get("commentEntityPayload").or(Some(*p)))?;

    let id = entity
        .pointer("/properties/commentId")
        .and_then(Value::as_str)
        .or_else(|| vm.get("commentId").and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_owned();
    let text = entity
        .pointer("/properties/content/content")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_default();
    let published_text = entity
        .pointer("/properties/publishedTime")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_owned();

    let author_obj = entity.get("author");
    let author = author_obj
        .and_then(|a| a.get("displayName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_owned();
    let author_avatar = sanitize_https_url(
        author_obj
            .and_then(|a| a.get("avatarThumbnailUrl"))
            .and_then(Value::as_str),
    );
    let is_owner = author_obj
        .and_then(|a| a.get("isCreator"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let is_member = author_obj.is_some_and(|a| {
        a.get("sponsorBadgeUrl")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
            || a.get("sponsorBadgeA11y")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.trim().is_empty())
    });

    let toolbar = entity.get("toolbar");
    let like_count_text = toolbar
        .and_then(|t| {
            t.get("likeCountNotliked")
                .or_else(|| t.get("likeCountLiked"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let reply_count = toolbar
        .and_then(|t| t.get("replyCount"))
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(parse_pt_br_compact_count))
        })
        .unwrap_or(0);

    let toolbar_state_key = vm
        .get("toolbarStateKey")
        .and_then(Value::as_str)
        .unwrap_or("");
    let state_payload = entities
        .get(toolbar_state_key)
        .and_then(|p| p.get("engagementToolbarStateEntityPayload").or(Some(*p)));
    let is_hearted = state_payload
        .and_then(|s| s.get("heartState"))
        .and_then(Value::as_str)
        .is_some_and(|st| st == "TOOLBAR_HEART_STATE_HEARTED")
        || toolbar
            .and_then(|t| t.get("heartActiveTooltip"))
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
    let is_pinned = thread_pinned
        || vm.get("pinnedText").is_some_and(|p| {
            !yt_text(Some(p)).is_empty() || p.as_str().is_some_and(|s| !s.trim().is_empty())
        });

    Some(InnerTubeComment {
        id,
        author,
        author_avatar,
        text,
        published_text,
        like_count_text,
        is_pinned,
        is_hearted,
        is_owner,
        is_member,
        reply_count,
        replies_continuation,
    })
}

fn parse_legacy_comment_renderer(
    r: &Value,
    thread_pinned: bool,
    replies_continuation: Option<String>,
) -> Option<InnerTubeComment> {
    let id = r
        .get("commentId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_owned();
    let author = yt_text(r.get("authorText"));
    let avatar_raw = select_yt_thumbnail(r.pointer("/authorThumbnail/thumbnails"), 88);
    let author_avatar = sanitize_https_url((!avatar_raw.is_empty()).then_some(avatar_raw.as_str()));
    let text = yt_text(r.get("contentText"));
    let published_text = yt_text(r.get("publishedTimeText"));
    let like_count_text = non_empty_yt_text(r.get("voteCount"));
    let is_pinned = thread_pinned || r.get("pinnedCommentBadge").is_some();
    let is_hearted = r
        .pointer("/actionButtons/commentActionButtonsRenderer/creatorHeart/creatorHeartRenderer/isHearted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let is_owner = r
        .get("authorIsChannelOwner")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let is_member = r.get("sponsorCommentBadge").is_some();
    let reply_count = r.get("replyCount").and_then(Value::as_u64).unwrap_or(0);

    Some(InnerTubeComment {
        id,
        author,
        author_avatar,
        text,
        published_text,
        like_count_text,
        is_pinned,
        is_hearted,
        is_owner,
        is_member,
        reply_count,
        replies_continuation,
    })
}

// ---------------------------------------------------------------------------
// Comprehensive Offline Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

    fn unique_temp_path(name: &str) -> PathBuf {
        let seq = TEST_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "fasttube-innertube-test-{}-{seq}-{name}",
            std::process::id()
        ))
    }

    fn sample_web_session(saved_at: u64, expires_at: Option<u64>) -> YouTubeWebSession {
        let mut cookies = BTreeMap::new();
        cookies.insert("SAPISID".into(), "test-sapisid".into());
        cookies.insert("SID".into(), "test-sid".into());
        cookies.insert("__Secure-1PAPISID".into(), "test-1papisid".into());
        cookies.insert("__Secure-3PAPISID".into(), "test-3papisid".into());
        YouTubeWebSession::new(
            cookies,
            Some("CgtWSVNJVE9SX1RFU1Q%3D".into()),
            saved_at,
            expires_at,
            0,
        )
        .unwrap()
    }

    #[test]
    fn sapisidhash_matches_known_deterministic_vector() {
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sapisidhash_header("test-sapisid", YOUTUBE_ORIGIN, 1_700_000_000),
            "SAPISIDHASH 1700000000_14963cac63f39c9532ddd26bf69ca8d5e4d8aab6"
        );

        let mut session = sample_web_session(1_700_000_000, Some(1_800_000_000));
        session.delegated_session_id = Some("102938475610293847561".into());
        session.api_key = Some("AIzaSyTestSecretKey_12345".into());
        let auth = session
            .authorization_header(YOUTUBE_ORIGIN, 1_700_000_000)
            .unwrap();
        assert!(
            auth.starts_with("SAPISIDHASH 1700000000_14963cac63f39c9532ddd26bf69ca8d5e4d8aab6 ")
        );
        assert!(auth.contains("SAPISID1PHASH 1700000000_"));
        assert!(auth.contains("SAPISID3PHASH 1700000000_"));

        // Verify Debug never leaks cookie, SAPISID, delegated_session_id, or api_key values.
        let debug_repr = format!("{session:?}");
        assert!(!debug_repr.contains("test-sapisid"));
        assert!(!debug_repr.contains("test-sid"));
        assert!(!debug_repr.contains("102938475610293847561"));
        assert!(!debug_repr.contains("AIzaSyTestSecretKey_12345"));
    }

    #[test]
    fn context_derives_locale_and_extracts_yt_initial_data() {
        let pt = InnerTubeContext::from_language(UiLanguage::PtBr, None);
        assert_eq!(pt.hl, "pt-BR");
        assert_eq!(pt.gl, "BR");
        assert_eq!(pt.client_name, "WEB");

        let en_gb = InnerTubeContext::from_language(UiLanguage::En, Some("gb"));
        assert_eq!(en_gb.hl, "en");
        assert_eq!(en_gb.gl, "GB");

        let html = r#"<html><script>var ytInitialData = {"contents":{"ok":true}};</script></html>"#;
        let parsed = extract_yt_initial_data(html).unwrap();
        assert_eq!(parsed["contents"]["ok"], Value::Bool(true));
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_web_session_roundtrip_expiry_and_corrupt_file_safety() {
        let path = unique_temp_path("web-session.dpapi");
        let _ = std::fs::remove_file(&path);

        let mut session = sample_web_session(1_700_000_000, Some(1_700_010_000));
        session.delegated_session_id = Some("102938475610293847561".into());
        session.api_key = Some("AIzaSyTestRoundtrip_123".into());
        save_web_session_to(&path, &session).unwrap();

        // Ensure persisted bytes are encrypted (plaintext secrets do not appear on disk).
        let raw_on_disk = std::fs::read(&path).unwrap();
        assert!(
            !String::from_utf8_lossy(&raw_on_disk).contains("test-sapisid"),
            "DPAPI file must never contain plaintext SAPISID"
        );
        assert!(
            !String::from_utf8_lossy(&raw_on_disk).contains("AIzaSyTestRoundtrip_123"),
            "DPAPI file must never contain plaintext api_key"
        );

        let loaded = load_web_session_from(&path, 1_700_005_000)
            .unwrap()
            .expect("active session should load");
        assert_eq!(loaded.effective_sapisid(), Some("test-sapisid"));
        assert_eq!(
            loaded.visitor_data.as_deref(),
            Some("CgtWSVNJVE9SX1RFU1Q%3D")
        );
        assert_eq!(loaded.auth_user_index, 0);
        assert_eq!(
            loaded.delegated_session_id.as_deref(),
            Some("102938475610293847561")
        );
        assert_eq!(loaded.api_key.as_deref(), Some("AIzaSyTestRoundtrip_123"));

        // Expired timestamp returns Ok(None).
        assert!(
            load_web_session_from(&path, 1_700_010_000)
                .unwrap()
                .is_none()
        );

        // Corrupt file must error AND remain untouched on disk.
        let corrupt_bytes = b"corrupt-dpapi-payload-bytes";
        std::fs::write(&path, corrupt_bytes).unwrap();
        let err = load_web_session_from(&path, 1_700_005_000).unwrap_err();
        assert!(matches!(err, InnerTubeError::Storage(_)));
        assert_eq!(std::fs::read(&path).unwrap(), corrupt_bytes);

        // Explicit invalidation removes the file cleanly.
        invalidate_web_session_at(&path).unwrap();
        assert!(!path.exists());
        invalidate_web_session_at(&path).unwrap();
    }

    #[test]
    fn home_swr_cache_roundtrip_staleness_cap_and_corrupt_preservation() {
        let path = unique_temp_path("home-cache.json");
        let _ = std::fs::remove_file(&path);

        assert!(HomeSwrCache::load_from(&path).unwrap().is_none());

        let mut page = InnerTubeHomePage {
            chips: vec![HomeFeedChip {
                title: "All".into(),
                token: Some("CHIP_ALL".into()),
                selected: true,
            }],
            continuation: Some("HOME_NEXT_1".into()),
            visitor_data: Some("VISITOR_1".into()),
            ..Default::default()
        };
        for i in 0..325 {
            let vid = format!("dQw4w9Wg{:03}", i);
            page.items.push(Video {
                title: format!("Video {i}"),
                channel: "Channel".into(),
                url: format!("{YOUTUBE_ORIGIN}/watch?v={vid}"),
                thumbnail: format!("https://i.ytimg.com/vi/{vid}/hqdefault.jpg"),
                duration: "10:00".into(),
            });
            page.metadata.push(VideoMetadata {
                view_count: Some(1000 + i as u64),
                channel_verified: i % 2 == 0,
                kind: SearchResultKind::Video,
                ..Default::default()
            });
        }

        let cache = HomeSwrCache::from_page(&page, UiLanguage::En, 1_700_000_000);
        assert_eq!(cache.items.len(), MAX_HOME_CACHE_ITEMS);
        assert_eq!(cache.metadata.len(), MAX_HOME_CACHE_ITEMS);
        assert!(!cache.is_stale(1_700_000_100, DEFAULT_HOME_CACHE_TTL_SECS));
        assert!(cache.is_stale(
            1_700_000_000 + DEFAULT_HOME_CACHE_TTL_SECS,
            DEFAULT_HOME_CACHE_TTL_SECS
        ));
        assert!(cache.matches_language(UiLanguage::En));
        assert!(!cache.matches_language(UiLanguage::PtBr));

        cache.save_to(&path).unwrap();
        let loaded = HomeSwrCache::load_from(&path).unwrap().unwrap();
        assert_eq!(loaded, cache);

        // Corrupt file must fail on load AND refuse overwrite on save while staying untouched.
        let corrupt = b"{not-valid-json";
        std::fs::write(&path, corrupt).unwrap();
        assert!(HomeSwrCache::load_from(&path).is_err());
        assert!(cache.save_to(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), corrupt);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn parse_home_browse_response_extracts_chips_videos_shorts_and_continuation() {
        let fixture = json!({
            "responseContext": { "visitorData": "HOME_VISITOR_42" },
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "selected": true,
                            "content": {
                                "richGridRenderer": {
                                    "header": {
                                        "feedFilterChipBarRenderer": {
                                            "contents": [
                                                {
                                                    "chipCloudChipRenderer": {
                                                        "text": { "simpleText": "All" },
                                                        "isSelected": true
                                                    }
                                                },
                                                {
                                                    "chipCloudChipRenderer": {
                                                        "text": { "simpleText": "Rust" },
                                                        "isSelected": false,
                                                        "navigationEndpoint": {
                                                            "continuationCommand": { "token": "CHIP_RUST_TOK" }
                                                        }
                                                    }
                                                }
                                            ]
                                        }
                                    },
                                    "contents": [
                                        {
                                            "richItemRenderer": {
                                                "content": {
                                                    "videoRenderer": {
                                                        "videoId": "abcDEF12345",
                                                        "title": { "runs": [{ "text": "FastTube Home Video" }] },
                                                        "ownerText": {
                                                            "runs": [{
                                                                "text": "Systems Channel",
                                                                "navigationEndpoint": {
                                                                    "browseEndpoint": {
                                                                        "browseId": "UC1234567890123456789012",
                                                                        "canonicalBaseUrl": "/@systems"
                                                                    }
                                                                }
                                                            }]
                                                        },
                                                        "ownerBadges": [{
                                                            "metadataBadgeRenderer": {
                                                                "style": "BADGE_STYLE_TYPE_VERIFIED"
                                                            }
                                                        }],
                                                        "lengthText": { "simpleText": "12:34" },
                                                        "shortViewCountText": { "simpleText": "42K views" },
                                                        "publishedTimeText": { "simpleText": "2 days ago" },
                                                        "thumbnail": { "thumbnails": [{ "url": "https://i.ytimg.com/vi/abcDEF12345/hqdefault.jpg", "width": 480 }] }
                                                    }
                                                }
                                            }
                                        },
                                        {
                                            "richSectionRenderer": {
                                                "content": {
                                                    "richShelfRenderer": {
                                                        "contents": [
                                                            {
                                                                "richItemRenderer": {
                                                                    "content": {
                                                                        "shortsLockupViewModel": {
                                                                            "entityId": "shorts-shelf-item-sh0rtID1234",
                                                                            "onTap": {
                                                                                "innertubeCommand": {
                                                                                    "reelWatchEndpoint": { "videoId": "sh0rtID1234" }
                                                                                }
                                                                            },
                                                                            "overlayMetadata": {
                                                                                "primaryText": { "content": "Quick Short" },
                                                                                "secondaryText": { "content": "1.2M views" }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            },
                                                            {
                                                                "richItemRenderer": {
                                                                    "content": {
                                                                        "reelItemRenderer": {
                                                                            "videoId": "reelID12345",
                                                                            "headline": { "simpleText": "Legacy Reel Short" },
                                                                            "viewCountText": { "simpleText": "500K views" }
                                                                        }
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
                                                    "continuationCommand": { "token": "HOME_CONT_TOKEN" }
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
        });

        let parsed = parse_home_browse_response(&fixture);
        assert_eq!(parsed.visitor_data.as_deref(), Some("HOME_VISITOR_42"));
        assert_eq!(parsed.chips.len(), 2);
        assert!(parsed.chips[0].selected);
        assert_eq!(parsed.chips[1].title, "Rust");
        assert_eq!(parsed.chips[1].token.as_deref(), Some("CHIP_RUST_TOK"));
        assert_eq!(parsed.items.len(), 3);
        assert_eq!(parsed.metadata.len(), 3);
        assert_eq!(parsed.items[0].title, "FastTube Home Video");
        assert!(parsed.metadata[0].channel_verified);
        assert_eq!(parsed.metadata[0].kind, SearchResultKind::Video);
        assert_eq!(parsed.metadata[1].kind, SearchResultKind::Short);
        assert_eq!(
            parsed.items[1].url,
            "https://www.youtube.com/shorts/sh0rtID1234"
        );
        assert_eq!(parsed.metadata[2].kind, SearchResultKind::Short);
        assert_eq!(parsed.continuation.as_deref(), Some("HOME_CONT_TOKEN"));
    }

    #[test]
    fn parse_subscriptions_browse_response_handles_live_shorts_and_continuation() {
        let fixture = json!({
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "selected": true,
                            "content": {
                                "richGridRenderer": {
                                    "contents": [
                                        {
                                            "richItemRenderer": {
                                                "content": {
                                                    "videoRenderer": {
                                                        "videoId": "liveVID1234",
                                                        "title": { "simpleText": "Live Coding Stream" },
                                                        "ownerText": { "simpleText": "Coder" },
                                                        "badges": [{
                                                            "metadataBadgeRenderer": {
                                                                "style": "BADGE_STYLE_TYPE_LIVE_NOW",
                                                                "label": "LIVE"
                                                            }
                                                        }],
                                                        "viewCountText": { "simpleText": "3.4K watching" }
                                                    }
                                                }
                                            }
                                        },
                                        {
                                            "richItemRenderer": {
                                                "content": {
                                                    "shortsLockupViewModel": {
                                                        "onTap": {
                                                            "innertubeCommand": {
                                                                "reelWatchEndpoint": { "videoId": "subShort123" }
                                                            }
                                                        },
                                                        "overlayMetadata": {
                                                            "primaryText": { "content": "Sub Short" }
                                                        }
                                                    }
                                                }
                                            }
                                        },
                                        {
                                            "continuationItemRenderer": {
                                                "continuationEndpoint": {
                                                    "continuationCommand": { "token": "SUBS_CONT_2" }
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
        });

        let parsed = parse_subscriptions_browse_response(&fixture);
        assert!(!parsed.signed_out);
        assert_eq!(parsed.items.len(), 2);
        assert!(parsed.metadata[0].is_live);
        assert_eq!(parsed.items[0].duration, "");
        assert_eq!(parsed.metadata[1].kind, SearchResultKind::Short);
        assert_eq!(parsed.continuation.as_deref(), Some("SUBS_CONT_2"));
    }

    #[test]
    fn encode_search_sp_and_parse_search_response_spelling_and_pagination() {
        assert_eq!(encode_search_sp(&SearchFilters::default()), None);

        let date_sort = SearchFilters {
            sort_by: SearchSortBy::UploadDate,
            ..Default::default()
        };
        assert_eq!(encode_search_sp(&date_sort).as_deref(), Some("CAI="));

        let multi_features = SearchFilters {
            features: vec![
                SearchFeature::Live,
                SearchFeature::FourK,
                SearchFeature::Hd,
                SearchFeature::Live, // duplicate should be ignored
            ],
            ..Default::default()
        };
        assert_eq!(
            encode_search_sp(&multi_features).as_deref(),
            Some("EgYgAUABcAE=")
        );

        let type_and_duration = SearchFilters {
            result_type: SearchResultType::Video,
            duration: SearchDuration::Between4And20Min,
            ..Default::default()
        };
        assert_eq!(
            encode_search_sp(&type_and_duration).as_deref(),
            Some("EgQQARgD")
        );

        let dym_fixture = json!({
            "estimatedResults": "12345",
            "contents": {
                "twoColumnSearchResultsRenderer": {
                    "primaryContents": {
                        "sectionListRenderer": {
                            "contents": [
                                {
                                    "itemSectionRenderer": {
                                        "contents": [
                                            {
                                                "didYouMeanRenderer": {
                                                    "correctedQuery": { "runs": [{ "text": "rust async" }] }
                                                }
                                            },
                                            {
                                                "videoRenderer": {
                                                    "videoId": "srchVID1234",
                                                    "title": { "simpleText": "Rust Async Deep Dive" },
                                                    "ownerText": { "simpleText": "Rustacean" },
                                                    "lengthText": { "simpleText": "18:20" }
                                                }
                                            }
                                        ]
                                    }
                                },
                                {
                                    "continuationItemRenderer": {
                                        "continuationEndpoint": {
                                            "continuationCommand": { "token": "SEARCH_PAGE_2" }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        });

        let parsed_dym = parse_search_response(&dym_fixture);
        assert_eq!(parsed_dym.estimated_results, Some(12_345));
        assert_eq!(parsed_dym.items.len(), 1);
        assert_eq!(parsed_dym.continuation.as_deref(), Some("SEARCH_PAGE_2"));
        assert_eq!(
            parsed_dym.spelling,
            Some(SearchSpellingCorrection {
                corrected_query: "rust async".into(),
                original_query: None,
                is_auto_corrected: false,
            })
        );

        let showing_fixture = json!({
            "contents": {
                "twoColumnSearchResultsRenderer": {
                    "primaryContents": {
                        "sectionListRenderer": {
                            "contents": [{
                                "itemSectionRenderer": {
                                    "contents": [{
                                        "showingResultsForRenderer": {
                                            "correctedQuery": { "runs": [{ "text": "fasttube" }] },
                                            "originalQuery": { "simpleText": "fasttbe" }
                                        }
                                    }]
                                }
                            }]
                        }
                    }
                }
            }
        });
        let parsed_showing = parse_search_response(&showing_fixture);
        assert_eq!(
            parsed_showing.spelling,
            Some(SearchSpellingCorrection {
                corrected_query: "fasttube".into(),
                original_query: Some("fasttbe".into()),
                is_auto_corrected: true,
            })
        );
    }

    #[test]
    fn parse_channel_browse_dynamic_tabs_sort_chips_and_lru_alias_cache() {
        let cid = "UC1234567890123456789012";
        let fixture = json!({
            "metadata": {
                "channelMetadataRenderer": {
                    "externalId": cid,
                    "title": "Veritasium",
                    "vanityChannelUrl": "https://www.youtube.com/@veritasium"
                }
            },
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [
                        {
                            "tabRenderer": {
                                "title": "Videos",
                                "selected": true,
                                "endpoint": {
                                    "commandMetadata": { "webCommandMetadata": { "url": "/@veritasium/videos" } },
                                    "browseEndpoint": { "browseId": cid, "params": "EgZ2aWRlb3M" }
                                },
                                "content": {
                                    "richGridRenderer": {
                                        "header": {
                                            "feedFilterChipBarRenderer": {
                                                "contents": [
                                                    {
                                                        "chipCloudChipRenderer": {
                                                            "text": { "simpleText": "Latest" },
                                                            "isSelected": true,
                                                            "navigationEndpoint": {
                                                                "continuationCommand": { "token": "SORT_LATEST" }
                                                            }
                                                        }
                                                    },
                                                    {
                                                        "chipCloudChipRenderer": {
                                                            "text": { "simpleText": "Popular" },
                                                            "isSelected": false,
                                                            "navigationEndpoint": {
                                                                "continuationCommand": { "token": "SORT_POPULAR" }
                                                            }
                                                        }
                                                    },
                                                    {
                                                        "chipCloudChipRenderer": {
                                                            "text": { "simpleText": "Oldest" },
                                                            "isSelected": false,
                                                            "navigationEndpoint": {
                                                                "continuationCommand": { "token": "SORT_OLDEST" }
                                                            }
                                                        }
                                                    }
                                                ]
                                            }
                                        },
                                        "contents": [
                                            {
                                                "richItemRenderer": {
                                                    "content": {
                                                        "videoRenderer": {
                                                            "videoId": "chanVID1234",
                                                            "title": { "simpleText": "Physics Explained" },
                                                            "lengthText": { "simpleText": "22:10" }
                                                        }
                                                    }
                                                }
                                            },
                                            {
                                                "continuationItemRenderer": {
                                                    "continuationEndpoint": {
                                                        "continuationCommand": { "token": "CHAN_PAGE_2" }
                                                    }
                                                }
                                            }
                                        ]
                                    }
                                }
                            }
                        },
                        {
                            "tabRenderer": {
                                "title": "Shorts",
                                "endpoint": {
                                    "commandMetadata": { "webCommandMetadata": { "url": "/@veritasium/shorts" } },
                                    "browseEndpoint": { "browseId": cid, "params": "EgZzaG9ydHM" }
                                }
                            }
                        },
                        {
                            "tabRenderer": {
                                "title": "Live",
                                "endpoint": {
                                    "commandMetadata": { "webCommandMetadata": { "url": "/@veritasium/streams" } },
                                    "browseEndpoint": { "browseId": cid, "params": "EgdzdHJlYW1z" }
                                }
                            }
                        },
                        {
                            "tabRenderer": {
                                "title": "Posts",
                                "endpoint": {
                                    "commandMetadata": { "webCommandMetadata": { "url": "/@veritasium/posts" } },
                                    "browseEndpoint": { "browseId": cid, "params": "EgVwb3N0cw" }
                                }
                            }
                        },
                        {
                            "tabRenderer": {
                                "title": "Podcasts",
                                "endpoint": {
                                    "commandMetadata": { "webCommandMetadata": { "url": "/@veritasium/podcasts" } },
                                    "browseEndpoint": { "browseId": cid, "params": "Eghwb2RjYXN0cw" }
                                }
                            }
                        },
                        {
                            "tabRenderer": {
                                "title": "Releases",
                                "endpoint": {
                                    "commandMetadata": { "webCommandMetadata": { "url": "/@veritasium/releases" } },
                                    "browseEndpoint": { "browseId": cid, "params": "EghyZWxlYXNlcw" }
                                }
                            }
                        }
                    ]
                }
            }
        });

        let parsed = parse_channel_browse_response(&fixture);
        assert_eq!(parsed.channel_id.as_deref(), Some(cid));
        assert_eq!(parsed.handle.as_deref(), Some("@veritasium"));
        assert_eq!(parsed.tabs.len(), 6);
        assert_eq!(parsed.tabs[0].kind, ChannelTabKind::Videos);
        assert_eq!(parsed.tabs[1].kind, ChannelTabKind::Shorts);
        assert_eq!(parsed.tabs[2].kind, ChannelTabKind::Live);
        assert_eq!(parsed.tabs[3].kind, ChannelTabKind::Posts);
        assert_eq!(parsed.tabs[4].kind, ChannelTabKind::Podcasts);
        assert_eq!(parsed.tabs[5].kind, ChannelTabKind::Releases);
        assert_eq!(parsed.sort_chips.len(), 3);
        assert_eq!(parsed.sort_chips[1].continuation, "SORT_POPULAR");
        assert_eq!(parsed.sort_chips[2].continuation, "SORT_OLDEST");
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].channel, "Veritasium");
        assert_eq!(parsed.continuation.as_deref(), Some("CHAN_PAGE_2"));

        // ChannelIdCache deduplicates @handle, URL, and UC... ID under one canonical key.
        let mut cache: ChannelIdCache<String> = ChannelIdCache::new(2, Duration::from_secs(600));
        let now = Instant::now();
        cache.insert(
            cid,
            &["@Veritasium", "https://www.youtube.com/@veritasium"],
            "cached-veritasium".into(),
            now,
        );
        assert_eq!(cache.len(), 1);
        assert!(cache.same_channel("@veritasium", cid));
        assert_eq!(
            cache.get("https://www.youtube.com/@VERITASIUM", now),
            Some(&"cached-veritasium".to_owned())
        );
        assert_eq!(cache.get(cid, now), Some(&"cached-veritasium".to_owned()));

        // LRU eviction when capacity (2) is exceeded.
        let cid2 = "UC2222222222222222222222";
        let cid3 = "UC3333333333333333333333";
        cache.insert(cid2, &["@second"], "two".into(), now);
        // Touch cid so cid2 becomes LRU victim.
        let _ = cache.get("@veritasium", now);
        cache.insert(cid3, &["@third"], "three".into(), now);
        assert_eq!(cache.len(), 2);
        assert!(cache.get("@second", now).is_none());
        assert!(cache.get("@veritasium", now).is_some());
        assert!(cache.get("@third", now).is_some());
    }

    #[test]
    fn parse_playlist_browse_response_handles_vl_playlist_and_continuation() {
        assert_eq!(
            playlist_browse_id("PL1234567890").unwrap(),
            "VLPL1234567890"
        );
        assert_eq!(playlist_browse_id("WL").unwrap(), "VLWL");

        let fixture = json!({
            "header": {
                "playlistHeaderRenderer": {
                    "title": { "simpleText": "Rust Talks" },
                    "ownerText": { "simpleText": "RustConf" },
                    "descriptionText": { "simpleText": "Best talks" },
                    "numVideosText": { "runs": [{ "text": "42" }, { "text": " videos" }] }
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
                                                                "videoId": "plVID123456",
                                                                "setVideoId": "SET_VID_1",
                                                                "title": { "runs": [{ "text": "Async Rust Talk" }] },
                                                                "shortBylineText": { "runs": [{ "text": "RustConf" }] },
                                                                "lengthText": { "simpleText": "35:12" },
                                                                "isPlayable": true
                                                            }
                                                        },
                                                        {
                                                            "playlistVideoRenderer": {
                                                                "videoId": "delVID12345",
                                                                "title": { "simpleText": "[Deleted video]" },
                                                                "isPlayable": false
                                                            }
                                                        },
                                                        {
                                                            "continuationItemRenderer": {
                                                                "continuationEndpoint": {
                                                                    "continuationCommand": { "token": "PL_CONT_2" }
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

        let parsed = parse_playlist_browse_response(&fixture, "VLPL1234567890");
        assert_eq!(parsed.playlist_id, "PL1234567890");
        assert_eq!(parsed.title, "Rust Talks");
        assert_eq!(parsed.owner, "RustConf");
        assert_eq!(parsed.description.as_deref(), Some("Best talks"));
        assert_eq!(parsed.total_videos, Some(42));
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.set_video_ids, vec![Some("SET_VID_1".to_owned())]);
        assert_eq!(parsed.continuation.as_deref(), Some("PL_CONT_2"));
    }

    #[test]
    fn parse_watch_next_and_comments_extracts_related_heatmap_and_badges() {
        let watch_fixture = json!({
            "contents": {
                "twoColumnWatchNextResults": {
                    "results": {
                        "results": {
                            "contents": [
                                {
                                    "videoPrimaryInfoRenderer": {
                                        "viewCount": {
                                            "videoViewCountRenderer": {
                                                "viewCount": { "simpleText": "38,412 views" },
                                                "shortViewCount": { "simpleText": "38K views" }
                                            }
                                        },
                                        "relativeDateText": { "simpleText": "3 hours ago" },
                                        "videoActions": {
                                            "menuRenderer": {
                                                "topLevelButtons": [{
                                                    "segmentedLikeDislikeButtonViewModel": {
                                                        "likeButtonViewModel": {
                                                            "likeButtonViewModel": {
                                                                "toggleButtonViewModel": {
                                                                    "toggleButtonViewModel": {
                                                                        "defaultButtonViewModel": {
                                                                            "buttonViewModel": { "title": "18K" }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }]
                                            }
                                        }
                                    }
                                },
                                {
                                    "videoSecondaryInfoRenderer": {
                                        "owner": {
                                            "videoOwnerRenderer": {
                                                "title": { "runs": [{ "text": "Ciência Todo Dia" }] },
                                                "thumbnail": { "thumbnails": [{ "url": "https://yt3.ggpht.com/avatar=s88", "width": 88 }] },
                                                "navigationEndpoint": {
                                                    "commandMetadata": {
                                                        "webCommandMetadata": { "url": "/@CienciaTodoDia" }
                                                    },
                                                    "browseEndpoint": {
                                                        "browseId": "UC1234567890123456789012",
                                                        "canonicalBaseUrl": "/@CienciaTodoDia"
                                                    }
                                                },
                                                "badges": [{
                                                    "metadataBadgeRenderer": {
                                                        "style": "BADGE_STYLE_TYPE_VERIFIED"
                                                    }
                                                }],
                                                "subscriberCountText": { "simpleText": "1.5M subscribers" }
                                            }
                                        },
                                        "attributedDescription": {
                                            "content": "Check https://example.com now!",
                                            "commandRuns": [{
                                                "startIndex": 6,
                                                "length": 19,
                                                "onTap": {
                                                    "innertubeCommand": {
                                                        "urlEndpoint": { "url": "https://example.com" }
                                                    }
                                                }
                                            }]
                                        }
                                    }
                                },
                                {
                                    "itemSectionRenderer": {
                                        "sectionIdentifier": "comment-item-section",
                                        "contents": [{
                                            "continuationItemRenderer": {
                                                "continuationEndpoint": {
                                                    "continuationCommand": { "token": "COMMENTS_CONT_1" }
                                                }
                                            }
                                        }]
                                    }
                                }
                            ]
                        }
                    },
                    "secondaryResults": {
                        "secondaryResults": {
                            "results": [
                                {
                                    "itemSectionRenderer": {
                                        "header": {
                                            "chipCloudRenderer": {
                                                "chips": [
                                                    {
                                                        "chipCloudChipRenderer": {
                                                            "text": { "simpleText": "All" },
                                                            "isSelected": true
                                                        }
                                                    },
                                                    {
                                                        "chipCloudChipRenderer": {
                                                            "text": { "simpleText": "From Channel" },
                                                            "isSelected": false,
                                                            "navigationEndpoint": {
                                                                "continuationCommand": { "token": "REL_CHIP_CHANNEL" }
                                                            }
                                                        }
                                                    }
                                                ]
                                            }
                                        },
                                        "contents": [
                                            {
                                                "compactVideoRenderer": {
                                                    "videoId": "relVID12345",
                                                    "title": { "simpleText": "Next Recommended Video" },
                                                    "shortBylineText": { "simpleText": "Creator" },
                                                    "lengthText": { "simpleText": "09:15" }
                                                }
                                            },
                                            {
                                                "continuationItemRenderer": {
                                                    "continuationEndpoint": {
                                                        "continuationCommand": { "token": "REL_NEXT_PAGE" }
                                                    }
                                                }
                                            }
                                        ]
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
                                    "markersMap": [{
                                        "key": "HEATSEEKER",
                                        "value": {
                                            "heatmap": {
                                                "heatmapRenderer": {
                                                    "heatMarkers": [
                                                        {
                                                            "heatMarkerRenderer": {
                                                                "timeRangeStartMillis": "0",
                                                                "markerDurationMillis": "10000",
                                                                "heatMarkerIntensityScoreNormalized": 0.5
                                                            }
                                                        },
                                                        {
                                                            "heatMarkerRenderer": {
                                                                "timeRangeStartMillis": "10000",
                                                                "markerDurationMillis": "10000",
                                                                "heatMarkerIntensityScoreNormalized": 2.0
                                                            }
                                                        }
                                                    ]
                                                }
                                            }
                                        }
                                    }]
                                }
                            }
                        }
                    }
                }
            }
        });

        let watch = parse_watch_next_response(&watch_fixture);
        assert_eq!(watch.like_count_text.as_deref(), Some("18K"));
        assert_eq!(watch.view_count_text.as_deref(), Some("38K"));
        assert_eq!(watch.published_date_text.as_deref(), Some("3 hours ago"));
        assert_eq!(
            watch.channel_id.as_deref(),
            Some("UC1234567890123456789012")
        );
        assert_eq!(
            watch.channel_url.as_deref(),
            Some("https://www.youtube.com/@CienciaTodoDia")
        );
        assert_eq!(watch.channel_title.as_deref(), Some("Ciência Todo Dia"));
        assert_eq!(
            watch.channel_avatar_url.as_deref(),
            Some("https://yt3.ggpht.com/avatar=s88")
        );
        assert!(watch.channel_verified);
        assert_eq!(
            watch.subscriber_count_text.as_deref(),
            Some("1.5M subscribers")
        );
        assert_eq!(watch.description_runs.len(), 3);
        assert_eq!(
            watch.description_runs[1].url.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            watch.comments_continuation.as_deref(),
            Some("COMMENTS_CONT_1")
        );
        assert_eq!(watch.related_videos.len(), 1);
        assert_eq!(watch.related_chips.len(), 2);
        assert_eq!(
            watch.related_chips[1].continuation.as_deref(),
            Some("REL_CHIP_CHANNEL")
        );
        assert_eq!(watch.related_continuation.as_deref(), Some("REL_NEXT_PAGE"));
        // Verify heatmap intensity normalization when max > 1.0 (0.5 / 2.0 = 0.25, 2.0 / 2.0 = 1.0).
        assert_eq!(watch.heatmap.len(), 2);
        assert_eq!(watch.heatmap[0].start_ms, 0);
        assert!((watch.heatmap[0].intensity - 0.25).abs() < 1e-5);
        assert!((watch.heatmap[1].intensity - 1.0).abs() < 1e-5);

        let comments_fixture = json!({
            "frameworkUpdates": {
                "entityBatchUpdate": {
                    "mutations": [
                        {
                            "entityKey": "ck_1",
                            "payload": {
                                "commentEntityPayload": {
                                    "properties": {
                                        "commentId": "UgxPinned1",
                                        "content": { "content": "Welcome everyone!" },
                                        "publishedTime": "1 day ago"
                                    },
                                    "author": {
                                        "displayName": "@Creator",
                                        "avatarThumbnailUrl": "https://yt3.ggpht.com/avatar.jpg",
                                        "isCreator": true,
                                        "sponsorBadgeUrl": "https://yt3.ggpht.com/member-badge.png"
                                    },
                                    "toolbar": {
                                        "likeCountNotliked": "412",
                                        "replyCount": "7",
                                        "heartActiveTooltip": "Hearted by Creator"
                                    }
                                }
                            }
                        },
                        {
                            "entityKey": "tst_1",
                            "payload": {
                                "engagementToolbarStateEntityPayload": {
                                    "heartState": "TOOLBAR_HEART_STATE_HEARTED"
                                }
                            }
                        }
                    ]
                }
            },
            "onResponseReceivedEndpoints": [{
                "reloadContinuationItemsCommand": {
                    "continuationItems": [
                        {
                            "commentsHeaderRenderer": {
                                "countText": { "runs": [{ "text": "250 Comments" }] },
                                "sortMenu": {
                                    "sortFilterSubMenuRenderer": {
                                        "subMenuItems": [
                                            { "serviceEndpoint": { "continuationCommand": { "token": "TOP_TOK" } } },
                                            { "serviceEndpoint": { "continuationCommand": { "token": "NEW_TOK" } } }
                                        ]
                                    }
                                }
                            }
                        },
                        {
                            "commentThreadRenderer": {
                                "commentViewModel": {
                                    "commentViewModel": {
                                        "commentId": "UgxPinned1",
                                        "commentKey": "ck_1",
                                        "toolbarStateKey": "tst_1",
                                        "pinnedText": "Pinned by Creator"
                                    }
                                },
                                "replies": {
                                    "commentRepliesRenderer": {
                                        "contents": [{
                                            "continuationItemRenderer": {
                                                "continuationEndpoint": {
                                                    "continuationCommand": { "token": "REPLIES_TOK_1" }
                                                }
                                            }
                                        }]
                                    }
                                }
                            }
                        },
                        {
                            "continuationItemRenderer": {
                                "continuationEndpoint": {
                                    "continuationCommand": { "token": "COMMENTS_PAGE_2" }
                                }
                            }
                        }
                    ]
                }
            }]
        });

        let comments = parse_comments_response(&comments_fixture);
        assert_eq!(comments.total_count_text.as_deref(), Some("250 Comments"));
        assert_eq!(comments.top_sort_continuation.as_deref(), Some("TOP_TOK"));
        assert_eq!(
            comments.newest_sort_continuation.as_deref(),
            Some("NEW_TOK")
        );
        assert_eq!(comments.continuation.as_deref(), Some("COMMENTS_PAGE_2"));
        assert_eq!(comments.comments.len(), 1);
        let c = &comments.comments[0];
        assert_eq!(c.id, "UgxPinned1");
        assert_eq!(c.author, "@Creator");
        assert_eq!(c.text, "Welcome everyone!");
        assert_eq!(c.like_count_text.as_deref(), Some("412"));
        assert!(c.is_pinned);
        assert!(c.is_hearted);
        assert!(c.is_owner);
        assert!(c.is_member);
        assert_eq!(c.reply_count, 7);
        assert_eq!(c.replies_continuation.as_deref(), Some("REPLIES_TOK_1"));
    }

    #[test]
    fn zero_quota_mutation_payload_builders_produce_expected_contracts() {
        let add = build_playlist_add_video_payload("WL", "dQw4w9WgXcQ").unwrap();
        assert_eq!(add["playlistId"], "WL");
        assert_eq!(add["actions"][0]["action"], "ACTION_ADD_VIDEO");
        assert_eq!(add["actions"][0]["addedVideoId"], "dQw4w9WgXcQ");

        let remove_with_set =
            build_playlist_remove_video_payload("WL", Some("SET_99"), "dQw4w9WgXcQ").unwrap();
        assert_eq!(
            remove_with_set["actions"][0]["action"],
            "ACTION_REMOVE_VIDEO"
        );
        assert_eq!(remove_with_set["actions"][0]["setVideoId"], "SET_99");

        let remove_by_vid = build_playlist_remove_video_payload("WL", None, "dQw4w9WgXcQ").unwrap();
        assert_eq!(
            remove_by_vid["actions"][0]["action"],
            "ACTION_REMOVE_VIDEO_BY_VIDEO_ID"
        );
        assert_eq!(remove_by_vid["actions"][0]["removedVideoId"], "dQw4w9WgXcQ");

        let (like_ep, like_body) =
            build_rate_video_payload("dQw4w9WgXcQ", VideoRating::Like).unwrap();
        assert_eq!(like_ep, "like/like");
        assert_eq!(like_body["target"]["videoId"], "dQw4w9WgXcQ");

        let (dislike_ep, _) =
            build_rate_video_payload("dQw4w9WgXcQ", VideoRating::Dislike).unwrap();
        assert_eq!(dislike_ep, "like/dislike");

        let (remove_ep, _) = build_rate_video_payload("dQw4w9WgXcQ", VideoRating::Remove).unwrap();
        assert_eq!(remove_ep, "like/removelike");

        let (sub_ep, sub_body) =
            build_subscription_payload("UC1234567890123456789012", true).unwrap();
        assert_eq!(sub_ep, "subscription/subscribe");
        assert_eq!(sub_body["channelIds"][0], "UC1234567890123456789012");

        let (unsub_ep, _) = build_subscription_payload("UC1234567890123456789012", false).unwrap();
        assert_eq!(unsub_ep, "subscription/unsubscribe");

        let fb = build_feedback_payload("AB9zfpJ_FEEDBACK_TOKEN").unwrap();
        assert_eq!(fb["feedbackTokens"][0], "AB9zfpJ_FEEDBACK_TOKEN");
        assert_eq!(fb["isFeedbackTokenUnencrypted"], false);

        // Invalid identifiers are rejected cleanly.
        assert!(build_playlist_add_video_payload("bad id!", "dQw4w9WgXcQ").is_err());
        assert!(build_rate_video_payload("short", VideoRating::Like).is_err());
        assert!(build_subscription_payload("not-a-channel", true).is_err());
        assert!(build_feedback_payload("   ").is_err());
    }

    #[test]
    fn restricted_mode_context_and_pref_cookie_enable_safety_mode() {
        let default_ctx = InnerTubeContext::default();
        assert!(!default_ctx.restricted_mode);
        assert!(default_ctx.to_context_json().get("user").is_none());

        let safe_ctx = InnerTubeContext::default().with_restricted_mode(true);
        let ctx_json = safe_ctx.to_context_json();
        assert_eq!(ctx_json["user"]["enableSafetyMode"], true);

        assert_eq!(
            apply_restricted_mode_pref_cookie("SID=abc; SAPISID=xyz", false),
            "SID=abc; SAPISID=xyz"
        );
        assert_eq!(
            apply_restricted_mode_pref_cookie("", true),
            "PREF=f2=8000000"
        );
        assert_eq!(
            apply_restricted_mode_pref_cookie("SID=abc", true),
            "SID=abc; PREF=f2=8000000"
        );
        assert_eq!(
            apply_restricted_mode_pref_cookie("SID=abc; PREF=tz=America.Sao_Paulo&hl=pt", true),
            "SID=abc; PREF=tz=America.Sao_Paulo&hl=pt&f2=8000000"
        );
        assert_eq!(
            apply_restricted_mode_pref_cookie("PREF=f2=4000000&hl=en", true),
            "PREF=f2=c000000&hl=en"
        );
    }

    #[test]
    fn parses_notifications_menu_and_builds_notification_mutation_payloads() {
        let fixture = json!({
            "actions": [{
                "openPopupAction": {
                    "popup": {
                        "multiPageMenuRenderer": {
                            "sections": [
                                {
                                    "multiPageMenuNotificationSectionRenderer": {
                                        "notificationSectionTitle": { "simpleText": "Importantes" },
                                        "items": [
                                            {
                                                "notificationRenderer": {
                                                    "notificationId": "notif_1",
                                                    "shortMessage": { "simpleText": "Rust Channel uploaded: Async Deep Dive" },
                                                    "channelName": { "simpleText": "Rust Channel" },
                                                    "sentTimeText": { "simpleText": "2 hours ago" },
                                                    "read": false,
                                                    "thumbnail": { "thumbnails": [{ "url": "https://yt3.ggpht.com/avatar1=s68" }] },
                                                    "videoThumbnail": { "thumbnails": [{ "url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg", "width": 480 }] },
                                                    "navigationEndpoint": {
                                                        "watchEndpoint": { "videoId": "dQw4w9WgXcQ" }
                                                    },
                                                    "recordClickEndpoint": {
                                                        "recordNotificationInteractionsEndpoint": {
                                                            "serializedInteractionsRequest": "TOKEN_MARK_READ_1"
                                                        }
                                                    },
                                                    "menu": {
                                                        "menuRenderer": {
                                                            "items": [
                                                                {
                                                                    "menuServiceItemRenderer": {
                                                                        "serviceEndpoint": {
                                                                            "feedbackEndpoint": { "feedbackToken": "FB_HIDE_1" },
                                                                            "recordNotificationInteractionsEndpoint": {
                                                                                "serializedInteractionsRequest": "TOKEN_HIDE_INT_1"
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            ]
                                                        }
                                                    }
                                                }
                                            },
                                            {
                                                "notificationRenderer": {
                                                    "notificationId": "notif_2",
                                                    "text": { "runs": [{ "text": "Already read notification" }] },
                                                    "timeStatusText": { "simpleText": "1 day ago" },
                                                    "read": true,
                                                    "thumbnail": {
                                                        "thumbnails": [{ "url": "https://yt3.ggpht.com/avatar2=s68" }],
                                                        "accessibility": { "accessibilityData": { "label": "Channel Two" } }
                                                    },
                                                    "navigationEndpoint": {
                                                        "reelWatchEndpoint": { "videoId": "9bZkp7q19f0" }
                                                    },
                                                    "contextualMenu": {
                                                        "menuRenderer": {
                                                            "items": [
                                                                {
                                                                    "menuServiceItemRenderer": {
                                                                        "serviceEndpoint": {
                                                                            "commandExecutorCommand": {
                                                                                "commands": [
                                                                                    {
                                                                                        "feedbackEndpoint": { "feedbackToken": "FB_HIDE_2" }
                                                                                    },
                                                                                    {
                                                                                        "recordNotificationInteractionsEndpoint": {
                                                                                            "serializedInteractionsRequest": "TOKEN_HIDE_INT_2"
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
                                            {
                                                "continuationItemRenderer": {
                                                    "continuationEndpoint": {
                                                        "getNotificationMenuEndpoint": {
                                                            "ctoken": "NOTIF_CTOK_PAGE_2"
                                                        }
                                                    }
                                                }
                                            }
                                        ]
                                    }
                                }
                            ]
                        }
                    }
                }
            }]
        });

        let menu = parse_notifications_menu_response(&fixture);
        assert_eq!(menu.items.len(), 2);
        assert_eq!(menu.unread_count, 1);
        assert_eq!(menu.continuation.as_deref(), Some("NOTIF_CTOK_PAGE_2"));

        let n1 = &menu.items[0];
        assert_eq!(n1.id, "notif_1");
        assert_eq!(n1.title, "Rust Channel uploaded: Async Deep Dive");
        assert_eq!(n1.channel, "Rust Channel");
        assert_eq!(n1.section_title.as_deref(), Some("Importantes"));
        assert_eq!(
            n1.thumbnail_url.as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
        );
        assert_eq!(
            n1.channel_avatar_url.as_deref(),
            Some("https://yt3.ggpht.com/avatar1=s68")
        );
        assert_eq!(
            n1.video_url.as_deref(),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
        );
        assert_eq!(n1.published_text, "2 hours ago");
        assert!(!n1.is_read);
        assert_eq!(n1.mark_read_token.as_deref(), Some("TOKEN_MARK_READ_1"));
        assert_eq!(n1.hide_feedback_token.as_deref(), Some("FB_HIDE_1"));
        assert_eq!(
            n1.hide_interaction_token.as_deref(),
            Some("TOKEN_HIDE_INT_1")
        );

        let n2 = &menu.items[1];
        assert_eq!(n2.channel, "Channel Two");
        assert_eq!(n2.published_text, "1 day ago");
        assert!(n2.is_read);
        assert_eq!(
            n2.video_url.as_deref(),
            Some("https://www.youtube.com/shorts/9bZkp7q19f0")
        );
        assert_eq!(n2.hide_feedback_token.as_deref(), Some("FB_HIDE_2"));
        assert_eq!(
            n2.hide_interaction_token.as_deref(),
            Some("TOKEN_HIDE_INT_2")
        );

        let ctok_payload = build_notifications_menu_payload_with_continuation(Some("CTOK_1"));
        assert_eq!(
            ctok_payload["notificationsMenuRequestType"],
            "NOTIFICATIONS_MENU_REQUEST_TYPE_INBOX"
        );
        assert_eq!(ctok_payload["ctoken"], "CTOK_1");
        assert!(build_notifications_menu_payload().get("ctoken").is_none());

        let mark_payload = build_mark_notifications_read_payload("TOKEN_MARK_READ_1").unwrap();
        assert_eq!(
            mark_payload["serializedRecordNotificationInteractionsRequest"],
            "TOKEN_MARK_READ_1"
        );
        assert!(build_mark_notifications_read_payload("   ").is_err());

        assert_eq!(InnerTubeNotifyLevel::All.label(true), "Todas");
        assert_eq!(InnerTubeNotifyLevel::All.label(false), "All");
        assert_eq!(
            InnerTubeNotifyLevel::Personalized.label(true),
            "Personalizadas"
        );
        assert_eq!(InnerTubeNotifyLevel::None.label(false), "None");

        let notify_payload = build_channel_notify_payload("CgASAhAA").unwrap();
        assert_eq!(notify_payload["params"], "CgASAhAA");
        assert!(build_channel_notify_payload("").is_err());
    }

    #[test]
    fn parses_ytcfg_fields_and_sends_brand_headers_and_follows_lazy_notifications_continuation() {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;

        let html = r#"<script>ytcfg.set({"INNERTUBE_CLIENT_VERSION":"2.20261121.01.00","VISITOR_DATA":"CgtWSVNJVE9SXzE%3D","INNERTUBE_API_KEY":"AIzaSyMockApiKey123","DELEGATED_SESSION_ID":"104829384756102938","SESSION_INDEX":2});</script>"#;
        let fields = parse_ytcfg_session_fields(html);
        assert_eq!(fields.client_version.as_deref(), Some("2.20261121.01.00"));
        assert_eq!(fields.visitor_data.as_deref(), Some("CgtWSVNJVE9SXzE%3D"));
        assert_eq!(fields.api_key.as_deref(), Some("AIzaSyMockApiKey123"));
        assert_eq!(
            fields.delegated_session_id.as_deref(),
            Some("104829384756102938")
        );
        assert_eq!(fields.auth_user_index, Some(2));

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}/youtubei/v1");

        let server = std::thread::spawn(move || {
            let mut captured = Vec::new();
            for req_idx in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                let mut headers = Vec::new();
                let mut content_len = 0usize;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        break;
                    }
                    if let Some((k, v)) = trimmed.split_once(':')
                        && k.trim().eq_ignore_ascii_case("content-length")
                    {
                        content_len = v.trim().parse().unwrap_or(0);
                    }
                    headers.push(trimmed.to_owned());
                }
                let mut body = vec![0u8; content_len];
                reader.read_exact(&mut body).unwrap();
                let body_str = String::from_utf8_lossy(&body).into_owned();
                captured.push((request_line, headers, body_str));

                let resp_json = if req_idx == 0 {
                    json!({
                        "actions": [{
                            "openPopupAction": {
                                "popup": {
                                    "multiPageMenuRenderer": {
                                        "sections": [{
                                            "multiPageMenuNotificationSectionRenderer": {
                                                "continuations": [{
                                                    "nextContinuationData": { "continuation": "LAZY_NOTIF_CTOK" }
                                                }]
                                            }
                                        }]
                                    }
                                }
                            }
                        }]
                    })
                } else {
                    json!({
                        "actions": [{
                            "appendContinuationItemsAction": {
                                "continuationItems": [{
                                    "notificationRenderer": {
                                        "notificationId": "notif_lazy_1",
                                        "shortMessage": { "simpleText": "Hayashii Lives: GTA 6" },
                                        "sentTimeText": { "simpleText": "há 15 horas" },
                                        "read": false,
                                        "navigationEndpoint": {
                                            "watchEndpoint": { "videoId": "dQw4w9WgXcQ" }
                                        }
                                    }
                                }]
                            }
                        }]
                    })
                }
                .to_string();
                let http_resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp_json.len(),
                    resp_json
                );
                stream.write_all(http_resp.as_bytes()).unwrap();
            }
            captured
        });

        let mut session = sample_web_session(1_700_000_000, Some(1_900_000_000));
        session.client_version = fields.client_version.unwrap();
        session.visitor_data = fields.visitor_data;
        session.api_key = fields.api_key;
        session.delegated_session_id = fields.delegated_session_id;
        session.auth_user_index = fields.auth_user_index.unwrap();

        let ctx = InnerTubeContext::from_language(UiLanguage::PtBr, None);
        let cancel = Cancellation::default();
        let menu = fetch_notifications_menu_at(&base_url, &ctx, &mut session, None, None, &cancel)
            .unwrap();
        assert_eq!(menu.items.len(), 1);
        assert_eq!(menu.items[0].id, "notif_lazy_1");
        assert_eq!(menu.items[0].title, "Hayashii Lives: GTA 6");

        let captured = server.join().unwrap();
        assert_eq!(captured.len(), 2);
        assert!(captured[0].0.contains("key=AIzaSyMockApiKey123"));
        let hdrs_lower = captured[0].1.join("\n").to_lowercase();
        assert!(hdrs_lower.contains("x-youtube-bootstrap-logged-in: true"));
        assert!(hdrs_lower.contains("x-goog-pageid: 104829384756102938"));
        assert!(hdrs_lower.contains("x-goog-authuser: 2"));
        let body0: Value = serde_json::from_str(&captured[0].2).unwrap();
        assert_eq!(
            body0["context"]["user"]["onBehalfOfUser"],
            "104829384756102938"
        );
        assert_eq!(body0["context"]["user"]["lockedSafetyMode"], false);
        let body1: Value = serde_json::from_str(&captured[1].2).unwrap();
        assert_eq!(body1["ctoken"], "LAZY_NOTIF_CTOK");
    }

    #[test]
    fn parses_shorts_sequence_from_browse_and_reel_watch_sequence_fixtures() {
        let browse_fixture = json!({
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "richGridRenderer": {
                                    "contents": [
                                        {
                                            "richItemRenderer": {
                                                "content": {
                                                    "shortsLockupViewModel": {
                                                        "entityId": "shorts-shelf-item-dQw4w9WgXcQ",
                                                        "overlayMetadata": {
                                                            "primaryText": { "content": "Fast Rust Trick" },
                                                            "secondaryText": { "content": "120K views" }
                                                        },
                                                        "onTap": {
                                                            "innertubeCommand": {
                                                                "reelWatchEndpoint": { "videoId": "dQw4w9WgXcQ" }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        },
                                        {
                                            "continuationItemRenderer": {
                                                "continuationEndpoint": {
                                                    "continuationCommand": { "token": "SHORTS_BROWSE_P2" }
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
        });

        let browse_page = parse_shorts_sequence_response(&browse_fixture);
        assert_eq!(browse_page.items.len(), 1);
        assert_eq!(browse_page.items[0].title, "Fast Rust Trick");
        assert_eq!(
            browse_page.items[0].url,
            "https://www.youtube.com/shorts/dQw4w9WgXcQ"
        );
        assert_eq!(browse_page.metadata[0].kind, SearchResultKind::Short);
        assert_eq!(
            browse_page.continuation.as_deref(),
            Some("SHORTS_BROWSE_P2")
        );

        let seq_fixture = json!({
            "entries": [{
                "command": {
                    "reelWatchEndpoint": {
                        "videoId": "9bZkp7q19f0",
                        "thumbnail": { "thumbnails": [{ "url": "https://i.ytimg.com/vi/9bZkp7q19f0/oar2.jpg", "width": 405 }] },
                        "overlay": {
                            "reelPlayerOverlayRenderer": {
                                "reelPlayerHeaderSupportedRenderers": {
                                    "reelPlayerHeaderRenderer": {
                                        "reelTitleText": { "simpleText": "Sequence Short #1" },
                                        "channelTitleText": { "simpleText": "@ShortCreator" }
                                    }
                                }
                            }
                        }
                    }
                }
            }],
            "continuation": "REEL_SEQ_NEXT_2"
        });

        let seq_page = parse_shorts_sequence_response(&seq_fixture);
        assert_eq!(seq_page.items.len(), 1);
        assert_eq!(seq_page.items[0].title, "Sequence Short #1");
        assert_eq!(seq_page.items[0].channel, "@ShortCreator");
        assert_eq!(seq_page.continuation.as_deref(), Some("REEL_SEQ_NEXT_2"));

        assert_eq!(build_shorts_browse_payload(None)["browseId"], "FEshorts");
        assert_eq!(
            build_shorts_browse_payload(Some("TOK_1"))["continuation"],
            "TOK_1"
        );
        assert_eq!(
            build_reel_watch_sequence_payload(None, None)["inputType"],
            "REEL_WATCH_INPUT_TYPE_SEEDLESS"
        );
        assert_eq!(
            build_reel_watch_sequence_payload(Some("REEL_SEQ_NEXT_2"), None)["continuation"],
            "REEL_SEQ_NEXT_2"
        );
    }

    #[test]
    fn parses_explore_hub_categories_and_videos() {
        assert_eq!(ExploreCategory::ALL.len(), 6);
        assert_eq!(ExploreCategory::Trending.label(true), "Em alta");
        assert_eq!(ExploreCategory::Trending.label(false), "Trending");
        assert_eq!(ExploreCategory::Gaming.label(true), "Jogos");
        assert_eq!(
            ExploreCategory::Trending.browse_id_and_params().0,
            "FEtrending"
        );
        assert_eq!(
            ExploreCategory::Podcasts.browse_id_and_params().0,
            "FEpodcasts"
        );

        let explore_fixture = json!({
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "selected": true,
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "itemSectionRenderer": {
                                            "contents": [
                                                {
                                                    "videoRenderer": {
                                                        "videoId": "dQw4w9WgXcQ",
                                                        "title": { "runs": [{ "text": "Trending Hit #1" }] },
                                                        "ownerText": { "runs": [{ "text": "Top Artist" }] },
                                                        "lengthText": { "simpleText": "3:33" },
                                                        "viewCountText": { "simpleText": "1.5M views" }
                                                    }
                                                },
                                                {
                                                    "continuationItemRenderer": {
                                                        "continuationEndpoint": {
                                                            "continuationCommand": { "token": "EXPLORE_NEXT_2" }
                                                        }
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
        });

        let page = parse_explore_response(ExploreCategory::Music, &explore_fixture);
        assert_eq!(page.category, ExploreCategory::Music);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].title, "Trending Hit #1");
        assert_eq!(page.continuation.as_deref(), Some("EXPLORE_NEXT_2"));
    }

    #[test]
    fn parses_channel_community_posts_polls_quizzes_and_upcoming_timestamps() {
        let poll_attachment = json!({
            "pollRenderer": {
                "type": "POLL_TYPE_QUIZ",
                "choices": [
                    {
                        "text": { "simpleText": "serde_json" },
                        "voteRatio": 0.85,
                        "isCorrect": true,
                        "explanation": { "simpleText": "Standard JSON crate in Rust." }
                    },
                    {
                        "text": { "simpleText": "reqwest" },
                        "votePercentage": "15%",
                        "isCorrect": false
                    }
                ]
            }
        });
        let backstage_post = json!({
            "backstagePostThreadRenderer": {
                "post": {
                    "backstagePostRenderer": {
                        "postId": "UgkxPostQuiz1",
                        "authorText": { "runs": [{ "text": "Dev Channel" }] },
                        "authorThumbnail": { "thumbnails": [{ "url": "https://yt3.ggpht.com/dev=s68" }] },
                        "publishedTimeText": { "runs": [{ "text": "3 days ago" }] },
                        "contentText": { "runs": [{ "text": "Which crate handles JSON?" }] },
                        "voteCount": { "simpleText": "1.2K" },
                        "actionButtons": {
                            "commentActionButtonsRenderer": {
                                "replyButton": {
                                    "buttonRenderer": { "text": { "simpleText": "84" } }
                                }
                            }
                        },
                        "backstageAttachment": poll_attachment
                    }
                }
            }
        });
        let channel_fixture = json!({
            "metadata": {
                "channelMetadataRenderer": {
                    "externalId": "UC1234567890123456789012",
                    "title": "Dev Channel"
                }
            },
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "title": "Posts",
                            "selected": true,
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "itemSectionRenderer": {
                                            "contents": [
                                                backstage_post,
                                                {
                                                    "videoRenderer": {
                                                        "videoId": "dQw4w9WgXcQ",
                                                        "title": { "simpleText": "Upcoming Rust Keynote" },
                                                        "upcomingEventData": {
                                                            "startTime": "1800000000",
                                                            "upcomingEventText": { "simpleText": "Premieres Jan 15" }
                                                        }
                                                    }
                                                },
                                                {
                                                    "lockupViewModel": {
                                                        "contentId": "PLpodcast12345678",
                                                        "contentType": "LOCKUP_CONTENT_TYPE_PODCAST",
                                                        "metadata": {
                                                            "lockupMetadataViewModel": {
                                                                "title": { "content": "Rustacean Podcast" }
                                                            }
                                                        }
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
        });

        let page = parse_channel_browse_response(&channel_fixture);
        assert_eq!(page.community_posts.len(), 1);
        let post = &page.community_posts[0];
        assert_eq!(post.id, "UgkxPostQuiz1");
        assert_eq!(post.author, "Dev Channel");
        assert_eq!(post.published_text, "3 days ago");
        assert_eq!(post.content_text, "Which crate handles JSON?");
        assert_eq!(post.like_count_text.as_deref(), Some("1.2K"));
        assert_eq!(post.comment_count_text.as_deref(), Some("84"));
        assert!(post.is_quiz);
        assert_eq!(post.poll_choices.len(), 2);
        assert_eq!(post.poll_choices[0].text, "serde_json");
        assert_eq!(post.poll_choices[0].is_correct, Some(true));
        assert!((post.poll_choices[0].vote_ratio.unwrap() - 0.85).abs() < 1e-4);
        assert_eq!(
            post.poll_choices[0].explanation.as_deref(),
            Some("Standard JSON crate in Rust.")
        );
        assert!((post.poll_choices[1].vote_ratio.unwrap() - 0.15).abs() < 1e-4);

        let upcoming_node = json!({ "upcomingEventData": { "startTime": "1800000000" } });
        assert_eq!(
            parse_upcoming_start_unix(&upcoming_node),
            Some(1_800_000_000)
        );
        assert_eq!(parse_upcoming_start_unix(&json!({})), None);

        // Both the upcoming video and the podcast lockup should be extracted.
        assert_eq!(page.items.len(), 2);
        assert!(page.metadata[0].badges.iter().any(|b| b == "Upcoming"));
        assert_eq!(
            page.metadata[0].published_time_text.as_deref(),
            Some("Premieres Jan 15")
        );
        assert_eq!(page.items[1].title, "Rustacean Podcast");
        assert_eq!(page.metadata[1].kind, SearchResultKind::Playlist);
    }

    #[test]
    fn parse_lockup_video_vm_detects_shorts_url_and_kind() {
        let lockup = serde_json::json!({
            "contentId": "dQw4w9WgXcQ",
            "contentType": "LOCKUP_CONTENT_TYPE_VIDEO",
            "rendererContext": {
                "commandContext": {
                    "onTap": {
                        "innertubeCommand": {
                            "reelWatchEndpoint": {
                                "videoId": "dQw4w9WgXcQ"
                            }
                        }
                    }
                }
            },
            "contentImage": {
                "thumbnailViewModel": {
                    "image": {
                        "sources": [{ "url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg" }]
                    },
                    "overlays": [{
                        "thumbnailOverlayBadgeViewModel": {
                            "thumbnailBadges": [{
                                "thumbnailBadgeViewModel": {
                                    "badgeStyle": "THUMBNAIL_OVERLAY_BADGE_STYLE_SHORTS",
                                    "text": "SHORTS"
                                }
                            }]
                        }
                    }]
                }
            },
            "metadata": {
                "lockupMetadataViewModel": {
                    "title": { "content": "Quick Short Clip" }
                }
            }
        });

        let result = parse_lockup_video_vm(&lockup).expect("short lockup parsed");
        assert_eq!(
            result.video.url,
            "https://www.youtube.com/shorts/dQw4w9WgXcQ"
        );
        assert_eq!(result.metadata.kind, SearchResultKind::Short);
    }

    #[test]
    fn playlist_rename_privacy_delete_and_move_payload_builders_validate_contracts() {
        let rename = build_playlist_rename_payload("PLtest1234", "My Rust Mix").unwrap();
        assert_eq!(rename["playlistId"], "PLtest1234");
        assert_eq!(rename["actions"][0]["action"], "ACTION_SET_PLAYLIST_NAME");
        assert_eq!(rename["actions"][0]["playlistName"], "My Rust Mix");
        assert!(build_playlist_rename_payload("PLtest1234", "   ").is_err());

        let priv_unlisted = build_playlist_privacy_payload("PLtest1234", "unlisted").unwrap();
        assert_eq!(
            priv_unlisted["actions"][0]["action"],
            "ACTION_SET_PLAYLIST_PRIVACY"
        );
        assert_eq!(priv_unlisted["actions"][0]["playlistPrivacy"], "UNLISTED");
        assert!(build_playlist_privacy_payload("PLtest1234", "FRIENDS_ONLY").is_err());

        let del = build_playlist_delete_payload("PLtest1234").unwrap();
        assert_eq!(del["playlistId"], "PLtest1234");
        assert!(build_playlist_delete_payload("WL").is_err());
        assert!(build_playlist_delete_payload("LL").is_err());

        let move_after =
            build_playlist_move_video_payload("PLtest1234", "SET_B", Some("SET_A")).unwrap();
        assert_eq!(
            move_after["actions"][0]["action"],
            "ACTION_MOVE_VIDEO_AFTER"
        );
        assert_eq!(move_after["actions"][0]["setVideoId"], "SET_B");
        assert_eq!(
            move_after["actions"][0]["movedSetVideoIdPredecessor"],
            "SET_A"
        );

        let move_top = build_playlist_move_video_payload("PLtest1234", "SET_B", None).unwrap();
        assert_eq!(move_top["actions"][0]["action"], "ACTION_MOVE_VIDEO_AFTER");
        assert_eq!(move_top["actions"][0]["setVideoId"], "SET_B");
        assert!(
            move_top["actions"][0]
                .get("movedSetVideoIdPredecessor")
                .is_none()
        );

        assert!(build_playlist_move_video_payload("PLtest1234", "SET_A", Some("SET_A")).is_err());
        assert!(build_playlist_move_video_payload("PLtest1234", "   ", None).is_err());
    }

    #[test]
    fn parse_lockup_playlist_and_watch_next_extract_upload_dates() {
        let lockup = serde_json::json!({
            "contentId": "TMZ00000001",
            "contentType": "LOCKUP_CONTENT_TYPE_VIDEO",
            "metadata": {
                "lockupMetadataViewModel": {
                    "title": { "content": "Harvey Levin GTA VI" },
                    "metadata": {
                        "contentMetadataViewModel": {
                            "metadataRows": [
                                { "metadataParts": [{ "text": { "content": "TMZ Clips" } }] },
                                {
                                    "metadataParts": [
                                        {
                                            "text": { "content": "105\u{00a0}mil" },
                                            "accessibilityLabel": "105 mil visualizações",
                                            "leadingIcon": { "name": "PLAY_ARROW_OUTLINED" }
                                        },
                                        {
                                            "text": { "content": "há\u{00a0}10\u{00a0}h" },
                                            "accessibilityLabel": "há 10 horas"
                                        }
                                    ]
                                }
                            ]
                        }
                    }
                }
            }
        });
        let res = super::parse_lockup_video_vm(&lockup).expect("lockup parsed");
        assert_eq!(res.video.channel, "TMZ Clips");
        assert_eq!(res.metadata.view_count, Some(105_000));
        assert_eq!(res.metadata.view_count_text.as_deref(), Some("105 mil"));
        assert_eq!(res.metadata.published_time_text.as_deref(), Some("há 10 h"));

        let playlist_fixture = serde_json::json!({
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
                                                                "videoId": "plVID123456",
                                                                "title": { "simpleText": "Old Playlist Item" },
                                                                "shortBylineText": { "simpleText": "Chan A" },
                                                                "videoInfo": { "simpleText": "12\u{00a0}mil visualizações • há\u{00a0}7\u{00a0}meses" }
                                                            }
                                                        },
                                                        {
                                                            "lockupViewModel": {
                                                                "contentId": "plVID999999",
                                                                "contentType": "LOCKUP_CONTENT_TYPE_VIDEO",
                                                                "metadata": {
                                                                    "lockupMetadataViewModel": {
                                                                        "title": { "content": "Modern Playlist Lockup" },
                                                                        "metadata": {
                                                                            "contentMetadataViewModel": {
                                                                                "metadataRows": [{
                                                                                    "metadataParts": [
                                                                                        { "text": { "content": "Chan B" } },
                                                                                        { "text": { "content": "854K views" } },
                                                                                        { "text": { "content": "1d ago" } }
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
        let detail = parse_playlist_browse_response(&playlist_fixture, "PLtest");
        assert_eq!(detail.items.len(), 2);
        assert_eq!(detail.metadata[0].view_count, Some(12_000));
        assert_eq!(
            detail.metadata[0].published_time_text.as_deref(),
            Some("há 7 meses")
        );
        assert_eq!(detail.metadata[1].view_count, Some(854_000));
        assert_eq!(
            detail.metadata[1].published_time_text.as_deref(),
            Some("1d ago")
        );

        let watch_next = serde_json::json!({
            "contents": {
                "twoColumnWatchNextResults": {
                    "results": {
                        "results": {
                            "contents": [{
                                "videoPrimaryInfoRenderer": {
                                    "relativeDateText": { "simpleText": "há\u{00a0}2\u{00a0}semanas" }
                                }
                            }]
                        }
                    }
                }
            }
        });
        let parsed_next = parse_watch_next_response(&watch_next);
        assert_eq!(
            parsed_next.published_date_text.as_deref(),
            Some("há 2 semanas")
        );
    }
}
