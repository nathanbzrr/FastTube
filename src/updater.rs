//! In-app yt-dlp version check and atomic binary updater.

use crate::process::{self, Cancellation};
use crate::search;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const GITHUB_LATEST_URL: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const API_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const MIN_EXE_BYTES: usize = 1024 * 1024;
const MAX_EXE_BYTES: usize = 96 * 1024 * 1024;

static UPDATE_TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YtDlpUpdateCheck {
    pub current_version: Option<String>,
    pub latest_version: String,
    pub download_url: String,
    pub update_available: bool,
    pub install_path: PathBuf,
}

pub fn local_bin_yt_dlp_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty())?;
    let root = PathBuf::from(local);
    if !root.is_absolute() {
        return None;
    }
    #[cfg(windows)]
    let name = "yt-dlp.exe";
    #[cfg(not(windows))]
    let name = "yt-dlp";
    Some(root.join("FastTube").join("bin").join(name))
}

pub fn preferred_install_path() -> Result<PathBuf, String> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        #[cfg(windows)]
        let bundled = parent.join("yt-dlp.exe");
        #[cfg(not(windows))]
        let bundled = parent.join("yt-dlp");
        if bundled.exists() && is_directory_writable(parent) {
            return Ok(bundled);
        }
    }
    local_bin_yt_dlp_path()
        .ok_or_else(|| "Diretório LOCALAPPDATA indisponível para atualizar o yt-dlp.".to_owned())
}

fn is_directory_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".fasttube-write-test-{}", std::process::id()));
    match OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn installed_version(cancel: &Cancellation) -> Option<String> {
    let mut cmd = search::yt_dlp_command();
    cmd.arg("--version");
    let out = process::output(&mut cmd, cancel, Duration::from_secs(6)).ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

fn is_allowed_github_download(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let lower = host.to_ascii_lowercase();
    lower == "github.com"
        || lower.ends_with(".githubusercontent.com")
        || lower == "release-assets.githubusercontent.com"
}

pub fn parse_github_release_json(bytes: &[u8]) -> Result<(String, String), String> {
    let json: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| "Resposta inválida da API do GitHub.".to_owned())?;
    let tag = json
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Versão não informada pela API do GitHub.".to_owned())?
        .to_owned();

    #[cfg(windows)]
    let wanted_asset = "yt-dlp.exe";
    #[cfg(not(windows))]
    let wanted_asset = "yt-dlp";

    let download_url = json
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .and_then(|assets| {
            assets.iter().find_map(|asset| {
                let name = asset.get("name").and_then(serde_json::Value::as_str)?;
                if name.eq_ignore_ascii_case(wanted_asset) {
                    asset
                        .get("browser_download_url")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                } else {
                    None
                }
            })
        })
        .filter(|u| is_allowed_github_download(u))
        .ok_or_else(|| "Binário yt-dlp.exe não encontrado na release mais recente.".to_owned())?;

    Ok((tag, download_url))
}

pub fn check_for_update(cancel: &Cancellation) -> Result<YtDlpUpdateCheck, String> {
    if cancel.cancelled() {
        return Err("Verificação cancelada.".to_owned());
    }
    let current_version = installed_version(cancel);
    let client = reqwest::blocking::Client::builder()
        .timeout(API_TIMEOUT)
        .user_agent("FastTube-Updater/0.3")
        .build()
        .map_err(|_| "Não foi possível iniciar o cliente HTTP.".to_owned())?;
    let resp = client
        .get(GITHUB_LATEST_URL)
        .send()
        .map_err(|_| "Não foi possível consultar atualizações do yt-dlp no GitHub.".to_owned())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub retornou HTTP {}.", resp.status().as_u16()));
    }
    let mut body = Vec::new();
    resp.take(512 * 1024)
        .read_to_end(&mut body)
        .map_err(|_| "Falha ao ler resposta do GitHub.".to_owned())?;
    let (latest_version, download_url) = parse_github_release_json(&body)?;
    let install_path = preferred_install_path()?;
    let update_available = current_version
        .as_deref()
        .is_none_or(|cur| cur.trim() != latest_version.trim());
    Ok(YtDlpUpdateCheck {
        current_version,
        latest_version,
        download_url,
        update_available,
        install_path,
    })
}

