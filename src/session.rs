//! A dedicated browser profile for YouTube sign-in. The user's normal profile is never opened.
use crate::process::Cancellation;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub fn profile_dir() -> Result<PathBuf, String> {
    let base = crate::settings::settings_path().map_err(|e| e.to_string())?;
    Ok(base
        .parent()
        .ok_or("Pasta do FastTube indisponível")?
        .join("browser-profile"))
}

pub fn has_saved_profile() -> bool {
    profile_dir().is_ok_and(|p| p.join("Default").exists())
}

const APP_PATHS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths";

fn first_existing_file(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|path| path.is_file())
}

fn registry_app_path(executable: &str) -> Option<PathBuf> {
    let candidates = ["HKCU", "HKLM"].into_iter().filter_map(|hive| {
        let key = format!(r"{hive}\{APP_PATHS_KEY}\{executable}");
        let output = Command::new("reg")
            .args(["query", &key, "/ve"])
            .creation_flags_no_window()
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        text.lines().find_map(|line| {
            line.split_once("REG_SZ")
                .map(|(_, value)| PathBuf::from(value.trim().trim_matches('"')))
        })
    });
    first_existing_file(candidates)
}

fn browser_executable() -> Result<PathBuf, String> {
    // Prefer Vivaldi, then Edge, Chrome, and Brave (App Paths registry first, then standard paths).
    for executable in ["vivaldi.exe", "msedge.exe", "chrome.exe", "brave.exe"] {
        if let Some(path) = registry_app_path(executable) {
            return Ok(path);
        }
    }
    let roots = [
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        std::env::var_os("ProgramFiles(x86)").map(PathBuf::from),
        std::env::var_os("ProgramFiles").map(PathBuf::from),
    ];
    let candidates = [
        r"Vivaldi\Application\vivaldi.exe",
        r"Microsoft\Edge\Application\msedge.exe",
        r"Google\Chrome\Application\chrome.exe",
        r"BraveSoftware\Brave-Browser\Application\brave.exe",
    ]
    .into_iter()
    .flat_map(|relative| roots.iter().flatten().map(move |root| root.join(relative)));
    if let Some(path) = first_existing_file(candidates) {
        return Ok(path);
    }
    Err("Instale Vivaldi, Microsoft Edge, Google Chrome ou Brave para entrar na sua conta.".into())
}

trait HiddenCommand {
    fn creation_flags_no_window(&mut self) -> &mut Self;
}
impl HiddenCommand for Command {
    fn creation_flags_no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(0x08000000);
        }
        self
    }
}

struct Browser(Child);
impl Drop for Browser {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn debug_json(port: u16, path: &str) -> Result<Value, String> {
    let mut stream =
        TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2))
            .map_err(|_| "Navegador indisponível")?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .map_err(|_| "Falha ao consultar navegador")?;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = stream
            .read(&mut buffer)
            .map_err(|_| "Falha ao consultar navegador")?;
        if count == 0 {
            break;
        }
        if bytes.len() + count > 2 * 1024 * 1024 {
            return Err("Resposta inválida do navegador".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(start) = bytes.windows(4).position(|x| x == b"\r\n\r\n")
            && let Ok(pages) = serde_json::from_slice::<Value>(&bytes[start + 4..])
        {
            return Ok(pages);
        }
    }
    let Some(start) = bytes.windows(4).position(|x| x == b"\r\n\r\n") else {
        return Err("Resposta inválida do navegador".into());
    };
    serde_json::from_slice::<Value>(&bytes[start + 4..])
        .map_err(|_| "Resposta inválida do navegador".to_owned())
}

fn page_socket(port: u16) -> Result<String, String> {
    debug_json(port, "/json/list")?
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|v| v.get("type").and_then(Value::as_str) == Some("page"))
        })
        .and_then(|v| v.get("webSocketDebuggerUrl"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "A janela de login foi fechada.".to_owned())
}

fn cookies(port: u16) -> Result<Vec<Value>, String> {
    let url = page_socket(port)?;
    if !url.starts_with(&format!("ws://127.0.0.1:{port}/")) {
        return Err("Endereço local do navegador inválido.".into());
    }
    let (mut socket, _) = tungstenite::connect(url.as_str())
        .map_err(|_| "Não foi possível acessar a sessão do navegador")?;
    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket.get_mut() {
        stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
    }
    socket.send(tungstenite::Message::Text(json!({"id":1,"method":"Network.getCookies","params":{"urls":["https://www.youtube.com/"]}}).to_string().into())).map_err(|_| "Falha ao verificar a sessão")?;
    loop {
        let msg = socket.read().map_err(|_| "Falha ao verificar a sessão")?;
        if let tungstenite::Message::Text(text) = msg {
            let v: Value =
                serde_json::from_str(&text).map_err(|_| "Resposta inválida da sessão")?;
            if v.get("id").and_then(Value::as_u64) == Some(1) {
                return v
                    .pointer("/result/cookies")
                    .and_then(Value::as_array)
                    .cloned()
                    .ok_or_else(|| "Não foi possível ler a sessão do YouTube.".into());
            }
        }
    }
}

