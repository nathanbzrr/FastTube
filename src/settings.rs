use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferSettings {
    pub max_mib: u32,
    pub back_mib: u32,
    pub cache_seconds: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BufferProfile {
    Small,
    #[default]
    Default,
    Large,
    Custom(BufferSettings),
}

impl BufferProfile {
    pub fn values(self) -> BufferSettings {
        match self {
            Self::Small => BufferSettings {
                max_mib: 32,
                back_mib: 8,
                cache_seconds: 15,
            },
            Self::Default => BufferSettings {
                max_mib: 64,
                back_mib: 16,
                cache_seconds: 30,
            },
            Self::Large => BufferSettings {
                max_mib: 128,
                back_mib: 32,
                cache_seconds: 60,
            },
            Self::Custom(values) => values,
        }
    }

    pub fn validate(self) -> Result<(), &'static str> {
        let values = self.values();
        if !(1..=1024).contains(&values.max_mib) {
            return Err("max_mib must be between 1 and 1024");
        }
        if values.back_mib > values.max_mib {
            return Err("back_mib must not exceed max_mib");
        }
        if !(1..=3600).contains(&values.cache_seconds) {
            return Err("cache_seconds must be between 1 and 3600");
        }
        Ok(())
    }
}

pub fn settings_path() -> io::Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))?;
    let path = PathBuf::from(local);
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LOCALAPPDATA must be absolute",
        ));
    }
    Ok(path.join("FastTube").join("buffer-settings.txt"))
}

pub fn load(path: &Path) -> io::Result<BufferProfile> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(BufferProfile::default());
        }
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(1025).read_to_end(&mut bytes)?;
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid buffer settings");
    if bytes.len() > 1024 {
        return Err(invalid());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
    let fields: Vec<_> = text.split_whitespace().collect();
    let profile = match fields.as_slice() {
        ["1", "small"] => BufferProfile::Small,
        ["1", "default"] => BufferProfile::Default,
        ["1", "large"] => BufferProfile::Large,
        ["1", "custom", max, back, seconds] => BufferProfile::Custom(BufferSettings {
            max_mib: max.parse().map_err(|_| invalid())?,
            back_mib: back.parse().map_err(|_| invalid())?,
            cache_seconds: seconds.parse().map_err(|_| invalid())?,
        }),
        _ => return Err(invalid()),
    };
    profile
        .validate()
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))?;
    Ok(profile)
}

pub fn save(path: &Path, profile: BufferProfile) -> io::Result<()> {
    profile
        .validate()
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
        })?;
    fs::create_dir_all(parent)?;
    let content = match profile {
        BufferProfile::Small => "1 small\n".to_owned(),
        BufferProfile::Default => "1 default\n".to_owned(),
        BufferProfile::Large => "1 large\n".to_owned(),
        BufferProfile::Custom(values) => format!(
            "1 custom {} {} {}\n",
            values.max_mib, values.back_mib, values.cache_seconds
        ),
    };
    let (temporary, mut file) = loop {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".fasttube-buffer-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Defaults for each newly opened video; quality is a preferred maximum height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaybackPreferences {
    pub speed_quarters: u8,
    pub max_height: Option<u32>,
}

impl Default for PlaybackPreferences {
    fn default() -> Self {
        Self {
            speed_quarters: 4,
            max_height: None,
        }
    }
}

impl PlaybackPreferences {
    fn valid(self) -> bool {
        (1..=8).contains(&self.speed_quarters)
            && matches!(
                self.max_height,
                None | Some(360 | 480 | 720 | 1080 | 1440 | 2160)
            )
    }

    pub fn format(self) -> String {
        let height = self.max_height.unwrap_or(1080);
        format!("bestvideo[height<={height}]+bestaudio/best[height<={height}]/best")
    }
}

pub fn playback_path() -> io::Result<PathBuf> {
    Ok(settings_path()?.with_file_name("playback-settings.txt"))
}