pub fn download_and_install(
    download_url: &str,
    install_path: &Path,
    cancel: &Cancellation,
) -> Result<PathBuf, String> {
    if cancel.cancelled() {
        return Err("Atualização cancelada.".to_owned());
    }
    if !is_allowed_github_download(download_url) {
        return Err("URL de download não permitida.".to_owned());
    }
    let parent = install_path
        .parent()
        .ok_or_else(|| "Caminho de instalação inválido.".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("Não foi possível criar diretório de destino: {e}"))?;

    let client = reqwest::blocking::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .user_agent("FastTube-Updater/0.3")
        .build()
        .map_err(|_| "Não foi possível iniciar o download.".to_owned())?;
    let resp = client
        .get(download_url)
        .send()
        .map_err(|_| "Falha de conexão ao baixar yt-dlp.".to_owned())?;
    if !resp.status().is_success() {
        return Err(format!(
            "Download retornou HTTP {}.",
            resp.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    resp.take((MAX_EXE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Falha durante a leitura do binário yt-dlp.".to_owned())?;
    if cancel.cancelled() {
        return Err("Atualização cancelada.".to_owned());
    }
    if bytes.len() < MIN_EXE_BYTES || bytes.len() > MAX_EXE_BYTES {
        return Err("Tamanho inesperado para o executável yt-dlp.".to_owned());
    }
    #[cfg(windows)]
    if !bytes.starts_with(b"MZ") {
        return Err("Arquivo baixado não é um executável Windows válido.".to_owned());
    }

    let (temp_path, mut temp_file) = loop {
        let seq = UPDATE_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".fasttube-ytdlp-{}-{seq}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Erro preparando arquivo temporário: {e}")),
        }
    };

    let write_res = (|| -> std::io::Result<()> {
        temp_file.write_all(&bytes)?;
        temp_file.sync_all()?;
        drop(temp_file);
        fs::rename(&temp_path, install_path)
    })();
    if let Err(err) = write_res {
        let _ = fs::remove_file(&temp_path);
        return Err(format!("Não foi possível substituir o yt-dlp: {err}"));
    }
    Ok(install_path.to_path_buf())
}

// ============================================================================
// FastTube self-updater (GitHub Releases)
// ============================================================================

pub const FASTTUBE_GITHUB_LATEST_URL: &str =
    "https://api.github.com/repos/nathanbzrr/FastTube/releases/latest";
pub const FASTTUBE_RELEASES_PAGE_URL: &str = "https://github.com/nathanbzrr/FastTube/releases";
pub const CURRENT_APP_VERSION: &str = env!("CARGO_PKG_VERSION");

const APP_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);
const MIN_APP_ARTIFACT_BYTES: usize = 1024 * 1024;
const MAX_APP_ARTIFACT_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppInstallKind {
    Installer,
    Portable,
}

impl AppInstallKind {
    pub fn label(self, pt: bool) -> &'static str {
        match (self, pt) {
            (Self::Installer, true) => "Instalado (Setup)",
            (Self::Installer, false) => "Installed (Setup)",
            (Self::Portable, true) => "Portátil (.zip)",
            (Self::Portable, false) => "Portable (.zip)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppUpdateCheck {
    pub current_version: String,
    pub latest_version: String,
    pub tag_name: String,
    pub release_url: String,
    pub release_notes: String,
    pub asset_name: String,
    pub download_url: String,
    pub checksums_url: Option<String>,
    pub install_kind: AppInstallKind,
    pub update_available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppUpdatePrepared {
    pub version: String,
    pub install_kind: AppInstallKind,
    pub artifact_path: PathBuf,
    pub staging_dir: Option<PathBuf>,
    pub target_dir: PathBuf,
    pub target_exe: PathBuf,
}

pub fn normalize_version_str(raw: &str) -> &str {
    raw.trim().trim_start_matches(['v', 'V']).trim()
}

pub fn parse_semver(raw: &str) -> Option<(u64, u64, u64, Option<String>)> {
    let cleaned = normalize_version_str(raw);
    if cleaned.is_empty() {
        return None;
    }
    let cleaned = cleaned.split('+').next().unwrap_or(cleaned);
    let (core, pre) = match cleaned.split_once('-') {
        Some((c, p)) if !p.trim().is_empty() => (c, Some(p.trim().to_ascii_lowercase())),
        Some((c, _)) => (c, None),
        None => (cleaned, None),
    };
    let mut parts = core.split('.');
    let major: u64 = parts.next()?.parse().ok()?;
    let minor: u64 = parts.next()?.parse().ok()?;
    let patch: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch, pre))
}

pub fn is_newer_version(current: &str, latest: &str) -> bool {
    let (Some((cmaj, cmin, cpat, cpre)), Some((lmaj, lmin, lpat, lpre))) =
        (parse_semver(current), parse_semver(latest))
    else {
        return false;
    };
    match (lmaj, lmin, lpat).cmp(&(cmaj, cmin, cpat)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match (cpre, lpre) {
            (Some(_), None) => true,
            (None, Some(_)) | (None, None) => false,
            (Some(c), Some(l)) => compare_prerelease(&l, &c) == std::cmp::Ordering::Greater,
        },
    }
}

fn compare_prerelease(a: &str, b: &str) -> std::cmp::Ordering {
    let mut a_parts = a.split('.');
    let mut b_parts = b.split('.');
    loop {
        match (a_parts.next(), b_parts.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(pa), Some(pb)) => {
                let ord = match (pa.parse::<u64>(), pb.parse::<u64>()) {
                    (Ok(na), Ok(nb)) => na.cmp(&nb),
                    (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                    (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                    (Err(_), Err(_)) => pa.cmp(pb),
                };
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

pub fn detect_install_kind_for_exe(exe_path: &Path) -> AppInstallKind {
    if let Some(parent) = exe_path.parent() {
        if parent.join("unins000.exe").exists() {
            return AppInstallKind::Installer;
        }
        let lower = parent.to_string_lossy().to_ascii_lowercase();
        if lower.contains("\\program files") || lower.contains("\\programs\\fasttube") {
            return AppInstallKind::Installer;
        }
    }
    AppInstallKind::Portable
}

pub fn current_install_kind() -> AppInstallKind {
    std::env::current_exe()
        .map(|p| detect_install_kind_for_exe(&p))
        .unwrap_or(AppInstallKind::Portable)
}

fn sanitize_version_component(version: &str) -> String {
    let out: String = version
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(32)
        .collect();
    if out.is_empty() {
        "latest".to_owned()
    } else {
        out
    }
}

fn sanitize_asset_filename(name: &str) -> Option<String> {
    let file_name = Path::new(name).file_name()?.to_str()?;
    let trimmed = file_name.trim();
    if trimmed.is_empty()
        || trimmed.contains("..")
        || trimmed.contains('/')
        || trimmed.contains('\\')
    {
        return None;
    }
    Some(trimmed.to_owned())
}

fn summarize_release_notes(raw: &str) -> String {
    let cleaned = raw.replace("\r\n", "\n");
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for line in trimmed.lines() {
        let line_trim = line.trim();
        if line_trim.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line_trim);
        if out.chars().count() >= 900 {
            out.push_str("\n...");
            break;
        }
    }
    out
}

pub fn parse_fasttube_release_json(
    bytes: &[u8],
    current_version: &str,
    preferred_kind: AppInstallKind,
) -> Result<AppUpdateCheck, String> {
    let json: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| "Resposta inválida da API de releases do GitHub.".to_owned())?;
    if json.get("draft").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err("A release encontrada ainda é um rascunho.".to_owned());
    }

    let tag_name = json
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Tag da release não informada pelo GitHub.".to_owned())?
        .to_owned();
    let latest_version = normalize_version_str(&tag_name).to_owned();
    if parse_semver(&latest_version).is_none() {
        return Err(format!("Formato de versão inválido na tag '{tag_name}'."));
    }

    let release_url = json
        .get("html_url")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|u| is_allowed_github_download(u))
        .unwrap_or(FASTTUBE_RELEASES_PAGE_URL)
        .to_owned();

    let release_notes = json
        .get("body")
        .and_then(serde_json::Value::as_str)
        .map(summarize_release_notes)
        .unwrap_or_default();

    let assets = json
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "Nenhum asset encontrado na release do FastTube.".to_owned())?;

    let mut installer_asset: Option<(String, String)> = None;
    let mut portable_asset: Option<(String, String)> = None;
    let mut checksums_url: Option<String> = None;

    for asset in assets {
        let Some(name_raw) = asset.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(name) = sanitize_asset_filename(name_raw) else {
            continue;
        };
        let Some(url) = asset
            .get("browser_download_url")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|u| is_allowed_github_download(u))
        else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if lower == "sha256sums.txt" {
            checksums_url = Some(url.to_owned());
        } else if lower.starts_with("fasttube-setup-") && lower.ends_with(".exe") {
            installer_asset = Some((name, url.to_owned()));
        } else if lower.starts_with("fasttube-") && lower.ends_with("-portable.zip") {
            portable_asset = Some((name, url.to_owned()));
        } else if lower.ends_with(".exe") && installer_asset.is_none() {
            installer_asset = Some((name, url.to_owned()));
        } else if lower.ends_with(".zip") && portable_asset.is_none() {
            portable_asset = Some((name, url.to_owned()));
        }
    }

    let (install_kind, (asset_name, download_url)) = match preferred_kind {
        AppInstallKind::Installer => {
            if let Some(inst) = installer_asset {
                (AppInstallKind::Installer, inst)
            } else if let Some(port) = portable_asset {
                (AppInstallKind::Portable, port)
            } else {
                return Err(
                    "Nenhum instalador ou pacote portátil foi encontrado na release do GitHub."
                        .to_owned(),
                );
            }
        }
        AppInstallKind::Portable => {
            if let Some(port) = portable_asset {
                (AppInstallKind::Portable, port)
            } else if let Some(inst) = installer_asset {
                (AppInstallKind::Installer, inst)
            } else {
                return Err(
                    "Nenhum pacote portátil ou instalador foi encontrado na release do GitHub."
                        .to_owned(),
                );
            }
        }
    };

    let update_available = is_newer_version(current_version, &latest_version);

    Ok(AppUpdateCheck {
        current_version: normalize_version_str(current_version).to_owned(),
        latest_version,
        tag_name,
        release_url,
        release_notes,
        asset_name,
        download_url,
        checksums_url,
        install_kind,
        update_available,
    })
}

