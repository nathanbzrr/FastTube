use crate::process::Cancellation;
use crate::search::{HomeChannel, HomeFeed, SearchResult, SearchResultKind, Video, VideoMetadata};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
#[cfg(not(test))]
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zeroize::Zeroize;

pub const COMMENT_SCOPE: &str = "https://www.googleapis.com/auth/youtube.force-ssl";
const SUBSCRIPTION_PAGE_SIZE: usize = 50;
const MAX_SUBSCRIPTIONS: usize = 500;
const MAX_UPLOADS_PER_CHANNEL: usize = 5;
const MAX_PLAYLISTS: usize = 100;
const MAX_PLAYLIST_VIDEOS: usize = 200;
const PROGRESS_CHANNEL_BATCH: usize = 5;
const PROGRESS_LATER_CHANNEL_BATCH: usize = 15;
const SUBSCRIPTION_WORKER_COUNT: usize = 24;
const RSS_MAX_BYTES: usize = 512 * 1024;
const RSS_TIMEOUT: Duration = Duration::from_secs(6);
const RSS_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const SUBSCRIPTIONS_CACHE_MAX_BYTES: usize = 4 * 1024 * 1024;
const SUBSCRIPTIONS_CACHE_VERSION: u32 = 1;
static SUBSCRIPTIONS_CACHE_TEMP_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
#[cfg(not(test))]
const MAX_SESSION_BYTES: usize = 64 * 1024;
#[cfg(not(test))]
static SESSION_LOCK: Mutex<()> = Mutex::new(());
#[cfg(not(test))]
static SESSION_TEMP_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Clone, Deserialize, Serialize)]
pub struct TokenSession {
    pub access_token: String,
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    expires_at: u64,
}

impl Drop for TokenSession {
    fn drop(&mut self) {
        self.access_token.zeroize();
        if let Some(token) = &mut self.refresh_token {
            token.zeroize();
        }
    }
}

impl TokenSession {
    #[cfg(test)]
    pub(crate) fn test_session(access_token: &str) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            access_token: access_token.to_owned(),
            refresh_token: Some("test-refresh-token".into()),
            scopes: vec![COMMENT_SCOPE.to_owned()],
            expires_at: now.saturating_add(3600),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_session_without_comment_scope(access_token: &str) -> Self {
        let mut session = Self::test_session(access_token);
        session.scopes.clear();
        session
    }

    fn from_response(json: &Value, previous: Option<&TokenSession>) -> Result<Self, String> {
        let access_token = json["access_token"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 16 * 1024)
            .ok_or_else(|| "O Google não retornou um token de acesso.".to_owned())?
            .to_owned();
        let refresh_token = json["refresh_token"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 16 * 1024)
            .map(str::to_owned)
            .or_else(|| previous.and_then(|session| session.refresh_token.clone()));
        let scopes = json["scope"]
            .as_str()
            .map(|scopes| scopes.split_whitespace().map(str::to_owned).collect())
            .or_else(|| previous.map(|session| session.scopes.clone()))
            .unwrap_or_default();
        let expires_in = json["expires_in"]
            .as_u64()
            .unwrap_or(3600)
            .clamp(60, 31_536_000);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "Relógio do sistema inválido.".to_owned())?
            .as_secs();
        Ok(Self {
            access_token,
            refresh_token,
            scopes,
            expires_at: now.saturating_add(expires_in),
        })
    }

    pub fn has_comment_scope(&self) -> bool {
        self.scopes.iter().any(|scope| scope == COMMENT_SCOPE)
    }

    pub fn refresh_if_needed(&mut self, cancel: &Cancellation) -> Result<(), String> {
        if cancel.cancelled() {
            return Err("Login cancelado.".into());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "Relógio do sistema inválido.".to_owned())?
            .as_secs();
        if self.expires_at > now.saturating_add(60) {
            return Ok(());
        }
        let refresh_token = self
            .refresh_token
            .as_deref()
            .ok_or_else(|| "A sessão Google expirou. Entre novamente.".to_owned())?;
        let (client_id, client_secret) = client_credentials()?;
        let response = http_client()?
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("client_id", client_id.as_str()),
                ("client_secret", client_secret.as_str()),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .map_err(|_| "Não foi possível renovar a sessão Google.")?;
        if cancel.cancelled() {
            return Err("Login cancelado.".into());
        }
        if !response.status().is_success() {
            return Err("Google recusou a renovação da sessão. Entre novamente.".into());
        }
        let json: Value = response
            .json()
            .map_err(|_| "Resposta de renovação inválida.")?;
        let refreshed = Self::from_response(&json, Some(self))?;
        *self = refreshed;
        Ok(())
    }
}

#[cfg(not(test))]
fn session_path() -> Result<PathBuf, String> {
    Ok(crate::settings::settings_path()
        .map_err(|_| "Não foi possível localizar a pasta de configurações.")?
        .with_file_name("google-session.bin"))
}

#[cfg(not(test))]
pub fn load_session() -> Result<Option<TokenSession>, String> {
    let _guard = SESSION_LOCK
        .lock()
        .map_err(|_| "Falha ao acessar a sessão local.")?;
    let file = match std::fs::File::open(session_path()?) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Não foi possível ler a sessão Google local.".into()),
    };
    let mut encrypted = Vec::new();
    file.take((MAX_SESSION_BYTES + 1) as u64)
        .read_to_end(&mut encrypted)
        .map_err(|_| "Não foi possível ler a sessão Google local.")?;
    if encrypted.is_empty() || encrypted.len() > MAX_SESSION_BYTES {
        return Err("Arquivo da sessão Google inválido.".into());
    }
    let mut plain = dpapi_unprotect(&encrypted)?;
    let parsed = serde_json::from_slice(&plain);
    plain.zeroize();
    parsed
        .map(Some)
        .map_err(|_| "Sessão Google local inválida.".into())
}