pub fn load_playback(path: &Path) -> io::Result<PlaybackPreferences> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(PlaybackPreferences::default());
        }
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(1025).read_to_end(&mut bytes)?;
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid playback settings");
    if bytes.len() > 1024 {
        return Err(invalid());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
    let fields: Vec<_> = text.split_whitespace().collect();
    let preferences = match fields.as_slice() {
        ["1", speed, height] => PlaybackPreferences {
            speed_quarters: speed.parse().map_err(|_| invalid())?,
            max_height: if *height == "auto" {
                None
            } else {
                Some(height.parse().map_err(|_| invalid())?)
            },
        },
        _ => return Err(invalid()),
    };
    if !preferences.valid() {
        return Err(invalid());
    }
    Ok(preferences)
}

pub fn save_playback(path: &Path, preferences: PlaybackPreferences) -> io::Result<()> {
    if !preferences.valid() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid playback settings",
        ));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
        })?;
    fs::create_dir_all(parent)?;
    let (temporary, mut file) = loop {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".fasttube-playback-{}-{id}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let height = preferences
        .max_height
        .map(|v| v.to_string())
        .unwrap_or_else(|| "auto".into());
    let result = (|| {
        writeln!(file, "1 {} {}", preferences.speed_quarters, height)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

pub fn theme_path() -> io::Result<PathBuf> {
    Ok(settings_path()?.with_file_name("theme-settings.txt"))
}

pub fn load_theme(path: &Path) -> io::Result<Theme> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Theme::default());
        }
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(1025).read_to_end(&mut bytes)?;
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid theme settings");
    if bytes.len() > 1024 {
        return Err(invalid());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
    let fields: Vec<_> = text.split_whitespace().collect();
    match fields.as_slice() {
        ["1", "dark"] => Ok(Theme::Dark),
        ["1", "light"] => Ok(Theme::Light),
        _ => Err(invalid()),
    }
}

pub fn save_theme(path: &Path, theme: Theme) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
        })?;
    fs::create_dir_all(parent)?;
    let (temporary, mut file) = loop {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".fasttube-theme-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let value = match theme {
        Theme::Dark => "dark",
        Theme::Light => "light",
    };
    let result = (|| {
        writeln!(file, "1 {value}")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HwdecMode {
    #[default]
    AutoSafe,
    Auto,
    Software,
}

impl HwdecMode {
    pub const ALL: [Self; 3] = [Self::AutoSafe, Self::Auto, Self::Software];

    pub fn as_mpv_value(self) -> &'static str {
        match self {
            Self::AutoSafe => "auto-safe",
            Self::Auto => "auto",
            Self::Software => "no",
        }
    }

    pub fn label_pt(self) -> &'static str {
        match self {
            Self::AutoSafe => "Automático seguro (auto-safe)",
            Self::Auto => "Automático agressivo (auto)",
            Self::Software => "Software / CPU (no)",
        }
    }

    pub fn description_pt(self) -> &'static str {
        match self {
            Self::AutoSafe => "Equilíbrio recomendado: usa aceleração por GPU quando segura.",
            Self::Auto => {
                "Tenta todos os backends de GPU disponíveis; pode variar conforme o driver."
            }
            Self::Software => {
                "Força decodificação por CPU, útil se o driver de vídeo apresentar artefatos."
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AppPreferences {
    pub sponsorblock: crate::sponsorblock::SponsorBlockSettings,
    pub hwdec_mode: HwdecMode,
    pub seek_step_secs: u32,
    pub default_autoplay: bool,
    pub default_loop: bool,
    pub resume_enabled: bool,
    pub remember_volume: bool,
    pub default_volume_percent: u8,
    pub audio_normalization: bool,
    pub auto_subtitles: bool,
    pub preferred_subtitle_lang: String,
    pub muted_topics: Vec<String>,
    pub pause_watch_history: bool,
    pub pause_recommendations: bool,
    pub hide_shorts_in_home: bool,
    pub keep_awake_during_playback: bool,
    pub compact_card_density: bool,
    pub cache_dir_override: Option<String>,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            sponsorblock: crate::sponsorblock::SponsorBlockSettings::default(),
            hwdec_mode: HwdecMode::AutoSafe,
            seek_step_secs: 10,
            default_autoplay: true,
            default_loop: false,
            resume_enabled: true,
            remember_volume: true,
            default_volume_percent: 100,
            audio_normalization: false,
            auto_subtitles: false,
            preferred_subtitle_lang: "pt".to_owned(),
            muted_topics: Vec::new(),
            pause_watch_history: false,
            pause_recommendations: false,
            hide_shorts_in_home: false,
            keep_awake_during_playback: true,
            compact_card_density: false,
            cache_dir_override: None,
        }
    }
}

impl AppPreferences {
    pub fn normalized(mut self) -> Self {
        self.sponsorblock = self.sponsorblock.normalized();
        self.preferred_subtitle_lang = self.preferred_subtitle_lang.trim().to_ascii_lowercase();
        let mut seen_topics = std::collections::HashSet::new();
        self.muted_topics = self
            .muted_topics
            .into_iter()
            .map(|topic| topic.trim().to_lowercase())
            .filter(|topic| !topic.is_empty() && seen_topics.insert(topic.clone()))
            .take(100)
            .collect();
        self.cache_dir_override = self
            .cache_dir_override
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        self
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.sponsorblock.valid() {
            return Err("configuração inválida do SponsorBlock");
        }
        if !matches!(self.seek_step_secs, 5 | 10 | 15 | 30) {
            return Err("passo de avanço deve ser 5, 10, 15 ou 30 segundos");
        }
        if self.default_volume_percent > 100 {
            return Err("volume padrão deve estar entre 0% e 100%");
        }
        if self.preferred_subtitle_lang.trim().len() > 16 {
            return Err("código de idioma de legenda muito longo");
        }
        if self.muted_topics.len() > 100
            || self
                .muted_topics
                .iter()
                .any(|topic| topic.trim().is_empty() || topic.len() > 64)
        {
            return Err("lista de tópicos silenciados inválida");
        }
        if let Some(dir) = &self.cache_dir_override {
            let trimmed = dir.trim();
            if trimmed.is_empty() || trimmed.len() > 1024 {
                return Err("caminho de cache inválido");
            }
            if !Path::new(trimmed).is_absolute() {
                return Err("o diretório de cache personalizado deve ser um caminho absoluto");
            }
        }
        Ok(())
    }
}

pub fn app_preferences_path() -> io::Result<PathBuf> {
    Ok(settings_path()?.with_file_name("app-preferences.json"))
}

pub fn load_app_preferences(path: &Path) -> io::Result<AppPreferences> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(AppPreferences::default());
        }
        Err(error) => return Err(error),
    };
    const MAX_BYTES: u64 = 64 * 1024;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid app preferences");
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    let prefs: AppPreferences = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    prefs
        .validate()
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))?;
    Ok(prefs.normalized())
}

