use crate::process::Cancellation;
use crate::search::{HomeChannel, HomeFeed, SearchResult, Video, VideoMetadata};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
#[cfg(not(test))]
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zeroize::Zeroize;

pub const COMMENT_SCOPE: &str = "https://www.googleapis.com/auth/youtube.force-ssl";
const SUBSCRIPTION_PAGE_SIZE: usize = 50;
const MAX_SUBSCRIPTIONS: usize = 100;
const MAX_UPLOADS_PER_CHANNEL: usize = 5;
const MAX_PLAYLISTS: usize = 100;
const MAX_PLAYLIST_VIDEOS: usize = 200;
const PROGRESS_CHANNEL_BATCH: usize = 5;
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

fn api(
    client: &reqwest::blocking::Client,
    token: &str,
    resource: &str,
    params: &[(&str, &str)],
) -> Result<Value, String> {
    let response = client
        .get(format!("https://www.googleapis.com/youtube/v3/{resource}"))
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
        Err(_) => return Err("Resposta inv\u{e1}lida da API do YouTube.".into()),
    };
    if !status.is_success() {
        return Err(api_failure_message(status.as_u16(), &body));
    }
    Ok(body)
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
        Some("videoNotFound" | "commentThreadNotFound" | "playlistNotFound" | "channelNotFound") => "O recurso solicitado n\u{00e3}o foi encontrado no YouTube.".into(),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub id: String,
    pub author: String,
    pub text: String,
    pub published: String,
    pub likes: u64,
    pub replies: u64,
}

pub struct CommentsPage {
    pub comments: Vec<Comment>,
    pub next_page: Option<String>,
}

fn parse_comments(json: &Value) -> Result<CommentsPage, String> {
    let items = json["items"]
        .as_array()
        .ok_or("Resposta de comentários inválida.")?;
    let comments = items
        .iter()
        .take(25)
        .filter_map(|item| {
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
                text: snippet["textDisplay"]
                    .as_str()?
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
        })
        .collect();
    let next_page = json["nextPageToken"]
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= 1024)
        .map(str::to_owned);
    Ok(CommentsPage {
        comments,
        next_page,
    })
}