fn signed_in(cookies: &[Value]) -> bool {
    cookies.iter().any(|c| {
        matches!(
            c.get("name").and_then(Value::as_str),
            Some("SAPISID" | "__Secure-3PAPISID")
        ) && c
            .get("value")
            .and_then(Value::as_str)
            .is_some_and(|v| !v.is_empty())
    }) && cookies.iter().any(|c| {
        matches!(
            c.get("name").and_then(Value::as_str),
            Some("SID" | "__Secure-3PSID")
        ) && c
            .get("value")
            .and_then(Value::as_str)
            .is_some_and(|v| !v.is_empty())
    })
}

pub fn youtube_cookies(interactive: bool, cancel: &Cancellation) -> Result<Vec<Value>, String> {
    let profile = profile_dir()?;
    if !interactive && !profile.join("Default").exists() {
        return Err("Entre na sua conta Google para continuar.".into());
    }
    std::fs::create_dir_all(&profile)
        .map_err(|_| "Não foi possível criar o perfil do navegador.")?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|_| "Não foi possível iniciar a sessão local.")?;
    let port = listener
        .local_addr()
        .map_err(|_| "Não foi possível iniciar a sessão local.")?
        .port();
    drop(listener);
    let mut command = Command::new(browser_executable()?);
    command
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--remote-debugging-port={port}"))
        .args([
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-mode",
        ]);
    if !interactive {
        command.args(["--headless=new", "--disable-gpu"]);
    }
    command.arg(if interactive { "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fwww.youtube.com%2F" } else { "https://www.youtube.com/" });
    command.stdout(Stdio::null()).stderr(Stdio::null());
    if !interactive {
        command.creation_flags_no_window();
    }
    let mut browser = Browser(
        command
            .spawn()
            .map_err(|_| "Não foi possível abrir o navegador para login.")?,
    );
    let start = Instant::now();
    let timeout = if interactive {
        Duration::from_secs(300)
    } else {
        Duration::from_secs(25)
    };
    loop {
        if cancel.cancelled() {
            return Err("Login cancelado.".into());
        }
        if browser.0.try_wait().ok().flatten().is_some() {
            return Err("A janela de login foi fechada.".into());
        }
        if start.elapsed() > Duration::from_secs(15) {
            return Err("O navegador não iniciou a sessão.".into());
        }
        if page_socket(port).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    loop {
        if cancel.cancelled() {
            return Err("Login cancelado.".into());
        }
        if browser.0.try_wait().ok().flatten().is_some() {
            return Err("A janela de login foi fechada.".into());
        }
        if start.elapsed() >= timeout {
            return Err(if interactive {
                "Tempo de login esgotado."
            } else {
                "Sua sessão expirou. Entre novamente."
            }
            .into());
        }
        if let Ok(cookies) = cookies(port)
            && signed_in(&cookies)
        {
            // Closing through DevTools flushes the dedicated browser profile.
            if let Ok(data) = debug_json(port, "/json/version")
                && let Some(url) = data
                    .get("webSocketDebuggerUrl")
                    .and_then(Value::as_str)
                    .filter(|url| url.starts_with(&format!("ws://127.0.0.1:{port}/")))
                && let Ok((mut socket, _)) = tungstenite::connect(url)
            {
                let _ = socket.send(tungstenite::Message::Text(
                    json!({"id":2,"method":"Browser.close"}).to_string().into(),
                ));
            }
            for _ in 0..50 {
                if browser.0.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            return Ok(cookies);
        }
        if !interactive && start.elapsed() > Duration::from_secs(8) {
            return Err("Entre na sua conta Google para continuar.".into());
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_requires_both_youtube_authentication_cookies() {
        let a = json!({"name":"SAPISID","value":"test"});
        let b = json!({"name":"__Secure-3PSID","value":"test"});
        assert!(!signed_in(&[]));
        assert!(!signed_in(std::slice::from_ref(&a)));
        assert!(signed_in(&[a, b]));
    }

    #[test]
    fn reads_browser_cookies_from_local_debug_endpoint() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let (mut http, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 512];
            loop {
                let count = http.read(&mut buffer).unwrap();
                assert_ne!(count, 0, "HTTP request ended before its headers");
                request.extend_from_slice(&buffer[..count]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let body = format!(
                r#"[{{"type":"page","webSocketDebuggerUrl":"ws://127.0.0.1:{port}/devtools/page/test"}}]"#
            );
            write!(
                http,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            drop(http);
            let (tcp, _) = listener.accept().unwrap();
            let mut socket = tungstenite::accept(tcp).unwrap();
            let request = socket.read().unwrap();
            assert!(request.to_text().unwrap().contains("Network.getCookies"));
            socket.send(tungstenite::Message::Text(json!({"id":1,"result":{"cookies":[{"name":"SAPISID","value":"test"},{"name":"SID","value":"test"}]}}).to_string().into())).unwrap();
        });
        let data = cookies(port).unwrap();
        assert!(signed_in(&data));
        worker.join().unwrap();
    }

    #[test]
    #[ignore = "requires an installed Chromium browser"]
    fn dedicated_browser_debug_interface_smoke() {
        let profile = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("browser-smoke-{}", std::process::id()));
        std::fs::create_dir_all(&profile).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut command = Command::new(browser_executable().unwrap());
        command
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg(format!("--remote-debugging-port={port}"))
            .args([
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-mode",
                "--headless=new",
                "--disable-gpu",
                "about:blank",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags_no_window();
        let _browser = Browser(command.spawn().unwrap());
        let started = Instant::now();
        let cookie_data = loop {
            if let Ok(values) = cookies(port) {
                break values;
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "browser page did not become ready"
            );
            std::thread::sleep(Duration::from_millis(200));
        };
        assert!(!signed_in(&cookie_data));
        let version = debug_json(port, "/json/version").unwrap();
        assert!(
            version
                .get("webSocketDebuggerUrl")
                .and_then(Value::as_str)
                .is_some_and(|url| url.starts_with(&format!("ws://127.0.0.1:{port}/")))
        );
        let (mut socket, _) = tungstenite::connect(page_socket(port).unwrap().as_str()).unwrap();
        for (id, name) in [(1, "SAPISID"), (2, "SID")] {
            socket.send(tungstenite::Message::Text(json!({"id":id,"method":"Network.setCookie","params":{"url":"https://www.youtube.com/","name":name,"value":"test-only","secure":true}}).to_string().into())).unwrap();
            loop {
                let msg = socket.read().unwrap();
                if let Ok(value) = serde_json::from_str::<Value>(msg.to_text().unwrap_or(""))
                    && value.get("id").and_then(Value::as_i64) == Some(id)
                {
                    assert_eq!(
                        value.pointer("/result/success").and_then(Value::as_bool),
                        Some(true)
                    );
                    break;
                }
            }
        }
        assert!(signed_in(&cookies(port).unwrap()));
    }

    #[test]
    fn debug_json_parses_fragmented_http_response_closed_at_eof() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let (mut http, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 512];
            loop {
                let count = http.read(&mut buffer).unwrap();
                assert_ne!(count, 0, "HTTP request ended before its headers");
                request.extend_from_slice(&buffer[..count]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            http.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n[{\"type\":")
                .unwrap();
            std::thread::sleep(Duration::from_millis(20));
            let rest = format!(
                r#""page","webSocketDebuggerUrl":"ws://127.0.0.1:{port}/devtools/page/test"}}]"#
            );
            http.write_all(rest.as_bytes()).unwrap();
            let _ = http.shutdown(std::net::Shutdown::Write);
            drop(http);
        });
        let value = debug_json(port, "/json/list").unwrap();
        assert_eq!(
            value,
            json!([{
                "type": "page",
                "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/test")
            }])
        );
        worker.join().unwrap();
    }

    #[test]
    fn first_existing_file_respects_priority_order() {
        struct TempDirGuard(PathBuf);
        impl Drop for TempDirGuard {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let dir =
            std::env::temp_dir().join(format!("fasttube-session-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _guard = TempDirGuard(dir.clone());

        let vivaldi = dir.join("vivaldi.exe");
        let edge = dir.join("msedge.exe");
        let chrome = dir.join("chrome.exe");
        let brave = dir.join("brave.exe");
        std::fs::write(&edge, b"").unwrap();
        std::fs::write(&chrome, b"").unwrap();

        assert_eq!(
            first_existing_file([vivaldi.clone(), edge.clone(), chrome.clone(), brave.clone()]),
            Some(edge.clone())
        );
        assert_eq!(
            first_existing_file([vivaldi.clone(), chrome.clone(), edge, brave.clone()]),
            Some(chrome)
        );
        assert_eq!(first_existing_file([vivaldi, brave]), None);
    }
}