#[cfg(not(test))]
pub fn save_session(session: &TokenSession) -> Result<(), String> {
    let _guard = SESSION_LOCK
        .lock()
        .map_err(|_| "Falha ao acessar a sessão local.")?;
    let path = session_path()?;
    let parent = path
        .parent()
        .ok_or("Não foi possível localizar a pasta de configurações.")?;
    std::fs::create_dir_all(parent).map_err(|_| "Não foi possível salvar a sessão Google.")?;
    let mut plain =
        serde_json::to_vec(session).map_err(|_| "Não foi possível codificar a sessão Google.")?;
    let encrypted_result = dpapi_protect(&plain);
    plain.zeroize();
    let encrypted = encrypted_result?;
    let (temporary, mut file) = loop {
        let id = SESSION_TEMP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".fasttube-google-session-{}-{id}.tmp",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err("Não foi possível salvar a sessão Google.".into()),
        }
    };
    let result = (|| {
        file.write_all(&encrypted)
            .map_err(|_| "Não foi possível salvar a sessão Google.")?;
        file.sync_all()
            .map_err(|_| "Não foi possível salvar a sessão Google.")?;
        drop(file);
        atomic_replace(&temporary, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(all(windows, not(test)))]
fn atomic_replace(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let from_wide: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to_wide: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe {
        MoveFileExW(
            from_wide.as_ptr(),
            to_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err("Não foi possível salvar a sessão Google.".into())
    } else {
        Ok(())
    }
}
#[cfg(all(not(windows), not(test)))]
fn atomic_replace(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    std::fs::rename(from, to).map_err(|_| "Não foi possível salvar a sessão Google.".into())
}

#[cfg(not(test))]
pub fn delete_session() -> Result<(), String> {
    let _guard = SESSION_LOCK
        .lock()
        .map_err(|_| "Falha ao acessar a sessão local.")?;
    match std::fs::remove_file(session_path()?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Não foi possível remover a sessão Google local.".into()),
    }
}

#[cfg(windows)]
fn dpapi_protect(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
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
    if ok == 0 {
        return Err("O Windows não conseguiu proteger a sessão Google.".into());
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as *mut _);
    }
    Ok(result)
}
#[cfg(not(windows))]
fn dpapi_protect(_: &[u8]) -> Result<Vec<u8>, String> {
    Err("Persistência segura da sessão está disponível apenas no Windows.".into())
}

#[cfg(windows)]
fn dpapi_unprotect(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
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
    if ok == 0 {
        return Err("O Windows não conseguiu descriptografar a sessão Google.".into());
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
fn dpapi_unprotect(_: &[u8]) -> Result<Vec<u8>, String> {
    Err("Persistência segura da sessão está disponível apenas no Windows.".into())
}

fn client_path() -> Result<PathBuf, String> {
    Ok(crate::settings::settings_path()
        .map_err(|_| "Não foi possível localizar a pasta de configurações.")?
        .with_file_name("oauth-client.json"))
}
fn client_credentials() -> Result<(String, String), String> {
    let data = std::fs::read(client_path()?).map_err(
        |_| "Coloque o JSON do cliente OAuth em %LOCALAPPDATA%\\FastTube\\oauth-client.json.",
    )?;
    if data.len() > 64 * 1024 {
        return Err("Arquivo de cliente OAuth inválido.".into());
    }
    let json: Value =
        serde_json::from_slice(&data).map_err(|_| "JSON do cliente OAuth inválido.")?;
    let installed = &json["installed"];
    let id = installed["client_id"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or("O JSON deve ser de um cliente OAuth para computador.")?;
    let secret = installed["client_secret"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or("O JSON do cliente OAuth não contém client_secret.")?;
    Ok((id.to_owned(), secret.to_owned()))
}
fn random_urlsafe() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| "Não foi possível gerar dados seguros para o login.")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
fn callback_query(request: &str, expected_state: &str) -> Option<Result<String, String>> {
    let mut parts = request.lines().next()?.split_whitespace();
    if parts.next()? != "GET" {
        return None;
    }
    let path = parts.next()?;
    let url = url::Url::parse(&format!("http://127.0.0.1{path}")).ok()?;
    if url.path() != "/oauth2callback" {
        return None;
    }
    let pairs: Vec<_> = url.query_pairs().collect();
    let states: Vec<_> = pairs.iter().filter(|(key, _)| key == "state").collect();
    if states.len() != 1 || states[0].1 != expected_state {
        return None;
    }
    if pairs.iter().any(|(key, _)| key == "error") {
        return Some(Err("A autorização Google foi recusada ou cancelada.".into()));
    }
    let codes: Vec<_> = pairs.iter().filter(|(key, _)| key == "code").collect();
    if codes.len() != 1 || codes[0].1.is_empty() {
        return None;
    }
    Some(Ok(codes[0].1.to_string()))
}

pub fn authorize(cancel: &Cancellation) -> Result<TokenSession, String> {
    let (client_id, client_secret) = client_credentials()?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|_| "Não foi possível abrir o retorno local do login.")?;
    listener
        .set_nonblocking(true)
        .map_err(|_| "Falha no retorno local do login.")?;
    let redirect = format!(
        "http://127.0.0.1:{}/oauth2callback",
        listener
            .local_addr()
            .map_err(|_| "Falha no retorno local.")?
            .port()
    );
    let state = random_urlsafe()?;
    let verifier = random_urlsafe()?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
        .map_err(|_| "URL OAuth inválida.")?;
    url.query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", &redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", COMMENT_SCOPE)
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    #[cfg(windows)]
    std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", url.as_str()])
        .spawn()
        .map_err(|_| "Não foi possível abrir o navegador padrão.")?;
    #[cfg(not(windows))]
    return Err("Login OAuth disponível apenas no Windows nesta versão.".into());
    let code = wait_for_callback(&listener, &state, cancel, Duration::from_secs(180))?;
    if cancel.cancelled() {
        return Err("Login cancelado.".into());
    }
    let response = http_client()?
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("code", code.as_str()),
            ("code_verifier", verifier.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .map_err(|_| "Não foi possível trocar a autorização por um token.")?;
    if !response.status().is_success() {
        return Err(
            "Google recusou a autorização. Confira a credencial e os usuários de teste.".into(),
        );
    }
    let json: Value = response
        .json()
        .map_err(|_| "Resposta de autorização inválida.")?;
    TokenSession::from_response(&json, None)
}

fn wait_for_callback(
    listener: &TcpListener,
    state: &str,
    cancel: &Cancellation,
    timeout: Duration,
) -> Result<String, String> {
    let started = Instant::now();
    loop {
        if cancel.cancelled() {
            return Err("Login cancelado.".into());
        }
        if started.elapsed() >= timeout {
            return Err("O login expirou; tente novamente.".into());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buffer = [0u8; 8192];
                let count = stream.read(&mut buffer).unwrap_or(0);
                let result = callback_query(&String::from_utf8_lossy(&buffer[..count]), state);
                let (status, message) = if result.is_some() {
                    ("200 OK", "Pode fechar esta aba e voltar ao FastTube.")
                } else {
                    ("400 Bad Request", "Resposta invalida.")
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{message}",
                    message.len()
                );
                if let Some(result) = result {
                    return result;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(_) => return Err("Falha ao receber o retorno do login.".into()),
        }
    }
}

fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Não foi possível iniciar a conexão segura.".into())
}

fn api_at(
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
    resource: &str,
    params: &[(&str, &str)],
) -> Result<Value, String> {
    let response = client
        .get(format!("{base_url}/{resource}"))
        .bearer_auth(token)
        .query(params)
        .send()
        .map_err(|_| "Falha de conexão com a API do YouTube.")?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Sess\u{00e3}o expirada. Saia da conta e entre novamente.".into());
    }
    let body: Value = match response.json() {
        Ok(body) => body,
        Err(_) if !status.is_success() => Value::Null,
        Err(_) => return Err("Resposta inv\u{00e1}lida da API do YouTube.".into()),
    };
    if !status.is_success() {
        return Err(api_failure_message(status.as_u16(), &body));
    }
    Ok(body)
}

fn api(
    client: &reqwest::blocking::Client,
    token: &str,
    resource: &str,
    params: &[(&str, &str)],
) -> Result<Value, String> {
    api_at(
        client,
        "https://www.googleapis.com/youtube/v3",
        token,
        resource,
        params,
    )
}

fn api_post_json_at(
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
    resource: &str,
    params: &[(&str, &str)],
    payload: &Value,
) -> Result<Value, String> {
    let response = client
        .post(format!("{base_url}/{resource}"))
        .bearer_auth(token)
        .query(params)
        .json(payload)
        .send()
        .map_err(|_| "Falha de conexão com a API do YouTube.")?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Sess\u{00e3}o expirada. Saia da conta e entre novamente.".into());
    }
    let body: Value = match response.json() {
        Ok(body) => body,
        Err(_) if !status.is_success() => Value::Null,
        Err(_) if status.is_success() => Value::Null,
        Err(_) => return Err("Resposta inv\u{00e1}lida da API do YouTube.".into()),
    };
    if !status.is_success() {
        return Err(api_failure_message(status.as_u16(), &body));
    }
    Ok(body)
}

fn api_delete_at(
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
    resource: &str,
    params: &[(&str, &str)],
) -> Result<(), String> {
    let response = client
        .delete(format!("{base_url}/{resource}"))
        .bearer_auth(token)
        .query(params)
        .send()
        .map_err(|_| "Falha de conexão com a API do YouTube.")?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Sess\u{00e3}o expirada. Saia da conta e entre novamente.".into());
    }
    if status.is_success() {
        return Ok(());
    }
    let body: Value = response.json().unwrap_or(Value::Null);
    Err(api_failure_message(status.as_u16(), &body))
}

fn api_failure_message(status: u16, body: &Value) -> String {
    let reason = body
        .pointer("/error/errors/0/reason")
        .and_then(Value::as_str);
    let _message = body.pointer("/error/message").and_then(Value::as_str);
    match reason {
        Some("commentsDisabled") => "Os coment\u{00e1}rios est\u{00e3}o desativados neste v\u{00ed}deo.".into(),
        Some("quotaExceeded" | "dailyLimitExceeded" | "rateLimitExceeded") => "Cota da API do YouTube esgotada. Tente novamente mais tarde.".into(),
        Some("authError") => "Sua sess\u{00e3}o Google expirou ou foi revogada. Entre novamente.".into(),
        Some("forbidden" | "insufficientPermissions") => "A conta conectada n\u{00e3}o tem permiss\u{00e3}o para esta consulta. Se o problema continuar, reconecte a conta.".into(),
        Some("subscriptionDuplicate") => "Você já está inscrito neste canal.".into(),
        Some("subscriptionForbidden") => "Não foi possível alterar a inscrição deste canal.".into(),
        Some("videoNotFound" | "commentThreadNotFound" | "playlistNotFound" | "channelNotFound" | "subscriptionNotFound") => "O recurso solicitado n\u{00e3}o foi encontrado no YouTube.".into(),
        Some(reason) => format!("A API do YouTube recusou a consulta (HTTP {status}, motivo: {reason}). Confira permiss\u{00f5}es e cota."),
        None if status == 401 => "Sua sess\u{00e3}o Google expirou ou foi revogada. Entre novamente.".into(),
        None => format!("A API do YouTube recusou a consulta (HTTP {status}). Confira API, permiss\u{00f5}es e cota."),
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CommentOrder {
    #[default]
    Relevance,
    Time,
}

impl CommentOrder {
    pub fn api_value(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::Time => "time",
        }
    }
}

fn sanitize_avatar_url(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 {
        return None;
    }
    let normalized = if let Some(rest) = trimmed.strip_prefix("//") {
        format!("https://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        trimmed.to_owned()
    };
    let parsed = url::Url::parse(&normalized).ok()?;
    (parsed.scheme() == "https" && parsed.host_str().is_some()).then_some(normalized)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub id: String,
    pub author: String,
    pub author_avatar: Option<String>,
    pub text: String,
    pub published: String,
    pub likes: u64,
    pub replies: u64,
}

pub struct CommentsPage {
    pub comments: Vec<Comment>,
    pub next_page: Option<String>,
    pub total_results: Option<u64>,
}

fn parse_top_level_comment(item: &Value) -> Option<Comment> {
    let snippet = item.pointer("/snippet/topLevelComment/snippet")?;
    Some(Comment {
        id: item
            .pointer("/snippet/topLevelComment/id")?
            .as_str()?
            .to_owned(),
        author: snippet["authorDisplayName"]
            .as_str()
            .unwrap_or("Usuário")
            .chars()
            .take(256)
            .collect(),
        author_avatar: sanitize_avatar_url(snippet["authorProfileImageUrl"].as_str()),
        text: snippet["textDisplay"]
            .as_str()
            .or_else(|| snippet["textOriginal"].as_str())?
            .chars()
            .take(8192)
            .collect(),
        published: snippet["publishedAt"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(40)
            .collect(),
        likes: snippet["likeCount"].as_u64().unwrap_or(0),
        replies: item
            .pointer("/snippet/totalReplyCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}

fn parse_reply_comment(item: &Value) -> Option<Comment> {
    let snippet = item.get("snippet")?;
    Some(Comment {
        id: item["id"].as_str()?.to_owned(),
        author: snippet["authorDisplayName"]
            .as_str()
            .unwrap_or("Usu\u{00e1}rio")
            .chars()
            .take(256)
            .collect(),
        author_avatar: sanitize_avatar_url(snippet["authorProfileImageUrl"].as_str()),
        text: snippet["textDisplay"]
            .as_str()
            .or_else(|| snippet["textOriginal"].as_str())?
            .chars()
            .take(8192)
            .collect(),
        published: snippet["publishedAt"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(40)
            .collect(),
        likes: snippet["likeCount"].as_u64().unwrap_or(0),
        replies: 0,
    })
}

fn parse_comments(json: &Value) -> Result<CommentsPage, String> {
    let items = json["items"]
        .as_array()
        .ok_or("Resposta de comentários inválida.")?;
    let comments = items
        .iter()
        .take(50)
        .filter_map(parse_top_level_comment)
        .collect();
    let next_page = json["nextPageToken"]
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= 1024)
        .map(str::to_owned);
    let total_results = json
        .pointer("/pageInfo/totalResults")
        .and_then(Value::as_u64);
    Ok(CommentsPage {
        comments,
        next_page,
        total_results,
    })
}

pub fn comments_page(
    token: &str,
    video_id: &str,
    page: Option<&str>,
    cancel: &Cancellation,
) -> Result<CommentsPage, String> {
    comments_page_ordered(token, video_id, CommentOrder::Relevance, page, cancel)
}

pub fn comments_page_ordered(
    token: &str,
    video_id: &str,
    order: CommentOrder,
    page: Option<&str>,
    cancel: &Cancellation,
) -> Result<CommentsPage, String> {
    if cancel.cancelled() {
        return Err("Consulta de comentários cancelada.".into());
    }
    if !valid_id(video_id) {
        return Err("Este vídeo não possui um ID válido do YouTube.".into());
    }
    let mut params = vec![
        ("part", "snippet"),
        ("videoId", video_id),
        ("maxResults", "50"),
        ("order", order.api_value()),
        ("textFormat", "plainText"),
    ];
    if let Some(page) = page {
        if page.is_empty() || page.len() > 1024 {
            return Err("Página de comentários inválida.".into());
        }
        params.push(("pageToken", page));
    }
    let json = api(&http_client()?, token, "commentThreads", &params).map_err(|error| {
        format!("N\u{00e3}o foi poss\u{00ed}vel carregar coment\u{00e1}rios. {error}")
    })?;
    if cancel.cancelled() {
        return Err("Consulta de comentários cancelada.".into());
    }
    parse_comments(&json)
}

pub fn post_comment(
    token: &str,
    video_id: &str,
    text: &str,
    cancel: &Cancellation,
) -> Result<Comment, String> {
    post_comment_at(
        "https://www.googleapis.com/youtube/v3",
        token,
        video_id,
        text,
        cancel,
    )
}

fn post_comment_at(
    base_url: &str,
    token: &str,
    video_id: &str,
    text: &str,
    cancel: &Cancellation,
) -> Result<Comment, String> {
    if cancel.cancelled() {
        return Err("Envio de comentário cancelado.".into());
    }
    if !valid_id(video_id) {
        return Err("Este vídeo não possui um ID válido do YouTube.".into());
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Escreva um comentário antes de enviar.".into());
    }
    let safe_text: String = trimmed.chars().take(10_000).collect();
    let payload = serde_json::json!({
        "snippet": {
            "videoId": video_id,
            "topLevelComment": {
                "snippet": {
                    "textOriginal": safe_text
                }
            }
        }
    });
    let json = api_post_json_at(
        &http_client()?,
        base_url,
        token,
        "commentThreads",
        &[("part", "snippet")],
        &payload,
    )
    .map_err(|error| format!("Não foi possível publicar o comentário. {error}"))?;
    if cancel.cancelled() {
        return Err("Envio de comentário cancelado.".into());
    }
    parse_top_level_comment(&json).ok_or_else(|| "Resposta de comentário inválida.".to_owned())
}

pub fn post_comment_reply(
    token: &str,
    parent_id: &str,
    text: &str,
    cancel: &Cancellation,
) -> Result<Comment, String> {
    post_comment_reply_at(
        "https://www.googleapis.com/youtube/v3",
        token,
        parent_id,
        text,
        cancel,
    )
}

fn post_comment_reply_at(
    base_url: &str,
    token: &str,
    parent_id: &str,
    text: &str,
    cancel: &Cancellation,
) -> Result<Comment, String> {
    if cancel.cancelled() {
        return Err("Envio de resposta cancelado.".into());
    }
    if !valid_id(parent_id) {
        return Err("Este comentário não possui um ID válido.".into());
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Escreva uma resposta antes de enviar.".into());
    }
    let safe_text: String = trimmed.chars().take(10_000).collect();
    let payload = serde_json::json!({
        "snippet": {
            "parentId": parent_id,
            "textOriginal": safe_text
        }
    });
    let json = api_post_json_at(
        &http_client()?,
        base_url,
        token,
        "comments",
        &[("part", "snippet")],
        &payload,
    )
    .map_err(|error| format!("Não foi possível publicar a resposta. {error}"))?;
    if cancel.cancelled() {
        return Err("Envio de resposta cancelado.".into());
    }
    parse_reply_comment(&json).ok_or_else(|| "Resposta de comentário inválida.".to_owned())
}

fn parse_comment_replies(json: &Value) -> Result<CommentsPage, String> {
    let items = json["items"]
        .as_array()
        .ok_or("Resposta de respostas inv\u{00e1}lida.")?;
    let comments = items
        .iter()
        .take(100)
        .filter_map(parse_reply_comment)
        .collect();
    let next_page = json["nextPageToken"]
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= 1024)
        .map(str::to_owned);
    Ok(CommentsPage {
        comments,
        next_page,
        total_results: None,
    })
}

pub fn comment_replies_page(
    token: &str,
    parent_id: &str,
    page: Option<&str>,
    cancel: &Cancellation,
) -> Result<CommentsPage, String> {
    if cancel.cancelled() {
        return Err("Consulta de respostas cancelada.".into());
    }
    if !valid_id(parent_id) {
        return Err("Este coment\u{00e1}rio n\u{00e3}o possui um ID v\u{00e1}lido.".into());
    }
    let mut params = vec![
        ("part", "snippet"),
        ("parentId", parent_id),
        ("maxResults", "100"),
        ("textFormat", "plainText"),
    ];
    if let Some(page) = page {
        if page.is_empty() || page.len() > 1024 {
            return Err("P\u{00e1}gina de respostas inv\u{00e1}lida.".into());
        }
        params.push(("pageToken", page));
    }
    let json = api(&http_client()?, token, "comments", &params).map_err(|error| {
        format!("N\u{00e3}o foi poss\u{00ed}vel carregar as respostas. {error}")
    })?;
    if cancel.cancelled() {
        return Err("Consulta de respostas cancelada.".into());
    }
    parse_comment_replies(&json)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscriptionStatus {
    pub channel_id: String,
    pub channel_title: Option<String>,
    pub subscription_id: Option<String>,
}

pub fn resolve_channel_id(
    token: &str,
    target: &crate::search::ChannelTarget,
    cancel: &Cancellation,
) -> Result<String, String> {
    resolve_channel_id_at(
        "https://www.googleapis.com/youtube/v3",
        token,
        target,
        cancel,
    )
}

fn resolve_channel_id_at(
    base_url: &str,
    token: &str,
    target: &crate::search::ChannelTarget,
    cancel: &Cancellation,
) -> Result<String, String> {
    if cancel.cancelled() {
        return Err("Consulta cancelada.".into());
    }
    match target {
        crate::search::ChannelTarget::ChannelId(id) => {
            if !valid_id(id) {
                return Err("ID de canal inválido.".into());
            }
            Ok(id.clone())
        }
        crate::search::ChannelTarget::Handle(handle) => {
            let clean = handle.trim().trim_start_matches('@');
            if clean.is_empty() || clean.len() > 128 {
                return Err("Identificador de canal inválido.".into());
            }
            let json = api_at(
                &http_client()?,
                base_url,
                token,
                "channels",
                &[("part", "id"), ("forHandle", clean), ("maxResults", "1")],
            )?;
            if cancel.cancelled() {
                return Err("Consulta cancelada.".into());
            }
            json.pointer("/items/0/id")
                .and_then(Value::as_str)
                .filter(|id| valid_id(id))
                .map(str::to_owned)
                .ok_or_else(|| "Não foi possível identificar o canal no YouTube.".to_owned())
        }
    }
}

pub fn check_subscription(
    token: &str,
    channel_id: &str,
    cancel: &Cancellation,
) -> Result<SubscriptionStatus, String> {
    check_subscription_at(
        "https://www.googleapis.com/youtube/v3",
        token,
        channel_id,
        cancel,
    )
}

fn check_subscription_at(
    base_url: &str,
    token: &str,
    channel_id: &str,
    cancel: &Cancellation,
) -> Result<SubscriptionStatus, String> {
    if cancel.cancelled() {
        return Err("Consulta cancelada.".into());
    }
    if !valid_id(channel_id) {
        return Err("ID de canal inválido.".into());
    }
    let json = api_at(
        &http_client()?,
        base_url,
        token,
        "subscriptions",
        &[
            ("part", "id,snippet"),
            ("mine", "true"),
            ("forChannelId", channel_id),
            ("maxResults", "1"),
        ],
    )?;
    if cancel.cancelled() {
        return Err("Consulta cancelada.".into());
    }
    let subscription_id = json
        .pointer("/items/0/id")
        .and_then(Value::as_str)
        .filter(|id| valid_id(id))
        .map(str::to_owned);
    let channel_title = json
        .pointer("/items/0/snippet/title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_owned);
    Ok(SubscriptionStatus {
        channel_id: channel_id.to_owned(),
        channel_title,
        subscription_id,
    })
}

pub fn subscribe_channel(
    token: &str,
    channel_id: &str,
    cancel: &Cancellation,
) -> Result<SubscriptionStatus, String> {
    subscribe_channel_at(
        "https://www.googleapis.com/youtube/v3",
        token,
        channel_id,
        cancel,
    )
}

fn subscribe_channel_at(
    base_url: &str,
    token: &str,
    channel_id: &str,
    cancel: &Cancellation,
) -> Result<SubscriptionStatus, String> {
    if cancel.cancelled() {
        return Err("Inscrição cancelada.".into());
    }
    if !valid_id(channel_id) {
        return Err("ID de canal inválido.".into());
    }
    let payload = serde_json::json!({
        "snippet": {
            "resourceId": {
                "kind": "youtube#channel",
                "channelId": channel_id
            }
        }
    });
    let json = api_post_json_at(
        &http_client()?,
        base_url,
        token,
        "subscriptions",
        &[("part", "snippet")],
        &payload,
    )?;
    if cancel.cancelled() {
        return Err("Inscrição cancelada.".into());
    }
    let subscription_id = json
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| valid_id(id))
        .map(str::to_owned)
        .ok_or_else(|| "Resposta de inscrição inválida.".to_owned())?;
    let channel_title = json
        .pointer("/snippet/title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_owned);
    Ok(SubscriptionStatus {
        channel_id: channel_id.to_owned(),
        channel_title,
        subscription_id: Some(subscription_id),
    })
}

pub fn unsubscribe_channel(
    token: &str,
    subscription_id: &str,
    channel_id: &str,
    cancel: &Cancellation,
) -> Result<SubscriptionStatus, String> {
    unsubscribe_channel_at(
        "https://www.googleapis.com/youtube/v3",
        token,
        subscription_id,
        channel_id,
        cancel,
    )
}

fn unsubscribe_channel_at(
    base_url: &str,
    token: &str,
    subscription_id: &str,
    channel_id: &str,
    cancel: &Cancellation,
) -> Result<SubscriptionStatus, String> {
    if cancel.cancelled() {
        return Err("Cancelamento de inscrição cancelado.".into());
    }
    if !valid_id(subscription_id) || !valid_id(channel_id) {
        return Err("Identificador de inscrição inválido.".into());
    }
    api_delete_at(
        &http_client()?,
        base_url,
        token,
        "subscriptions",
        &[("id", subscription_id)],
    )?;
    if cancel.cancelled() {
        return Err("Cancelamento de inscrição cancelado.".into());
    }
    Ok(SubscriptionStatus {
        channel_id: channel_id.to_owned(),
        channel_title: None,
        subscription_id: None,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MyChannelProfile {
    pub target: crate::search::ChannelTarget,
    pub title: Option<String>,
    pub avatar_url: Option<String>,
    pub subscriber_count: Option<u64>,
}

fn parse_my_channel_profile(json: &Value) -> Result<MyChannelProfile, String> {
    let item = json
        .pointer("/items/0")
        .ok_or_else(|| "Esta conta Google ainda não possui um canal do YouTube.".to_owned())?;
    let id = item["id"]
        .as_str()
        .filter(|id| valid_id(id))
        .ok_or_else(|| "Esta conta Google ainda não possui um canal do YouTube.".to_owned())?;
    let title = item
        .pointer("/snippet/title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(256).collect());
    let avatar_url = sanitize_avatar_url(
        item.pointer("/snippet/thumbnails/default/url")
            .or_else(|| item.pointer("/snippet/thumbnails/medium/url"))
            .and_then(Value::as_str),
    );
    let hidden_subs = item
        .pointer("/statistics/hiddenSubscriberCount")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let subscriber_count = if hidden_subs {
        None
    } else {
        item.pointer("/statistics/subscriberCount").and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
        })
    };
    Ok(MyChannelProfile {
        target: crate::search::ChannelTarget::ChannelId(id.to_owned()),
        title,
        avatar_url,
        subscriber_count,
    })
}

pub fn my_channel_profile(token: &str, cancel: &Cancellation) -> Result<MyChannelProfile, String> {
    if cancel.cancelled() {
        return Err("Consulta cancelada.".into());
    }
    let json = api(
        &http_client()?,
        token,
        "channels",
        &[
            ("part", "id,snippet,statistics"),
            ("mine", "true"),
            ("maxResults", "1"),
        ],
    )?;
    if cancel.cancelled() {
        return Err("Consulta cancelada.".into());
    }
    parse_my_channel_profile(&json)
}

pub fn create_playlist(
    token: &str,
    title: &str,
    privacy_status: &str,
    cancel: &Cancellation,
) -> Result<(String, String), String> {
    create_playlist_at(
        "https://www.googleapis.com/youtube/v3",
        token,
        title,
        privacy_status,
        cancel,
    )
}

fn create_playlist_at(
    base_url: &str,
    token: &str,
    title: &str,
    privacy_status: &str,
    cancel: &Cancellation,
) -> Result<(String, String), String> {
    if cancel.cancelled() {
        return Err("Criação de playlist cancelada.".into());
    }
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err("Informe um nome para a playlist.".into());
    }
    let safe_title: String = trimmed.chars().take(150).collect();
    let privacy = match privacy_status {
        "public" | "unlisted" | "private" => privacy_status,
        _ => "private",
    };
    let payload = serde_json::json!({
        "snippet": { "title": safe_title },
        "status": { "privacyStatus": privacy }
    });
    let json = api_post_json_at(
        &http_client()?,
        base_url,
        token,
        "playlists",
        &[("part", "snippet,status")],
        &payload,
    )?;
    if cancel.cancelled() {
        return Err("Criação de playlist cancelada.".into());
    }
    let id = json
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| valid_id(id))
        .ok_or("Resposta de playlist inválida.")?
        .to_owned();
    let saved_title = json
        .pointer("/snippet/title")
        .and_then(Value::as_str)
        .unwrap_or(&safe_title)
        .to_owned();
    Ok((id, saved_title))
}

pub fn add_video_to_playlist(
    token: &str,
    playlist_id: &str,
    video_id: &str,
    cancel: &Cancellation,
) -> Result<String, String> {
    add_video_to_playlist_at(
        "https://www.googleapis.com/youtube/v3",
        token,
        playlist_id,
        video_id,
        cancel,
    )
}

fn add_video_to_playlist_at(
    base_url: &str,
    token: &str,
    playlist_id: &str,
    video_id: &str,
    cancel: &Cancellation,
) -> Result<String, String> {
    if cancel.cancelled() {
        return Err("Adição à playlist cancelada.".into());
    }
    if !valid_id(playlist_id) || !valid_id(video_id) {
        return Err("Playlist ou vídeo inválido.".into());
    }
    let payload = serde_json::json!({
        "snippet": {
            "playlistId": playlist_id,
            "resourceId": {
                "kind": "youtube#video",
                "videoId": video_id
            }
        }
    });
    let json = api_post_json_at(
        &http_client()?,
        base_url,
        token,
        "playlistItems",
        &[("part", "snippet")],
        &payload,
    )?;
    if cancel.cancelled() {
        return Err("Adição à playlist cancelada.".into());
    }
    json.get("id")
        .and_then(Value::as_str)
        .filter(|id| valid_id(id))
        .map(str::to_owned)
        .ok_or_else(|| "Resposta ao adicionar item na playlist inválida.".to_owned())
}

fn parse_iso8601_duration(raw: &str) -> String {
    let Some(rest) = raw.strip_prefix("PT") else {
        return String::new();
    };
    let mut hours = 0u64;
    let mut minutes = 0u64;
    let mut seconds = 0u64;
    let mut current = 0u64;
    for ch in rest.chars() {
        if let Some(digit) = ch.to_digit(10) {
            current = current.saturating_mul(10).saturating_add(u64::from(digit));
        } else {
            match ch {
                'H' => hours = current,
                'M' => minutes = current,
                'S' => seconds = current,
                _ => return String::new(),
            }
            current = 0;
        }
    }
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else if minutes > 0 || seconds > 0 {
        format!("{minutes}:{seconds:02}")
    } else {
        String::new()
    }
}

fn parse_liked_video_item(item: &Value) -> Option<SearchResult> {
    let id = item.get("id")?.as_str()?;
    if !valid_id(id) {
        return None;
    }
    let snippet = item.get("snippet")?;
    let title = snippet["title"].as_str()?;
    let date = snippet["publishedAt"].as_str().unwrap_or("").to_owned();
    let thumbnail = snippet
        .pointer("/thumbnails/medium/url")
        .or_else(|| snippet.pointer("/thumbnails/default/url"))
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("https://i.ytimg.com/"))
        .unwrap_or("")
        .to_owned();
    let channel_id = snippet["channelId"]
        .as_str()
        .filter(|id| valid_id(id))
        .map(str::to_owned);
    let duration = item
        .pointer("/contentDetails/duration")
        .and_then(Value::as_str)
        .map(parse_iso8601_duration)
        .unwrap_or_default();
    let view_count = item
        .pointer("/statistics/viewCount")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<u64>().ok());
    Some(SearchResult {
        video: Video {
            title: title.chars().take(512).collect(),
            channel: snippet["channelTitle"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(256)
                .collect(),
            url: format!("https://www.youtube.com/watch?v={id}"),
            thumbnail,
            duration,
        },
        metadata: VideoMetadata {
            view_count,
            upload_date: (!date.is_empty()).then_some(date),
            description: snippet["description"]
                .as_str()
                .map(|d| d.chars().take(2048).collect()),
            channel_id,
            ..Default::default()
        },
    })
}

pub fn liked_videos_feed(token: &str, cancel: &Cancellation) -> Result<HomeFeed, String> {
    let client = http_client()?;
    liked_videos_feed_with(cancel, |params| api(&client, token, "videos", params))
}

fn liked_videos_feed_with<F>(cancel: &Cancellation, mut fetch: F) -> Result<HomeFeed, String>
where
    F: FnMut(&[(&str, &str)]) -> Result<Value, String>,
{
    let mut items = Vec::new();
    let mut page = None;
    loop {
        if cancel.cancelled() {
            return Err("Consulta cancelada.".into());
        }
        let mut params = vec![
            ("part", "snippet,contentDetails,statistics"),
            ("myRating", "like"),
            ("maxResults", "50"),
        ];
        if let Some(token) = page.as_deref() {
            params.push(("pageToken", token));
        }
        let response = fetch(&params)?;
        if cancel.cancelled() {
            return Err("Consulta cancelada.".into());
        }
        let rows = response["items"]
            .as_array()
            .ok_or("Lista de vídeos com Gostei inválida.")?;
        items.extend(
            rows.iter()
                .take(MAX_PLAYLIST_VIDEOS - items.len())
                .filter_map(parse_liked_video_item),
        );
        let next = response["nextPageToken"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 1024)
            .map(str::to_owned);
        if items.len() >= MAX_PLAYLIST_VIDEOS || next.is_none() {
            break;
        }
        page = next;
    }
    Ok(HomeFeed {
        items,
        playlists: Vec::new(),
        channels: Vec::new(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoRating {
    Like,
    Dislike,
    None,
}

impl VideoRating {
    fn api_value(self) -> &'static str {
        match self {
            Self::Like => "like",
            Self::Dislike => "dislike",
            Self::None => "none",
        }
    }
}

pub fn rate_video(
    token: &str,
    video_id: &str,
    rating: VideoRating,
    cancel: &Cancellation,
) -> Result<(), String> {
    rate_video_at(
        "https://www.googleapis.com/youtube/v3/videos/rate",
        token,
        video_id,
        rating,
        cancel,
    )
}

fn rate_video_at(
    endpoint: &str,
    token: &str,
    video_id: &str,
    rating: VideoRating,
    cancel: &Cancellation,
) -> Result<(), String> {
    if cancel.cancelled() {
        return Err("Avaliação cancelada.".into());
    }
    if !valid_id(video_id) {
        return Err("Este vídeo não possui um ID válido do YouTube.".into());
    }
    let response = http_client()?
        .post(endpoint)
        .bearer_auth(token)
        .query(&[("id", video_id), ("rating", rating.api_value())])
        .header(reqwest::header::CONTENT_LENGTH, 0)
        .body(Vec::new())
        .send()
        .map_err(|_| "Falha de conexão ao avaliar o vídeo.".to_owned())?;
    if cancel.cancelled() {
        return Err("Avaliação cancelada.".into());
    }
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body: Value = response.json().unwrap_or(Value::Null);
    Err(api_failure_message(status.as_u16(), &body))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbuseReportReason {
    SexualContent,
    ViolentOrRepulsive,
    HatefulOrAbusive,
    HarmfulOrDangerous,
    SpamOrMisleading,
}

impl AbuseReportReason {
    pub const ALL: [Self; 5] = [
        Self::SexualContent,
        Self::ViolentOrRepulsive,
        Self::HatefulOrAbusive,
        Self::HarmfulOrDangerous,
        Self::SpamOrMisleading,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::SexualContent => "Conteúdo sexual",
            Self::ViolentOrRepulsive => "Conteúdo violento ou repulsivo",
            Self::HatefulOrAbusive => "Conteúdo abominável ou abusivo",
            Self::HarmfulOrDangerous => "Atos perigosos ou nocivos",
            Self::SpamOrMisleading => "Spam ou enganoso",
        }
    }

    fn fallback_reason_ids(self) -> (&'static str, Option<&'static str>) {
        match self {
            Self::SexualContent => ("N", None),
            Self::ViolentOrRepulsive => ("V", Some("41")),
            Self::HatefulOrAbusive => ("H", None),
            Self::HarmfulOrDangerous => ("V", Some("35")),
            Self::SpamOrMisleading => ("S", None),
        }
    }

    fn label_keywords(self) -> &'static [&'static str] {
        match self {
            Self::SexualContent => &["sexual", "nudez", "nudity"],
            Self::ViolentOrRepulsive => &["violento", "repulsivo", "violent", "repulsive"],
            Self::HatefulOrAbusive => &[
                "abominável",
                "abusivo",
                "ódio",
                "hateful",
                "abusive",
                "harassment",
            ],
            Self::HarmfulOrDangerous => &["perigosos", "nocivos", "harmful", "dangerous"],
            Self::SpamOrMisleading => &["spam", "enganoso", "misleading"],
        }
    }
}

fn resolve_abuse_reason_ids(
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
    reason: AbuseReportReason,
) -> (String, Option<String>) {
    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/videoAbuseReportReasons");
    let response = client
        .get(&url)
        .bearer_auth(token)
        .query(&[("part", "id,snippet"), ("hl", "pt")])
        .send();
    let Ok(response) = response else {
        let (primary, secondary) = reason.fallback_reason_ids();
        return (primary.to_owned(), secondary.map(str::to_owned));
    };
    if !response.status().is_success() {
        let (primary, secondary) = reason.fallback_reason_ids();
        return (primary.to_owned(), secondary.map(str::to_owned));
    }
    let Ok(body) = response.json::<Value>() else {
        let (primary, secondary) = reason.fallback_reason_ids();
        return (primary.to_owned(), secondary.map(str::to_owned));
    };
    let Some(items) = body["items"].as_array() else {
        let (primary, secondary) = reason.fallback_reason_ids();
        return (primary.to_owned(), secondary.map(str::to_owned));
    };

    let keywords = reason.label_keywords();
    let matches_keywords = |text: &str| {
        let lower = text.to_lowercase();
        keywords.iter().any(|kw| lower.contains(kw))
    };

    for item in items {
        let Some(id) = item["id"].as_str().filter(|id| !id.trim().is_empty()) else {
            continue;
        };
        let label = item
            .pointer("/snippet/label")
            .and_then(Value::as_str)
            .unwrap_or("");
        if matches_keywords(label) {
            let secondary = item
                .pointer("/snippet/secondaryReasons")
                .and_then(Value::as_array)
                .and_then(|rows| {
                    rows.iter()
                        .find(|row| row["label"].as_str().is_some_and(matches_keywords))
                        .or_else(|| rows.first())
                        .and_then(|row| row["id"].as_str())
                        .filter(|sid| !sid.trim().is_empty())
                        .map(str::to_owned)
                });
            return (id.to_owned(), secondary);
        }
    }

    for item in items {
        let Some(id) = item["id"].as_str().filter(|id| !id.trim().is_empty()) else {
            continue;
        };
        if let Some(secondary) = item
            .pointer("/snippet/secondaryReasons")
            .and_then(Value::as_array)
            .and_then(|rows| {
                rows.iter()
                    .find(|row| row["label"].as_str().is_some_and(matches_keywords))
                    .and_then(|row| row["id"].as_str())
                    .filter(|sid| !sid.trim().is_empty())
                    .map(str::to_owned)
            })
        {
            return (id.to_owned(), Some(secondary));
        }
    }

    let (primary, secondary) = reason.fallback_reason_ids();
    (primary.to_owned(), secondary.map(str::to_owned))
}

pub fn report_video_abuse(
    token: &str,
    video_id: &str,
    reason: AbuseReportReason,
    cancel: &Cancellation,
) -> Result<(), String> {
    report_video_abuse_at(
        "https://www.googleapis.com/youtube/v3",
        token,
        video_id,
        reason,
        cancel,
    )
}

fn report_video_abuse_at(
    base_url: &str,
    token: &str,
    video_id: &str,
    reason: AbuseReportReason,
    cancel: &Cancellation,
) -> Result<(), String> {
    if cancel.cancelled() {
        return Err("Denúncia cancelada.".into());
    }
    if !valid_id(video_id) {
        return Err("Este vídeo não possui um ID válido do YouTube.".into());
    }
    let client = http_client()?;
    let (reason_id, secondary_reason_id) =
        resolve_abuse_reason_ids(&client, base_url, token, reason);
    if cancel.cancelled() {
        return Err("Denúncia cancelada.".into());
    }
    let mut payload = serde_json::json!({
        "videoId": video_id,
        "reasonId": reason_id,
        "language": "pt",
    });
    if let Some(secondary) = secondary_reason_id {
        payload["secondaryReasonId"] = Value::String(secondary);
    }
    let base = base_url.trim_end_matches('/');
    let endpoint = format!("{base}/videos/reportAbuse");
    let response = client
        .post(endpoint)
        .bearer_auth(token)
        .json(&payload)
        .send()
        .map_err(|_| "Falha de conexão ao enviar a denúncia.".to_owned())?;
    if cancel.cancelled() {
        return Err("Denúncia cancelada.".into());
    }
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body: Value = response.json().unwrap_or(Value::Null);
    Err(api_failure_message(status.as_u16(), &body))
}

fn parse_upload(item: &Value) -> Option<(String, SearchResult)> {
    let snippet = item.get("snippet")?;
    let id = snippet.pointer("/resourceId/videoId")?.as_str()?;
    if !valid_id(id) {
        return None;
    }
    let title = snippet["title"].as_str()?;
    if title == "Private video" || title == "Deleted video" {
        return None;
    }
    let date = snippet["publishedAt"].as_str().unwrap_or("").to_owned();
    let thumbnail = snippet
        .pointer("/thumbnails/medium/url")
        .or_else(|| snippet.pointer("/thumbnails/default/url"))
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("https://i.ytimg.com/"))
        .unwrap_or("")
        .to_owned();
    let channel_id = snippet["channelId"]
        .as_str()
        .filter(|id| valid_id(id))
        .map(str::to_owned);
    Some((
        date.clone(),
        SearchResult {
            video: Video {
                title: title.to_owned(),
                channel: snippet["channelTitle"].as_str().unwrap_or("").to_owned(),
                url: format!("https://www.youtube.com/watch?v={id}"),
                thumbnail,
                duration: String::new(),
            },
            metadata: VideoMetadata {
                upload_date: (!date.is_empty()).then_some(date),
                channel_id,
                ..Default::default()
            },
        },
    ))
}

fn parse_playlist(item: &Value) -> Option<(String, SearchResult)> {
    let snippet = item.get("snippet")?;
    let id = item.get("id")?.as_str()?;
    if !valid_id(id) {
        return None;
    }
    let thumbnail = snippet
        .pointer("/thumbnails/medium/url")
        .or_else(|| snippet.pointer("/thumbnails/default/url"))
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("https://i.ytimg.com/"))
        .unwrap_or("")
        .to_owned();
    let count = item
        .pointer("/contentDetails/itemCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some((
        id.to_owned(),
        SearchResult {
            video: Video {
                title: snippet["title"].as_str()?.chars().take(512).collect(),
                channel: snippet["channelTitle"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(256)
                    .collect(),
                url: format!("fasttube-google-playlist:{id}"),
                thumbnail,
                duration: format!("{count} vídeos"),
            },
            metadata: VideoMetadata::default(),
        },
    ))
}

pub fn playlists_feed(token: &str, cancel: &Cancellation) -> Result<HomeFeed, String> {
    let client = http_client()?;
    playlists_feed_with(cancel, |params| api(&client, token, "playlists", params))
}

fn playlists_feed_with<F>(cancel: &Cancellation, mut fetch: F) -> Result<HomeFeed, String>
where
    F: FnMut(&[(&str, &str)]) -> Result<Value, String>,
{
    let mut items = Vec::new();
    let mut page = None;
    loop {
        if cancel.cancelled() {
            return Err("Consulta cancelada.".into());
        }
        let mut params = vec![
            ("part", "snippet,contentDetails"),
            ("mine", "true"),
            ("maxResults", "50"),
        ];
        if let Some(token) = page.as_deref() {
            params.push(("pageToken", token));
        }
        let response = fetch(&params)?;
        if cancel.cancelled() {
            return Err("Consulta cancelada.".into());
        }
        let rows = response["items"]
            .as_array()
            .ok_or("Lista de playlists inválida.")?;
        items.extend(
            rows.iter()
                .filter_map(parse_playlist)
                .map(|(_, item)| item)
                .take(MAX_PLAYLISTS - items.len()),
        );
        let next = response["nextPageToken"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 1024)
            .map(str::to_owned);
        if items.len() >= MAX_PLAYLISTS || next.is_none() {
            break;
        }
        page = next;
    }
    let playlists = items.iter().map(|item| item.video.url.clone()).collect();
    Ok(HomeFeed {
        items,
        playlists,
        channels: Vec::new(),
    })
}

pub fn playlist_videos_feed(
    token: &str,
    playlist_id: &str,
    cancel: &Cancellation,
) -> Result<HomeFeed, String> {
    if !valid_id(playlist_id) {
        return Err("Playlist inválida.".into());
    }
    let client = http_client()?;
    playlist_videos_feed_with(playlist_id, cancel, |params| {
        api(&client, token, "playlistItems", params)
    })
}

fn playlist_videos_feed_with<F>(
    playlist_id: &str,
    cancel: &Cancellation,
    mut fetch: F,
) -> Result<HomeFeed, String>
where
    F: FnMut(&[(&str, &str)]) -> Result<Value, String>,
{
    let mut items = Vec::new();
    let mut page = None;
    loop {
        if cancel.cancelled() {
            return Err("Consulta cancelada.".into());
        }
        let mut params = vec![
            ("part", "snippet"),
            ("playlistId", playlist_id),
            ("maxResults", "50"),
        ];
        if let Some(token) = page.as_deref() {
            params.push(("pageToken", token));
        }
        let response = fetch(&params)?;
        if cancel.cancelled() {
            return Err("Consulta cancelada.".into());
        }
        let rows = response["items"]
            .as_array()
            .ok_or("Lista de vídeos da playlist inválida.")?;
        items.extend(
            rows.iter()
                .take(MAX_PLAYLIST_VIDEOS - items.len())
                .filter_map(parse_upload)
                .map(|(_, item)| item),
        );
        let next = response["nextPageToken"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 1024)
            .map(str::to_owned);
        if items.len() >= MAX_PLAYLIST_VIDEOS || next.is_none() {
            break;
        }
        page = next;
    }
    Ok(HomeFeed {
        items,
        playlists: Vec::new(),
        channels: Vec::new(),
    })
}

fn append_subscription_channels(
    items: &[Value],
    ids: &mut Vec<String>,
    channels: &mut Vec<HomeChannel>,
) {
    for item in items {
        if ids.len() >= MAX_SUBSCRIPTIONS {
            break;
        }
        let Some(channel_id) = item
            .pointer("/snippet/resourceId/channelId")
            .and_then(Value::as_str)
            .filter(|id| valid_id(id))
        else {
            continue;
        };
        if ids.iter().any(|known| known == channel_id) {
            continue;
        }
        let title = item
            .pointer("/snippet/title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|title| !title.is_empty() && title.len() <= 4096)
            .unwrap_or(channel_id);
        let avatar_url = sanitize_avatar_url(
            item.pointer("/snippet/thumbnails/default/url")
                .and_then(Value::as_str)
                .or_else(|| {
                    item.pointer("/snippet/thumbnails/medium/url")
                        .and_then(Value::as_str)
                })
                .or_else(|| {
                    item.pointer("/snippet/thumbnails/high/url")
                        .and_then(Value::as_str)
                }),
        );
        ids.push(channel_id.to_owned());
        channels.push(HomeChannel {
            title: title.to_owned(),
            channel_id: channel_id.to_owned(),
            avatar_url,
        });
    }
}

fn fetch_subscription_channels_with<F>(
    cancel: &Cancellation,
    mut fetch: F,
) -> Result<(Vec<String>, Vec<HomeChannel>, bool), String>
where
    F: FnMut(&str, &[(&str, &str)]) -> Result<Value, String>,
{
    let mut ids = Vec::new();
    let mut subscription_channels = Vec::new();
    let mut page_token = None;
    let mut reached_limit = false;
    loop {
        if cancel.cancelled() {
            return Err("Consulta cancelada.".into());
        }
        let max_results = (MAX_SUBSCRIPTIONS - ids.len()).min(SUBSCRIPTION_PAGE_SIZE);
        let max_results_text = max_results.to_string();
        let mut params = vec![
            ("part", "snippet"),
            ("mine", "true"),
            ("maxResults", max_results_text.as_str()),
            ("order", "alphabetical"),
        ];
        if let Some(page) = page_token.as_deref() {
            params.push(("pageToken", page));
        }
        let subscriptions = fetch("subscriptions", &params)?;
        let items = subscriptions["items"]
            .as_array()
            .ok_or("Lista de inscri\u{e7}\u{f5}es inv\u{e1}lida.")?;
        append_subscription_channels(items, &mut ids, &mut subscription_channels);
        let next = subscriptions["nextPageToken"]
            .as_str()
            .filter(|page| !page.is_empty() && page.len() <= 1024)
            .map(str::to_owned);
        if ids.len() == MAX_SUBSCRIPTIONS {
            reached_limit = next.is_some();
            break;
        }
        let Some(next) = next else { break };
        page_token = Some(next);
    }
    Ok((ids, subscription_channels, reached_limit))
}

pub fn list_subscriptions(token: &str, cancel: &Cancellation) -> Result<Vec<HomeChannel>, String> {
    let client = http_client()?;
    let (_, channels, _) = fetch_subscription_channels_with(cancel, |resource, params| {
        api(&client, token, resource, params)
    })?;
    Ok(channels)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedFeedItem {
    pub title: String,
    pub channel: String,
    pub url: String,
    pub thumbnail: String,
    pub duration: String,
    pub upload_date: Option<String>,
    pub channel_id: Option<String>,
    pub view_count: Option<u64>,
    #[serde(default)]
    pub is_short: bool,
}

impl From<&SearchResult> for CachedFeedItem {
    fn from(item: &SearchResult) -> Self {
        Self {
            title: item.video.title.clone(),
            channel: item.video.channel.clone(),
            url: item.video.url.clone(),
            thumbnail: item.video.thumbnail.clone(),
            duration: item.video.duration.clone(),
            upload_date: item.metadata.upload_date.clone(),
            channel_id: item.metadata.channel_id.clone(),
            view_count: item.metadata.view_count,
            is_short: item.metadata.kind == SearchResultKind::Short,
        }
    }
}

impl From<CachedFeedItem> for SearchResult {
    fn from(item: CachedFeedItem) -> Self {
        let is_short = item.is_short || item.url.contains("/shorts/");
        Self {
            video: Video {
                title: item.title,
                channel: item.channel,
                url: item.url,
                thumbnail: item.thumbnail,
                duration: item.duration,
            },
            metadata: VideoMetadata {
                upload_date: item.upload_date,
                channel_id: item.channel_id,
                view_count: item.view_count,
                kind: if is_short {
                    SearchResultKind::Short
                } else {
                    SearchResultKind::Video
                },
                ..Default::default()
            },
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedRssHeaders {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelRssCacheEntry {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub items: Vec<SearchResult>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedSubscriptionChannel {
    pub title: String,
    pub channel_id: String,
    pub avatar_url: Option<String>,
}

impl From<&HomeChannel> for CachedSubscriptionChannel {
    fn from(channel: &HomeChannel) -> Self {
        Self {
            title: channel.title.clone(),
            channel_id: channel.channel_id.clone(),
            avatar_url: channel.avatar_url.clone(),
        }
    }
}

impl From<CachedSubscriptionChannel> for HomeChannel {
    fn from(channel: CachedSubscriptionChannel) -> Self {
        Self {
            title: channel.title,
            channel_id: channel.channel_id,
            avatar_url: channel.avatar_url,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionsDiskCache {
    pub version: u32,
    pub saved_at_unix_secs: u64,
    pub channels: Vec<CachedSubscriptionChannel>,
    pub items: Vec<CachedFeedItem>,
    #[serde(default)]
    pub rss_headers: HashMap<String, CachedRssHeaders>,
}

impl SubscriptionsDiskCache {
    pub fn from_feed(
        feed: &HomeFeed,
        rss_cache: &HashMap<String, ChannelRssCacheEntry>,
        saved_at_unix_secs: u64,
    ) -> Self {
        let channels = feed
            .channels
            .iter()
            .filter(|ch| valid_id(&ch.channel_id))
            .map(CachedSubscriptionChannel::from)
            .collect();
        let items = feed.items.iter().map(CachedFeedItem::from).collect();
        let rss_headers = rss_cache
            .iter()
            .filter(|(id, entry)| {
                valid_id(id) && (entry.etag.is_some() || entry.last_modified.is_some())
            })
            .map(|(id, entry)| {
                (
                    id.clone(),
                    CachedRssHeaders {
                        etag: entry.etag.clone(),
                        last_modified: entry.last_modified.clone(),
                    },
                )
            })
            .collect();
        Self {
            version: SUBSCRIPTIONS_CACHE_VERSION,
            saved_at_unix_secs,
            channels,
            items,
            rss_headers,
        }
    }

    pub fn into_parts(
        self,
    ) -> (
        u64,
        Vec<HomeChannel>,
        Vec<SearchResult>,
        HashMap<String, ChannelRssCacheEntry>,
    ) {
        let channels: Vec<HomeChannel> = self
            .channels
            .into_iter()
            .filter(|ch| valid_id(&ch.channel_id))
            .map(|mut ch| {
                ch.avatar_url = sanitize_avatar_url(ch.avatar_url.as_deref());
                HomeChannel::from(ch)
            })
            .collect();
        let items: Vec<SearchResult> = self.items.into_iter().map(SearchResult::from).collect();
        let mut items_by_channel: HashMap<String, Vec<SearchResult>> = HashMap::new();
        for item in &items {
            if let Some(cid) = item.metadata.channel_id.as_deref()
                && valid_id(cid)
            {
                items_by_channel
                    .entry(cid.to_owned())
                    .or_default()
                    .push(item.clone());
            }
        }
        let mut rss_cache = HashMap::new();
        for (cid, headers) in self.rss_headers {
            if !valid_id(&cid) {
                continue;
            }
            if let Some(channel_items) = items_by_channel.remove(&cid) {
                rss_cache.insert(
                    cid,
                    ChannelRssCacheEntry {
                        etag: headers.etag,
                        last_modified: headers.last_modified,
                        items: channel_items,
                    },
                );
            }
        }
        (self.saved_at_unix_secs, channels, items, rss_cache)
    }
}

pub fn load_subscriptions_cache_from_path(
    path: &Path,
) -> Result<Option<SubscriptionsDiskCache>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Não foi possível ler o cache de inscrições.".into()),
    };
    let mut bytes = Vec::new();
    file.take((SUBSCRIPTIONS_CACHE_MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Não foi possível ler o cache de inscrições.".to_owned())?;
    if bytes.is_empty() || bytes.len() > SUBSCRIPTIONS_CACHE_MAX_BYTES {
        return Err("Arquivo de cache de inscrições inválido.".into());
    }
    let parsed: SubscriptionsDiskCache = serde_json::from_slice(&bytes)
        .map_err(|_| "Arquivo de cache de inscrições corrompido.".to_owned())?;
    if parsed.version != SUBSCRIPTIONS_CACHE_VERSION {
        return Ok(None);
    }
    Ok(Some(parsed))
}

pub fn save_subscriptions_cache_to_path(
    path: &Path,
    cache: &SubscriptionsDiskCache,
) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or("Diretório do cache de inscrições inválido.")?;
    std::fs::create_dir_all(parent).map_err(|_| "Não foi possível criar pasta de cache.")?;
    let bytes = serde_json::to_vec(cache)
        .map_err(|_| "Não foi possível codificar o cache de inscrições.")?;
    if bytes.len() > SUBSCRIPTIONS_CACHE_MAX_BYTES {
        return Err("Cache de inscrições excede o limite de tamanho.".into());
    }
    let (temporary, mut file) = loop {
        let id = SUBSCRIPTIONS_CACHE_TEMP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".fasttube-subs-cache-{}-{id}.tmp",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err("Não foi possível gravar o cache de inscrições.".into()),
        }
    };
    let result = (|| {
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| "Não foi possível gravar o cache de inscrições.".to_owned())?;
        drop(file);
        std::fs::rename(&temporary, path)
            .map_err(|_| "Não foi possível finalizar o cache de inscrições.".to_owned())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

pub fn delete_subscriptions_cache_at_path(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Não foi possível limpar o cache de inscrições.".into()),
    }
}

#[cfg(not(test))]
pub fn subscriptions_cache_path() -> Result<PathBuf, String> {
    Ok(crate::settings::settings_path()
        .map_err(|_| "Não foi possível localizar a pasta de configurações.")?
        .with_file_name("subscriptions-cache.json"))
}

#[derive(Clone, Debug, Default)]
pub struct SubscriptionFeedOptions {
    pub known_channels: Option<Vec<HomeChannel>>,
    pub prioritized_channel_ids: Vec<String>,
    pub rss_cache: HashMap<String, ChannelRssCacheEntry>,
}

#[derive(Clone, Debug)]
pub struct SubscriptionFeedOutput {
    pub feed: HomeFeed,
    pub rss_cache: HashMap<String, ChannelRssCacheEntry>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub used_cached_channels: bool,
    #[cfg_attr(not(test), allow(dead_code))]
    pub rss_hits: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    pub not_modified_hits: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    pub api_fallbacks: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscriptionProgress {
    pub items: Vec<SearchResult>,
    pub channels: Vec<HomeChannel>,
    pub completed_channels: usize,
    pub total_channels: usize,
    pub reached_limit: bool,
}

fn rss_http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .connect_timeout(RSS_CONNECT_TIMEOUT)
        .timeout(RSS_TIMEOUT)
        .pool_max_idle_per_host(SUBSCRIPTION_WORKER_COUNT + 4)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
        .build()
        .map_err(|_| "Não foi possível iniciar o cliente RSS.".into())
}

fn sanitize_http_cache_header(value: Option<&reqwest::header::HeaderValue>) -> Option<String> {
    value
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty() && v.len() <= 256 && v.is_ascii())
        .map(str::to_owned)
}

fn sanitize_rss_thumbnail_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 1024 {
        return None;
    }
    let parsed = url::Url::parse(trimmed).ok()?;
    let host = parsed.host_str()?;
    let valid_host = host == "i.ytimg.com"
        || host == "ytimg.com"
        || (host.ends_with(".ytimg.com") && !host.contains(".."));
    (parsed.scheme() == "https" && valid_host).then(|| trimmed.to_owned())
}

fn decode_xml_entities(input: &str) -> String {
    let inner = input
        .trim()
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
        .unwrap_or(input);
    if !inner.contains('&') {
        return inner.to_owned();
    }
    let mut out = String::with_capacity(inner.len());
    let mut rest = inner;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        if let Some(semi) = after.find(';')
            && semi <= 12
        {
            let entity = &after[1..semi];
            let decoded = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                    u32::from_str_radix(&entity[2..], 16)
                        .ok()
                        .and_then(char::from_u32)
                }
                _ if entity.starts_with('#') => {
                    entity[1..].parse::<u32>().ok().and_then(char::from_u32)
                }
                _ => None,
            };
            if let Some(ch) = decoded {
                out.push(ch);
                rest = &after[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &rest[amp + 1..];
    }
    out.push_str(rest);
    out
}

fn extract_xml_tag_text<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open_prefix = format!("<{tag}");
    let close_tag = format!("</{tag}>");
    let mut search_from = 0;
    while let Some(rel_start) = block[search_from..].find(&open_prefix) {
        let start = search_from + rel_start;
        let after_name = &block[start + open_prefix.len()..];
        if let Some(first) = after_name.chars().next()
            && first != '>'
            && !first.is_ascii_whitespace()
        {
            search_from = start + open_prefix.len();
            continue;
        }
        let open_end_rel = after_name.find('>')?;
        if after_name[..open_end_rel].ends_with('/') {
            return None;
        }
        let content_start = start + open_prefix.len() + open_end_rel + 1;
        let close_rel = block[content_start..].find(&close_tag)?;
        return Some(&block[content_start..content_start + close_rel]);
    }
    None
}

fn extract_xml_attr<'a>(tag_text: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=\"");
    if let Some(pos) = tag_text.find(&needle) {
        let rest = &tag_text[pos + needle.len()..];
        let end = rest.find('"')?;
        return Some(&rest[..end]);
    }
    let needle_single = format!("{attr}='");
    let pos = tag_text.find(&needle_single)?;
    let rest = &tag_text[pos + needle_single.len()..];
    let end = rest.find('\'')?;
    Some(&rest[..end])
}

pub(crate) fn parse_youtube_atom_feed(
    xml: &str,
    fallback_channel_title: &str,
    channel_id_hint: &str,
) -> Option<Vec<(String, SearchResult)>> {
    if !xml.contains("<feed") {
        return None;
    }
    let feed_author = extract_xml_tag_text(xml, "author")
        .and_then(|author| extract_xml_tag_text(author, "name"))
        .map(decode_xml_entities)
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let mut results = Vec::new();
    let mut cursor = xml;
    while let Some(entry_start) = cursor.find("<entry>") {
        let after_open = &cursor[entry_start + "<entry>".len()..];
        let Some(entry_end) = after_open.find("</entry>") else {
            break;
        };
        let entry = &after_open[..entry_end];
        cursor = &after_open[entry_end + "</entry>".len()..];

        let Some(video_id) = extract_xml_tag_text(entry, "yt:videoId")
            .map(str::trim)
            .filter(|id| valid_id(id))
        else {
            continue;
        };
        let Some(raw_title) = extract_xml_tag_text(entry, "title")
            .or_else(|| extract_xml_tag_text(entry, "media:title"))
        else {
            continue;
        };
        let title = decode_xml_entities(raw_title).trim().to_owned();
        if title.is_empty() || title == "Private video" || title == "Deleted video" {
            continue;
        }
        let published = extract_xml_tag_text(entry, "published")
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.len() <= 64)
            .unwrap_or("")
            .to_owned();
        let entry_channel_id = extract_xml_tag_text(entry, "yt:channelId")
            .map(str::trim)
            .filter(|id| valid_id(id))
            .unwrap_or(channel_id_hint);
        let channel_name = extract_xml_tag_text(entry, "author")
            .and_then(|author| extract_xml_tag_text(author, "name"))
            .map(decode_xml_entities)
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .or_else(|| feed_author.clone())
            .unwrap_or_else(|| fallback_channel_title.to_owned());

        let is_short = entry.find("<link ").is_some_and(|pos| {
            entry[pos..]
                .find('>')
                .and_then(|end| extract_xml_attr(&entry[pos..pos + end], "href"))
                .is_some_and(|href| href.contains("/shorts/"))
        });

        let thumbnail = entry
            .find("<media:thumbnail")
            .and_then(|pos| {
                let end = entry[pos..].find('>')?;
                extract_xml_attr(&entry[pos..pos + end], "url")
            })
            .and_then(|raw| sanitize_rss_thumbnail_url(&decode_xml_entities(raw)))
            .unwrap_or_else(|| format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"));

        let description = extract_xml_tag_text(entry, "media:description")
            .map(decode_xml_entities)
            .map(|s| s.trim().chars().take(512).collect::<String>())
            .filter(|s| !s.is_empty());

        let view_count = entry.find("<media:statistics").and_then(|pos| {
            let end = entry[pos..].find('>')?;
            extract_xml_attr(&entry[pos..pos + end], "views")?
                .parse::<u64>()
                .ok()
        });

        results.push((
            published.clone(),
            SearchResult {
                video: Video {
                    title,
                    channel: channel_name,
                    url: format!("https://www.youtube.com/watch?v={video_id}"),
                    thumbnail,
                    duration: String::new(),
                },
                metadata: VideoMetadata {
                    upload_date: (!published.is_empty()).then_some(published),
                    channel_id: valid_id(entry_channel_id).then(|| entry_channel_id.to_owned()),
                    description,
                    view_count,
                    kind: if is_short {
                        SearchResultKind::Short
                    } else {
                        SearchResultKind::Video
                    },
                    ..Default::default()
                },
            },
        ));
        if results.len() >= MAX_UPLOADS_PER_CHANNEL {
            break;
        }
    }
    Some(results)
}

enum ChannelRssOutcome {
    NotModified,
    Updated {
        items: Vec<(String, SearchResult)>,
        etag: Option<String>,
        last_modified: Option<String>,
    },
    FallbackNeeded,
}

fn fetch_channel_rss_at(
    client: &reqwest::blocking::Client,
    rss_base_url: &str,
    channel: &HomeChannel,
    cached: Option<&ChannelRssCacheEntry>,
) -> ChannelRssOutcome {
    if !valid_id(&channel.channel_id) {
        return ChannelRssOutcome::FallbackNeeded;
    }
    let url = format!(
        "{rss_base_url}/feeds/videos.xml?channel_id={}",
        channel.channel_id
    );
    let mut request = client.get(&url);
    if let Some(entry) = cached {
        if let Some(etag) = entry.etag.as_deref() {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        if let Some(last_modified) = entry.last_modified.as_deref() {
            request = request.header(reqwest::header::IF_MODIFIED_SINCE, last_modified);
        }
    }
    let Ok(response) = request.send() else {
        return ChannelRssOutcome::FallbackNeeded;
    };
    let status = response.status();
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return if cached.is_some() {
            ChannelRssOutcome::NotModified
        } else {
            ChannelRssOutcome::FallbackNeeded
        };
    }
    if !status.is_success() {
        return ChannelRssOutcome::FallbackNeeded;
    }
    let etag = sanitize_http_cache_header(response.headers().get(reqwest::header::ETAG));
    let last_modified =
        sanitize_http_cache_header(response.headers().get(reqwest::header::LAST_MODIFIED));
    let mut body = Vec::new();
    if response
        .take((RSS_MAX_BYTES + 1) as u64)
        .read_to_end(&mut body)
        .is_err()
        || body.len() > RSS_MAX_BYTES
    {
        return ChannelRssOutcome::FallbackNeeded;
    }
    let Ok(xml) = std::str::from_utf8(&body) else {
        return ChannelRssOutcome::FallbackNeeded;
    };
    let Some(items) = parse_youtube_atom_feed(xml, &channel.title, &channel.channel_id) else {
        return ChannelRssOutcome::FallbackNeeded;
    };
    ChannelRssOutcome::Updated {
        items,
        etag,
        last_modified,
    }
}

fn fetch_channel_uploads_via_api_at(
    client: &reqwest::blocking::Client,
    api_base_url: &str,
    token: &str,
    channel_id: &str,
) -> Result<Vec<(String, SearchResult)>, String> {
    if !valid_id(channel_id) {
        return Ok(Vec::new());
    }
    // Standard YouTube channels (`UC...`) map deterministically to their uploads playlist (`UU...`),
    // avoiding a `channels?part=contentDetails` roundtrip unless that lookup is missing/non-UC.
    let direct_playlist = channel_id
        .strip_prefix("UC")
        .filter(|rest| !rest.is_empty())
        .map(|rest| format!("UU{rest}"))
        .filter(|id| valid_id(id));

    if let Some(playlist_id) = direct_playlist.as_deref() {
        match api_at(
            client,
            api_base_url,
            token,
            "playlistItems",
            &[
                ("part", "snippet"),
                ("playlistId", playlist_id),
                ("maxResults", "5"),
            ],
        ) {
            Ok(uploads) => {
                let items = uploads["items"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .take(MAX_UPLOADS_PER_CHANNEL)
                            .filter_map(parse_upload)
                            .collect()
                    })
                    .unwrap_or_default();
                return Ok(items);
            }
            Err(err) if err.starts_with("Sess\u{00e3}o expirada") => return Err(err),
            Err(_) => {}
        }
    }

    let channels = api_at(
        client,
        api_base_url,
        token,
        "channels",
        &[
            ("part", "contentDetails"),
            ("id", channel_id),
            ("maxResults", "1"),
        ],
    )?;
    let playlist_id = channels["items"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|ch| ch.pointer("/contentDetails/relatedPlaylists/uploads"))
        .and_then(Value::as_str)
        .filter(|id| valid_id(id));
    let Some(playlist_id) = playlist_id else {
        return Ok(Vec::new());
    };
    let uploads = api_at(
        client,
        api_base_url,
        token,
        "playlistItems",
        &[
            ("part", "snippet"),
            ("playlistId", playlist_id),
            ("maxResults", "5"),
        ],
    )?;
    Ok(uploads["items"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .take(MAX_UPLOADS_PER_CHANNEL)
                .filter_map(parse_upload)
                .collect()
        })
        .unwrap_or_default())
}

fn order_channels_by_priority(
    channels: &[HomeChannel],
    prioritized_ids: &[String],
) -> Vec<HomeChannel> {
    if prioritized_ids.is_empty() || channels.len() <= 1 {
        return channels.to_vec();
    }
    let rank_map: HashMap<&str, usize> = prioritized_ids
        .iter()
        .enumerate()
        .map(|(idx, id)| (id.as_str(), idx))
        .collect();
    let mut indexed: Vec<(usize, usize, HomeChannel)> = channels
        .iter()
        .enumerate()
        .map(|(orig_idx, ch)| {
            let prio = rank_map
                .get(ch.channel_id.as_str())
                .copied()
                .unwrap_or(usize::MAX);
            (prio, orig_idx, ch.clone())
        })
        .collect();
    indexed.sort_by_key(|(prio, orig_idx, _)| (*prio, *orig_idx));
    indexed.into_iter().map(|(_, _, ch)| ch).collect()
}

pub fn subscriptions_feed(
    token: &str,
    cancel: &Cancellation,
    progress: impl FnMut(SubscriptionProgress),
) -> Result<HomeFeed, String> {
    subscriptions_feed_fast(token, SubscriptionFeedOptions::default(), cancel, progress)
        .map(|output| output.feed)
}

pub fn subscriptions_feed_fast(
    token: &str,
    options: SubscriptionFeedOptions,
    cancel: &Cancellation,
    progress: impl FnMut(SubscriptionProgress),
) -> Result<SubscriptionFeedOutput, String> {
    subscriptions_feed_fast_at(
        "https://www.googleapis.com/youtube/v3",
        "https://www.youtube.com",
        token,
        options,
        cancel,
        progress,
    )
}

fn subscriptions_feed_fast_at(
    api_base_url: &str,
    rss_base_url: &str,
    token: &str,
    options: SubscriptionFeedOptions,
    cancel: &Cancellation,
    mut progress: impl FnMut(SubscriptionProgress),
) -> Result<SubscriptionFeedOutput, String> {
    let api_client = http_client()?;
    let rss_client = rss_http_client()?;

    let (subscription_channels, reached_limit, used_cached_channels) = match options.known_channels
    {
        Some(channels) if !channels.is_empty() => (
            channels
                .into_iter()
                .filter(|ch| valid_id(&ch.channel_id))
                .take(MAX_SUBSCRIPTIONS)
                .collect(),
            false,
            true,
        ),
        _ => {
            let (_, channels, reached_limit) =
                fetch_subscription_channels_with(cancel, |resource, params| {
                    api_at(&api_client, api_base_url, token, resource, params)
                })?;
            (channels, reached_limit, false)
        }
    };

    let total_channels = subscription_channels.len();
    progress(SubscriptionProgress {
        items: Vec::new(),
        channels: subscription_channels.clone(),
        completed_channels: 0,
        total_channels,
        reached_limit,
    });

    if total_channels == 0 {
        return Ok(SubscriptionFeedOutput {
            feed: HomeFeed {
                items: Vec::new(),
                playlists: Vec::new(),
                channels: subscription_channels,
            },
            rss_cache: options.rss_cache,
            used_cached_channels,
            rss_hits: 0,
            not_modified_hits: 0,
            api_fallbacks: 0,
        });
    }

    let ordered_channels =
        order_channels_by_priority(&subscription_channels, &options.prioritized_channel_ids);
    let worker_count = ordered_channels.len().min(SUBSCRIPTION_WORKER_COUNT);
    let next_index = std::sync::atomic::AtomicUsize::new(0);
    let (sender, receiver) = std::sync::mpsc::channel();

    enum WorkerChannelResult {
        RssUpdated {
            channel_id: String,
            items: Vec<(String, SearchResult)>,
            etag: Option<String>,
            last_modified: Option<String>,
        },
        RssNotModified {
            channel_id: String,
        },
        ApiSuccess {
            items: Vec<(String, SearchResult)>,
        },
        Error {
            fatal_auth: bool,
            message: String,
        },
    }

    let mut videos: Vec<(String, SearchResult)> = Vec::new();
    let initial_rss_cache = options.rss_cache;
    let mut updated_rss_cache = initial_rss_cache.clone();
    let mut completed_channels = 0usize;
    let mut succeeded_channels = 0usize;
    let mut rss_hits = 0usize;
    let mut not_modified_hits = 0usize;
    let mut api_fallbacks = 0usize;
    let mut first_error: Option<String> = None;
    let mut fatal_auth_error: Option<String> = None;

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let sender = sender.clone();
            let api_client = &api_client;
            let rss_client = &rss_client;
            let channels = &ordered_channels;
            let rss_cache_ref = &initial_rss_cache;
            let next_index = &next_index;
            scope.spawn(move || {
                loop {
                    if cancel.cancelled() {
                        break;
                    }
                    let index = next_index.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(channel) = channels.get(index) else {
                        break;
                    };
                    let cached_entry = rss_cache_ref.get(&channel.channel_id);
                    let msg =
                        match fetch_channel_rss_at(rss_client, rss_base_url, channel, cached_entry)
                        {
                            ChannelRssOutcome::NotModified => WorkerChannelResult::RssNotModified {
                                channel_id: channel.channel_id.clone(),
                            },
                            ChannelRssOutcome::Updated {
                                items,
                                etag,
                                last_modified,
                            } => WorkerChannelResult::RssUpdated {
                                channel_id: channel.channel_id.clone(),
                                items,
                                etag,
                                last_modified,
                            },
                            ChannelRssOutcome::FallbackNeeded => {
                                if cancel.cancelled() {
                                    break;
                                }
                                match fetch_channel_uploads_via_api_at(
                                    api_client,
                                    api_base_url,
                                    token,
                                    &channel.channel_id,
                                ) {
                                    Ok(items) => WorkerChannelResult::ApiSuccess { items },
                                    Err(err) => WorkerChannelResult::Error {
                                        fatal_auth: err.starts_with("Sess\u{00e3}o expirada"),
                                        message: err,
                                    },
                                }
                            }
                        };
                    if sender.send(msg).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        while let Ok(msg) = receiver.recv() {
            completed_channels += 1;
            match msg {
                WorkerChannelResult::RssNotModified { channel_id } => {
                    succeeded_channels += 1;
                    not_modified_hits += 1;
                    if let Some(entry) = updated_rss_cache.get(&channel_id) {
                        videos.extend(entry.items.iter().map(|item| {
                            (
                                item.metadata.upload_date.clone().unwrap_or_default(),
                                item.clone(),
                            )
                        }));
                    }
                }
                WorkerChannelResult::RssUpdated {
                    channel_id,
                    items,
                    etag,
                    last_modified,
                } => {
                    succeeded_channels += 1;
                    rss_hits += 1;
                    let cache_items: Vec<SearchResult> =
                        items.iter().map(|(_, item)| item.clone()).collect();
                    updated_rss_cache.insert(
                        channel_id,
                        ChannelRssCacheEntry {
                            etag,
                            last_modified,
                            items: cache_items,
                        },
                    );
                    videos.extend(items);
                }
                WorkerChannelResult::ApiSuccess { items } => {
                    succeeded_channels += 1;
                    api_fallbacks += 1;
                    videos.extend(items);
                }
                WorkerChannelResult::Error {
                    fatal_auth,
                    message,
                } => {
                    if fatal_auth && fatal_auth_error.is_none() {
                        fatal_auth_error = Some(message);
                        cancel.cancel();
                    } else if first_error.is_none() {
                        first_error = Some(message);
                    }
                }
            }

            let should_emit_progress = completed_channels == 1
                || (completed_channels <= 30
                    && completed_channels.is_multiple_of(PROGRESS_CHANNEL_BATCH))
                || completed_channels.is_multiple_of(PROGRESS_LATER_CHANNEL_BATCH)
                || completed_channels == total_channels;
            if should_emit_progress {
                let mut ordered = videos.clone();
                ordered.sort_by(|left, right| right.0.cmp(&left.0));
                progress(SubscriptionProgress {
                    items: ordered.into_iter().map(|(_, item)| item).collect(),
                    channels: subscription_channels.clone(),
                    completed_channels,
                    total_channels,
                    reached_limit,
                });
            }
        }
    });

    if let Some(auth_err) = fatal_auth_error {
        return Err(auth_err);
    }
    if cancel.cancelled() {
        return Err("Consulta cancelada.".into());
    }
    if succeeded_channels == 0
        && let Some(error) = first_error
    {
        return Err(error);
    }
    videos.sort_by(|left, right| right.0.cmp(&left.0));
    let mut seen_urls = HashSet::with_capacity(videos.len());
    let mut deduped_items = Vec::with_capacity(videos.len());
    for (_, item) in videos {
        if seen_urls.insert(item.video.url.clone()) {
            deduped_items.push(item);
        }
    }
    Ok(SubscriptionFeedOutput {
        feed: HomeFeed {
            items: deduped_items,
            playlists: Vec::new(),
            channels: subscription_channels,
        },
        rss_cache: updated_rss_cache,
        used_cached_channels,
        rss_hits,
        not_modified_hits,
        api_fallbacks,
    })
}

#[cfg(test)]
fn subscriptions_feed_with<F>(
    cancel: &Cancellation,
    mut progress: impl FnMut(SubscriptionProgress),
    mut fetch: F,
) -> Result<HomeFeed, String>
where
    F: FnMut(&str, &[(&str, &str)]) -> Result<Value, String>,
{
    let (ids, subscription_channels, reached_limit) =
        fetch_subscription_channels_with(cancel, |resource, params| fetch(resource, params))?;

    let total_channels = ids.len();
    progress(SubscriptionProgress {
        items: Vec::new(),
        channels: subscription_channels.clone(),
        completed_channels: 0,
        total_channels,
        reached_limit,
    });
    let mut videos = Vec::new();
    for (batch, channel_ids) in ids.chunks(50).enumerate() {
        if cancel.cancelled() {
            return Err("Consulta cancelada.".into());
        }
        let joined_ids = channel_ids.join(",");
        let channels = fetch(
            "channels",
            &[
                ("part", "contentDetails"),
                ("id", &joined_ids),
                ("maxResults", "50"),
            ],
        )?;
        let channels = channels["items"]
            .as_array()
            .ok_or("Lista de canais inv\u{e1}lida.")?;
        for (offset, channel) in channels.iter().enumerate() {
            if cancel.cancelled() {
                return Err("Consulta cancelada.".into());
            }
            if let Some(playlist) = channel
                .pointer("/contentDetails/relatedPlaylists/uploads")
                .and_then(Value::as_str)
                .filter(|id| valid_id(id))
            {
                let uploads = fetch(
                    "playlistItems",
                    &[
                        ("part", "snippet"),
                        ("playlistId", playlist),
                        ("maxResults", "5"),
                    ],
                )?;
                if let Some(items) = uploads["items"].as_array() {
                    videos.extend(
                        items
                            .iter()
                            .take(MAX_UPLOADS_PER_CHANNEL)
                            .filter_map(parse_upload),
                    );
                }
            }
            let completed_channels = batch * 50 + offset + 1;
            if completed_channels == 1
                || completed_channels % PROGRESS_CHANNEL_BATCH == 0
                || completed_channels == total_channels
            {
                let mut ordered = videos.clone();
                ordered.sort_by(|left, right| right.0.cmp(&left.0));
                progress(SubscriptionProgress {
                    items: ordered.into_iter().map(|(_, item)| item).collect(),
                    channels: subscription_channels.clone(),
                    completed_channels,
                    total_channels,
                    reached_limit,
                });
            }
        }
    }
    videos.sort_by(|left, right| right.0.cmp(&left.0));
    Ok(HomeFeed {
        items: videos.into_iter().map(|(_, video)| video).collect(),
        playlists: Vec::new(),
        channels: subscription_channels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn video_rating_values_match_youtube_api() {
        assert_eq!(VideoRating::Like.api_value(), "like");
        assert_eq!(VideoRating::Dislike.api_value(), "dislike");
        assert_eq!(VideoRating::None.api_value(), "none");
    }

    #[test]
    fn video_rating_rejects_invalid_ids_before_network_request() {
        let cancel = Cancellation::default();
        for video_id in ["", "id with spaces", "id/with/slashes", "é"] {
            assert!(rate_video("token", video_id, VideoRating::Like, &cancel).is_err());
        }
        assert!(!valid_id(&"a".repeat(129)));
        assert!(valid_id("a".repeat(128).as_str()));
    }

    #[test]
    fn video_rating_sends_zero_content_length_post_and_handles_auth_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut captured = Vec::new();
            for status_line in ["HTTP/1.1 204 No Content", "HTTP/1.1 401 Unauthorized"] {
                let (mut stream, _) = listener.accept().unwrap();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = [0u8; 4096];
                let count = stream.read(&mut buf).unwrap_or(0);
                captured.push(String::from_utf8_lossy(&buf[..count]).into_owned());
                let response =
                    format!("{status_line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                stream.write_all(response.as_bytes()).unwrap();
            }
            captured
        });

        let endpoint = format!("http://{addr}/youtube/v3/videos/rate");
        let cancel = Cancellation::default();
        rate_video_at(
            &endpoint,
            "tok-123",
            "dQw4w9WgXcQ",
            VideoRating::Like,
            &cancel,
        )
        .unwrap();
        let err = rate_video_at(
            &endpoint,
            "tok-123",
            "dQw4w9WgXcQ",
            VideoRating::Dislike,
            &cancel,
        )
        .unwrap_err();
        assert!(err.contains("sess\u{00e3}o Google expirou"));

        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        let first_lower = requests[0].to_ascii_lowercase();
        assert!(
            requests[0].starts_with("POST /youtube/v3/videos/rate?id=dQw4w9WgXcQ&rating=like ")
        );
        assert!(first_lower.contains("authorization: bearer tok-123\r\n"));
        assert!(first_lower.contains("content-length: 0\r\n"));
        assert!(
            requests[1].starts_with("POST /youtube/v3/videos/rate?id=dQw4w9WgXcQ&rating=dislike ")
        );
        assert!(
            requests[1]
                .to_ascii_lowercase()
                .contains("content-length: 0\r\n")
        );
    }

    #[test]
    fn parses_comment_replies_and_page_token() {
        let json = serde_json::json!({
            "items": [{
                "id": "reply-id",
                "snippet": {
                    "authorDisplayName": "Ana",
                    "textDisplay": "Resposta",
                    "publishedAt": "2026-11-08T00:00:00Z",
                    "likeCount": 2
                }
            }],
            "nextPageToken": "next"
        });
        let page = parse_comment_replies(&json).unwrap();
        assert_eq!(page.comments.len(), 1);
        assert_eq!(page.comments[0].id, "reply-id");
        assert_eq!(page.comments[0].text, "Resposta");
        assert_eq!(page.next_page.as_deref(), Some("next"));
    }
    fn subscription_page(start: usize, count: usize, next: Option<&str>) -> Value {
        let mut page = serde_json::json!({"items": []});
        for index in start..start + count {
            page["items"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "snippet": {
                        "title": format!("channel-{index}"),
                        "resourceId": {"channelId": format!("UC{index:09}")}
                    }
                }));
        }
        if let Some(next) = next {
            page["nextPageToken"] = next.into();
        }
        page
    }

    fn channel_page(ids: &str) -> Value {
        serde_json::json!({"items": ids.split(',').map(|id| serde_json::json!({
            "contentDetails": {"relatedPlaylists": {"uploads": id.replace("UC", "PL")}}
        })).collect::<Vec<_>>()})
    }

    fn upload_page(playlist: &str, count: usize) -> Value {
        let channel_index: usize = playlist[2..].parse().unwrap();
        serde_json::json!({"items": (0..count).map(|offset| {
            let index = channel_index * 10 + offset;
            serde_json::json!({"snippet": {
                "resourceId": {"videoId": format!("v{index:010}")},
                "title": format!("video-{index}"),
                "channelTitle": format!("channel-{channel_index}"),
                "channelId": format!("UC{channel_index:09}"),
                "publishedAt": format!("2026-01-{:02}T00:00:00Z", offset + 1)
            }})
        }).collect::<Vec<_>>()})
    }

    #[test]
    fn google_playlists_paginate_and_stop_at_100() {
        let cancel = Cancellation::default();
        let mut calls = 0;
        let feed = playlists_feed_with(&cancel, |params| {
            calls += 1;
            assert!(params.contains(&("mine", "true")));
            assert!(params.contains(&("maxResults", "50")));
            if calls == 1 {
                Ok(serde_json::json!({"items": (0..50).map(|index| serde_json::json!({"id": format!("PL{index:08}"), "snippet": {"title": "Playlist"}, "contentDetails": {"itemCount": 2}})).collect::<Vec<_>>(), "nextPageToken": "next"}))
            } else {
                assert!(params.contains(&("pageToken", "next")));
                Ok(serde_json::json!({"items": (50..100).map(|index| serde_json::json!({"id": format!("PL{index:08}"), "snippet": {"title": "Playlist"}, "contentDetails": {"itemCount": 2}})).collect::<Vec<_>>(), "nextPageToken": "more"}))
            }
        }).unwrap();
        assert_eq!(calls, 2);
        assert_eq!(feed.items.len(), 100);
        assert_eq!(feed.playlists.len(), 100);
        assert_eq!(
            feed.items[0].video.url,
            "fasttube-google-playlist:PL00000000"
        );
    }

    #[test]
    fn google_playlist_videos_paginate_and_stop_at_200() {
        let cancel = Cancellation::default();
        let mut calls = 0;
        let feed = playlist_videos_feed_with("PL00000001", &cancel, |params| {
            calls += 1;
            assert!(params.contains(&("playlistId", "PL00000001")));
            assert!(params.contains(&("maxResults", "50")));
            Ok(serde_json::json!({"items": (0..50).map(|index| serde_json::json!({"snippet": {"resourceId": {"videoId": format!("video{index:05}")}, "title": format!("Video {index}"), "channelTitle": "Owner"}})).collect::<Vec<_>>(), "nextPageToken": format!("page-{calls}")}))
        }).unwrap();
        assert_eq!(calls, 4);
        assert_eq!(feed.items.len(), 200);
    }

    #[test]
    fn google_playlist_queries_honor_cancellation_before_network() {
        let cancel = Cancellation::default();
        cancel.cancel();
        let mut called = false;
        assert!(
            playlists_feed_with(&cancel, |_| {
                called = true;
                Ok(Value::Null)
            })
            .is_err()
        );
        assert!(!called);
        assert!(
            playlist_videos_feed_with("PL00000001", &cancel, |_| {
                called = true;
                Ok(Value::Null)
            })
            .is_err()
        );
        assert!(!called);
    }

    #[test]
    fn google_playlist_queries_discard_responses_after_cancellation() {
        let cancel = Cancellation::default();
        let result = playlists_feed_with(&cancel, |_| {
            cancel.cancel();
            Ok(serde_json::json!({"items": []}))
        });
        assert!(matches!(result, Err(error) if error == "Consulta cancelada."));

        let cancel = Cancellation::default();
        let result = playlist_videos_feed_with("PL00000001", &cancel, |_| {
            cancel.cancel();
            Ok(serde_json::json!({"items": []}))
        });
        assert!(matches!(result, Err(error) if error == "Consulta cancelada."));
    }

    #[test]
    fn subscriptions_paginate_and_publish_sorted_upload_results_progressively() {
        let cancel = Cancellation::default();
        let mut progress = Vec::new();
        let feed = subscriptions_feed_with(
            &cancel,
            |update| progress.push(update),
            |resource, params| match resource {
                "subscriptions" => {
                    let token = params
                        .iter()
                        .find(|(key, _)| *key == "pageToken")
                        .map(|(_, value)| *value);
                    assert_eq!(
                        params
                            .iter()
                            .find(|(key, _)| *key == "maxResults")
                            .unwrap()
                            .1,
                        "50"
                    );
                    match token {
                        None => Ok(subscription_page(0, 50, Some("page-2"))),
                        Some("page-2") => Ok(subscription_page(50, 5, None)),
                        _ => panic!("unexpected subscription page token"),
                    }
                }
                "channels" => Ok(channel_page(
                    params.iter().find(|(key, _)| *key == "id").unwrap().1,
                )),
                "playlistItems" => {
                    assert_eq!(
                        params
                            .iter()
                            .find(|(key, _)| *key == "maxResults")
                            .unwrap()
                            .1,
                        "5"
                    );
                    Ok(upload_page(
                        params
                            .iter()
                            .find(|(key, _)| *key == "playlistId")
                            .unwrap()
                            .1,
                        6,
                    ))
                }
                _ => panic!("unexpected resource {resource}"),
            },
        )
        .unwrap();

        assert_eq!(feed.items.len(), 55 * MAX_UPLOADS_PER_CHANNEL);
        assert_eq!(feed.channels.len(), 55);
        assert_eq!(feed.channels[0].title, "channel-0");
        assert_eq!(feed.channels[0].channel_id, "UC000000000");
        assert!(
            feed.items
                .windows(2)
                .all(|pair| { pair[0].metadata.upload_date >= pair[1].metadata.upload_date })
        );
        assert_eq!(progress.first().unwrap().completed_channels, 0);
        assert_eq!(progress.last().unwrap().completed_channels, 55);
        assert_eq!(progress.last().unwrap().items.len(), feed.items.len());
        assert_eq!(progress.last().unwrap().channels, feed.channels);
        assert!(progress.iter().any(|update| update.completed_channels == 5));
        assert!(!progress.last().unwrap().reached_limit);
    }

    #[test]
    fn subscriptions_stop_at_the_documented_channel_limit() {
        let cancel = Cancellation::default();
        let mut progress = Vec::new();
        let mut pages = 0;
        let expected_pages = MAX_SUBSCRIPTIONS / SUBSCRIPTION_PAGE_SIZE;
        let feed = subscriptions_feed_with(
            &cancel,
            |update| progress.push(update),
            |resource, params| match resource {
                "subscriptions" => {
                    pages += 1;
                    assert_eq!(
                        params
                            .iter()
                            .find(|(key, _)| *key == "maxResults")
                            .unwrap()
                            .1,
                        "50"
                    );
                    let next = Some(format!("page-{}", pages + 1));
                    Ok(subscription_page((pages - 1) * 50, 50, next.as_deref()))
                }
                "channels" => Ok(channel_page(
                    params.iter().find(|(key, _)| *key == "id").unwrap().1,
                )),
                "playlistItems" => Ok(upload_page(
                    params
                        .iter()
                        .find(|(key, _)| *key == "playlistId")
                        .unwrap()
                        .1,
                    0,
                )),
                _ => panic!("unexpected resource {resource}"),
            },
        )
        .unwrap();

        assert_eq!(pages, expected_pages);
        assert!(feed.items.is_empty());
        assert_eq!(feed.channels.len(), MAX_SUBSCRIPTIONS);
        assert_eq!(progress.last().unwrap().total_channels, MAX_SUBSCRIPTIONS);
        assert_eq!(progress.last().unwrap().channels, feed.channels);
        assert!(progress.last().unwrap().reached_limit);
    }

    #[test]
    fn subscription_channels_extract_valid_https_avatar_urls_without_upload_calls() {
        let cancel = Cancellation::default();
        let mut calls = Vec::new();
        let (_, channels, reached_limit) =
            fetch_subscription_channels_with(&cancel, |resource, _| {
                calls.push(resource.to_owned());
                Ok(serde_json::json!({
                    "items": [
                        {
                            "snippet": {
                                "title": "Almanaque SOS",
                                "resourceId": { "channelId": "UC000000001" },
                                "thumbnails": {
                                    "default": { "url": "https://yt3.ggpht.com/almanaque=s88" }
                                }
                            }
                        },
                        {
                            "snippet": {
                                "title": "Canal Sem Avatar",
                                "resourceId": { "channelId": "UC000000002" },
                                "thumbnails": {
                                    "default": { "url": "javascript:alert(1)" }
                                }
                            }
                        }
                    ]
                }))
            })
            .unwrap();

        assert_eq!(calls, vec!["subscriptions"]);
        assert!(!reached_limit);
        assert_eq!(channels.len(), 2);
        assert_eq!(
            channels[0].avatar_url.as_deref(),
            Some("https://yt3.ggpht.com/almanaque=s88")
        );
        assert_eq!(channels[1].avatar_url, None);
    }

    #[test]
    fn cancellation_stops_requests_and_keeps_the_last_partial_progress() {
        let cancel = Cancellation::default();
        let worker_cancel = cancel.clone();
        let mut progress = Vec::new();
        let result = subscriptions_feed_with(
            &cancel,
            |update| progress.push(update),
            move |resource, params| match resource {
                "subscriptions" => Ok(subscription_page(0, 20, None)),
                "channels" => Ok(channel_page(
                    params.iter().find(|(key, _)| *key == "id").unwrap().1,
                )),
                "playlistItems" => {
                    let page = upload_page(
                        params
                            .iter()
                            .find(|(key, _)| *key == "playlistId")
                            .unwrap()
                            .1,
                        1,
                    );
                    if params
                        .iter()
                        .find(|(key, _)| *key == "playlistId")
                        .unwrap()
                        .1
                        == "PL000000004"
                    {
                        worker_cancel.cancel();
                    }
                    Ok(page)
                }
                _ => panic!("unexpected resource {resource}"),
            },
        );

        assert_eq!(result.err().as_deref(), Some("Consulta cancelada."));
        assert_eq!(progress[0].completed_channels, 0);
        assert_eq!(progress[0].channels.len(), 20);
        assert!(progress[0].items.is_empty());
        assert!(
            progress
                .iter()
                .any(|update| { update.completed_channels == 5 && update.items.len() == 5 })
        );
    }

    #[test]
    fn token_response_parses_and_preserves_scopes() {
        let session = TokenSession::from_response(
            &serde_json::json!({"access_token":"access", "scope": format!("openid {}", COMMENT_SCOPE)}),
            None,
        ).unwrap();
        assert!(session.has_comment_scope());
        assert_eq!(session.scopes, vec!["openid", COMMENT_SCOPE]);
        let refreshed = TokenSession::from_response(
            &serde_json::json!({"access_token":"refresh"}),
            Some(&session),
        )
        .unwrap();
        assert!(refreshed.has_comment_scope());
        let old: TokenSession = serde_json::from_value(serde_json::json!({
            "access_token":"old", "refresh_token":null, "expires_at":0
        }))
        .unwrap();
        assert!(old.scopes.is_empty());
        assert!(!old.has_comment_scope());
        assert!(!TokenSession::test_session_without_comment_scope("old").has_comment_scope());
    }

    #[test]
    fn api_failure_messages_explain_known_and_unknown_reasons() {
        let cases = [
            (
                "commentsDisabled",
                "Os coment\u{00e1}rios est\u{00e3}o desativados neste v\u{00ed}deo.",
            ),
            (
                "quotaExceeded",
                "Cota da API do YouTube esgotada. Tente novamente mais tarde.",
            ),
            (
                "forbidden",
                "A conta conectada n\u{00e3}o tem permiss\u{00e3}o para esta consulta. Se o problema continuar, reconecte a conta.",
            ),
            (
                "videoNotFound",
                "O recurso solicitado n\u{00e3}o foi encontrado no YouTube.",
            ),
        ];
        for (reason, expected) in cases {
            let body = serde_json::json!({"error":{"errors":[{"reason":reason}]}});
            assert_eq!(api_failure_message(403, &body), expected);
        }
        let unknown =
            serde_json::json!({"error":{"errors":[{"reason":"customReason"}],"message":"details"}});
        assert_eq!(
            api_failure_message(403, &unknown),
            "A API do YouTube recusou a consulta (HTTP 403, motivo: customReason). Confira permiss\u{00f5}es e cota."
        );
        assert_eq!(
            api_failure_message(403, &Value::Null),
            "A API do YouTube recusou a consulta (HTTP 403). Confira API, permiss\u{00f5}es e cota."
        );
    }

    #[test]
    fn token_response_preserves_refresh_token_and_expiration() {
        let session = TokenSession::from_response(
            &serde_json::json!({"access_token":"access", "refresh_token":"refresh", "expires_in":3600}), None,
        ).unwrap();
        assert_eq!(session.access_token, "access");
        assert_eq!(session.refresh_token.as_deref(), Some("refresh"));
        assert!(
            session.expires_at
                > SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
        );
        let refreshed = TokenSession::from_response(
            &serde_json::json!({"access_token":"new-access", "expires_in":1800}),
            Some(&session),
        )
        .unwrap();
        assert_eq!(refreshed.refresh_token.as_deref(), Some("refresh"));
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_round_trip_protects_session_bytes() {
        let plaintext = br#"{"access_token":"secret","refresh_token":"refresh"}"#;
        let encrypted = dpapi_protect(plaintext).unwrap();
        assert_ne!(encrypted, plaintext);
        assert_eq!(dpapi_unprotect(&encrypted).unwrap(), plaintext);
    }

    #[test]
    fn parses_plain_text_comments_and_page_token() {
        let input = serde_json::json!({"nextPageToken":"next", "items":[{"snippet":{"totalReplyCount":2,"topLevelComment":{"id":"comment-id","snippet":{"authorDisplayName":"Ana","textDisplay":"Olá\nMundo","likeCount":3,"publishedAt":"2026-01-01"}}}}]});
        let page = parse_comments(&input).unwrap();
        assert_eq!(page.next_page.as_deref(), Some("next"));
        assert_eq!(page.comments[0].text, "Olá\nMundo");
        assert_eq!(page.comments[0].likes, 3);
        assert_eq!(page.comments[0].replies, 2);
        assert!(parse_comments(&serde_json::json!({})).is_err());
    }

    #[test]
    fn callback_wait_expires_and_honors_cancellation() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let cancel = Cancellation::default();
        assert_eq!(
            wait_for_callback(&listener, "state", &cancel, Duration::from_millis(10)),
            Err("O login expirou; tente novamente.".into())
        );
        cancel.cancel();
        assert_eq!(
            wait_for_callback(&listener, "state", &cancel, Duration::from_secs(5)),
            Err("Login cancelado.".into())
        );
        let cancel = Cancellation::default();
        let worker_cancel = cancel.clone();
        let worker = std::thread::spawn(move || {
            wait_for_callback(&listener, "state", &worker_cancel, Duration::from_secs(5))
        });
        cancel.cancel();
        assert_eq!(worker.join().unwrap(), Err("Login cancelado.".into()));
    }

    #[test]
    fn callback_rejects_invalid_state_and_duplicate_parameters() {
        assert!(
            callback_query(
                "GET /oauth2callback?code=abc&state=bad HTTP/1.1\r\n",
                "good"
            )
            .is_none()
        );
        assert!(
            callback_query(
                "GET /oauth2callback?code=abc&state=good&state=good HTTP/1.1\r\n",
                "good"
            )
            .is_none()
        );
        assert_eq!(
            callback_query(
                "GET /oauth2callback?code=abc&state=good HTTP/1.1\r\n",
                "good"
            ),
            Some(Ok("abc".into()))
        );
        assert!(callback_query("GET /other?code=abc&state=good HTTP/1.1\r\n", "good").is_none());
    }

    #[test]
    fn upload_parser_checks_ids_and_thumbnails() {
        let mut item = serde_json::json!({"snippet":{"title":"Video", "channelTitle":"Canal", "publishedAt":"2026-01-02T00:00:00Z", "resourceId":{"videoId":"abc_123"}, "thumbnails":{"medium":{"url":"https://evil.example/x"}}}});
        let (_, video) = parse_upload(&item).unwrap();
        assert_eq!(video.video.url, "https://www.youtube.com/watch?v=abc_123");
        assert!(video.video.thumbnail.is_empty());
        item["snippet"]["resourceId"]["videoId"] = "x&other".into();
        assert!(parse_upload(&item).is_none());
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        use std::io::Read;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut raw = Vec::new();
        let mut buf = [0u8; 2048];
        let mut header_end = None;
        let mut content_length = 0usize;
        while let Ok(read) = stream.read(&mut buf) {
            if read == 0 {
                break;
            }
            raw.extend_from_slice(&buf[..read]);
            if header_end.is_none()
                && let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n")
            {
                let end = pos + 4;
                header_end = Some(end);
                let headers = String::from_utf8_lossy(&raw[..end]);
                for line in headers.lines() {
                    if let Some((name, val)) = line.split_once(':')
                        && name.trim().eq_ignore_ascii_case("content-length")
                    {
                        content_length = val.trim().parse().unwrap_or(0);
                    }
                }
            }
            if let Some(end) = header_end
                && raw.len() >= end + content_length
            {
                break;
            }
        }
        String::from_utf8_lossy(&raw).into_owned()
    }

    fn spawn_mock_json_server(
        responses: Vec<(&'static str, String)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::Write;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::with_capacity(responses.len());
            for (status_line, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                requests.push(read_http_request(&mut stream));
                let resp = format!(
                    "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(resp.as_bytes()).unwrap();
            }
            requests
        });
        (base_url, handle)
    }

    #[test]
    fn comment_order_values_match_youtube_api() {
        assert_eq!(CommentOrder::Relevance.api_value(), "relevance");
        assert_eq!(CommentOrder::Time.api_value(), "time");
    }

    #[test]
    fn parses_iso8601_durations_into_player_labels() {
        assert_eq!(parse_iso8601_duration("PT45S"), "0:45");
        assert_eq!(parse_iso8601_duration("PT3M5S"), "3:05");
        assert_eq!(parse_iso8601_duration("PT1H2M3S"), "1:02:03");
        assert_eq!(parse_iso8601_duration("PT0S"), "");
        assert_eq!(parse_iso8601_duration("invalid"), "");
    }

    #[test]
    fn liked_videos_feed_paginates_and_filters_invalid_items() {
        let cancel = Cancellation::default();
        let mut calls = 0;
        let feed = liked_videos_feed_with(&cancel, |params| {
            calls += 1;
            assert!(params.contains(&("myRating", "like")));
            if calls == 1 {
                Ok(serde_json::json!({
                    "nextPageToken": "p2",
                    "items": [
                        {
                            "id": "dQw4w9WgXcQ",
                            "snippet": {
                                "title": "Rust Demo",
                                "channelTitle": "Canal Rust",
                                "channelId": "UC0000000000000000000001",
                                "publishedAt": "2026-02-01T10:00:00Z",
                                "thumbnails": {"medium": {"url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg"}}
                            },
                            "contentDetails": {"duration": "PT4M12S"},
                            "statistics": {"viewCount": "12345"}
                        },
                        {"id": "bad id!", "snippet": {"title": "Invalid"}}
                    ]
                }))
            } else {
                assert!(params.contains(&("pageToken", "p2")));
                Ok(serde_json::json!({"items": []}))
            }
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(feed.items.len(), 1);
        assert_eq!(feed.items[0].video.title, "Rust Demo");
        assert_eq!(feed.items[0].video.duration, "4:12");
        assert_eq!(feed.items[0].metadata.view_count, Some(12345));
    }

    #[test]
    fn post_comment_and_reply_send_payloads_and_validate_inputs() {
        let cancel = Cancellation::default();
        assert!(post_comment("tok", "dQw4w9WgXcQ", "   ", &cancel).is_err());
        assert!(post_comment("tok", "bad id", "Olá", &cancel).is_err());
        assert!(post_comment_reply("tok", "parent1", "   ", &cancel).is_err());

        let comment_resp = serde_json::json!({
            "snippet": {
                "totalReplyCount": 0,
                "topLevelComment": {
                    "id": "UgxNewComment1",
                    "snippet": {
                        "authorDisplayName": "Dev",
                        "textDisplay": "Ótimo vídeo!",
                        "likeCount": 0,
                        "publishedAt": "2026-11-12T00:00:00Z"
                    }
                }
            }
        })
        .to_string();
        let reply_resp = serde_json::json!({
            "id": "UgxNewReply1",
            "snippet": {
                "authorDisplayName": "Dev",
                "textDisplay": "Concordo!",
                "likeCount": 0,
                "publishedAt": "2026-11-12T00:01:00Z"
            }
        })
        .to_string();

        let (base, server) = spawn_mock_json_server(vec![
            ("HTTP/1.1 200 OK", comment_resp),
            ("HTTP/1.1 200 OK", reply_resp),
        ]);
        let posted =
            post_comment_at(&base, "tok-abc", "dQw4w9WgXcQ", "  Ótimo vídeo! ", &cancel).unwrap();
        assert_eq!(posted.id, "UgxNewComment1");
        assert_eq!(posted.text, "Ótimo vídeo!");

        let reply = post_comment_reply_at(&base, "tok-abc", "UgxNewComment1", "Concordo!", &cancel)
            .unwrap();
        assert_eq!(reply.id, "UgxNewReply1");
        assert_eq!(reply.text, "Concordo!");

        let reqs = server.join().unwrap();
        assert!(reqs[0].starts_with("POST /commentThreads?part=snippet "));
        assert!(reqs[0].contains("\"videoId\":\"dQw4w9WgXcQ\""));
        assert!(reqs[1].starts_with("POST /comments?part=snippet "));
        assert!(reqs[1].contains("\"parentId\":\"UgxNewComment1\""));
    }

    #[test]
    fn subscriptions_and_playlists_mutations_work_against_mock_api() {
        let cancel = Cancellation::default();
        let ch_id = "UC0000000000000000000001";
        let (base, server) = spawn_mock_json_server(vec![
            (
                "HTTP/1.1 200 OK",
                serde_json::json!({"items": [{"id": ch_id}]}).to_string(),
            ),
            (
                "HTTP/1.1 200 OK",
                serde_json::json!({"items": []}).to_string(),
            ),
            (
                "HTTP/1.1 200 OK",
                serde_json::json!({"id": "SUB_123", "snippet": {"title": "Canal Rust"}})
                    .to_string(),
            ),
            ("HTTP/1.1 204 No Content", String::new()),
            (
                "HTTP/1.1 200 OK",
                serde_json::json!({"id": "PL_999", "snippet": {"title": "Estudos Rust"}})
                    .to_string(),
            ),
            (
                "HTTP/1.1 200 OK",
                serde_json::json!({"id": "PLI_111"}).to_string(),
            ),
        ]);

        let resolved = resolve_channel_id_at(
            &base,
            "tok",
            &crate::search::ChannelTarget::Handle("@rustlang".into()),
            &cancel,
        )
        .unwrap();
        assert_eq!(resolved, ch_id);

        let status = check_subscription_at(&base, "tok", ch_id, &cancel).unwrap();
        assert!(status.subscription_id.is_none());

        let sub = subscribe_channel_at(&base, "tok", ch_id, &cancel).unwrap();
        assert_eq!(sub.subscription_id.as_deref(), Some("SUB_123"));

        let unsub = unsubscribe_channel_at(&base, "tok", "SUB_123", ch_id, &cancel).unwrap();
        assert!(unsub.subscription_id.is_none());

        let (pl_id, pl_title) =
            create_playlist_at(&base, "tok", "Estudos Rust", "private", &cancel).unwrap();
        assert_eq!(pl_id, "PL_999");
        assert_eq!(pl_title, "Estudos Rust");

        let item_id =
            add_video_to_playlist_at(&base, "tok", "PL_999", "dQw4w9WgXcQ", &cancel).unwrap();
        assert_eq!(item_id, "PLI_111");

        let reqs = server.join().unwrap();
        assert!(reqs[0].contains("GET /channels?part=id&forHandle=rustlang"));
        assert!(reqs[1].contains("GET /subscriptions?part=id%2Csnippet&mine=true"));
        assert!(reqs[2].starts_with("POST /subscriptions?part=snippet "));
        assert!(reqs[3].starts_with("DELETE /subscriptions?id=SUB_123 "));
        assert!(reqs[4].starts_with("POST /playlists?part=snippet%2Cstatus "));
        assert!(reqs[5].starts_with("POST /playlistItems?part=snippet "));
    }

    #[test]
    fn comment_and_channel_profile_parsers_extract_valid_https_avatar_urls() {
        let top_level = serde_json::json!({
            "snippet": {
                "totalReplyCount": 2,
                "topLevelComment": {
                    "id": "UgxAvatar1",
                    "snippet": {
                        "authorDisplayName": "@Carlos-be6gp",
                        "authorProfileImageUrl": "//yt3.ggpht.com/avatar48=s48-c-k-c0x00ffffff-no-rj",
                        "textDisplay": "teste",
                        "publishedAt": "2026-11-13T10:00:00Z",
                        "likeCount": 66
                    }
                }
            }
        });
        let parsed = parse_top_level_comment(&top_level).unwrap();
        assert_eq!(
            parsed.author_avatar.as_deref(),
            Some("https://yt3.ggpht.com/avatar48=s48-c-k-c0x00ffffff-no-rj")
        );

        let reply = serde_json::json!({
            "id": "UgxReply1",
            "snippet": {
                "authorDisplayName": "@Pedro",
                "authorProfileImageUrl": "javascript:alert(1)",
                "textOriginal": "oi",
                "publishedAt": "2026-11-13T10:05:00Z",
                "likeCount": 1
            }
        });
        let parsed_reply = parse_reply_comment(&reply).unwrap();
        assert!(parsed_reply.author_avatar.is_none());

        let profile_json = serde_json::json!({
            "items": [{
                "id": "UC0000000000000000000001",
                "snippet": {
                    "title": "Meu Canal",
                    "thumbnails": {
                        "default": {
                            "url": "https://yt3.googleusercontent.com/me=s88-c-k-c0x00ffffff-no-rj"
                        }
                    }
                },
                "statistics": {
                    "subscriberCount": "3",
                    "hiddenSubscriberCount": false
                }
            }]
        });
        let profile = parse_my_channel_profile(&profile_json).unwrap();
        assert_eq!(
            profile.target,
            crate::search::ChannelTarget::ChannelId("UC0000000000000000000001".into())
        );
        assert_eq!(profile.title.as_deref(), Some("Meu Canal"));
        assert_eq!(
            profile.avatar_url.as_deref(),
            Some("https://yt3.googleusercontent.com/me=s88-c-k-c0x00ffffff-no-rj")
        );
        assert_eq!(profile.subscriber_count, Some(3));
    }

    #[test]
    fn report_video_abuse_resolves_reason_and_posts_to_youtube() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for step in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = [0u8; 4096];
                let count = stream.read(&mut buf).unwrap_or(0);
                requests.push(String::from_utf8_lossy(&buf[..count]).into_owned());
                if step == 0 {
                    let body = serde_json::json!({
                        "items": [{
                            "id": "V",
                            "snippet": {
                                "label": "Conte\u{00fa}do violento ou repulsivo",
                                "secondaryReasons": [
                                    { "id": "41", "label": "Viol\u{00ea}ncia gr\u{00e1}fica" }
                                ]
                            }
                        }]
                    })
                    .to_string();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                } else {
                    stream
                        .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .unwrap();
                }
            }
            requests
        });

        let base_url = format!("http://{addr}/youtube/v3");
        let cancel = Cancellation::default();
        report_video_abuse_at(
            &base_url,
            "tok-report",
            "dQw4w9WgXcQ",
            AbuseReportReason::ViolentOrRepulsive,
            &cancel,
        )
        .unwrap();

        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("GET /youtube/v3/videoAbuseReportReasons?"));
        assert!(requests[1].starts_with("POST /youtube/v3/videos/reportAbuse "));
        let body_start = requests[1].find("\r\n\r\n").unwrap() + 4;
        let payload: Value = serde_json::from_str(&requests[1][body_start..]).unwrap();
        assert_eq!(payload["videoId"], "dQw4w9WgXcQ");
        assert_eq!(payload["reasonId"], "V");
        assert_eq!(payload["secondaryReasonId"], "41");
    }

    #[test]
    fn parse_youtube_atom_feed_extracts_entries_entities_shorts_and_thumbnails() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns:yt="http://www.youtube.com/xml/schemas/2015" xmlns:media="http://search.yahoo.com/mrss/" xmlns="http://www.w3.org/2005/Atom">
  <title>Canal Principal</title>
  <author><name>Canal Principal</name></author>
  <entry>
    <yt:videoId>vidNormal01</yt:videoId>
    <yt:channelId>UC000000001</yt:channelId>
    <title>Rust &amp; FastTube &quot;v2&quot; &#39;Demo&#39;</title>
    <link rel="alternate" href="https://www.youtube.com/watch?v=vidNormal01"/>
    <author><name>Canal Principal</name></author>
    <published>2026-11-14T18:00:00+00:00</published>
    <media:group>
      <media:thumbnail url="https://i2.ytimg.com/vi/vidNormal01/hqdefault.jpg" width="480" height="360"/>
      <media:description><![CDATA[Descrição com <tags> & detalhes]]></media:description>
      <media:community><media:statistics views="4200"/></media:community>
    </media:group>
  </entry>
  <entry>
    <yt:videoId>vidShort001</yt:videoId>
    <yt:channelId>UC000000001</yt:channelId>
    <title>Quick Short</title>
    <link rel="alternate" href="https://www.youtube.com/shorts/vidShort001"/>
    <published>2026-11-15T01:00:00+00:00</published>
    <media:group>
      <media:thumbnail url="http://insecure.example.com/bad.jpg"/>
    </media:group>
  </entry>
</feed>"#;
        let parsed = parse_youtube_atom_feed(xml, "Fallback", "UC000000001").unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "2026-11-14T18:00:00+00:00");
        assert_eq!(parsed[0].1.video.title, "Rust & FastTube \"v2\" 'Demo'");
        assert_eq!(
            parsed[0].1.video.thumbnail,
            "https://i2.ytimg.com/vi/vidNormal01/hqdefault.jpg"
        );
        assert_eq!(parsed[0].1.metadata.view_count, Some(4200));
        assert_eq!(
            parsed[0].1.metadata.description.as_deref(),
            Some("Descrição com <tags> & detalhes")
        );
        assert_eq!(parsed[0].1.metadata.kind, SearchResultKind::Video);

        assert_eq!(parsed[1].1.metadata.kind, SearchResultKind::Short);
        assert_eq!(
            parsed[1].1.video.thumbnail,
            "https://i.ytimg.com/vi/vidShort001/hqdefault.jpg"
        );
    }

    #[test]
    fn subscriptions_feed_fast_uses_rss_etag_priority_and_uu_fallback() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let captured_requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let captured_for_server = std::sync::Arc::clone(&captured_requests);

        let server = std::thread::spawn(move || {
            // Round 1: UC000000002 (prioritized!) + UC000000001 (RSS fails 404 -> UU fallback)
            // Round 2: UC000000002 sends If-None-Match -> 304 Not Modified; UC000000001 -> UU fallback
            // Total expected HTTP requests = 2 RSS + 1 UU fallback (Round 1) + 2 RSS + 1 UU fallback (Round 2) = 6
            for _ in 0..6 {
                let (mut stream, _) = listener.accept().unwrap();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = [0u8; 4096];
                let count = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..count]).into_owned();
                captured_for_server.lock().unwrap().push(req.clone());

                let first_line = req.lines().next().unwrap_or("");
                let has_etag = req.lines().any(|line| {
                    line.to_ascii_lowercase()
                        .starts_with("if-none-match: \"etag-c2\"")
                });

                let response = if first_line.contains("/feeds/videos.xml?channel_id=UC000000002") {
                    if has_etag {
                        "HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_owned()
                    } else {
                        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns:yt="http://www.youtube.com/xml/schemas/2015" xmlns:media="http://search.yahoo.com/mrss/" xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <yt:videoId>c2Video0001</yt:videoId>
    <yt:channelId>UC000000002</yt:channelId>
    <title>Priority RSS Video</title>
    <published>2026-11-15T02:00:00+00:00</published>
  </entry>
</feed>"#;
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/atom+xml\r\nETag: \"etag-c2\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            xml.len(),
                            xml
                        )
                    }
                } else if first_line.contains("/feeds/videos.xml?channel_id=UC000000001") {
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_owned()
                } else if first_line.contains("/youtube/v3/playlistItems?")
                    && first_line.contains("playlistId=UU000000001")
                {
                    let body = serde_json::json!({
                        "items": [{
                            "snippet": {
                                "title": "Fallback UU Video",
                                "channelId": "UC000000001",
                                "channelTitle": "Canal 1",
                                "publishedAt": "2026-11-14T20:00:00Z",
                                "resourceId": { "videoId": "c1Fallback01" }
                            }
                        }]
                    })
                    .to_string();
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                } else {
                    "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_owned()
                };
                stream.write_all(response.as_bytes()).unwrap();
            }
        });

        let api_base = format!("http://{addr}/youtube/v3");
        let rss_base = format!("http://{addr}");
        let cancel = Cancellation::default();
        let known_channels = vec![
            HomeChannel {
                title: "Canal 1".into(),
                channel_id: "UC000000001".into(),
                avatar_url: None,
            },
            HomeChannel {
                title: "Canal 2".into(),
                channel_id: "UC000000002".into(),
                avatar_url: None,
            },
        ];

        // Round 1: prioritizes UC000000002, hits RSS for C2, falls back to UU playlist for C1
        let mut first_progress = Vec::new();
        let out1 = subscriptions_feed_fast_at(
            &api_base,
            &rss_base,
            "test-token",
            SubscriptionFeedOptions {
                known_channels: Some(known_channels.clone()),
                prioritized_channel_ids: vec!["UC000000002".into()],
                rss_cache: HashMap::new(),
            },
            &cancel,
            |p| first_progress.push(p),
        )
        .unwrap();

        assert!(out1.used_cached_channels);
        assert_eq!(out1.rss_hits, 1);
        assert_eq!(out1.api_fallbacks, 1);
        assert_eq!(out1.feed.items.len(), 2);
        assert_eq!(out1.feed.items[0].video.title, "Priority RSS Video");
        assert_eq!(out1.feed.items[1].video.title, "Fallback UU Video");
        assert_eq!(
            out1.rss_cache
                .get("UC000000002")
                .and_then(|e| e.etag.as_deref()),
            Some("\"etag-c2\"")
        );

        // Round 2: passes previous rss_cache -> receives 304 Not Modified for C2
        let out2 = subscriptions_feed_fast_at(
            &api_base,
            &rss_base,
            "test-token",
            SubscriptionFeedOptions {
                known_channels: Some(known_channels),
                prioritized_channel_ids: vec!["UC000000002".into()],
                rss_cache: out1.rss_cache,
            },
            &cancel,
            |_| {},
        )
        .unwrap();

        assert_eq!(out2.not_modified_hits, 1);
        assert_eq!(out2.feed.items.len(), 2);
        assert_eq!(out2.feed.items[0].video.title, "Priority RSS Video");

        server.join().unwrap();
        let recorded = captured_requests.lock().unwrap();
        assert!(
            !recorded
                .iter()
                .any(|req| req.contains("/youtube/v3/subscriptions")
                    || req.contains("/youtube/v3/channels")),
            "unexpected subscriptions/channels API call: {recorded:#?}"
        );
    }

    #[test]
    fn subscriptions_disk_cache_round_trips_atomically_and_rejects_corrupt_files() {
        let dir =
            std::env::temp_dir().join(format!("fasttube-subs-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cache_path = dir.join("subscriptions-cache.json");

        let feed = HomeFeed {
            items: vec![SearchResult {
                video: Video {
                    title: "Cached Video".into(),
                    channel: "Canal 1".into(),
                    url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
                    thumbnail: "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".into(),
                    duration: String::new(),
                },
                metadata: VideoMetadata {
                    upload_date: Some("2026-11-15T01:00:00+00:00".into()),
                    channel_id: Some("UC000000001".into()),
                    view_count: Some(99),
                    kind: SearchResultKind::Video,
                    ..Default::default()
                },
            }],
            playlists: Vec::new(),
            channels: vec![HomeChannel {
                title: "Canal 1".into(),
                channel_id: "UC000000001".into(),
                avatar_url: Some("https://yt3.ggpht.com/avatar=s88".into()),
            }],
        };
        let mut rss_cache = HashMap::new();
        rss_cache.insert(
            "UC000000001".into(),
            ChannelRssCacheEntry {
                etag: Some("\"etag-1\"".into()),
                last_modified: None,
                items: feed.items.clone(),
            },
        );
        let snapshot = SubscriptionsDiskCache::from_feed(&feed, &rss_cache, 1_794_000_000);
        save_subscriptions_cache_to_path(&cache_path, &snapshot).unwrap();

        let loaded = load_subscriptions_cache_from_path(&cache_path)
            .unwrap()
            .unwrap();
        let (saved_at, channels, items, restored_rss) = loaded.into_parts();
        assert_eq!(saved_at, 1_794_000_000);
        assert_eq!(channels.len(), 1);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].video.title, "Cached Video");
        assert_eq!(
            restored_rss
                .get("UC000000001")
                .and_then(|e| e.etag.as_deref()),
            Some("\"etag-1\"")
        );

        delete_subscriptions_cache_at_path(&cache_path).unwrap();
        assert!(
            load_subscriptions_cache_from_path(&cache_path)
                .unwrap()
                .is_none()
        );

        std::fs::write(&cache_path, b"{not valid json").unwrap();
        assert!(load_subscriptions_cache_from_path(&cache_path).is_err());
        assert_eq!(std::fs::read(&cache_path).unwrap(), b"{not valid json");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