pub fn parse_sha256sums(text: &str, asset_name: &str) -> Option<String> {
    let target = asset_name.trim();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let hash = parts.next()?.trim();
        let file = parts.next()?.trim().trim_start_matches('*').trim();
        let file_base = Path::new(file)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(file);
        if file_base.eq_ignore_ascii_case(target)
            && hash.len() == 64
            && hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

pub fn verify_sha256_bytes(bytes: &[u8], expected_hex: &str) -> bool {
    use sha2::Digest;
    let expected = expected_hex.trim();
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let digest = sha2::Sha256::digest(bytes);
    let actual_hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    actual_hex.eq_ignore_ascii_case(expected)
}

pub fn check_fasttube_update(cancel: &Cancellation) -> Result<AppUpdateCheck, String> {
    if cancel.cancelled() {
        return Err("Verificação de atualização cancelada.".to_owned());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(API_TIMEOUT)
        .user_agent(format!("FastTube-Updater/{CURRENT_APP_VERSION}"))
        .build()
        .map_err(|_| "Não foi possível iniciar o cliente HTTP.".to_owned())?;
    let resp = client
        .get(FASTTUBE_GITHUB_LATEST_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|_| "Não foi possível consultar a release mais recente no GitHub.".to_owned())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub retornou HTTP {}.", resp.status().as_u16()));
    }
    let mut body = Vec::new();
    resp.take(1024 * 1024)
        .read_to_end(&mut body)
        .map_err(|_| "Falha ao ler os metadados da release no GitHub.".to_owned())?;
    if cancel.cancelled() {
        return Err("Verificação de atualização cancelada.".to_owned());
    }
    parse_fasttube_release_json(&body, CURRENT_APP_VERSION, current_install_kind())
}

fn app_updates_root_dir() -> Result<PathBuf, String> {
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        let root = PathBuf::from(local);
        if root.is_absolute() {
            return Ok(root.join("FastTube").join("updates"));
        }
    }
    Ok(std::env::temp_dir().join("FastTube-updates"))
}