pub fn save_app_preferences(path: &Path, prefs: &AppPreferences) -> io::Result<()> {
    let normalized = prefs.clone().normalized();
    normalized
        .validate()
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
        })?;
    fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec_pretty(&normalized)?;
    let (temporary, mut file) = loop {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".fasttube-prefs-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn default_cache_dir() -> io::Result<PathBuf> {
    Ok(settings_path()?.with_file_name("cache"))
}

pub fn effective_cache_dir(override_dir: Option<&str>) -> io::Result<PathBuf> {
    if let Some(raw) = override_dir.map(str::trim).filter(|s| !s.is_empty()) {
        let path = PathBuf::from(raw);
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "o diretório de cache deve ser absoluto",
            ));
        }
        return Ok(path);
    }
    default_cache_dir()
}

pub fn yt_dlp_cache_subdir(base_cache_dir: &Path) -> PathBuf {
    base_cache_dir.join("yt-dlp")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirUsage {
    pub bytes: u64,
    pub files: usize,
}

pub fn measure_dir_usage(root: &Path) -> DirUsage {
    let mut usage = DirUsage::default();
    if !root.is_dir() {
        return usage;
    }
    let mut stack = vec![(root.to_path_buf(), 0_usize)];
    let mut visited_dirs = 0_usize;
    while let Some((dir, depth)) = stack.pop() {
        if depth > 6 || visited_dirs >= 512 {
            break;
        }
        visited_dirs += 1;
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push((entry.path(), depth + 1));
            } else if meta.is_file() {
                usage.bytes = usage.bytes.saturating_add(meta.len());
                usage.files = usage.files.saturating_add(1);
            }
        }
    }
    usage
}

