use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

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
    let content = match profile {
        BufferProfile::Small => "1 small\n".to_owned(),
        BufferProfile::Default => "1 default\n".to_owned(),
        BufferProfile::Large => "1 large\n".to_owned(),
        BufferProfile::Custom(values) => format!(
            "1 custom {} {} {}\n",
            values.max_mib, values.back_mib, values.cache_seconds
        ),
    };
    crate::history::write_atomic(path, content.as_bytes())
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

    #[allow(dead_code)]
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
    let height = preferences
        .max_height
        .map(|v| v.to_string())
        .unwrap_or_else(|| "auto".into());
    let content = format!("1 {} {}\n", preferences.speed_quarters, height);
    crate::history::write_atomic(path, content.as_bytes())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResolvedTheme {
    #[default]
    Dark,
    Light,
}

impl ResolvedTheme {
    pub fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }

    pub fn to_concrete_theme(self) -> Theme {
        match self {
            Self::Dark => Theme::Dark,
            Self::Light => Theme::Light,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    System,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::System => "system",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            "system" => Some(Self::System),
            _ => None,
        }
    }

    pub fn resolve(self, system_is_dark: bool) -> ResolvedTheme {
        match self {
            Self::Dark => ResolvedTheme::Dark,
            Self::Light => ResolvedTheme::Light,
            Self::System => {
                if system_is_dark {
                    ResolvedTheme::Dark
                } else {
                    ResolvedTheme::Light
                }
            }
        }
    }
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
        ["1", "system"] => Ok(Theme::System),
        _ => Err(invalid()),
    }
}