fn resolve_staged_portable_root(staging_dir: &Path) -> Result<PathBuf, String> {
    if staging_dir.join("fasttube.exe").exists() {
        return Ok(staging_dir.to_path_buf());
    }
    let mut dirs = Vec::new();
    let entries = fs::read_dir(staging_dir)
        .map_err(|e| format!("Falha ao inspecionar pacote extraído: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    if dirs.len() == 1 && dirs[0].join("fasttube.exe").exists() {
        return Ok(dirs.remove(0));
    }
    Err("O pacote portátil baixado não contém fasttube.exe.".to_owned())
}

pub fn download_and_prepare_app_update(
    check: &AppUpdateCheck,
    cancel: &Cancellation,
) -> Result<AppUpdatePrepared, String> {
    if cancel.cancelled() {
        return Err("Atualização cancelada.".to_owned());
    }
    if !is_allowed_github_download(&check.download_url) {
        return Err("URL de download da release não permitida.".to_owned());
    }
    let asset_name = sanitize_asset_filename(&check.asset_name)
        .ok_or_else(|| "Nome de arquivo da release inválido.".to_owned())?;

    let target_exe = std::env::current_exe()
        .map_err(|e| format!("Não foi possível localizar o executável atual: {e}"))?;
    let target_dir = target_exe
        .parent()
        .ok_or_else(|| "Pasta do executável atual indisponível.".to_owned())?
        .to_path_buf();

    if check.install_kind == AppInstallKind::Portable && !is_directory_writable(&target_dir) {
        return Err(format!(
            "A pasta de instalação '{}' não permite escrita direta pelo usuário. Baixe o instalador pelo GitHub.",
            target_dir.display()
        ));
    }

    let seq = UPDATE_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let work_dir = app_updates_root_dir()?.join(format!(
        "v{}-{}-{}",
        sanitize_version_component(&check.latest_version),
        std::process::id(),
        seq
    ));
    fs::create_dir_all(&work_dir)
        .map_err(|e| format!("Não foi possível criar pasta de atualização: {e}"))?;

    let client = reqwest::blocking::Client::builder()
        .timeout(APP_DOWNLOAD_TIMEOUT)
        .user_agent(format!("FastTube-Updater/{CURRENT_APP_VERSION}"))
        .build()
        .map_err(|_| "Não foi possível iniciar o cliente HTTP de download.".to_owned())?;

    let resp = client
        .get(&check.download_url)
        .send()
        .map_err(|_| "Falha de rede ao baixar a atualização do FastTube.".to_owned())?;
    if !resp.status().is_success() {
        let _ = fs::remove_dir_all(&work_dir);
        return Err(format!(
            "Download da atualização retornou HTTP {}.",
            resp.status().as_u16()
        ));
    }

    let mut bytes = Vec::new();
    resp.take((MAX_APP_ARTIFACT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Falha durante a leitura do arquivo de atualização.".to_owned())?;
    if cancel.cancelled() {
        let _ = fs::remove_dir_all(&work_dir);
        return Err("Atualização cancelada.".to_owned());
    }
    if bytes.len() < MIN_APP_ARTIFACT_BYTES || bytes.len() > MAX_APP_ARTIFACT_BYTES {
        let _ = fs::remove_dir_all(&work_dir);
        return Err("Tamanho inesperado para o arquivo de atualização do FastTube.".to_owned());
    }

    match check.install_kind {
        AppInstallKind::Installer if !bytes.starts_with(b"MZ") => {
            let _ = fs::remove_dir_all(&work_dir);
            return Err("O instalador baixado não é um executável Windows válido.".to_owned());
        }
        AppInstallKind::Portable if !bytes.starts_with(b"PK\x03\x04") => {
            let _ = fs::remove_dir_all(&work_dir);
            return Err("O pacote portátil baixado não é um arquivo ZIP válido.".to_owned());
        }
        _ => {}
    }

    if let Some(sums_url) = check.checksums_url.as_deref() {
        if !is_allowed_github_download(sums_url) {
            let _ = fs::remove_dir_all(&work_dir);
            return Err("URL do arquivo SHA256SUMS.txt não permitida.".to_owned());
        }
        let sums_resp = client
            .get(sums_url)
            .send()
            .map_err(|_| "Não foi possível baixar SHA256SUMS.txt para verificação.".to_owned())?;
        if !sums_resp.status().is_success() {
            let _ = fs::remove_dir_all(&work_dir);
            return Err(format!(
                "SHA256SUMS.txt retornou HTTP {}.",
                sums_resp.status().as_u16()
            ));
        }
        let mut sums_bytes = Vec::new();
        sums_resp
            .take(256 * 1024)
            .read_to_end(&mut sums_bytes)
            .map_err(|_| "Falha ao ler SHA256SUMS.txt.".to_owned())?;
        let sums_text = String::from_utf8_lossy(&sums_bytes);
        let expected_hash = parse_sha256sums(&sums_text, &asset_name).ok_or_else(|| {
            let _ = fs::remove_dir_all(&work_dir);
            format!("Hash SHA-256 para '{asset_name}' não encontrado em SHA256SUMS.txt.")
        })?;
        if !verify_sha256_bytes(&bytes, &expected_hash) {
            let _ = fs::remove_dir_all(&work_dir);
            return Err(
                "Falha na verificação de integridade SHA-256 da atualização. Abortado.".to_owned(),
            );
        }
    }

    let artifact_path = work_dir.join(&asset_name);
    if let Err(err) = fs::write(&artifact_path, &bytes) {
        let _ = fs::remove_dir_all(&work_dir);
        return Err(format!(
            "Não foi possível gravar o arquivo de atualização: {err}"
        ));
    }

    let staging_dir = if check.install_kind == AppInstallKind::Portable {
        let raw_staging = work_dir.join("staging");
        fs::create_dir_all(&raw_staging)
            .map_err(|e| format!("Não foi possível criar pasta de extração: {e}"))?;

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let script = work_dir.join("expand.ps1");
            fs::write(
                &script,
                "param([string]$ZipPath, [string]$DestDir)\n\
                 $ErrorActionPreference = 'Stop'\n\
                 Expand-Archive -LiteralPath $ZipPath -DestinationPath $DestDir -Force\n",
            )
            .map_err(|e| format!("Erro preparando script de extração: {e}"))?;

            let status = std::process::Command::new("powershell.exe")
                .creation_flags(CREATE_NO_WINDOW)
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                ])
                .arg("-File")
                .arg(&script)
                .arg("-ZipPath")
                .arg(&artifact_path)
                .arg("-DestDir")
                .arg(&raw_staging)
                .status()
                .map_err(|e| format!("Falha ao executar PowerShell para extrair ZIP: {e}"))?;
            let _ = fs::remove_file(&script);
            if !status.success() {
                let _ = fs::remove_dir_all(&work_dir);
                return Err("Falha ao descompactar o pacote portátil da atualização.".to_owned());
            }
        }
        #[cfg(not(windows))]
        {
            let _ = fs::remove_dir_all(&work_dir);
            return Err("Atualização automática portátil disponível apenas no Windows.".to_owned());
        }

        let resolved = resolve_staged_portable_root(&raw_staging)?;
        let exe_bytes = fs::read(resolved.join("fasttube.exe"))
            .map_err(|_| "Falha ao validar fasttube.exe extraído.".to_owned())?;
        if !exe_bytes.starts_with(b"MZ") {
            let _ = fs::remove_dir_all(&work_dir);
            return Err("O executável fasttube.exe extraído é inválido.".to_owned());
        }
        Some(resolved)
    } else {
        None
    };

    Ok(AppUpdatePrepared {
        version: check.latest_version.clone(),
        install_kind: check.install_kind,
        artifact_path,
        staging_dir,
        target_dir,
        target_exe,
    })
}