pub fn clear_yt_dlp_cache_dir(base_cache_dir: &Path) -> io::Result<usize> {
    let target = yt_dlp_cache_subdir(base_cache_dir);
    if !target.exists() {
        return Ok(0);
    }
    let usage = measure_dir_usage(&target);
    fs::remove_dir_all(&target)?;
    fs::create_dir_all(&target)?;
    Ok(usage.files)
}

#[cfg(windows)]
pub fn pick_folder_dialog(initial_dir: Option<&Path>) -> Result<Option<PathBuf>, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let mut command = Command::new("powershell");
    command.creation_flags(0x08000000);
    if let Some(dir) = initial_dir.filter(|p| p.is_dir()) {
        command.env("FASTTUBE_PICKER_INIT", dir);
    }
    let script = concat!(
        "Add-Type -AssemblyName System.Windows.Forms; ",
        "$d = New-Object System.Windows.Forms.FolderBrowserDialog; ",
        "$d.Description = 'Selecione a pasta de cache do FastTube'; ",
        "$d.UseDescriptionForTitle = $true; ",
        "$d.ShowNewFolderButton = $true; ",
        "if ($env:FASTTUBE_PICKER_INIT -and (Test-Path -LiteralPath $env:FASTTUBE_PICKER_INIT)) { $d.SelectedPath = $env:FASTTUBE_PICKER_INIT }; ",
        "if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; Write-Output $d.SelectedPath }"
    );
    let output = command
        .args(["-NoProfile", "-STA", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|error| format!("Não foi possível abrir o seletor de pastas: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Seletor de pastas falhou: {}", stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if stdout.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(stdout)))
    }
}

#[cfg(not(windows))]
pub fn pick_folder_dialog(_initial_dir: Option<&Path>) -> Result<Option<PathBuf>, String> {
    Err("Seletor nativo de pastas disponível apenas no Windows.".into())
}

#[cfg(windows)]
pub fn pick_json_file_dialog(
    save: bool,
    title: &str,
    initial_path: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let mut command = Command::new("powershell");
    command.creation_flags(0x08000000);
    command.env("FASTTUBE_PICKER_TITLE", title);
    if let Some(path) = initial_path {
        if let Some(dir) = path.parent().filter(|p| p.is_dir()) {
            command.env("FASTTUBE_PICKER_DIR", dir);
        } else if path.is_dir() {
            command.env("FASTTUBE_PICKER_DIR", path);
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && !path.is_dir()
        {
            command.env("FASTTUBE_PICKER_NAME", name);
        }
    }
    let script = if save {
        concat!(
            "Add-Type -AssemblyName System.Windows.Forms; ",
            "$d = New-Object System.Windows.Forms.SaveFileDialog; ",
            "$d.Title = $env:FASTTUBE_PICKER_TITLE; ",
            "$d.Filter = 'Arquivos JSON (*.json)|*.json|Todos os arquivos (*.*)|*.*'; ",
            "$d.DefaultExt = 'json'; ",
            "$d.AddExtension = $true; ",
            "if ($env:FASTTUBE_PICKER_DIR -and (Test-Path -LiteralPath $env:FASTTUBE_PICKER_DIR)) { $d.InitialDirectory = $env:FASTTUBE_PICKER_DIR }; ",
            "if ($env:FASTTUBE_PICKER_NAME) { $d.FileName = $env:FASTTUBE_PICKER_NAME }; ",
            "if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; Write-Output $d.FileName }"
        )
    } else {
        concat!(
            "Add-Type -AssemblyName System.Windows.Forms; ",
            "$d = New-Object System.Windows.Forms.OpenFileDialog; ",
            "$d.Title = $env:FASTTUBE_PICKER_TITLE; ",
            "$d.Filter = 'Arquivos JSON (*.json)|*.json|Todos os arquivos (*.*)|*.*'; ",
            "$d.CheckFileExists = $true; ",
            "if ($env:FASTTUBE_PICKER_DIR -and (Test-Path -LiteralPath $env:FASTTUBE_PICKER_DIR)) { $d.InitialDirectory = $env:FASTTUBE_PICKER_DIR }; ",
            "if ($env:FASTTUBE_PICKER_NAME) { $d.FileName = $env:FASTTUBE_PICKER_NAME }; ",
            "if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; Write-Output $d.FileName }"
        )
    };
    let output = command
        .args(["-NoProfile", "-STA", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|error| format!("Não foi possível abrir o seletor de arquivos: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Seletor de arquivos falhou: {}", stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if stdout.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(stdout)))
    }
}

#[cfg(not(windows))]
pub fn pick_json_file_dialog(
    _save: bool,
    _title: &str,
    _initial_path: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    Err("Seletor nativo de arquivos disponível apenas no Windows.".into())
}

#[cfg(windows)]
pub fn open_in_explorer(path: &Path) -> Result<(), String> {
    use std::process::Command;

    if path.is_file() {
        let arg = format!("/select,{}", path.display());
        Command::new("explorer.exe")
            .arg(arg)
            .spawn()
            .map_err(|error| format!("Não foi possível abrir o Explorer: {error}"))?;
        return Ok(());
    }
    let target = if path.is_dir() {
        path.to_path_buf()
    } else if let Some(parent) = path.parent().filter(|p| p.is_dir()) {
        parent.to_path_buf()
    } else {
        return Err(format!("Pasta não encontrada: {}", path.display()));
    };
    Command::new("explorer.exe")
        .arg(target)
        .spawn()
        .map_err(|error| format!("Não foi possível abrir o Explorer: {error}"))?;
    Ok(())
}

#[cfg(not(windows))]
pub fn open_in_explorer(_path: &Path) -> Result<(), String> {
    Err("Abertura no Explorer disponível apenas no Windows.".into())
}

pub fn oauth_client_path() -> io::Result<PathBuf> {
    Ok(settings_path()?.with_file_name("oauth-client.json"))
}

pub fn validate_oauth_client_json(bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > 64 * 1024 {
        return Err("Arquivo de cliente OAuth inválido ou maior que 64 KiB.".into());
    }
    let json: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "JSON do cliente OAuth inválido.".to_owned())?;
    let installed = &json["installed"];
    let id = installed["client_id"]
        .as_str()
        .filter(|v| !v.trim().is_empty())
        .ok_or("O JSON deve ser de um cliente OAuth Desktop ('installed.client_id').")?;
    let secret = installed["client_secret"]
        .as_str()
        .filter(|v| !v.trim().is_empty())
        .ok_or("O JSON do cliente OAuth não contém 'installed.client_secret'.")?;
    let _ = (id, secret);
    Ok(())
}