pub fn save_theme(path: &Path, theme: Theme) -> io::Result<()> {
    let content = format!("1 {}\n", theme.as_str());
    crate::history::write_atomic(path, content.as_bytes())
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeybindingConflict {
    pub action_a: &'static str,
    pub action_b: &'static str,
    pub binding: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Keybindings {
    pub play_pause: String,
    pub seek_back: String,
    pub seek_forward: String,
    pub mute: String,
    pub volume_up: String,
    pub volume_down: String,
    pub theater: String,
    pub fullscreen: String,
    pub fullscreen_alt: String,
    pub mini_player: String,
    pub audio_only: String,
    pub captions: String,
    pub caption_size_up: String,
    pub caption_size_down: String,
    pub seek_home: String,
    pub seek_end: String,
    pub prev_chapter: String,
    pub next_chapter: String,
    pub prev_video: String,
    pub next_video: String,
    pub speed_down: String,
    pub speed_up: String,
    pub frame_back: String,
    pub frame_forward: String,
    pub screenshot: String,
    pub ab_loop_a: String,
    pub ab_loop_b: String,
    pub ab_loop_clear: String,
    pub focus_search: String,
    pub show_shortcuts: String,
    pub command_palette: String,
}

impl Default for Keybindings {
    fn default() -> Self {
        Self {
            play_pause: "K".to_owned(),
            seek_back: "ArrowLeft".to_owned(),
            seek_forward: "ArrowRight".to_owned(),
            mute: "M".to_owned(),
            volume_up: "ArrowUp".to_owned(),
            volume_down: "ArrowDown".to_owned(),
            theater: "T".to_owned(),
            fullscreen: "F".to_owned(),
            fullscreen_alt: "F11".to_owned(),
            mini_player: "I".to_owned(),
            audio_only: "A".to_owned(),
            captions: "C".to_owned(),
            caption_size_up: "+".to_owned(),
            caption_size_down: "-".to_owned(),
            seek_home: "Home".to_owned(),
            seek_end: "End".to_owned(),
            prev_chapter: "Ctrl+ArrowLeft".to_owned(),
            next_chapter: "Ctrl+ArrowRight".to_owned(),
            prev_video: "Shift+P".to_owned(),
            next_video: "Shift+N".to_owned(),
            speed_down: "Shift+,".to_owned(),
            speed_up: "Shift+.".to_owned(),
            frame_back: ",".to_owned(),
            frame_forward: ".".to_owned(),
            screenshot: "S".to_owned(),
            ab_loop_a: "[".to_owned(),
            ab_loop_b: "]".to_owned(),
            ab_loop_clear: "\\".to_owned(),
            focus_search: "/".to_owned(),
            show_shortcuts: "?".to_owned(),
            command_palette: "Ctrl+K".to_owned(),
        }
    }
}

impl Keybindings {
    pub fn entries(&self) -> Vec<(&'static str, &str)> {
        vec![
            ("play_pause", self.play_pause.as_str()),
            ("seek_back", self.seek_back.as_str()),
            ("seek_forward", self.seek_forward.as_str()),
            ("mute", self.mute.as_str()),
            ("volume_up", self.volume_up.as_str()),
            ("volume_down", self.volume_down.as_str()),
            ("theater", self.theater.as_str()),
            ("fullscreen", self.fullscreen.as_str()),
            ("fullscreen_alt", self.fullscreen_alt.as_str()),
            ("mini_player", self.mini_player.as_str()),
            ("audio_only", self.audio_only.as_str()),
            ("captions", self.captions.as_str()),
            ("caption_size_up", self.caption_size_up.as_str()),
            ("caption_size_down", self.caption_size_down.as_str()),
            ("seek_home", self.seek_home.as_str()),
            ("seek_end", self.seek_end.as_str()),
            ("prev_chapter", self.prev_chapter.as_str()),
            ("next_chapter", self.next_chapter.as_str()),
            ("prev_video", self.prev_video.as_str()),
            ("next_video", self.next_video.as_str()),
            ("speed_down", self.speed_down.as_str()),
            ("speed_up", self.speed_up.as_str()),
            ("frame_back", self.frame_back.as_str()),
            ("frame_forward", self.frame_forward.as_str()),
            ("screenshot", self.screenshot.as_str()),
            ("ab_loop_a", self.ab_loop_a.as_str()),
            ("ab_loop_b", self.ab_loop_b.as_str()),
            ("ab_loop_clear", self.ab_loop_clear.as_str()),
            ("focus_search", self.focus_search.as_str()),
            ("show_shortcuts", self.show_shortcuts.as_str()),
            ("command_palette", self.command_palette.as_str()),
        ]
    }

    pub fn get_slot(&self, slot: &str) -> Option<&str> {
        match slot {
            "play_pause" => Some(&self.play_pause),
            "seek_back" => Some(&self.seek_back),
            "seek_forward" => Some(&self.seek_forward),
            "mute" => Some(&self.mute),
            "volume_up" => Some(&self.volume_up),
            "volume_down" => Some(&self.volume_down),
            "theater" => Some(&self.theater),
            "fullscreen" => Some(&self.fullscreen),
            "fullscreen_alt" => Some(&self.fullscreen_alt),
            "mini_player" => Some(&self.mini_player),
            "audio_only" => Some(&self.audio_only),
            "captions" => Some(&self.captions),
            "caption_size_up" => Some(&self.caption_size_up),
            "caption_size_down" => Some(&self.caption_size_down),
            "seek_home" => Some(&self.seek_home),
            "seek_end" => Some(&self.seek_end),
            "prev_chapter" => Some(&self.prev_chapter),
            "next_chapter" => Some(&self.next_chapter),
            "prev_video" => Some(&self.prev_video),
            "next_video" => Some(&self.next_video),
            "speed_down" => Some(&self.speed_down),
            "speed_up" => Some(&self.speed_up),
            "frame_back" => Some(&self.frame_back),
            "frame_forward" => Some(&self.frame_forward),
            "screenshot" => Some(&self.screenshot),
            "ab_loop_a" => Some(&self.ab_loop_a),
            "ab_loop_b" => Some(&self.ab_loop_b),
            "ab_loop_clear" => Some(&self.ab_loop_clear),
            "focus_search" => Some(&self.focus_search),
            "show_shortcuts" => Some(&self.show_shortcuts),
            "command_palette" => Some(&self.command_palette),
            _ => None,
        }
    }

    pub fn set_slot(&mut self, slot: &str, binding: String) -> bool {
        let target = match slot {
            "play_pause" => &mut self.play_pause,
            "seek_back" => &mut self.seek_back,
            "seek_forward" => &mut self.seek_forward,
            "mute" => &mut self.mute,
            "volume_up" => &mut self.volume_up,
            "volume_down" => &mut self.volume_down,
            "theater" => &mut self.theater,
            "fullscreen" => &mut self.fullscreen,
            "fullscreen_alt" => &mut self.fullscreen_alt,
            "mini_player" => &mut self.mini_player,
            "audio_only" => &mut self.audio_only,
            "captions" => &mut self.captions,
            "caption_size_up" => &mut self.caption_size_up,
            "caption_size_down" => &mut self.caption_size_down,
            "seek_home" => &mut self.seek_home,
            "seek_end" => &mut self.seek_end,
            "prev_chapter" => &mut self.prev_chapter,
            "next_chapter" => &mut self.next_chapter,
            "prev_video" => &mut self.prev_video,
            "next_video" => &mut self.next_video,
            "speed_down" => &mut self.speed_down,
            "speed_up" => &mut self.speed_up,
            "frame_back" => &mut self.frame_back,
            "frame_forward" => &mut self.frame_forward,
            "screenshot" => &mut self.screenshot,
            "ab_loop_a" => &mut self.ab_loop_a,
            "ab_loop_b" => &mut self.ab_loop_b,
            "ab_loop_clear" => &mut self.ab_loop_clear,
            "focus_search" => &mut self.focus_search,
            "show_shortcuts" => &mut self.show_shortcuts,
            "command_palette" => &mut self.command_palette,
            _ => return false,
        };
        *target = binding;
        true
    }

    pub fn default_for_slot(slot: &str) -> Option<String> {
        Self::default().get_slot(slot).map(str::to_owned)
    }

    pub fn reset_slot(&mut self, slot: &str) -> bool {
        let Some(def) = Self::default_for_slot(slot) else {
            return false;
        };
        self.set_slot(slot, def)
    }

    pub fn slot_label(slot: &str, pt: bool, seek_step_secs: u32) -> String {
        match slot {
            "play_pause" => if pt {
                "Reproduzir / pausar"
            } else {
                "Play / pause"
            }
            .to_owned(),
            "seek_back" => {
                if pt {
                    format!("Voltar {seek_step_secs} segundos")
                } else {
                    format!("Seek back {seek_step_secs}s")
                }
            }
            "seek_forward" => {
                if pt {
                    format!("Avançar {seek_step_secs} segundos")
                } else {
                    format!("Seek forward {seek_step_secs}s")
                }
            }
            "mute" => if pt { "Silenciar áudio" } else { "Mute audio" }.to_owned(),
            "volume_up" => if pt {
                "Aumentar volume (+5%)"
            } else {
                "Volume up (+5%)"
            }
            .to_owned(),
            "volume_down" => if pt {
                "Diminuir volume (-5%)"
            } else {
                "Volume down (-5%)"
            }
            .to_owned(),
            "theater" => if pt { "Modo teatro" } else { "Theater mode" }.to_owned(),
            "fullscreen" => if pt { "Tela cheia" } else { "Fullscreen" }.to_owned(),
            "fullscreen_alt" => if pt {
                "Tela cheia (alternativo)"
            } else {
                "Fullscreen (alternate)"
            }
            .to_owned(),
            "mini_player" => "Mini player".to_owned(),
            "audio_only" => if pt {
                "Modo somente áudio"
            } else {
                "Audio-only mode"
            }
            .to_owned(),
            "captions" => if pt {
                "Alternar legendas"
            } else {
                "Toggle captions"
            }
            .to_owned(),
            "caption_size_up" => if pt {
                "Aumentar tamanho da legenda"
            } else {
                "Increase caption size"
            }
            .to_owned(),
            "caption_size_down" => if pt {
                "Diminuir tamanho da legenda"
            } else {
                "Decrease caption size"
            }
            .to_owned(),
            "seek_home" => if pt {
                "Ir para o início do vídeo"
            } else {
                "Seek to start"
            }
            .to_owned(),
            "seek_end" => if pt {
                "Ir para o fim do vídeo"
            } else {
                "Seek to end"
            }
            .to_owned(),
            "prev_chapter" => if pt {
                "Capítulo anterior"
            } else {
                "Previous chapter"
            }
            .to_owned(),
            "next_chapter" => if pt {
                "Próximo capítulo"
            } else {
                "Next chapter"
            }
            .to_owned(),
            "prev_video" => if pt {
                "Vídeo anterior"
            } else {
                "Previous video"
            }
            .to_owned(),
            "next_video" => if pt { "Próximo vídeo" } else { "Next video" }.to_owned(),
            "speed_down" => if pt {
                "Reduzir velocidade"
            } else {
                "Decrease speed"
            }
            .to_owned(),
            "speed_up" => if pt {
                "Aumentar velocidade"
            } else {
                "Increase speed"
            }
            .to_owned(),
            "frame_back" => if pt {
                "Quadro anterior (pausado)"
            } else {
                "Previous frame (paused)"
            }
            .to_owned(),
            "frame_forward" => if pt {
                "Próximo quadro (pausado)"
            } else {
                "Next frame (paused)"
            }
            .to_owned(),
            "screenshot" => if pt {
                "Capturar frame (PNG)"
            } else {
                "Screenshot frame (PNG)"
            }
            .to_owned(),
            "ab_loop_a" => if pt {
                "Marcar ponto A do loop"
            } else {
                "Set A-B loop start"
            }
            .to_owned(),
            "ab_loop_b" => if pt {
                "Marcar ponto B do loop"
            } else {
                "Set A-B loop end"
            }
            .to_owned(),
            "ab_loop_clear" => if pt {
                "Limpar loop A-B"
            } else {
                "Clear A-B loop"
            }
            .to_owned(),
            "focus_search" => if pt {
                "Focar barra de busca"
            } else {
                "Focus search bar"
            }
            .to_owned(),
            "show_shortcuts" => if pt {
                "Exibir guia de atalhos"
            } else {
                "Show shortcuts guide"
            }
            .to_owned(),
            "command_palette" => if pt {
                "Paleta de comandos"
            } else {
                "Command palette"
            }
            .to_owned(),
            other => other.to_owned(),
        }
    }

    fn bindings_collide(a: &str, b: &str) -> bool {
        let a_trim = a.trim();
        let b_trim = b.trim();
        if a_trim.is_empty() || b_trim.is_empty() {
            return false;
        }
        match (Self::parse_egui_key(a_trim), Self::parse_egui_key(b_trim)) {
            (Some(ka), Some(kb)) => ka == kb,
            _ => a_trim.eq_ignore_ascii_case(b_trim),
        }
    }

    pub fn detect_conflicts(&self) -> Vec<KeybindingConflict> {
        let entries = self.entries();
        let mut conflicts = Vec::new();
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                let (action_a, bind_a) = entries[i];
                let (action_b, bind_b) = entries[j];
                if Self::bindings_collide(bind_a, bind_b) {
                    conflicts.push(KeybindingConflict {
                        action_a,
                        action_b,
                        binding: bind_a.trim().to_owned(),
                    });
                }
            }
        }
        conflicts
    }

    pub fn resolve_conflict(&mut self, action_a: &str, action_b: &str) -> bool {
        let defaults = Self::default();
        let a_is_default = self
            .get_slot(action_a)
            .zip(defaults.get_slot(action_a))
            .is_some_and(|(cur, def)| Self::bindings_collide(cur, def));
        let b_is_default = self
            .get_slot(action_b)
            .zip(defaults.get_slot(action_b))
            .is_some_and(|(cur, def)| Self::bindings_collide(cur, def));

        let mut changed = false;
        if b_is_default && !a_is_default {
            changed |= self.reset_slot(action_a);
        } else {
            changed |= self.reset_slot(action_b);
        }
        if let (Some(a_now), Some(b_now)) = (self.get_slot(action_a), self.get_slot(action_b))
            && Self::bindings_collide(a_now, b_now)
        {
            changed |= self.reset_slot(action_a);
            changed |= self.reset_slot(action_b);
        }
        changed
    }

    pub fn normalized(mut self) -> Self {
        let norm = |value: &mut String, fallback: &str| {
            let trimmed = value.trim();
            if trimmed.is_empty() || trimmed.len() > 32 {
                *value = fallback.to_owned();
            } else {
                *value = trimmed.to_owned();
            }
        };
        let defaults = Self::default();
        norm(&mut self.play_pause, &defaults.play_pause);
        norm(&mut self.seek_back, &defaults.seek_back);
        norm(&mut self.seek_forward, &defaults.seek_forward);
        norm(&mut self.mute, &defaults.mute);
        norm(&mut self.volume_up, &defaults.volume_up);
        norm(&mut self.volume_down, &defaults.volume_down);
        norm(&mut self.theater, &defaults.theater);
        norm(&mut self.fullscreen, &defaults.fullscreen);
        norm(&mut self.fullscreen_alt, &defaults.fullscreen_alt);
        norm(&mut self.mini_player, &defaults.mini_player);
        norm(&mut self.audio_only, &defaults.audio_only);
        norm(&mut self.captions, &defaults.captions);
        norm(&mut self.caption_size_up, &defaults.caption_size_up);
        norm(&mut self.caption_size_down, &defaults.caption_size_down);
        norm(&mut self.seek_home, &defaults.seek_home);
        norm(&mut self.seek_end, &defaults.seek_end);
        norm(&mut self.prev_chapter, &defaults.prev_chapter);
        norm(&mut self.next_chapter, &defaults.next_chapter);
        norm(&mut self.prev_video, &defaults.prev_video);
        norm(&mut self.next_video, &defaults.next_video);
        norm(&mut self.speed_down, &defaults.speed_down);
        norm(&mut self.speed_up, &defaults.speed_up);
        norm(&mut self.frame_back, &defaults.frame_back);
        norm(&mut self.frame_forward, &defaults.frame_forward);
        norm(&mut self.screenshot, &defaults.screenshot);
        norm(&mut self.ab_loop_a, &defaults.ab_loop_a);
        norm(&mut self.ab_loop_b, &defaults.ab_loop_b);
        norm(&mut self.ab_loop_clear, &defaults.ab_loop_clear);
        norm(&mut self.focus_search, &defaults.focus_search);
        norm(&mut self.show_shortcuts, &defaults.show_shortcuts);
        norm(&mut self.command_palette, &defaults.command_palette);
        self
    }

    pub fn valid(&self) -> bool {
        self.entries()
            .into_iter()
            .all(|(_, s)| !s.trim().is_empty() && s.len() <= 32)
    }

    pub fn parse_egui_key(name: &str) -> Option<(bool, bool, bool, eframe::egui::Key)> {
        use eframe::egui::Key;
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return None;
        }
        let (mod_prefix, key_token) = if trimmed == "+" {
            ("", "+")
        } else if let Some(prefix) = trimmed.strip_suffix("++") {
            (prefix, "+")
        } else if let Some(idx) = trimmed.rfind('+') {
            (&trimmed[..idx], trimmed[idx + 1..].trim())
        } else {
            ("", trimmed)
        };
        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        if !mod_prefix.is_empty() {
            for modifier in mod_prefix.split('+').map(str::trim) {
                if modifier.is_empty() {
                    return None;
                }
                if modifier.eq_ignore_ascii_case("ctrl") {
                    ctrl = true;
                } else if modifier.eq_ignore_ascii_case("alt") {
                    alt = true;
                } else if modifier.eq_ignore_ascii_case("shift") {
                    shift = true;
                } else {
                    return None;
                }
            }
        }
        let key = match key_token.to_ascii_lowercase().as_str() {
            "a" => Key::A,
            "b" => Key::B,
            "c" => Key::C,
            "d" => Key::D,
            "e" => Key::E,
            "f" => Key::F,
            "g" => Key::G,
            "h" => Key::H,
            "i" => Key::I,
            "j" => Key::J,
            "k" => Key::K,
            "l" => Key::L,
            "m" => Key::M,
            "n" => Key::N,
            "o" => Key::O,
            "p" => Key::P,
            "q" => Key::Q,
            "r" => Key::R,
            "s" => Key::S,
            "t" => Key::T,
            "u" => Key::U,
            "v" => Key::V,
            "w" => Key::W,
            "x" => Key::X,
            "y" => Key::Y,
            "z" => Key::Z,
            "0" => Key::Num0,
            "1" => Key::Num1,
            "2" => Key::Num2,
            "3" => Key::Num3,
            "4" => Key::Num4,
            "5" => Key::Num5,
            "6" => Key::Num6,
            "7" => Key::Num7,
            "8" => Key::Num8,
            "9" => Key::Num9,
            "f1" => Key::F1,
            "f2" => Key::F2,
            "f3" => Key::F3,
            "f4" => Key::F4,
            "f5" => Key::F5,
            "f6" => Key::F6,
            "f7" => Key::F7,
            "f8" => Key::F8,
            "f9" => Key::F9,
            "f10" => Key::F10,
            "f11" => Key::F11,
            "f12" => Key::F12,
            "space" => Key::Space,
            "enter" => Key::Enter,
            "tab" => Key::Tab,
            "backspace" => Key::Backspace,
            "insert" => Key::Insert,
            "delete" => Key::Delete,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" => Key::PageUp,
            "pagedown" => Key::PageDown,
            "arrowleft" | "left" => Key::ArrowLeft,
            "arrowright" | "right" => Key::ArrowRight,
            "arrowup" | "up" => Key::ArrowUp,
            "arrowdown" | "down" => Key::ArrowDown,
            "[" | "openbracket" => Key::OpenBracket,
            "]" | "closebracket" => Key::CloseBracket,
            "\\" | "backslash" => Key::Backslash,
            "," | "comma" => Key::Comma,
            "." | "period" => Key::Period,
            "/" | "slash" => Key::Slash,
            "?" | "questionmark" => Key::Questionmark,
            "+" | "plus" => Key::Plus,
            "-" | "minus" => Key::Minus,
            "=" | "equals" => Key::Equals,
            ";" | "semicolon" => Key::Semicolon,
            "'" | "quote" => Key::Quote,
            "`" | "backtick" => Key::Backtick,
            _ => return None,
        };
        Some((ctrl, alt, shift, key))
    }

    pub fn format_egui_key(
        ctrl: bool,
        alt: bool,
        shift: bool,
        key: eframe::egui::Key,
    ) -> Option<String> {
        use eframe::egui::Key;
        let name = match key {
            Key::A => "A",
            Key::B => "B",
            Key::C => "C",
            Key::D => "D",
            Key::E => "E",
            Key::F => "F",
            Key::G => "G",
            Key::H => "H",
            Key::I => "I",
            Key::J => "J",
            Key::K => "K",
            Key::L => "L",
            Key::M => "M",
            Key::N => "N",
            Key::O => "O",
            Key::P => "P",
            Key::Q => "Q",
            Key::R => "R",
            Key::S => "S",
            Key::T => "T",
            Key::U => "U",
            Key::V => "V",
            Key::W => "W",
            Key::X => "X",
            Key::Y => "Y",
            Key::Z => "Z",
            Key::Num0 => "0",
            Key::Num1 => "1",
            Key::Num2 => "2",
            Key::Num3 => "3",
            Key::Num4 => "4",
            Key::Num5 => "5",
            Key::Num6 => "6",
            Key::Num7 => "7",
            Key::Num8 => "8",
            Key::Num9 => "9",
            Key::F1 => "F1",
            Key::F2 => "F2",
            Key::F3 => "F3",
            Key::F4 => "F4",
            Key::F5 => "F5",
            Key::F6 => "F6",
            Key::F7 => "F7",
            Key::F8 => "F8",
            Key::F9 => "F9",
            Key::F10 => "F10",
            Key::F11 => "F11",
            Key::F12 => "F12",
            Key::Space => "Space",
            Key::Enter => "Enter",
            Key::Tab => "Tab",
            Key::Backspace => "Backspace",
            Key::Insert => "Insert",
            Key::Delete => "Delete",
            Key::Home => "Home",
            Key::End => "End",
            Key::PageUp => "PageUp",
            Key::PageDown => "PageDown",
            Key::ArrowLeft => "ArrowLeft",
            Key::ArrowRight => "ArrowRight",
            Key::ArrowUp => "ArrowUp",
            Key::ArrowDown => "ArrowDown",
            Key::OpenBracket => "[",
            Key::CloseBracket => "]",
            Key::Backslash => "\\",
            Key::Comma => ",",
            Key::Period => ".",
            Key::Slash => "/",
            Key::Questionmark => "?",
            Key::Plus => "+",
            Key::Minus => "-",
            Key::Equals => "=",
            Key::Semicolon => ";",
            Key::Quote => "'",
            Key::Backtick => "`",
            _ => return None,
        };
        let mut parts = Vec::new();
        if ctrl {
            parts.push("Ctrl");
        }
        if alt {
            parts.push("Alt");
        }
        // Shifted punctuation keys (`?`, `+`) carry their own identity on most keyboard layouts.
        let include_shift = shift && !matches!(key, Key::Questionmark | Key::Plus);
        if include_shift {
            parts.push("Shift");
        }
        parts.push(name);
        Some(parts.join("+"))
    }

    pub fn matches_input(input: &eframe::egui::InputState, binding: &str) -> bool {
        let Some((ctrl, alt, shift, target_key)) = Self::parse_egui_key(binding) else {
            return false;
        };
        let shift_is_implicit = !shift
            && matches!(
                target_key,
                eframe::egui::Key::Questionmark | eframe::egui::Key::Plus
            );
        let mods_match = |mods: eframe::egui::Modifiers| {
            (mods.ctrl || mods.command) == ctrl
                && mods.alt == alt
                && (shift_is_implicit || mods.shift == shift)
        };
        if input.key_pressed(target_key) && mods_match(input.modifiers) {
            return true;
        }
        input.events.iter().any(|ev| {
            matches!(
                ev,
                eframe::egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if *key == target_key && mods_match(*modifiers)
            )
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelNotifyLevel {
    #[default]
    Personalized,
    All,
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UltrawideZoomMode {
    #[default]
    Fit,
    Crop21x9,
    Custom,
}

impl UltrawideZoomMode {
    #[allow(dead_code)]
    pub const ALL: [Self; 3] = [Self::Fit, Self::Crop21x9, Self::Custom];

    pub fn label_pt(self) -> &'static str {
        match self {
            Self::Fit => "Ajustar (padrão)",
            Self::Crop21x9 => "Recortar 21:9",
            Self::Custom => "Zoom personalizado",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::Fit => "Fit (default)",
            Self::Crop21x9 => "Crop 21:9",
            Self::Custom => "Custom zoom",
        }
    }

    pub fn label(self, pt: bool) -> &'static str {
        if pt { self.label_pt() } else { self.label_en() }
    }
}

fn is_valid_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit)
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CaptionStylePrefs {
    pub font_scale: f32,
    pub bg_opacity: f32,
    pub text_color_hex: String,
    pub translate_lang: Option<String>,
}

impl Default for CaptionStylePrefs {
    fn default() -> Self {
        Self {
            font_scale: 1.0,
            bg_opacity: 0.72,
            text_color_hex: "#FFFFFF".to_owned(),
            translate_lang: None,
        }
    }
}

impl CaptionStylePrefs {
    pub fn normalized(mut self) -> Self {
        self.font_scale = if self.font_scale.is_finite() {
            self.font_scale.clamp(0.5, 2.0)
        } else {
            1.0
        };
        self.bg_opacity = if self.bg_opacity.is_finite() {
            self.bg_opacity.clamp(0.0, 1.0)
        } else {
            0.72
        };
        let trimmed_hex = self.text_color_hex.trim();
        self.text_color_hex = if is_valid_hex_color(trimmed_hex) {
            trimmed_hex.to_ascii_uppercase()
        } else {
            "#FFFFFF".to_owned()
        };
        self.translate_lang = self
            .translate_lang
            .map(|l| l.trim().to_owned())
            .filter(|l| !l.is_empty() && l.len() <= 16);
        self
    }

    pub fn valid(&self) -> bool {
        self.font_scale.is_finite()
            && (0.5..=2.0).contains(&self.font_scale)
            && self.bg_opacity.is_finite()
            && (0.0..=1.0).contains(&self.bg_opacity)
            && is_valid_hex_color(self.text_color_hex.trim())
            && self
                .translate_lang
                .as_deref()
                .is_none_or(|l| !l.trim().is_empty() && l.trim().len() <= 16)
    }

    pub fn parse_rgb(&self) -> (u8, u8, u8) {
        let hex = self.text_color_hex.trim();
        if !is_valid_hex_color(hex) {
            return (255, 255, 255);
        }
        let r = u8::from_str_radix(&hex[1..3], 16).unwrap_or(255);
        let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or(255);
        let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or(255);
        (r, g, b)
    }
}

pub const CAPTION_TRANSLATE_LANGUAGES: &[(&str, &str)] = &[
    ("pt-BR", "PT-BR"),
    ("en", "EN"),
    ("es", "ES"),
    ("fr", "FR"),
    ("de", "DE"),
    ("ja", "JA"),
    ("ko", "KO"),
];

pub const CAPTION_COLOR_PRESETS: &[(&str, &str, &str)] = &[
    ("#FFFFFF", "Branco", "White"),
    ("#FFF176", "Amarelo", "Yellow"),
    ("#80DEEA", "Ciano", "Cyan"),
    ("#A5D6A7", "Verde", "Green"),
];

pub const COUNTRY_REGION_OPTIONS: &[(&str, &str, &str)] = &[
    ("BR", "Brasil (BR)", "Brazil (BR)"),
    ("US", "Estados Unidos (US)", "United States (US)"),
    ("GB", "Reino Unido (GB)", "United Kingdom (GB)"),
    ("PT", "Portugal (PT)", "Portugal (PT)"),
    ("DE", "Alemanha (DE)", "Germany (DE)"),
    ("FR", "França (FR)", "France (FR)"),
    ("ES", "Espanha (ES)", "Spain (ES)"),
    ("JP", "Japão (JP)", "Japan (JP)"),
    ("KR", "Coreia do Sul (KR)", "South Korea (KR)"),
    ("CA", "Canadá (CA)", "Canada (CA)"),
    ("AU", "Austrália (AU)", "Australia (AU)"),
    ("MX", "México (MX)", "Mexico (MX)"),
    ("AR", "Argentina (AR)", "Argentina (AR)"),
    ("IN", "Índia (IN)", "India (IN)"),
    ("IT", "Itália (IT)", "Italy (IT)"),
];

fn default_download_max_concurrent() -> usize {
    2
}

fn default_true() -> bool {
    true
}

pub const MAX_PER_CHANNEL_SPEEDS: usize = 250;
pub const MAX_AUTO_SKIP_CHAPTER_KEYWORDS: usize = 32;
pub const DEFAULT_CHAPTER_SKIP_KEYWORD_PRESETS: &[&str] = &[
    "sponsor",
    "intro",
    "outro",
    "merch",
    "ad",
    "patrocinador",
    "anúncio",
];

pub fn normalize_channel_speed_key(channel: &str) -> String {
    channel.trim().to_lowercase()
}

pub fn set_channel_playback_speed(
    map: &mut std::collections::HashMap<String, f64>,
    channel: &str,
    speed: f64,
) {
    let key = normalize_channel_speed_key(channel);
    if key.is_empty() || key.len() > 128 || !speed.is_finite() {
        return;
    }
    let clamped = speed.clamp(0.25, 4.0);
    if (clamped - 1.0).abs() <= 0.01 {
        map.remove(&key);
        return;
    }
    if let Some(existing) = map.get_mut(&key) {
        *existing = clamped;
        return;
    }
    while map.len() >= MAX_PER_CHANNEL_SPEEDS {
        let Some(evict) = map.keys().min().cloned() else {
            break;
        };
        map.remove(&evict);
    }
    map.insert(key, clamped);
}

pub fn get_channel_playback_speed(
    map: &std::collections::HashMap<String, f64>,
    channel: &str,
) -> Option<f64> {
    let key = normalize_channel_speed_key(channel);
    if key.is_empty() {
        return None;
    }
    map.get(&key)
        .copied()
        .filter(|speed| speed.is_finite())
        .map(|speed| speed.clamp(0.25, 4.0))
}

pub fn should_auto_skip_chapter(chapter_title: &str, keywords: &[String]) -> bool {
    let title = chapter_title.trim();
    if title.is_empty() || keywords.is_empty() {
        return false;
    }
    let title_lower = title.to_lowercase();
    keywords.iter().any(|kw| {
        let trimmed = kw.trim();
        !trimmed.is_empty() && title_lower.contains(&trimmed.to_lowercase())
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum VideoCodecPreference {
    #[default]
    Auto,
    Vp9,
    H264,
    Av1,
}

impl VideoCodecPreference {
    #[allow(dead_code)]
    pub const ALL: [Self; 4] = [Self::Auto, Self::Vp9, Self::H264, Self::Av1];

    pub fn label_pt(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Vp9 => "VP9",
            Self::H264 => "H.264",
            Self::Av1 => "AV1",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Vp9 => "VP9",
            Self::H264 => "H.264",
            Self::Av1 => "AV1",
        }
    }

    pub fn description_pt(self) -> &'static str {
        match self {
            Self::Auto => "Deixa o player selecionar automaticamente o melhor codec disponível.",
            Self::Vp9 => "Prefere VP9 (boa compressão e ampla aceleração por GPU).",
            Self::H264 => "Prefere H.264/AVC (máxima compatibilidade e menor uso de CPU).",
            Self::Av1 => "Prefere AV1 (menor banda em resoluções altas; requer GPU moderna).",
        }
    }

    pub fn description_en(self) -> &'static str {
        match self {
            Self::Auto => "Lets the player automatically pick the best available codec.",
            Self::Vp9 => "Prefers VP9 (strong compression and wide GPU hardware support).",
            Self::H264 => "Prefers H.264/AVC (broadest compatibility and lowest CPU usage).",
            Self::Av1 => "Prefers AV1 (lowest bandwidth at high resolutions; best on modern GPUs).",
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AppPreferences {
    pub sponsorblock: crate::sponsorblock::SponsorBlockSettings,
    pub hwdec_mode: HwdecMode,
    #[serde(default)]
    pub video_codec_preference: VideoCodecPreference,
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
    pub muted_channels: Vec<String>,
    pub pinned_topics: Vec<String>,
    pub pause_watch_history: bool,
    pub pause_recommendations: bool,
    pub hide_shorts_in_home: bool,
    pub keep_awake_during_playback: bool,
    pub compact_card_density: bool,
    pub cache_dir_override: Option<String>,
    pub language: crate::i18n::UiLanguage,
    pub auto_mini_player: bool,
    pub ryd_enabled: bool,
    pub dearrow_enabled: bool,
    pub download_dir_override: Option<String>,
    pub minimize_to_tray: bool,
    pub close_to_tray: bool,
    pub start_minimized_to_tray: bool,
    pub auto_check_updates: bool,
    pub skipped_app_update_version: Option<String>,
    pub keybindings: Keybindings,
    #[serde(default)]
    pub country_code: Option<String>,
    #[serde(default)]
    pub restricted_mode: bool,
    #[serde(default)]
    pub ambient_mode: bool,
    #[serde(default)]
    pub show_remaining_time: bool,
    #[serde(default)]
    pub caption_style: CaptionStylePrefs,
    #[serde(default)]
    pub ultrawide_zoom_mode: UltrawideZoomMode,
    #[serde(default)]
    pub ultrawide_custom_zoom: f32,
    #[serde(default)]
    pub per_channel_notifications: std::collections::HashMap<String, ChannelNotifyLevel>,
    #[serde(default = "default_download_max_concurrent")]
    pub download_max_concurrent: usize,
    #[serde(default)]
    pub download_embed_subs: bool,
    #[serde(default)]
    pub download_embed_chapters: bool,
    #[serde(default)]
    pub download_embed_thumbnail: bool,
    #[serde(default)]
    pub mica_backdrop: bool,
    #[serde(default = "default_true")]
    pub windows_toast_notifications: bool,
    #[serde(default = "default_true")]
    pub remember_speed_per_channel: bool,
    #[serde(default)]
    pub per_channel_speeds: std::collections::HashMap<String, f64>,
    #[serde(default)]
    pub auto_skip_chapters_enabled: bool,
    #[serde(default)]
    pub auto_skip_chapter_keywords: Vec<String>,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            sponsorblock: crate::sponsorblock::SponsorBlockSettings::default(),
            hwdec_mode: HwdecMode::AutoSafe,
            video_codec_preference: VideoCodecPreference::default(),
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
            muted_channels: Vec::new(),
            pinned_topics: Vec::new(),
            pause_watch_history: false,
            pause_recommendations: false,
            hide_shorts_in_home: false,
            keep_awake_during_playback: true,
            compact_card_density: false,
            cache_dir_override: None,
            language: crate::i18n::UiLanguage::default(),
            auto_mini_player: false,
            ryd_enabled: false,
            dearrow_enabled: false,
            download_dir_override: None,
            minimize_to_tray: false,
            close_to_tray: false,
            start_minimized_to_tray: false,
            auto_check_updates: true,
            skipped_app_update_version: None,
            keybindings: Keybindings::default(),
            country_code: None,
            restricted_mode: false,
            ambient_mode: false,
            show_remaining_time: false,
            caption_style: CaptionStylePrefs::default(),
            ultrawide_zoom_mode: UltrawideZoomMode::default(),
            ultrawide_custom_zoom: 0.0,
            per_channel_notifications: std::collections::HashMap::new(),
            download_max_concurrent: default_download_max_concurrent(),
            download_embed_subs: false,
            download_embed_chapters: false,
            download_embed_thumbnail: false,
            mica_backdrop: false,
            windows_toast_notifications: true,
            remember_speed_per_channel: true,
            per_channel_speeds: std::collections::HashMap::new(),
            auto_skip_chapters_enabled: false,
            auto_skip_chapter_keywords: Vec::new(),
        }
    }
}

impl AppPreferences {
    pub fn normalized(mut self) -> Self {
        self.sponsorblock = self.sponsorblock.normalized();
        self.keybindings = self.keybindings.normalized();
        self.caption_style = self.caption_style.normalized();
        self.preferred_subtitle_lang = self.preferred_subtitle_lang.trim().to_ascii_lowercase();
        self.country_code = self
            .country_code
            .map(|c| c.trim().to_ascii_uppercase())
            .filter(|c| c.len() == 2 && c.bytes().all(|b| b.is_ascii_alphabetic()));
        self.ultrawide_custom_zoom = if self.ultrawide_custom_zoom.is_finite() {
            self.ultrawide_custom_zoom.clamp(0.0, 0.5)
        } else {
            0.0
        };
        self.download_max_concurrent = self.download_max_concurrent.clamp(1, 4);
        self.per_channel_notifications = self
            .per_channel_notifications
            .into_iter()
            .filter_map(|(k, v)| {
                let trimmed = k.trim();
                (!trimmed.is_empty() && trimmed.len() <= 128).then(|| (trimmed.to_owned(), v))
            })
            .take(500)
            .collect();
        let mut seen_topics = std::collections::HashSet::new();
        self.muted_topics = self
            .muted_topics
            .into_iter()
            .map(|topic| topic.trim().to_lowercase())
            .filter(|topic| !topic.is_empty() && seen_topics.insert(topic.clone()))
            .take(100)
            .collect();
        let mut seen_channels = std::collections::HashSet::new();
        self.muted_channels = self
            .muted_channels
            .into_iter()
            .map(|ch| ch.trim().to_lowercase())
            .filter(|ch| !ch.is_empty() && seen_channels.insert(ch.clone()))
            .take(100)
            .collect();
        let mut seen_pinned = std::collections::HashSet::new();
        self.pinned_topics = self
            .pinned_topics
            .into_iter()
            .filter_map(|topic| {
                let trimmed = topic.trim();
                if trimmed.is_empty() {
                    return None;
                }
                let key = trimmed.to_lowercase();
                seen_pinned.insert(key).then(|| trimmed.to_owned())
            })
            .take(24)
            .collect();
        self.cache_dir_override = self
            .cache_dir_override
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        self.download_dir_override = self
            .download_dir_override
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        self.skipped_app_update_version = self
            .skipped_app_update_version
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty() && s.len() <= 64);
        let mut normalized_speeds = std::collections::HashMap::new();
        let mut speed_entries: Vec<(String, f64)> = self.per_channel_speeds.into_iter().collect();
        speed_entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (channel, speed) in speed_entries {
            set_channel_playback_speed(&mut normalized_speeds, &channel, speed);
        }
        self.per_channel_speeds = normalized_speeds;
        let mut seen_chapter_keywords = std::collections::HashSet::new();
        self.auto_skip_chapter_keywords = self
            .auto_skip_chapter_keywords
            .into_iter()
            .filter_map(|kw| {
                let trimmed = kw.trim();
                if trimmed.is_empty() || trimmed.len() > 64 {
                    return None;
                }
                let dedupe_key = trimmed.to_lowercase();
                seen_chapter_keywords
                    .insert(dedupe_key)
                    .then(|| trimmed.to_owned())
            })
            .take(MAX_AUTO_SKIP_CHAPTER_KEYWORDS)
            .collect();
        self
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.sponsorblock.valid() {
            return Err("configuração inválida do SponsorBlock");
        }
        if !self.keybindings.valid() {
            return Err("configuração de atalhos inválida");
        }
        if !self.caption_style.valid() {
            return Err("configuração de estilo de legendas inválida");
        }
        if let Some(code) = &self.country_code {
            let trimmed = code.trim();
            if trimmed.len() != 2 || !trimmed.bytes().all(|b| b.is_ascii_alphabetic()) {
                return Err("código de país inválido");
            }
        }
        if !self.ultrawide_custom_zoom.is_finite()
            || !(0.0..=0.5).contains(&self.ultrawide_custom_zoom)
        {
            return Err("zoom ultrawide personalizado deve estar entre 0.0 e 0.5");
        }
        if !(1..=4).contains(&self.download_max_concurrent) {
            return Err("downloads simultâneos deve estar entre 1 e 4");
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
        if self.muted_channels.len() > 100
            || self
                .muted_channels
                .iter()
                .any(|ch| ch.trim().is_empty() || ch.len() > 128)
        {
            return Err("lista de canais silenciados inválida");
        }
        if self.pinned_topics.len() > 24
            || self
                .pinned_topics
                .iter()
                .any(|topic| topic.trim().is_empty() || topic.len() > 64)
        {
            return Err("lista de tópicos fixados inválida");
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
        if let Some(dir) = &self.download_dir_override {
            let trimmed = dir.trim();
            if trimmed.is_empty() || trimmed.len() > 1024 {
                return Err("caminho de downloads inválido");
            }
            if !Path::new(trimmed).is_absolute() {
                return Err("o diretório de downloads personalizado deve ser um caminho absoluto");
            }
        }
        if self.per_channel_speeds.len() > MAX_PER_CHANNEL_SPEEDS
            || self.per_channel_speeds.iter().any(|(channel, speed)| {
                let trimmed = channel.trim();
                trimmed.is_empty()
                    || trimmed.len() > 128
                    || !speed.is_finite()
                    || !(0.25..=4.0).contains(speed)
            })
        {
            return Err("mapa de velocidades por canal inválido");
        }
        if self.auto_skip_chapter_keywords.len() > MAX_AUTO_SKIP_CHAPTER_KEYWORDS
            || self
                .auto_skip_chapter_keywords
                .iter()
                .any(|kw| kw.trim().is_empty() || kw.trim().len() > 64)
        {
            return Err("lista de palavras-chave de capítulos inválida");
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
    let bytes = serde_json::to_vec_pretty(&normalized)?;
    crate::history::write_atomic(path, &bytes)
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

#[allow(dead_code)]
mod com_dialog {
    #[repr(C)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct Guid {
        pub data1: u32,
        pub data2: u16,
        pub data3: u16,
        pub data4: [u8; 8],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub(super) struct ComDlgFilterSpec {
        pub psz_name: *const u16,
        pub psz_spec: *const u16,
    }

    // {DC1C5A9C-E88A-4DDE-A5A1-60F82A20AEF7}
    pub(super) const CLSID_FILE_OPEN_DIALOG: Guid = Guid {
        data1: 0xDC1C_5A9C,
        data2: 0xE88A,
        data3: 0x4DDE,
        data4: [0xA5, 0xA1, 0x60, 0xF8, 0x2A, 0x20, 0xAE, 0xF7],
    };

    // {D57C7288-D4AD-4768-BE02-9D969532D960}
    pub(super) const IID_IFILE_OPEN_DIALOG: Guid = Guid {
        data1: 0xD57C_7288,
        data2: 0xD4AD,
        data3: 0x4768,
        data4: [0xBE, 0x02, 0x9D, 0x96, 0x95, 0x32, 0xD9, 0x60],
    };

    // {C0B4E2F3-BA21-4773-8DBA-335EC946EB8B}
    pub(super) const CLSID_FILE_SAVE_DIALOG: Guid = Guid {
        data1: 0xC0B4_E2F3,
        data2: 0xBA21,
        data3: 0x4773,
        data4: [0x8D, 0xBA, 0x33, 0x5E, 0xC9, 0x46, 0xEB, 0x8B],
    };

    // {84BCCD23-5FDE-4CDB-AEA4-AF64B83D78AB}
    pub(super) const IID_IFILE_SAVE_DIALOG: Guid = Guid {
        data1: 0x84BC_CD23,
        data2: 0x5FDE,
        data3: 0x4CDB,
        data4: [0xAE, 0xA4, 0xAF, 0x64, 0xB8, 0x3D, 0x78, 0xAB],
    };

    // {43826D1E-E718-42EE-BC55-A1E261C37BFE}
    pub(super) const IID_ISHELL_ITEM: Guid = Guid {
        data1: 0x4382_6D1E,
        data2: 0xE718,
        data3: 0x42EE,
        data4: [0xBC, 0x55, 0xA1, 0xE2, 0x61, 0xC3, 0x7B, 0xFE],
    };

    pub(super) fn encode_wide_null(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub(super) struct OwnedFilterSpecs {
        // Buffers must stay alive and unmoved while `specs` pointers are used.
        _buffers: Vec<(Vec<u16>, Vec<u16>)>,
        specs: Vec<ComDlgFilterSpec>,
    }

    impl OwnedFilterSpecs {
        pub(super) fn new(filters: &[(&str, &str)]) -> Self {
            let buffers: Vec<(Vec<u16>, Vec<u16>)> = filters
                .iter()
                .map(|(name, spec)| (encode_wide_null(name), encode_wide_null(spec)))
                .collect();
            let specs = buffers
                .iter()
                .map(|(name, spec)| ComDlgFilterSpec {
                    psz_name: name.as_ptr(),
                    psz_spec: spec.as_ptr(),
                })
                .collect();
            Self {
                _buffers: buffers,
                specs,
            }
        }

        pub(super) fn as_slice(&self) -> &[ComDlgFilterSpec] {
            &self.specs
        }
    }

    #[cfg(windows)]
    pub(super) mod win32 {
        use super::{
            CLSID_FILE_OPEN_DIALOG, CLSID_FILE_SAVE_DIALOG, ComDlgFilterSpec, Guid,
            IID_IFILE_OPEN_DIALOG, IID_IFILE_SAVE_DIALOG, IID_ISHELL_ITEM, OwnedFilterSpecs,
            encode_wide_null,
        };
        use std::ffi::{OsString, c_void};
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use std::path::{Path, PathBuf};
        use std::ptr;

        const COINIT_APARTMENTTHREADED: u32 = 0x2;
        const COINIT_DISABLE_OLE1DDE: u32 = 0x4;
        const CLSCTX_INPROC_SERVER: u32 = 0x1;

        const S_OK: i32 = 0;
        const S_FALSE: i32 = 1;
        const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;
        const HRESULT_ERROR_CANCELLED: i32 = 0x8007_04C7u32 as i32;

        pub(crate) const FOS_OVERWRITEPROMPT: u32 = 0x0000_0002;
        pub(crate) const FOS_NOCHANGEDIR: u32 = 0x0000_0008;
        pub(crate) const FOS_PICKFOLDERS: u32 = 0x0000_0020;
        pub(crate) const FOS_FORCEFILESYSTEM: u32 = 0x0000_0040;
        pub(crate) const FOS_PATHMUSTEXIST: u32 = 0x0000_0800;
        pub(crate) const FOS_FILEMUSTEXIST: u32 = 0x0000_1000;

        const SIGDN_FILESYSPATH: u32 = 0x8005_8000;

        #[repr(C)]
        pub(super) struct IShellItem {
            vtbl: *const IShellItemVtbl,
        }

        #[repr(C)]
        #[allow(dead_code)]
        pub(crate) struct IShellItemVtbl {
            query_interface:
                unsafe extern "system" fn(*mut IShellItem, *const Guid, *mut *mut c_void) -> i32,
            add_ref: unsafe extern "system" fn(*mut IShellItem) -> u32,
            release: unsafe extern "system" fn(*mut IShellItem) -> u32,
            bind_to_handler: unsafe extern "system" fn(
                *mut IShellItem,
                *mut c_void,
                *const Guid,
                *const Guid,
                *mut *mut c_void,
            ) -> i32,
            get_parent: unsafe extern "system" fn(*mut IShellItem, *mut *mut IShellItem) -> i32,
            get_display_name: unsafe extern "system" fn(*mut IShellItem, u32, *mut *mut u16) -> i32,
            get_attributes: unsafe extern "system" fn(*mut IShellItem, u32, *mut u32) -> i32,
            compare:
                unsafe extern "system" fn(*mut IShellItem, *mut IShellItem, u32, *mut i32) -> i32,
        }

        #[repr(C)]
        pub(super) struct IFileDialog {
            vtbl: *const IFileDialogVtbl,
        }

        // Exact `IFileDialog` vtable prefix (slots 0..26) shared by both
        // `IFileOpenDialog` and `IFileSaveDialog`.
        #[repr(C)]
        #[allow(dead_code)]
        pub(crate) struct IFileDialogVtbl {
            query_interface:
                unsafe extern "system" fn(*mut IFileDialog, *const Guid, *mut *mut c_void) -> i32,
            add_ref: unsafe extern "system" fn(*mut IFileDialog) -> u32,
            release: unsafe extern "system" fn(*mut IFileDialog) -> u32,
            show: unsafe extern "system" fn(*mut IFileDialog, *mut c_void) -> i32,
            set_file_types:
                unsafe extern "system" fn(*mut IFileDialog, u32, *const ComDlgFilterSpec) -> i32,
            set_file_type_index: unsafe extern "system" fn(*mut IFileDialog, u32) -> i32,
            get_file_type_index: unsafe extern "system" fn(*mut IFileDialog, *mut u32) -> i32,
            advise: unsafe extern "system" fn(*mut IFileDialog, *mut c_void, *mut u32) -> i32,
            unadvise: unsafe extern "system" fn(*mut IFileDialog, u32) -> i32,
            set_options: unsafe extern "system" fn(*mut IFileDialog, u32) -> i32,
            get_options: unsafe extern "system" fn(*mut IFileDialog, *mut u32) -> i32,
            set_default_folder: unsafe extern "system" fn(*mut IFileDialog, *mut IShellItem) -> i32,
            set_folder: unsafe extern "system" fn(*mut IFileDialog, *mut IShellItem) -> i32,
            get_folder: unsafe extern "system" fn(*mut IFileDialog, *mut *mut IShellItem) -> i32,
            get_current_selection:
                unsafe extern "system" fn(*mut IFileDialog, *mut *mut IShellItem) -> i32,
            set_file_name: unsafe extern "system" fn(*mut IFileDialog, *const u16) -> i32,
            get_file_name: unsafe extern "system" fn(*mut IFileDialog, *mut *mut u16) -> i32,
            set_title: unsafe extern "system" fn(*mut IFileDialog, *const u16) -> i32,
            set_ok_button_label: unsafe extern "system" fn(*mut IFileDialog, *const u16) -> i32,
            set_file_name_label: unsafe extern "system" fn(*mut IFileDialog, *const u16) -> i32,
            get_result: unsafe extern "system" fn(*mut IFileDialog, *mut *mut IShellItem) -> i32,
            add_place: unsafe extern "system" fn(*mut IFileDialog, *mut IShellItem, u32) -> i32,
            set_default_extension: unsafe extern "system" fn(*mut IFileDialog, *const u16) -> i32,
            close: unsafe extern "system" fn(*mut IFileDialog, i32) -> i32,
            set_client_guid: unsafe extern "system" fn(*mut IFileDialog, *const Guid) -> i32,
            clear_client_data: unsafe extern "system" fn(*mut IFileDialog) -> i32,
            set_filter: unsafe extern "system" fn(*mut IFileDialog, *mut c_void) -> i32,
        }

        #[link(name = "ole32")]
        unsafe extern "system" {
            fn CoInitializeEx(pv_reserved: *const c_void, dw_co_init: u32) -> i32;
            fn CoUninitialize();
            fn CoCreateInstance(
                rclsid: *const Guid,
                p_unk_outer: *mut c_void,
                dw_cls_context: u32,
                riid: *const Guid,
                ppv: *mut *mut c_void,
            ) -> i32;
            fn CoTaskMemFree(pv: *mut c_void);
        }

        #[link(name = "shell32")]
        unsafe extern "system" {
            fn SHCreateItemFromParsingName(
                psz_path: *const u16,
                pbc: *mut c_void,
                riid: *const Guid,
                ppv: *mut *mut c_void,
            ) -> i32;
        }

        struct ComApartmentGuard {
            should_uninit: bool,
        }

        impl ComApartmentGuard {
            fn init_sta() -> Result<Self, String> {
                let hr = unsafe {
                    CoInitializeEx(
                        ptr::null(),
                        COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE,
                    )
                };
                if hr == S_OK || hr == S_FALSE {
                    Ok(Self {
                        should_uninit: true,
                    })
                } else if hr == RPC_E_CHANGED_MODE {
                    Ok(Self {
                        should_uninit: false,
                    })
                } else {
                    Err(format!(
                        "Falha ao inicializar COM para o seletor nativo (0x{:08X}).",
                        hr as u32
                    ))
                }
            }
        }

        impl Drop for ComApartmentGuard {
            fn drop(&mut self) {
                if self.should_uninit {
                    unsafe {
                        CoUninitialize();
                    }
                }
            }
        }

        struct FileDialogGuard(*mut IFileDialog);

        impl Drop for FileDialogGuard {
            fn drop(&mut self) {
                if !self.0.is_null() {
                    unsafe {
                        ((*(*self.0).vtbl).release)(self.0);
                    }
                    self.0 = ptr::null_mut();
                }
            }
        }

        struct ShellItemGuard(*mut IShellItem);

        impl Drop for ShellItemGuard {
            fn drop(&mut self) {
                if !self.0.is_null() {
                    unsafe {
                        ((*(*self.0).vtbl).release)(self.0);
                    }
                    self.0 = ptr::null_mut();
                }
            }
        }

        struct CoTaskMemWideGuard(*mut u16);

        impl Drop for CoTaskMemWideGuard {
            fn drop(&mut self) {
                if !self.0.is_null() {
                    unsafe {
                        CoTaskMemFree(self.0.cast::<c_void>());
                    }
                    self.0 = ptr::null_mut();
                }
            }
        }

        fn encode_path_wide_null(path: &Path) -> Vec<u16> {
            path.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        }

        pub(crate) struct NativeDialogConfig<'a> {
            pub save: bool,
            pub title: &'a str,
            pub options: u32,
            pub filters: &'a [(&'a str, &'a str)],
            pub default_ext: Option<&'a str>,
            pub initial_dir: Option<&'a Path>,
            pub initial_name: Option<&'a str>,
        }

        pub(crate) fn show_native_file_dialog(
            config: &NativeDialogConfig<'_>,
        ) -> Result<Option<PathBuf>, String> {
            let _com = ComApartmentGuard::init_sta()?;

            let (clsid, iid) = if config.save {
                (&CLSID_FILE_SAVE_DIALOG, &IID_IFILE_SAVE_DIALOG)
            } else {
                (&CLSID_FILE_OPEN_DIALOG, &IID_IFILE_OPEN_DIALOG)
            };

            let mut raw_dialog: *mut c_void = ptr::null_mut();
            let hr = unsafe {
                CoCreateInstance(
                    clsid,
                    ptr::null_mut(),
                    CLSCTX_INPROC_SERVER,
                    iid,
                    &mut raw_dialog,
                )
            };
            if hr < 0 || raw_dialog.is_null() {
                return Err(format!(
                    "Não foi possível abrir o seletor nativo do Windows (0x{:08X}).",
                    hr as u32
                ));
            }
            let dialog = FileDialogGuard(raw_dialog.cast::<IFileDialog>());
            let vtbl = unsafe { &*(*dialog.0).vtbl };

            let mut current_options = 0u32;
            let hr_get = unsafe { (vtbl.get_options)(dialog.0, &mut current_options) };
            if hr_get >= 0 {
                let _ = unsafe { (vtbl.set_options)(dialog.0, current_options | config.options) };
            } else {
                let _ = unsafe { (vtbl.set_options)(dialog.0, config.options) };
            }

            if !config.title.is_empty() {
                let title_w = encode_wide_null(config.title);
                let _ = unsafe { (vtbl.set_title)(dialog.0, title_w.as_ptr()) };
            }

            let _owned_filters = if !config.filters.is_empty() {
                let specs = OwnedFilterSpecs::new(config.filters);
                let slice = specs.as_slice();
                let _ =
                    unsafe { (vtbl.set_file_types)(dialog.0, slice.len() as u32, slice.as_ptr()) };
                Some(specs)
            } else {
                None
            };

            if let Some(ext) = config.default_ext.filter(|e| !e.is_empty()) {
                let ext_w = encode_wide_null(ext);
                let _ = unsafe { (vtbl.set_default_extension)(dialog.0, ext_w.as_ptr()) };
            }

            if let Some(dir) = config.initial_dir.filter(|p| p.is_dir()) {
                let dir_w = encode_path_wide_null(dir);
                let mut raw_item: *mut c_void = ptr::null_mut();
                let hr_item = unsafe {
                    SHCreateItemFromParsingName(
                        dir_w.as_ptr(),
                        ptr::null_mut(),
                        &IID_ISHELL_ITEM,
                        &mut raw_item,
                    )
                };
                if hr_item >= 0 && !raw_item.is_null() {
                    let folder_item = ShellItemGuard(raw_item.cast::<IShellItem>());
                    let _ = unsafe { (vtbl.set_folder)(dialog.0, folder_item.0) };
                }
            }

            if let Some(name) = config.initial_name.filter(|n| !n.is_empty()) {
                let name_w = encode_wide_null(name);
                let _ = unsafe { (vtbl.set_file_name)(dialog.0, name_w.as_ptr()) };
            }

            let hr_show = unsafe { (vtbl.show)(dialog.0, ptr::null_mut()) };
            if hr_show == HRESULT_ERROR_CANCELLED {
                return Ok(None);
            }
            if hr_show < 0 {
                return Err(format!(
                    "Seletor nativo do Windows falhou (0x{:08X}).",
                    hr_show as u32
                ));
            }

            let mut raw_result: *mut IShellItem = ptr::null_mut();
            let hr_res = unsafe { (vtbl.get_result)(dialog.0, &mut raw_result) };
            if hr_res < 0 || raw_result.is_null() {
                return Err(format!(
                    "Não foi possível obter o caminho selecionado (0x{:08X}).",
                    hr_res as u32
                ));
            }
            let result_item = ShellItemGuard(raw_result);
            let item_vtbl = unsafe { &*(*result_item.0).vtbl };

            let mut raw_path: *mut u16 = ptr::null_mut();
            let hr_name = unsafe {
                (item_vtbl.get_display_name)(result_item.0, SIGDN_FILESYSPATH, &mut raw_path)
            };
            if hr_name < 0 || raw_path.is_null() {
                return Err(format!(
                    "Não foi possível ler o caminho selecionado (0x{:08X}).",
                    hr_name as u32
                ));
            }
            let path_guard = CoTaskMemWideGuard(raw_path);
            let mut len = 0usize;
            unsafe {
                while *path_guard.0.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(path_guard.0, len);
                Ok(Some(PathBuf::from(OsString::from_wide(slice))))
            }
        }
    }
}

#[cfg(windows)]
pub fn pick_folder_dialog(initial_dir: Option<&Path>) -> Result<Option<PathBuf>, String> {
    use com_dialog::win32::{
        FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
        NativeDialogConfig, show_native_file_dialog,
    };

    show_native_file_dialog(&NativeDialogConfig {
        save: false,
        title: "Selecionar pasta",
        options: FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR,
        filters: &[],
        default_ext: None,
        initial_dir,
        initial_name: None,
    })
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
    use com_dialog::win32::{
        FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_OVERWRITEPROMPT,
        FOS_PATHMUSTEXIST, NativeDialogConfig, show_native_file_dialog,
    };

    let initial_dir = initial_path.and_then(|path| {
        if path.is_dir() {
            Some(path)
        } else {
            path.parent().filter(|p| p.is_dir())
        }
    });
    let initial_name = initial_path.and_then(|path| {
        (!path.is_dir())
            .then(|| path.file_name().and_then(|n| n.to_str()))
            .flatten()
    });
    let options = if save {
        FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR
    } else {
        FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR
    };
    show_native_file_dialog(&NativeDialogConfig {
        save,
        title,
        options,
        filters: &[
            ("Arquivos JSON (*.json)", "*.json"),
            ("Todos os arquivos (*.*)", "*.*"),
        ],
        default_ext: save.then_some("json"),
        initial_dir,
        initial_name,
    })
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
pub fn pick_text_export_dialog(title: &str, default_name: &str) -> Result<Option<PathBuf>, String> {
    use com_dialog::win32::{
        FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST,
        NativeDialogConfig, show_native_file_dialog,
    };

    show_native_file_dialog(&NativeDialogConfig {
        save: true,
        title,
        options: FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR,
        filters: &[
            ("Arquivo de Texto (*.txt)", "*.txt"),
            ("Markdown (*.md)", "*.md"),
            ("Legenda SubRip (*.srt)", "*.srt"),
            ("Todos os arquivos (*.*)", "*.*"),
        ],
        default_ext: Some("txt"),
        initial_dir: None,
        initial_name: Some(default_name),
    })
}

#[cfg(not(windows))]
pub fn pick_text_export_dialog(
    _title: &str,
    _default_name: &str,
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
    crate::history::write_atomic(dest, &bytes)
        .map_err(|error| format!("Falha ao salvar oauth-client.json: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_ID: AtomicU64 = AtomicU64::new(0);

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
        for theme in [Theme::Light, Theme::Dark, Theme::System] {
            save_theme(&path, theme).unwrap();
            assert_eq!(load_theme(&path).unwrap(), theme);
        }
        assert_eq!(
            save_theme(Path::new(""), Theme::Light).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        for content in ["2 dark", "1 unknown", "1 dark extra", "1", ""] {
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
        assert!(legacy.muted_channels.is_empty());
        assert!(legacy.pinned_topics.is_empty());

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
            muted_channels: vec!["  Canal X ".into(), "canal x".into(), "Canal Y".into()],
            pinned_topics: vec!["  Rust ".into(), "rust".into(), "Música".into()],
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
        assert_eq!(loaded.muted_channels, vec!["canal x", "canal y"]);
        assert_eq!(loaded.pinned_topics, vec!["Rust", "Música"]);

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

    #[test]
    fn video_codec_preference_serde_round_trip_and_legacy_default() {
        let dir = test_path().parent().unwrap().to_path_buf();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app-preferences.json");

        // Legacy JSON missing `video_codec_preference` must default to Auto.
        fs::write(&path, br#"{"seek_step_secs":10}"#).unwrap();
        let legacy = load_app_preferences(&path).unwrap();
        assert_eq!(legacy.video_codec_preference, VideoCodecPreference::Auto);

        for codec in VideoCodecPreference::ALL {
            let prefs = AppPreferences {
                video_codec_preference: codec,
                ..AppPreferences::default()
            };
            save_app_preferences(&path, &prefs).unwrap();
            let loaded = load_app_preferences(&path).unwrap();
            assert_eq!(loaded.video_codec_preference, codec);
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn com_dialog_guids_and_filter_specs_builder() {
        assert_eq!(std::mem::size_of::<com_dialog::Guid>(), 16);
        assert_eq!(com_dialog::CLSID_FILE_OPEN_DIALOG.data1, 0xDC1C_5A9C);
        assert_eq!(com_dialog::IID_IFILE_OPEN_DIALOG.data1, 0xD57C_7288);
        assert_eq!(com_dialog::CLSID_FILE_SAVE_DIALOG.data1, 0xC0B4_E2F3);
        assert_eq!(com_dialog::IID_IFILE_SAVE_DIALOG.data1, 0x84BC_CD23);
        assert_eq!(com_dialog::IID_ISHELL_ITEM.data1, 0x4382_6D1E);

        let filters =
            com_dialog::OwnedFilterSpecs::new(&[("JSON (*.json)", "*.json"), ("All (*.*)", "*.*")]);
        let slice = filters.as_slice();
        assert_eq!(slice.len(), 2);
        unsafe {
            assert_eq!(*slice[0].psz_name, 'J' as u16);
            assert_eq!(*slice[0].psz_spec, '*' as u16);
            assert_eq!(*slice[0].psz_spec.add(6), 0);
            assert_eq!(*slice[1].psz_name, 'A' as u16);
            assert_eq!(*slice[1].psz_spec.add(3), 0);
        }

        #[cfg(windows)]
        {
            assert_eq!(
                std::mem::size_of::<com_dialog::win32::IShellItemVtbl>(),
                8 * std::mem::size_of::<usize>()
            );
            assert_eq!(
                std::mem::size_of::<com_dialog::win32::IFileDialogVtbl>(),
                27 * std::mem::size_of::<usize>()
            );
        }
    }

    #[test]
    fn keybindings_detect_conflicts_and_resolve_duplicates() {
        let mut kb = Keybindings::default();
        assert!(
            kb.detect_conflicts().is_empty(),
            "default keybindings must be conflict-free"
        );
        assert_eq!(
            Keybindings::parse_egui_key("+"),
            Some((false, false, false, eframe::egui::Key::Plus))
        );
        assert_eq!(
            Keybindings::parse_egui_key("Ctrl++"),
            Some((true, false, false, eframe::egui::Key::Plus))
        );
        assert_eq!(
            Keybindings::parse_egui_key("F11"),
            Some((false, false, false, eframe::egui::Key::F11))
        );
        assert_eq!(
            Keybindings::parse_egui_key("Ctrl+ArrowLeft"),
            Some((true, false, false, eframe::egui::Key::ArrowLeft))
        );
        assert_eq!(
            Keybindings::parse_egui_key("?"),
            Some((false, false, false, eframe::egui::Key::Questionmark))
        );
        // Unknown modifiers must be rejected rather than silently stripped.
        assert_eq!(Keybindings::parse_egui_key("Meta+K"), None);

        kb.fullscreen = "m".to_owned();
        let conflicts = kb.detect_conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].action_a, "mute");
        assert_eq!(conflicts[0].action_b, "fullscreen");

        assert!(kb.resolve_conflict("mute", "fullscreen"));
        assert!(kb.detect_conflicts().is_empty());
        assert_eq!(kb.fullscreen, "F");
        assert_eq!(kb.mute, "M");
    }

    #[test]
    fn wave3_app_preferences_serde_round_trip_and_legacy_defaults() {
        let dir = test_path().parent().unwrap().to_path_buf();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app-preferences.json");

        // Legacy JSON missing all Wave 3 fields must deserialize with defaults.
        fs::write(
            &path,
            br#"{"seek_step_secs":10,"keybindings":{"mute":"M"}}"#,
        )
        .unwrap();
        let legacy = load_app_preferences(&path).unwrap();
        assert_eq!(legacy.country_code, None);
        assert!(!legacy.restricted_mode);
        assert!(!legacy.ambient_mode);
        assert!(!legacy.show_remaining_time);
        assert_eq!(legacy.caption_style, CaptionStylePrefs::default());
        assert_eq!(legacy.ultrawide_zoom_mode, UltrawideZoomMode::Fit);
        assert_eq!(legacy.ultrawide_custom_zoom, 0.0);
        assert!(legacy.per_channel_notifications.is_empty());
        assert_eq!(legacy.download_max_concurrent, 2);
        assert!(!legacy.download_embed_subs);
        assert!(!legacy.download_embed_chapters);
        assert!(!legacy.download_embed_thumbnail);
        assert_eq!(legacy.keybindings.fullscreen_alt, "F11");
        assert_eq!(legacy.keybindings.volume_up, "ArrowUp");
        assert_eq!(legacy.keybindings.prev_chapter, "Ctrl+ArrowLeft");
        assert!(legacy.keybindings.detect_conflicts().is_empty());

        let mut per_ch = std::collections::HashMap::new();
        per_ch.insert("UC123".to_owned(), ChannelNotifyLevel::All);
        per_ch.insert("UC456".to_owned(), ChannelNotifyLevel::None);
        let custom = AppPreferences {
            country_code: Some(" br ".to_owned()),
            restricted_mode: true,
            ambient_mode: true,
            show_remaining_time: true,
            caption_style: CaptionStylePrefs {
                font_scale: 1.4,
                bg_opacity: 0.5,
                text_color_hex: "#fff176".to_owned(),
                translate_lang: Some("pt-BR".to_owned()),
            },
            ultrawide_zoom_mode: UltrawideZoomMode::Custom,
            ultrawide_custom_zoom: 0.25,
            per_channel_notifications: per_ch,
            download_max_concurrent: 4,
            download_embed_subs: true,
            download_embed_chapters: true,
            download_embed_thumbnail: true,
            ..AppPreferences::default()
        };
        save_app_preferences(&path, &custom).unwrap();
        let loaded = load_app_preferences(&path).unwrap();
        assert_eq!(loaded.country_code.as_deref(), Some("BR"));
        assert!(loaded.restricted_mode);
        assert!(loaded.ambient_mode);
        assert!(loaded.show_remaining_time);
        assert!((loaded.caption_style.font_scale - 1.4).abs() < 1e-5);
        assert!((loaded.caption_style.bg_opacity - 0.5).abs() < 1e-5);
        assert_eq!(loaded.caption_style.text_color_hex, "#FFF176");
        assert_eq!(
            loaded.caption_style.translate_lang.as_deref(),
            Some("pt-BR")
        );
        assert_eq!(loaded.ultrawide_zoom_mode, UltrawideZoomMode::Custom);
        assert!((loaded.ultrawide_custom_zoom - 0.25).abs() < 1e-5);
        assert_eq!(
            loaded.per_channel_notifications.get("UC123"),
            Some(&ChannelNotifyLevel::All)
        );
        assert_eq!(
            loaded.per_channel_notifications.get("UC456"),
            Some(&ChannelNotifyLevel::None)
        );
        assert_eq!(loaded.download_max_concurrent, 4);
        assert!(loaded.download_embed_subs);
        assert!(loaded.download_embed_chapters);
        assert!(loaded.download_embed_thumbnail);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn theme_system_resolves_to_dark_or_light_and_round_trips_serde() {
        assert_eq!(Theme::default(), Theme::Dark);
        assert_eq!(Theme::Dark.resolve(false), ResolvedTheme::Dark);
        assert_eq!(Theme::Dark.resolve(true), ResolvedTheme::Dark);
        assert_eq!(Theme::Light.resolve(false), ResolvedTheme::Light);
        assert_eq!(Theme::Light.resolve(true), ResolvedTheme::Light);
        assert_eq!(Theme::System.resolve(true), ResolvedTheme::Dark);
        assert_eq!(Theme::System.resolve(false), ResolvedTheme::Light);

        assert!(ResolvedTheme::Dark.is_dark());
        assert!(!ResolvedTheme::Light.is_dark());
        assert_eq!(ResolvedTheme::Dark.to_concrete_theme(), Theme::Dark);
        assert_eq!(ResolvedTheme::Light.to_concrete_theme(), Theme::Light);

        for (theme, expected_str) in [
            (Theme::Dark, "dark"),
            (Theme::Light, "light"),
            (Theme::System, "system"),
        ] {
            assert_eq!(theme.as_str(), expected_str);
            assert_eq!(Theme::parse(expected_str), Some(theme));
            assert_eq!(
                Theme::parse(&format!("  {}  ", expected_str.to_uppercase())),
                Some(theme)
            );
            let serialized = serde_json::to_string(&theme).unwrap();
            assert_eq!(serialized, format!("\"{expected_str}\""));
            let deserialized: Theme = serde_json::from_str(&serialized).unwrap();
            assert_eq!(deserialized, theme);
        }
        assert_eq!(Theme::parse("sepia"), None);
    }

    #[test]
    fn per_channel_playback_speed_normalizes_clamps_clears_default_and_caps() {
        assert_eq!(
            normalize_channel_speed_key("  @FastTube_Dev  "),
            "@fasttube_dev"
        );

        let mut map = std::collections::HashMap::new();
        set_channel_playback_speed(&mut map, "  ChannelOne ", 1.5);
        assert_eq!(get_channel_playback_speed(&map, "channelone"), Some(1.5));

        // Clamping out-of-range speeds.
        set_channel_playback_speed(&mut map, "ChannelLow", 0.05);
        assert_eq!(get_channel_playback_speed(&map, "channellow"), Some(0.25));
        set_channel_playback_speed(&mut map, "ChannelHigh", 9.0);
        assert_eq!(get_channel_playback_speed(&map, "channelhigh"), Some(4.0));

        // Non-finite and blank inputs are ignored.
        set_channel_playback_speed(&mut map, "   ", 1.5);
        set_channel_playback_speed(&mut map, "ChannelNaN", f64::NAN);
        assert_eq!(get_channel_playback_speed(&map, "channelnan"), None);

        // Speed within 0.01 of 1.0 clears the entry.
        set_channel_playback_speed(&mut map, "CHANNELONE", 1.005);
        assert_eq!(get_channel_playback_speed(&map, "channelone"), None);
        set_channel_playback_speed(&mut map, "ChannelLow", 0.995);
        assert_eq!(get_channel_playback_speed(&map, "channellow"), None);

        // Cap enforcement at MAX_PER_CHANNEL_SPEEDS.
        map.clear();
        for idx in 0..(MAX_PER_CHANNEL_SPEEDS + 25) {
            set_channel_playback_speed(&mut map, &format!("channel_{idx:04}"), 1.25);
        }
        assert_eq!(map.len(), MAX_PER_CHANNEL_SPEEDS);
        assert_eq!(
            get_channel_playback_speed(
                &map,
                &format!("channel_{:04}", MAX_PER_CHANNEL_SPEEDS + 24)
            ),
            Some(1.25)
        );
    }

    #[test]
    fn should_auto_skip_chapter_matches_keywords_case_insensitively_and_ignores_blanks() {
        let keywords = vec![
            "".to_owned(),
            "   ".to_owned(),
            "Sponsor".to_owned(),
            "  Anúncio  ".to_owned(),
            "outro".to_owned(),
        ];
        assert!(should_auto_skip_chapter(
            "Today's SPONSOR message",
            &keywords
        ));
        assert!(should_auto_skip_chapter("ANÚNCIO especial", &keywords));
        assert!(should_auto_skip_chapter("Final Outro & Thanks", &keywords));
        assert!(!should_auto_skip_chapter("Main Benchmark", &keywords));
        assert!(!should_auto_skip_chapter("   ", &keywords));
        assert!(!should_auto_skip_chapter("Sponsor", &[]));
        assert!(!should_auto_skip_chapter(
            "Any Chapter",
            &["".to_owned(), "   ".to_owned()]
        ));
    }

    #[test]
    fn wave4_app_preferences_serde_round_trip_and_legacy_defaults() {
        let dir = test_path().parent().unwrap().to_path_buf();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app-preferences.json");

        // Legacy JSON missing all Wave 4 fields must deserialize with Wave 4 defaults.
        fs::write(&path, br#"{"seek_step_secs":10}"#).unwrap();
        let legacy = load_app_preferences(&path).unwrap();
        assert!(!legacy.mica_backdrop);
        assert!(legacy.windows_toast_notifications);
        assert!(legacy.remember_speed_per_channel);
        assert!(legacy.per_channel_speeds.is_empty());
        assert!(!legacy.auto_skip_chapters_enabled);
        assert!(legacy.auto_skip_chapter_keywords.is_empty());

        let mut speeds = std::collections::HashMap::new();
        speeds.insert("  TechChannel  ".to_owned(), 1.75);
        speeds.insert("DefaultSpeedChannel".to_owned(), 1.0);
        speeds.insert("FastChannel".to_owned(), 5.0);
        let custom = AppPreferences {
            mica_backdrop: true,
            windows_toast_notifications: false,
            remember_speed_per_channel: false,
            per_channel_speeds: speeds,
            auto_skip_chapters_enabled: true,
            auto_skip_chapter_keywords: vec![
                "  Sponsor ".into(),
                "sponsor".into(),
                "  ".into(),
                "Intro".into(),
                "Patrocinador".into(),
            ],
            ..AppPreferences::default()
        };
        save_app_preferences(&path, &custom).unwrap();
        let loaded = load_app_preferences(&path).unwrap();
        assert!(loaded.mica_backdrop);
        assert!(!loaded.windows_toast_notifications);
        assert!(!loaded.remember_speed_per_channel);
        assert_eq!(
            get_channel_playback_speed(&loaded.per_channel_speeds, "techchannel"),
            Some(1.75)
        );
        assert_eq!(
            get_channel_playback_speed(&loaded.per_channel_speeds, "defaultspeedchannel"),
            None
        );
        assert_eq!(
            get_channel_playback_speed(&loaded.per_channel_speeds, "fastchannel"),
            Some(4.0)
        );
        assert!(loaded.auto_skip_chapters_enabled);
        assert_eq!(
            loaded.auto_skip_chapter_keywords,
            vec!["Sponsor", "Intro", "Patrocinador"]
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