pub fn launch_app_update_helper(prepared: &AppUpdatePrepared) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let work_dir = prepared
            .artifact_path
            .parent()
            .ok_or_else(|| "Diretório de trabalho da atualização inválido.".to_owned())?;
        let script_path = work_dir.join("apply-update.ps1");
        let script_body = r#"param(
    [int]$TargetPid,
    [string]$Mode,
    [string]$ArtifactPath,
    [string]$StagingDir,
    [string]$TargetDir,
    [string]$TargetExe,
    [string]$WorkDir
)
$ErrorActionPreference = 'Stop'
for ($i = 0; $i -lt 120; $i++) {
    $proc = Get-Process -Id $TargetPid -ErrorAction SilentlyContinue
    if (-not $proc) { break }
    Start-Sleep -Milliseconds 250
}
Start-Sleep -Milliseconds 300
if ($Mode -eq 'installer') {
    Start-Process -FilePath $ArtifactPath -ArgumentList @('/SILENT', '/CLOSEAPPLICATIONS', '/NORESTART') -Wait
    if (Test-Path -LiteralPath $TargetExe) {
        Start-Process -FilePath $TargetExe
    }
} elseif ($Mode -eq 'portable') {
    Get-ChildItem -LiteralPath $StagingDir -Force | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $TargetDir -Recurse -Force
    }
    if (Test-Path -LiteralPath $TargetExe) {
        Start-Process -FilePath $TargetExe
    }
}
Start-Sleep -Milliseconds 500
if ($WorkDir -and (Test-Path -LiteralPath $WorkDir)) {
    Remove-Item -LiteralPath $WorkDir -Recurse -Force -ErrorAction SilentlyContinue
}
"#;
        fs::write(&script_path, script_body)
            .map_err(|e| format!("Não foi possível criar script de atualização: {e}"))?;

        let mode_str = match prepared.install_kind {
            AppInstallKind::Installer => "installer",
            AppInstallKind::Portable => "portable",
        };
        let staging_str = prepared
            .staging_dir
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        std::process::Command::new("powershell.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&script_path)
            .args(["-TargetPid", &std::process::id().to_string()])
            .args(["-Mode", mode_str])
            .arg("-ArtifactPath")
            .arg(&prepared.artifact_path)
            .arg("-StagingDir")
            .arg(staging_str)
            .arg("-TargetDir")
            .arg(&prepared.target_dir)
            .arg("-TargetExe")
            .arg(&prepared.target_exe)
            .arg("-WorkDir")
            .arg(work_dir)
            .spawn()
            .map_err(|e| format!("Não foi possível iniciar o instalador da atualização: {e}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = prepared;
        Err("Instalação automática disponível apenas no Windows.".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_release_and_validates_host() {
        let json = br#"{
            "tag_name": "2026.11.10",
            "assets": [
                {"name": "yt-dlp.exe", "browser_download_url": "https://github.com/yt-dlp/yt-dlp/releases/download/2026.11.10/yt-dlp.exe"}
            ]
        }"#;
        let (tag, url) = parse_github_release_json(json).unwrap();
        assert_eq!(tag, "2026.11.10");
        assert!(url.ends_with("yt-dlp.exe"));
        assert!(!is_allowed_github_download("http://github.com/a/b"));
        assert!(!is_allowed_github_download(
            "https://evil.example.com/yt-dlp.exe"
        ));
    }

    #[test]
    fn semver_comparison_handles_v_prefix_and_prereleases() {
        assert!(is_newer_version("0.3.0", "v0.3.1"));
        assert!(is_newer_version("0.3.0", "0.4.0"));
        assert!(is_newer_version("0.3.0", "1.0.0"));
        assert!(is_newer_version("0.4.0-beta.1", "0.4.0"));
        assert!(is_newer_version("0.4.0-beta.1", "0.4.0-beta.2"));
        assert!(!is_newer_version("0.3.0", "v0.3.0"));
        assert!(!is_newer_version("0.3.1", "0.3.0"));
        assert!(!is_newer_version("0.4.0", "0.4.0-rc.1"));
    }

    #[test]
    fn parses_fasttube_github_release_for_installer_and_portable() {
        let json = br###"{
            "tag_name": "v0.3.1",
            "html_url": "https://github.com/nathanbzrr/FastTube/releases/tag/v0.3.1",
            "body": "## Novidades\r\n- Atualizador automatico\r\n- Bandeja do sistema",
            "draft": false,
            "assets": [
                {
                    "name": "FastTube-Setup-0.3.1-x64.exe",
                    "browser_download_url": "https://github.com/nathanbzrr/FastTube/releases/download/v0.3.1/FastTube-Setup-0.3.1-x64.exe"
                },
                {
                    "name": "FastTube-v0.3.1-x64-portable.zip",
                    "browser_download_url": "https://github.com/nathanbzrr/FastTube/releases/download/v0.3.1/FastTube-v0.3.1-x64-portable.zip"
                },
                {
                    "name": "SHA256SUMS.txt",
                    "browser_download_url": "https://github.com/nathanbzrr/FastTube/releases/download/v0.3.1/SHA256SUMS.txt"
                }
            ]
        }"###;

        let portable =
            parse_fasttube_release_json(json, "0.3.0", AppInstallKind::Portable).unwrap();
        assert!(portable.update_available);
        assert_eq!(portable.latest_version, "0.3.1");
        assert_eq!(portable.install_kind, AppInstallKind::Portable);
        assert_eq!(portable.asset_name, "FastTube-v0.3.1-x64-portable.zip");
        assert!(portable.checksums_url.is_some());
        assert!(portable.release_notes.contains("Bandeja do sistema"));

        let installer =
            parse_fasttube_release_json(json, "0.3.0", AppInstallKind::Installer).unwrap();
        assert!(installer.update_available);
        assert_eq!(installer.install_kind, AppInstallKind::Installer);
        assert_eq!(installer.asset_name, "FastTube-Setup-0.3.1-x64.exe");

        let same_ver =
            parse_fasttube_release_json(json, "0.3.1", AppInstallKind::Portable).unwrap();
        assert!(!same_ver.update_available);
    }

    #[test]
    fn parses_sha256sums_and_verifies_digest() {
        let payload = b"FastTube release binary test payload";
        use sha2::Digest;
        let expected: String = sha2::Sha256::digest(payload)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let sums = format!(
            "{expected}  FastTube-v0.3.1-x64-portable.zip\n0000000000000000000000000000000000000000000000000000000000000000  other.exe\n"
        );
        let parsed = parse_sha256sums(&sums, "FastTube-v0.3.1-x64-portable.zip").unwrap();
        assert_eq!(parsed, expected);
        assert!(verify_sha256_bytes(payload, &parsed));
        assert!(!verify_sha256_bytes(b"tampered", &parsed));
    }
}