pub fn install_oauth_client_file(source: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(source)
        .map_err(|error| format!("Não foi possível abrir o arquivo selecionado: {error}"))?;
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Falha ao ler o arquivo selecionado: {error}"))?;
    validate_oauth_client_json(&bytes)?;
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| "Pasta de destino do cliente OAuth inválida.".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Não foi possível criar a pasta de configurações: {error}"))?;
    let (temporary, mut out_file) = loop {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".fasttube-oauth-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "Não foi possível preparar a gravação do oauth-client.json: {error}"
                ));
            }
        }
    };
    let result = (|| -> io::Result<()> {
        out_file.write_all(&bytes)?;
        out_file.sync_all()?;
        drop(out_file);
        fs::rename(&temporary, dest)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(format!("Falha ao salvar oauth-client.json: {error}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path() -> PathBuf {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!(
                "fasttube-settings-test-{}-{id}",
                std::process::id()
            ))
            .join("settings.txt")
    }

    #[test]
    fn theme_round_trip_default_and_corrupt_rejection() {
        let path = test_path();
        assert_eq!(load_theme(&path).unwrap(), Theme::Dark);
        for theme in [Theme::Light, Theme::Dark] {
            save_theme(&path, theme).unwrap();
            assert_eq!(load_theme(&path).unwrap(), theme);
        }
        assert_eq!(
            save_theme(Path::new(""), Theme::Light).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        for content in ["2 dark", "1 system", "1 dark extra", "1", ""] {
            fs::write(&path, content).unwrap();
            assert_eq!(
                load_theme(&path).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
        }
        fs::write(&path, vec![b'a'; 1025]).unwrap();
        assert_eq!(
            load_theme(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::write(&path, [0xff]).unwrap();
        assert_eq!(
            load_theme(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn playback_preferences_round_trip_validation_and_fallback_format() {
        let path = test_path();
        assert_eq!(
            load_playback(&path).unwrap(),
            PlaybackPreferences::default()
        );
        for speed_quarters in 1..=8 {
            for max_height in [
                None,
                Some(360),
                Some(480),
                Some(720),
                Some(1080),
                Some(1440),
                Some(2160),
            ] {
                let preferences = PlaybackPreferences {
                    speed_quarters,
                    max_height,
                };
                save_playback(&path, preferences).unwrap();
                assert_eq!(load_playback(&path).unwrap(), preferences);
                let height = max_height.unwrap_or(1080);
                assert_eq!(
                    preferences.format(),
                    format!("bestvideo[height<={height}]+bestaudio/best[height<={height}]/best")
                );
            }
        }
        let previous = fs::read(&path).unwrap();
        assert!(
            save_playback(
                &path,
                PlaybackPreferences {
                    speed_quarters: 0,
                    max_height: None
                }
            )
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), previous);
        for content in [
            "2 4 auto",
            "1 0 auto",
            "1 9 auto",
            "1 4 999",
            "1 4 auto extra",
            "",
        ] {
            fs::write(&path, content).unwrap();
            assert_eq!(
                load_playback(&path).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
        }
        fs::write(&path, vec![b'a'; 1025]).unwrap();
        assert_eq!(
            load_playback(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn profiles_and_validation() {
        assert_eq!(
            BufferProfile::default().values(),
            BufferSettings {
                max_mib: 64,
                back_mib: 16,
                cache_seconds: 30
            }
        );
        for profile in [
            BufferProfile::Small,
            BufferProfile::Default,
            BufferProfile::Large,
        ] {
            assert!(profile.validate().is_ok());
        }
        for values in [
            BufferSettings {
                max_mib: 0,
                back_mib: 0,
                cache_seconds: 1,
            },
            BufferSettings {
                max_mib: 1025,
                back_mib: 0,
                cache_seconds: 1,
            },
            BufferSettings {
                max_mib: 1,
                back_mib: 2,
                cache_seconds: 1,
            },
            BufferSettings {
                max_mib: 1,
                back_mib: 0,
                cache_seconds: 0,
            },
            BufferSettings {
                max_mib: 1,
                back_mib: 0,
                cache_seconds: 3601,
            },
        ] {
            assert!(BufferProfile::Custom(values).validate().is_err());
        }
        assert!(
            BufferProfile::Custom(BufferSettings {
                max_mib: 1024,
                back_mib: 1024,
                cache_seconds: 3600
            })
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn round_trip_and_replace() {
        let path = test_path();
        assert_eq!(load(&path).unwrap(), BufferProfile::Default);
        for profile in [
            BufferProfile::Small,
            BufferProfile::Default,
            BufferProfile::Large,
            BufferProfile::Custom(BufferSettings {
                max_mib: 100,
                back_mib: 0,
                cache_seconds: 42,
            }),
        ] {
            save(&path, profile).unwrap();
            assert_eq!(load(&path).unwrap(), profile);
        }
        let invalid = BufferProfile::Custom(BufferSettings {
            max_mib: 0,
            back_mib: 0,
            cache_seconds: 1,
        });
        assert_eq!(
            save(&path, invalid).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(matches!(load(&path).unwrap(), BufferProfile::Custom(_)));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn rejects_corrupt_and_oversized_files() {
        let path = test_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for content in [
            "2 default",
            "1 custom 1 2 1",
            "1 custom -1 0 1",
            "1 default extra",
            "",
        ] {
            fs::write(&path, content).unwrap();
            assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        }
        fs::write(&path, vec![b'a'; 1025]).unwrap();
        assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        fs::write(&path, [0xff]).unwrap();
        assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn app_preferences_round_trip_validation_and_corrupt_preservation() {
        let dir = test_path().parent().unwrap().to_path_buf();
        let path = dir.join("app-preferences.json");
        assert_eq!(
            load_app_preferences(&path).unwrap(),
            AppPreferences::default()
        );

        let custom = AppPreferences {
            hwdec_mode: HwdecMode::Software,
            seek_step_secs: 15,
            default_autoplay: false,
            default_loop: true,
            resume_enabled: false,
            pause_watch_history: true,
            pause_recommendations: true,
            hide_shorts_in_home: true,
            keep_awake_during_playback: false,
            compact_card_density: true,
            cache_dir_override: Some(dir.display().to_string()),
            ..AppPreferences::default()
        };
        save_app_preferences(&path, &custom).unwrap();
        assert_eq!(load_app_preferences(&path).unwrap(), custom);

        let previous = fs::read(&path).unwrap();
        let invalid_step = AppPreferences {
            seek_step_secs: 7,
            ..custom.clone()
        };
        assert_eq!(
            save_app_preferences(&path, &invalid_step)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(fs::read(&path).unwrap(), previous);

        let invalid_rel_dir = AppPreferences {
            cache_dir_override: Some("relative/cache".into()),
            ..custom.clone()
        };
        assert_eq!(
            save_app_preferences(&path, &invalid_rel_dir)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(fs::read(&path).unwrap(), previous);

        fs::write(&path, b"{not-json").unwrap();
        assert_eq!(
            load_app_preferences(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(fs::read(&path).unwrap(), b"{not-json");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn yt_dlp_cache_measurement_and_clearing_only_touches_subdir() {
        let dir = test_path().parent().unwrap().to_path_buf();
        let keep_file = dir.join("unrelated.txt");
        let ytdlp_dir = yt_dlp_cache_subdir(&dir);
        fs::create_dir_all(&ytdlp_dir).unwrap();
        fs::write(&keep_file, b"keep-me").unwrap();
        fs::write(ytdlp_dir.join("entry.json"), b"0123456789").unwrap();

        let usage = measure_dir_usage(&ytdlp_dir);
        assert_eq!(usage.files, 1);
        assert_eq!(usage.bytes, 10);

        let cleared = clear_yt_dlp_cache_dir(&dir).unwrap();
        assert_eq!(cleared, 1);
        assert!(keep_file.is_file());
        assert_eq!(measure_dir_usage(&ytdlp_dir).files, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn app_preferences_new_fields_legacy_compat_and_validation() {
        let dir = test_path().parent().unwrap().to_path_buf();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app-preferences.json");

        fs::write(
            &path,
            br#"{"seek_step_secs":15,"compact_card_density":true}"#,
        )
        .unwrap();
        let legacy = load_app_preferences(&path).unwrap();
        assert_eq!(legacy.seek_step_secs, 15);
        assert!(legacy.compact_card_density);
        assert_eq!(legacy.default_volume_percent, 100);
        assert!(legacy.remember_volume);
        assert!(!legacy.audio_normalization);
        assert!(!legacy.auto_subtitles);
        assert_eq!(legacy.preferred_subtitle_lang, "pt");
        assert!(legacy.muted_topics.is_empty());

        let invalid_vol = AppPreferences {
            default_volume_percent: 101,
            ..AppPreferences::default()
        };
        assert!(save_app_preferences(&path, &invalid_vol).is_err());

        let custom = AppPreferences {
            remember_volume: false,
            default_volume_percent: 65,
            audio_normalization: true,
            auto_subtitles: true,
            preferred_subtitle_lang: " EN ".into(),
            muted_topics: vec!["  Rust ".into(), "rust".into(), "Gaming".into()],
            ..AppPreferences::default()
        };
        save_app_preferences(&path, &custom).unwrap();
        let loaded = load_app_preferences(&path).unwrap();
        assert!(!loaded.remember_volume);
        assert_eq!(loaded.default_volume_percent, 65);
        assert!(loaded.audio_normalization);
        assert!(loaded.auto_subtitles);
        assert_eq!(loaded.preferred_subtitle_lang, "en");
        assert_eq!(loaded.muted_topics, vec!["rust", "gaming"]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn oauth_client_validation_and_install_rejects_invalid_and_copies_valid() {
        let dir = test_path().parent().unwrap().to_path_buf();
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("source.json");
        let dst = dir.join("oauth-client.json");

        fs::write(&src, br#"{"web":{"client_id":"x"}}"#).unwrap();
        assert!(install_oauth_client_file(&src, &dst).is_err());
        assert!(!dst.exists());

        let valid = br#"{"installed":{"client_id":"abc.apps.googleusercontent.com","client_secret":"secret123"}}"#;
        fs::write(&src, valid).unwrap();
        install_oauth_client_file(&src, &dst).unwrap();
        assert_eq!(fs::read(&dst).unwrap(), valid);

        let _ = fs::remove_dir_all(&dir);
    }
}