pub fn comments_page(
    token: &str,
    video_id: &str,
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
        ("maxResults", "25"),
        ("order", "relevance"),
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

fn parse_comment_replies(json: &Value) -> Result<CommentsPage, String> {
    let items = json["items"]
        .as_array()
        .ok_or("Resposta de respostas inv\u{00e1}lida.")?;
    let comments = items
        .iter()
        .take(100)
        .filter_map(|item| {
            let snippet = item.get("snippet")?;
            Some(Comment {
                id: item["id"].as_str()?.to_owned(),
                author: snippet["authorDisplayName"]
                    .as_str()
                    .unwrap_or("Usu\u{00e1}rio")
                    .chars()
                    .take(256)
                    .collect(),
                text: snippet["textDisplay"]
                    .as_str()?
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
        })
        .collect();
    let next_page = json["nextPageToken"]
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= 1024)
        .map(str::to_owned);
    Ok(CommentsPage {
        comments,
        next_page,
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
        ids.push(channel_id.to_owned());
        channels.push(HomeChannel {
            title: title.to_owned(),
            channel_id: channel_id.to_owned(),
        });
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscriptionProgress {
    pub items: Vec<SearchResult>,
    pub channels: Vec<HomeChannel>,
    pub completed_channels: usize,
    pub total_channels: usize,
    pub reached_limit: bool,
}

pub fn subscriptions_feed(
    token: &str,
    cancel: &Cancellation,
    mut progress: impl FnMut(SubscriptionProgress),
) -> Result<HomeFeed, String> {
    let client = http_client()?;
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
        let subscriptions = api(&client, token, "subscriptions", &params)?;
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

    let total_channels = ids.len();
    progress(SubscriptionProgress {
        items: Vec::new(),
        channels: subscription_channels.clone(),
        completed_channels: 0,
        total_channels,
        reached_limit,
    });
    let mut upload_playlists = Vec::new();
    for channel_ids in ids.chunks(50) {
        if cancel.cancelled() {
            return Err("Consulta cancelada.".into());
        }
        let joined_ids = channel_ids.join(",");
        let channels = api(
            &client,
            token,
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
        let uploads_by_channel: std::collections::HashMap<_, _> = channels
            .iter()
            .enumerate()
            .filter_map(|(index, channel)| {
                let channel_id = channel
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| channel_ids.get(index).map(String::as_str))?;
                let playlist_id = channel
                    .pointer("/contentDetails/relatedPlaylists/uploads")?
                    .as_str()?;
                valid_id(playlist_id).then(|| (channel_id.to_owned(), playlist_id.to_owned()))
            })
            .collect();
        upload_playlists.extend(
            channel_ids
                .iter()
                .map(|channel_id| uploads_by_channel.get(channel_id).cloned()),
        );
    }

    let worker_count = upload_playlists.len().min(8);
    if worker_count == 0 {
        return Ok(HomeFeed {
            items: Vec::new(),
            playlists: Vec::new(),
            channels: subscription_channels,
        });
    }
    let next_index = std::sync::atomic::AtomicUsize::new(0);
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut videos = Vec::new();
    let mut completed_channels = 0;
    let mut fetch_error = None;
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let sender = sender.clone();
            let client = &client;
            let playlists = &upload_playlists;
            let next_index = &next_index;
            scope.spawn(move || {
                loop {
                    if cancel.cancelled() {
                        break;
                    }
                    let index = next_index.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(playlist) = playlists.get(index) else {
                        break;
                    };
                    let result = match playlist {
                        Some(playlist) => api(
                            client,
                            token,
                            "playlistItems",
                            &[
                                ("part", "snippet"),
                                ("playlistId", playlist),
                                ("maxResults", "5"),
                            ],
                        ),
                        None => Ok(Value::Null),
                    };
                    if sender.send((result, index + 1)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        while let Ok((result, _index)) = receiver.recv() {
            completed_channels += 1;
            match result {
                Ok(uploads) => {
                    if let Some(items) = uploads["items"].as_array() {
                        videos.extend(
                            items
                                .iter()
                                .take(MAX_UPLOADS_PER_CHANNEL)
                                .filter_map(parse_upload),
                        );
                    }
                }
                Err(error) => {
                    if fetch_error.is_none() {
                        fetch_error = Some(error);
                    }
                }
            }
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
    });
    if cancel.cancelled() {
        return Err("Consulta cancelada.".into());
    }
    if let Some(error) = fetch_error {
        return Err(error);
    }
    videos.sort_by(|left, right| right.0.cmp(&left.0));
    Ok(HomeFeed {
        items: videos.into_iter().map(|(_, video)| video).collect(),
        playlists: Vec::new(),
        channels: subscription_channels,
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
        if let Some(token) = page_token.as_deref() {
            params.push(("pageToken", token));
        }
        let subscriptions = fetch("subscriptions", &params)?;
        let items = subscriptions["items"]
            .as_array()
            .ok_or("Lista de inscri\u{e7}\u{f5}es inv\u{e1}lida.")?;
        append_subscription_channels(items, &mut ids, &mut subscription_channels);
        let next_page = subscriptions["nextPageToken"]
            .as_str()
            .filter(|token| !token.is_empty() && token.len() <= 1024)
            .map(str::to_owned);
        if ids.len() == MAX_SUBSCRIPTIONS {
            reached_limit = next_page.is_some();
            break;
        }
        let Some(next_page) = next_page else { break };
        page_token = Some(next_page);
    }

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
        let feed = subscriptions_feed_with(
            &cancel,
            |update| progress.push(update),
            |resource, params| match resource {
                "subscriptions" => {
                    pages += 1;
                    if pages == 1 {
                        Ok(subscription_page(0, 50, Some("page-2")))
                    } else {
                        assert_eq!(
                            params
                                .iter()
                                .find(|(key, _)| *key == "maxResults")
                                .unwrap()
                                .1,
                            "50"
                        );
                        Ok(subscription_page(50, 50, Some("page-3")))
                    }
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

        assert_eq!(pages, 2);
        assert!(feed.items.is_empty());
        assert_eq!(feed.channels.len(), MAX_SUBSCRIPTIONS);
        assert_eq!(progress.last().unwrap().total_channels, MAX_SUBSCRIPTIONS);
        assert_eq!(progress.last().unwrap().channels, feed.channels);
        assert!(progress.last().unwrap().reached_limit);
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
}
