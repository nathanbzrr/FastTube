use crate::i18n::UiLanguage;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    Login,
    Quota,
    Connection,
    Playback,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FailureGuidance {
    pub summary: &'static str,
    pub steps: [&'static str; 3],
}

#[allow(dead_code)]
pub fn guidance_for_lang(
    kind: FailureKind,
    authenticated: bool,
    player_active: bool,
    lang: UiLanguage,
) -> FailureGuidance {
    match kind {
        FailureKind::Login => FailureGuidance {
            summary: if authenticated {
                lang.pick(
                    "A sessão existe, mas o acesso pode ter expirado ou não incluir a permissão necessária.",
                    "A session exists, but access may have expired or lack the required permission.",
                )
            } else {
                lang.pick(
                    "Conecte a conta Google para usar inscrições, playlists e comentários.",
                    "Connect your Google account to use subscriptions, playlists, and comments.",
                )
            },
            steps: [
                lang.pick(
                    "Confirme o oauth-client.json na pasta de dados do FastTube.",
                    "Verify oauth-client.json is in the FastTube data folder.",
                ),
                lang.pick(
                    "Entre novamente e aceite as permissões solicitadas.",
                    "Sign in again and grant the requested permissions.",
                ),
                lang.pick(
                    "Teste uma consulta pública antes de repetir a ação privada.",
                    "Test a public request before retrying the private action.",
                ),
            ],
        },
        FailureKind::Quota => FailureGuidance {
            summary: lang.pick(
                "A cota da YouTube Data API foi atingida; a Home pública e os dados locais continuam disponíveis.",
                "The YouTube Data API quota has been reached; the public Home feed and local data remain available.",
            ),
            steps: [
                lang.pick(
                    "Aguarde a renovação diária da cota do projeto Google.",
                    "Wait for your Google project's daily quota reset.",
                ),
                lang.pick(
                    "Evite repetir buscas de inscrições e playlists em sequência.",
                    "Avoid repeating subscription and playlist requests in quick succession.",
                ),
                lang.pick(
                    "Use a Home, a biblioteca e a fila até a cota voltar.",
                    "Use Home, your library, and the queue until the quota resets.",
                ),
            ],
        },
        FailureKind::Connection => FailureGuidance {
            summary: lang.pick(
                "O FastTube não conseguiu alcançar o serviço remoto.",
                "FastTube could not reach the remote service.",
            ),
            steps: [
                lang.pick(
                    "Confirme que outros sites abrem no mesmo computador.",
                    "Verify that other websites open on this computer.",
                ),
                lang.pick(
                    "Desative temporariamente VPN ou proxy e tente novamente.",
                    "Temporarily disable any VPN or proxy and try again.",
                ),
                lang.pick(
                    "Se o erro persistir, copie o relatório redigido para diagnóstico.",
                    "If the error persists, copy the redacted report for diagnostics.",
                ),
            ],
        },
        FailureKind::Playback => FailureGuidance {
            summary: if player_active {
                lang.pick(
                    "O player está aberto, mas a faixa ou uma dependência não carregou corretamente.",
                    "The player is open, but the stream or a dependency failed to load properly.",
                )
            } else {
                lang.pick(
                    "O player ainda não foi iniciado para esta reprodução.",
                    "The player has not been started for this playback yet.",
                )
            },
            steps: [
                lang.pick(
                    "Confirme que libmpv-2.dll está ao lado do executável.",
                    "Verify that libmpv-2.dll is next to the executable.",
                ),
                lang.pick(
                    "Confirme que yt-dlp está no PATH ou junto do FastTube.",
                    "Verify that yt-dlp is on PATH or alongside FastTube.",
                ),
                lang.pick(
                    "Reabra o vídeo; se falhar, teste outra qualidade ou formato.",
                    "Reopen the video; if it fails, try a different quality or format.",
                ),
            ],
        },
        FailureKind::Unknown => FailureGuidance {
            summary: lang.pick(
                "A mensagem não permitiu identificar uma causa específica com segurança.",
                "The message did not allow reliably identifying a specific cause.",
            ),
            steps: [
                lang.pick(
                    "Repita a ação uma vez para distinguir falha temporária.",
                    "Retry the action once to rule out a transient failure.",
                ),
                lang.pick(
                    "Copie o relatório redigido e confira categoria e estado do player.",
                    "Copy the redacted report and check the category and player state.",
                ),
                lang.pick(
                    "Reinicie o FastTube se o problema ocorrer em várias telas.",
                    "Restart FastTube if the issue happens across multiple views.",
                ),
            ],
        },
    }
}

#[allow(dead_code)]
pub fn guidance(kind: FailureKind, authenticated: bool, player_active: bool) -> FailureGuidance {
    guidance_for_lang(kind, authenticated, player_active, UiLanguage::PtBr)
}

fn looks_like_url_or_path(token: &str) -> bool {
    let trimmed = token.trim_matches(|c: char| "\"'`()[]{}<>,;:.".contains(c));
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains("://") || trimmed.starts_with("\\\\") {
        return true;
    }
    let b = trimmed.as_bytes();
    if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
    {
        return true;
    }
    trimmed.matches(['\\', '/']).count() >= 2
}

pub fn classify_failure(message: &str) -> FailureKind {
    let text = message
        .split_whitespace()
        .filter(|part| !looks_like_url_or_path(part))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let contains_any = |keywords: &[&str]| keywords.iter().copied().any(|s| text.contains(s));
    if contains_any(&[
        "token",
        "login",
        "sessão",
        "autoriz",
        "oauth",
        "permissão",
        "401",
        "403",
        "unauthorized",
        "unauthenticated",
        "forbidden",
        "credential",
        "credencial",
        "cookie",
        "dpapi",
        "sapisid",
        "expirad",
        "expired",
    ]) {
        FailureKind::Login
    } else if contains_any(&[
        "quota",
        "cota",
        "rate limit",
        "429",
        "too many requests",
        "resource_exhausted",
        "daily limit",
        "limite diário",
    ]) {
        FailureKind::Quota
    } else if contains_any(&[
        "conexão",
        "connect",
        "network",
        "rede",
        "timeout",
        "dns",
        "timed out",
        "eof",
        "tls",
        "ssl",
        "certificate",
        "certificado",
        "offline",
        "unreachable",
        "socket",
        "refused",
        "reset",
    ]) {
        FailureKind::Connection
    } else if contains_any(&[
        "libmpv",
        "yt-dlp",
        "player",
        "reprodução",
        "playback",
        "decoder",
        "mpv",
        "ffmpeg",
        "codec",
        "audio",
        "áudio",
        "format",
        "formato",
        "stream",
        "dll",
        "opengl",
        "hwnd",
    ]) {
        FailureKind::Playback
    } else {
        FailureKind::Unknown
    }
}

#[allow(dead_code)]
pub fn safe_report_for_lang(
    kind: FailureKind,
    authenticated: bool,
    player_active: bool,
    lang: UiLanguage,
) -> String {
    let guidance = guidance_for_lang(kind, authenticated, player_active, lang);
    let category = match kind {
        FailureKind::Login => lang.pick("login", "login"),
        FailureKind::Quota => lang.pick("cota da API", "API quota"),
        FailureKind::Connection => lang.pick("conexão", "connection"),
        FailureKind::Playback => lang.pick("reprodução/dependências", "playback/dependencies"),
        FailureKind::Unknown => lang.pick("não classificada", "unclassified"),
    };
    let header = lang.pick("FastTube diagnóstico", "FastTube diagnostics");
    let category_label = lang.pick("Categoria", "Category");
    let session_label = lang.pick("Sessão Google", "Google session");
    let session_status = if authenticated {
        lang.pick("conectada", "connected")
    } else {
        lang.pick("desconectada", "disconnected")
    };
    let player_label = lang.pick("Player ativo", "Player active");
    let player_status = if player_active {
        lang.pick("sim", "yes")
    } else {
        lang.pick("não", "no")
    };
    let steps_label = lang.pick("Próximos passos:", "Next steps:");
    let notice = lang.pick(
        "Dados sensíveis e URLs foram omitidos.",
        "Sensitive data and URLs have been omitted.",
    );
    format!(
        "{header}\n{category_label}: {category}\n{session_label}: {session_status}\n{player_label}: {player_status}\n{steps_label}\n- {}\n- {}\n- {}\n{notice}",
        guidance.steps[0], guidance.steps[1], guidance.steps[2],
    )
}

#[allow(dead_code)]
pub fn safe_report(kind: FailureKind, authenticated: bool, player_active: bool) -> String {
    safe_report_for_lang(kind, authenticated, player_active, UiLanguage::PtBr)
}

pub const PROP_HWDEC: &str = "hwdec";
pub const PROP_HWDEC_CURRENT: &str = "hwdec-current";
pub const PROP_VIDEO_CODEC: &str = "video-codec";
pub const PROP_WIDTH: &str = "width";
pub const PROP_HEIGHT: &str = "height";

const MAX_DIMENSION: u32 = 65_535;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoSize {
    pub width: u32,
    pub height: u32,
}

impl VideoSize {
    pub fn new(width: u32, height: u32) -> Option<Self> {
        if (1..=MAX_DIMENSION).contains(&width) && (1..=MAX_DIMENSION).contains(&height) {
            Some(Self { width, height })
        } else {
            None
        }
    }

    pub fn from_strings(width: Option<&str>, height: Option<&str>) -> Option<Self> {
        Self::new(parse_dim(width?)?, parse_dim(height?)?)
    }

    pub fn from_numbers(width: Option<f64>, height: Option<f64>) -> Option<Self> {
        Self::new(parse_f64_dim(width?)?, parse_f64_dim(height?)?)
    }
}

impl fmt::Display for VideoSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} × {}", self.width, self.height)
    }
}

/// Only explicit known `libmpv` hardware decoding backends count as active hardware decoding;
/// policy strings (`"auto"`, `"auto-safe"`, `"yes"`) and unknown values remain `Unknown`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EffectiveDecoding {
    HardwareActive(String),
    Software,
    Unknown(Option<String>),
}

impl EffectiveDecoding {
    pub fn is_hardware_active(&self) -> bool {
        matches!(self, Self::HardwareActive(_))
    }

    pub fn is_software(&self) -> bool {
        matches!(self, Self::Software)
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown(_))
    }

    pub fn active_backend(&self) -> Option<&str> {
        match self {
            Self::HardwareActive(backend) => Some(backend.as_str()),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn short_label_for_lang(&self, lang: UiLanguage) -> String {
        match self {
            Self::HardwareActive(backend) => {
                let prefix = lang.pick("hardware ativo", "active hardware");
                format!("{prefix} ({backend})")
            }
            Self::Software => "software (no)".to_owned(),
            Self::Unknown(Some(raw)) => {
                let prefix = lang.pick("desconhecido", "unknown");
                format!("{prefix} ({raw})")
            }
            Self::Unknown(None) => lang.pick("desconhecido", "unknown").to_owned(),
        }
    }

    #[allow(dead_code)]
    pub fn short_label(&self) -> String {
        self.short_label_for_lang(UiLanguage::PtBr)
    }
}

pub fn classify_hwdec_current(raw: Option<&str>) -> EffectiveDecoding {
    let Some(trimmed) = clean_non_empty(raw) else {
        return EffectiveDecoding::Unknown(None);
    };
    let lower = trimmed.to_ascii_lowercase();
    if lower == "no" {
        EffectiveDecoding::Software
    } else if is_known_hwdec_backend(&lower) {
        EffectiveDecoding::HardwareActive(lower)
    } else {
        EffectiveDecoding::Unknown(Some(trimmed.to_owned()))
    }
}

fn is_known_hwdec_backend(lower: &str) -> bool {
    matches!(
        lower,
        "d3d11va"
            | "d3d11va-copy"
            | "d3d12va"
            | "d3d12va-copy"
            | "dxva2"
            | "dxva2-copy"
            | "nvdec"
            | "nvdec-copy"
            | "amf"
            | "amf-copy"
            | "vulkan"
            | "vulkan-copy"
            | "qsv"
            | "qsv-copy"
            | "cuda"
            | "cuda-copy"
            | "vaapi"
            | "vaapi-copy"
            | "videotoolbox"
            | "videotoolbox-copy"
            | "mediacodec"
            | "mediacodec-copy"
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodingDiagnostics {
    pub configured_hwdec: Option<String>,
    pub effective_decoding: EffectiveDecoding,
    pub codec: Option<String>,
    pub size: Option<VideoSize>,
}

impl DecodingDiagnostics {
    pub fn from_raw(
        hwdec: Option<&str>,
        hwdec_current: Option<&str>,
        codec: Option<&str>,
        width: Option<&str>,
        height: Option<&str>,
    ) -> Self {
        Self {
            configured_hwdec: clean_field(hwdec),
            effective_decoding: classify_hwdec_current(hwdec_current),
            codec: clean_field(codec),
            size: VideoSize::from_strings(width, height),
        }
    }

    pub fn from_property_reader<F>(mut read_property: F) -> Self
    where
        F: FnMut(&str) -> Option<String>,
    {
        let hwdec = read_property(PROP_HWDEC);
        let hwdec_current = read_property(PROP_HWDEC_CURRENT);
        let codec = read_property(PROP_VIDEO_CODEC);
        let width = read_property(PROP_WIDTH);
        let height = read_property(PROP_HEIGHT);
        Self::from_raw(
            hwdec.as_deref(),
            hwdec_current.as_deref(),
            codec.as_deref(),
            width.as_deref(),
            height.as_deref(),
        )
    }

    pub fn unknown(configured_hwdec: Option<&str>) -> Self {
        Self {
            configured_hwdec: clean_field(configured_hwdec),
            effective_decoding: EffectiveDecoding::Unknown(None),
            codec: None,
            size: None,
        }
    }

    #[allow(dead_code)]
    pub fn configured_hwdec_label_for_lang(&self, lang: UiLanguage) -> &str {
        self.configured_hwdec
            .as_deref()
            .unwrap_or_else(|| lang.pick("desconhecido", "unknown"))
    }

    #[allow(dead_code)]
    pub fn configured_hwdec_label(&self) -> &str {
        self.configured_hwdec_label_for_lang(UiLanguage::PtBr)
    }

    #[allow(dead_code)]
    pub fn summary_line_for_lang(&self, lang: UiLanguage) -> String {
        let effective = self.effective_decoding.short_label_for_lang(lang);
        let configured = self.configured_hwdec_label_for_lang(lang);
        let mut parts = Vec::with_capacity(4);
        parts.push(format!("hwdec-current: {effective}"));
        parts.push(format!("hwdec: {configured}"));
        if let Some(codec) = &self.codec {
            parts.push(format!("codec: {codec}"));
        }
        if let Some(size) = &self.size {
            let size_label = lang.pick("tamanho", "size");
            parts.push(format!("{size_label}: {size}"));
        }
        parts.join(" • ")
    }

    #[allow(dead_code)]
    pub fn summary_line(&self) -> String {
        self.summary_line_for_lang(UiLanguage::PtBr)
    }
}

pub fn collect_from_property_reader<F>(read_property: F) -> DecodingDiagnostics
where
    F: FnMut(&str) -> Option<String>,
{
    DecodingDiagnostics::from_property_reader(read_property)
}

pub struct PlayerDebugInfoInput<'a> {
    pub app_version: &'a str,
    pub video_id: Option<&'a str>,
    pub video_size: Option<VideoSize>,
    pub viewport_size: Option<VideoSize>,
    pub fps: Option<f64>,
    pub dropped_frames: Option<u64>,
    pub video_codec: Option<&'a str>,
    pub audio_codec: Option<&'a str>,
    pub hwdec_configured: Option<&'a str>,
    pub hwdec_effective: &'a EffectiveDecoding,
    pub speed: f64,
    pub volume_percent: f32,
    pub muted: bool,
    pub buffer_profile: &'a str,
    pub buffer_ahead_secs: Option<f64>,
    pub is_live: bool,
    pub audio_only: bool,
    pub loop_video: bool,
    pub player_backend: &'a str,
    pub failure_category: Option<FailureKind>,
}

fn sanitize_video_id(raw: Option<&str>) -> Option<&str> {
    let trimmed = raw?.trim();
    if trimmed.is_empty()
        || trimmed.len() > 16
        || !trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    Some(trimmed)
}

fn clean_safe_metadata_token(raw: Option<&str>) -> Option<String> {
    let cleaned = clean_field(raw)?;
    if cleaned.len() > 64
        || cleaned.contains('?')
        || cleaned.contains('=')
        || cleaned.split_whitespace().any(looks_like_url_or_path)
    {
        return None;
    }
    Some(cleaned)
}

fn failure_kind_token(kind: FailureKind) -> &'static str {
    match kind {
        FailureKind::Login => "login",
        FailureKind::Quota => "quota",
        FailureKind::Connection => "connection",
        FailureKind::Playback => "playback",
        FailureKind::Unknown => "unknown",
    }
}

pub fn player_debug_info_json(input: &PlayerDebugInfoInput<'_>) -> String {
    let video_res = input
        .video_size
        .map(|s| format!("{}x{}", s.width, s.height));
    let viewport_res = input
        .viewport_size
        .map(|s| format!("{}x{}", s.width, s.height));
    let fps = input
        .fps
        .filter(|v| v.is_finite() && *v > 0.0)
        .map(|v| (v * 100.0).round() / 100.0);
    let speed = if input.speed.is_finite() && input.speed > 0.0 {
        (input.speed * 100.0).round() / 100.0
    } else {
        1.0
    };
    let volume = if input.volume_percent.is_finite() {
        input.volume_percent.clamp(0.0, 100.0).round() as u32
    } else {
        100
    };
    let buffer_ahead = input
        .buffer_ahead_secs
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| (v * 10.0).round() / 10.0);
    let hwdec_effective = match input.hwdec_effective {
        EffectiveDecoding::HardwareActive(backend) => clean_safe_metadata_token(Some(backend))
            .map(|b| format!("hardware:{b}"))
            .unwrap_or_else(|| "hardware".to_owned()),
        EffectiveDecoding::Software => "software".to_owned(),
        EffectiveDecoding::Unknown(raw) => {
            if let Some(tok) = clean_safe_metadata_token(raw.as_deref()) {
                format!("unknown:{tok}")
            } else {
                "unknown".to_owned()
            }
        }
    };

    let payload = serde_json::json!({
        "app": "FastTube",
        "version": clean_safe_metadata_token(Some(input.app_version)).unwrap_or_else(|| "unknown".to_owned()),
        "backend": clean_safe_metadata_token(Some(input.player_backend)).unwrap_or_else(|| "unknown".to_owned()),
        "video_id": sanitize_video_id(input.video_id),
        "video_resolution": video_res,
        "viewport_resolution": viewport_res,
        "fps": fps,
        "dropped_frames": input.dropped_frames,
        "video_codec": clean_safe_metadata_token(input.video_codec),
        "audio_codec": clean_safe_metadata_token(input.audio_codec),
        "hwdec_configured": clean_safe_metadata_token(input.hwdec_configured),
        "hwdec_effective": hwdec_effective,
        "playback_speed": speed,
        "volume_percent": volume,
        "muted": input.muted,
        "buffer_profile": clean_safe_metadata_token(Some(input.buffer_profile)).unwrap_or_else(|| "default".to_owned()),
        "buffer_ahead_secs": buffer_ahead,
        "is_live": input.is_live,
        "audio_only": input.audio_only,
        "loop_video": input.loop_video,
        "failure_category": input.failure_category.map(failure_kind_token),
    });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_owned())
}

fn clean_non_empty(raw: Option<&str>) -> Option<&str> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn clean_field(raw: Option<&str>) -> Option<String> {
    let trimmed = clean_non_empty(raw)?;
    if matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "unknown" | "unavailable" | "null" | "n/a" | "?"
    ) {
        return None;
    }
    Some(trimmed.to_owned())
}

fn parse_dim(raw: &str) -> Option<u32> {
    let trimmed = raw.trim();
    if let Ok(val) = trimmed.parse::<u32>() {
        return (1..=MAX_DIMENSION).contains(&val).then_some(val);
    }
    parse_f64_dim(trimmed.parse::<f64>().ok()?)
}

fn parse_f64_dim(val: f64) -> Option<u32> {
    if !val.is_finite() || val < 1.0 || val > MAX_DIMENSION as f64 {
        return None;
    }
    let rounded = val.round();
    if (val - rounded).abs() > 1e-3 {
        return None;
    }
    Some(rounded as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_configured_hwdec_from_effective_hardware_and_software() {
        let hw = DecodingDiagnostics::from_raw(
            Some("auto-safe"),
            Some("d3d11va"),
            Some("vp9"),
            Some("1920"),
            Some("1080"),
        );
        assert_eq!(hw.configured_hwdec.as_deref(), Some("auto-safe"));
        assert_eq!(
            hw.effective_decoding,
            EffectiveDecoding::HardwareActive("d3d11va".to_owned())
        );
        assert!(hw.effective_decoding.is_hardware_active());
        assert_eq!(hw.effective_decoding.active_backend(), Some("d3d11va"));
        assert_eq!(hw.codec.as_deref(), Some("vp9"));
        assert_eq!(
            hw.size,
            Some(VideoSize {
                width: 1920,
                height: 1080
            })
        );
        assert_eq!(
            hw.summary_line(),
            "hwdec-current: hardware ativo (d3d11va) • hwdec: auto-safe • codec: vp9 • tamanho: 1920 × 1080"
        );

        let sw = DecodingDiagnostics::from_raw(
            Some("auto-safe"),
            Some("no"),
            Some("av1"),
            Some("3840"),
            Some("2160"),
        );
        assert_eq!(sw.configured_hwdec.as_deref(), Some("auto-safe"));
        assert_eq!(sw.effective_decoding, EffectiveDecoding::Software);
        assert!(sw.effective_decoding.is_software());
        assert_eq!(
            sw.summary_line(),
            "hwdec-current: software (no) • hwdec: auto-safe • codec: av1 • tamanho: 3840 × 2160"
        );
    }

    #[test]
    fn treats_missing_policy_or_unrecognized_backend_as_unknown() {
        assert_eq!(
            classify_hwdec_current(None),
            EffectiveDecoding::Unknown(None)
        );
        assert_eq!(
            classify_hwdec_current(Some("   ")),
            EffectiveDecoding::Unknown(None)
        );
        assert_eq!(
            classify_hwdec_current(Some("auto-safe")),
            EffectiveDecoding::Unknown(Some("auto-safe".to_owned()))
        );
        assert_eq!(
            classify_hwdec_current(Some("yes")),
            EffectiveDecoding::Unknown(Some("yes".to_owned()))
        );
        assert_eq!(
            classify_hwdec_current(Some("mystery-hw-9000")),
            EffectiveDecoding::Unknown(Some("mystery-hw-9000".to_owned()))
        );

        let unknown_diag = DecodingDiagnostics::unknown(Some("auto-safe"));
        assert!(unknown_diag.effective_decoding.is_unknown());
        assert_eq!(
            unknown_diag.summary_line(),
            "hwdec-current: desconhecido • hwdec: auto-safe"
        );
    }

    #[test]
    fn collects_via_property_reader_and_omits_absent_metadata() {
        let diag = collect_from_property_reader(|name| match name {
            PROP_HWDEC => Some("auto-safe".to_owned()),
            PROP_HWDEC_CURRENT => Some("d3d11va-copy".to_owned()),
            PROP_VIDEO_CODEC => None,
            PROP_WIDTH => Some("1280.0".to_owned()),
            PROP_HEIGHT => Some("720".to_owned()),
            _ => None,
        });

        assert_eq!(
            diag.effective_decoding,
            EffectiveDecoding::HardwareActive("d3d11va-copy".to_owned())
        );
        assert_eq!(diag.codec, None);
        assert_eq!(
            diag.size,
            Some(VideoSize {
                width: 1280,
                height: 720
            })
        );
        assert_eq!(
            diag.summary_line(),
            "hwdec-current: hardware ativo (d3d11va-copy) • hwdec: auto-safe • tamanho: 1280 × 720"
        );
    }

    #[test]
    fn validates_video_dimensions() {
        assert_eq!(
            VideoSize::from_strings(Some("1920"), Some("1080")),
            Some(VideoSize {
                width: 1920,
                height: 1080
            })
        );
        assert_eq!(
            VideoSize::from_numbers(Some(1920.0), Some(1080.0)),
            Some(VideoSize {
                width: 1920,
                height: 1080
            })
        );
        assert_eq!(VideoSize::from_strings(Some("0"), Some("1080")), None);
        assert_eq!(VideoSize::from_strings(Some("1920"), None), None);
        assert_eq!(VideoSize::from_numbers(Some(1920.5), Some(1080.0)), None);
        assert_eq!(VideoSize::from_numbers(Some(f64::NAN), Some(1080.0)), None);
    }

    #[test]
    fn guidance_contains_actionable_steps_without_echoing_input_urls() {
        let report = safe_report(
            classify_failure("network timeout https://private.example/watch?v=secret"),
            false,
            false,
        );
        assert!(report.contains("Próximos passos:"));
        assert!(report.contains("Confirme que outros sites abrem"));
        assert!(!report.contains("private.example"));
        assert!(!report.contains("secret"));
    }
    #[test]
    fn safe_diagnostic_omits_sensitive_error_text() {
        let report = safe_report(
            classify_failure("quota https://example.test/?token=secret"),
            false,
            true,
        );
        assert!(report.contains("cota da API"));
        assert!(!report.contains("secret"));
        assert!(!report.contains("example.test"));
    }

    #[test]
    fn recognizes_extended_hwdec_backends() {
        for backend in [
            "d3d12va",
            "d3d12va-copy",
            "amf",
            "amf-copy",
            "vaapi",
            "vaapi-copy",
            "videotoolbox",
            "videotoolbox-copy",
            "mediacodec",
            "mediacodec-copy",
        ] {
            assert_eq!(
                classify_hwdec_current(Some(backend)),
                EffectiveDecoding::HardwareActive(backend.to_owned())
            );
        }
        assert_eq!(
            classify_hwdec_current(Some("D3D12VA")),
            EffectiveDecoding::HardwareActive("d3d12va".to_owned())
        );
    }

    #[test]
    fn classify_failure_redacts_paths_and_matches_expanded_keywords() {
        assert_eq!(
            classify_failure(r"failed to open C:\Users\token\oauth.json"),
            FailureKind::Unknown
        );
        assert_eq!(
            classify_failure(r#"cannot read "C:/Users/token/oauth.json":"#),
            FailureKind::Unknown
        );
        assert_eq!(
            classify_failure(r"unc error \\server\share\cookie\a"),
            FailureKind::Unknown
        );
        assert_eq!(
            classify_failure("missing /home/user/token/file"),
            FailureKind::Unknown
        );
        assert_eq!(
            classify_failure("https://example.test/oauth/token?cookie=secret"),
            FailureKind::Unknown
        );
        assert_eq!(
            classify_failure("falha em reprodução/dependências"),
            FailureKind::Playback
        );

        assert_eq!(
            classify_failure("HTTP 401 unauthorized"),
            FailureKind::Login
        );
        assert_eq!(classify_failure("HTTP 403 forbidden"), FailureKind::Login);
        assert_eq!(
            classify_failure("unauthenticated credential expired"),
            FailureKind::Login
        );
        assert_eq!(
            classify_failure("DPAPI cookie SAPISID credencial expirada"),
            FailureKind::Login
        );

        assert_eq!(
            classify_failure("HTTP 429 too many requests"),
            FailureKind::Quota
        );
        assert_eq!(
            classify_failure("RESOURCE_EXHAUSTED daily limit"),
            FailureKind::Quota
        );
        assert_eq!(
            classify_failure("limite diário atingido"),
            FailureKind::Quota
        );

        assert_eq!(
            classify_failure("TLS handshake reset"),
            FailureKind::Connection
        );
        assert_eq!(
            classify_failure("SSL certificate error: socket refused"),
            FailureKind::Connection
        );
        assert_eq!(
            classify_failure("request timed out on offline host unreachable eof"),
            FailureKind::Connection
        );
        assert_eq!(
            classify_failure("erro de certificado"),
            FailureKind::Connection
        );

        assert_eq!(
            classify_failure("ffmpeg codec error"),
            FailureKind::Playback
        );
        assert_eq!(
            classify_failure("mpv audio stream format dll opengl hwnd"),
            FailureKind::Playback
        );
        assert_eq!(
            classify_failure("áudio em formato inválido"),
            FailureKind::Playback
        );
    }

    #[test]
    fn bilingual_reports_and_summaries_preserve_ptbr_and_translate_en() {
        for kind in [
            FailureKind::Login,
            FailureKind::Quota,
            FailureKind::Connection,
            FailureKind::Playback,
            FailureKind::Unknown,
        ] {
            for authenticated in [false, true] {
                for player_active in [false, true] {
                    assert_eq!(
                        guidance(kind, authenticated, player_active),
                        guidance_for_lang(kind, authenticated, player_active, UiLanguage::PtBr)
                    );
                    assert_eq!(
                        safe_report(kind, authenticated, player_active),
                        safe_report_for_lang(kind, authenticated, player_active, UiLanguage::PtBr)
                    );
                }
            }
        }

        let en_report = safe_report_for_lang(
            classify_failure(
                r"HTTP 401 unauthorized https://private.example/?token=secret C:\Users\admin\oauth.json",
            ),
            true,
            false,
            UiLanguage::En,
        );
        assert!(en_report.contains("FastTube diagnostics"));
        assert!(en_report.contains("Category: login"));
        assert!(en_report.contains("Google session: connected"));
        assert!(en_report.contains("Player active: no"));
        assert!(en_report.contains("Next steps:"));
        assert!(en_report.contains("Verify oauth-client.json"));
        assert!(en_report.contains("Sensitive data and URLs have been omitted."));
        assert!(!en_report.contains("private.example"));
        assert!(!en_report.contains("secret"));
        assert!(!en_report.contains("oauth.json"));
        assert!(!en_report.contains("admin"));
        assert!(!en_report.contains("Users"));

        let quota_en = safe_report_for_lang(FailureKind::Quota, false, true, UiLanguage::En);
        assert!(quota_en.contains("Category: API quota"));
        assert!(quota_en.contains("Google session: disconnected"));
        assert!(quota_en.contains("Player active: yes"));

        let hw = DecodingDiagnostics::from_raw(
            Some("auto-safe"),
            Some("d3d12va"),
            Some("vp9"),
            Some("1920"),
            Some("1080"),
        );
        assert_eq!(
            hw.effective_decoding.short_label_for_lang(UiLanguage::En),
            "active hardware (d3d12va)"
        );
        assert_eq!(
            hw.summary_line_for_lang(UiLanguage::En),
            "hwdec-current: active hardware (d3d12va) • hwdec: auto-safe • codec: vp9 • size: 1920 × 1080"
        );
        assert_eq!(
            hw.summary_line(),
            hw.summary_line_for_lang(UiLanguage::PtBr)
        );
        assert_eq!(hw.configured_hwdec_label(), "auto-safe");
        assert_eq!(
            hw.configured_hwdec_label_for_lang(UiLanguage::PtBr),
            "auto-safe"
        );
        assert_eq!(
            hw.effective_decoding.short_label(),
            hw.effective_decoding.short_label_for_lang(UiLanguage::PtBr)
        );

        let unknown = DecodingDiagnostics::unknown(None);
        assert_eq!(
            unknown.configured_hwdec_label_for_lang(UiLanguage::En),
            "unknown"
        );
        assert_eq!(
            unknown.summary_line_for_lang(UiLanguage::En),
            "hwdec-current: unknown • hwdec: unknown"
        );
        let policy_unknown =
            DecodingDiagnostics::from_raw(Some("auto-safe"), Some("yes"), None, None, None);
        assert_eq!(
            policy_unknown.summary_line_for_lang(UiLanguage::En),
            "hwdec-current: unknown (yes) • hwdec: auto-safe"
        );
    }

    #[test]
    fn player_debug_info_json_omits_secrets_paths_and_raw_errors() {
        let diag = DecodingDiagnostics::from_raw(
            Some("auto-safe"),
            Some("d3d11va"),
            Some("av1"),
            Some("1920"),
            Some("1080"),
        );
        let failure_kind =
            classify_failure(r"HTTP 401 token=secret C:\Users\alice\oauth-client.json");
        let safe_json = player_debug_info_json(&PlayerDebugInfoInput {
            app_version: "0.3.0",
            video_id: Some("dQw4w9WgXcQ"),
            video_size: diag.size,
            viewport_size: VideoSize::new(1280, 720),
            fps: Some(59.94),
            dropped_frames: Some(2),
            video_codec: diag.codec.as_deref(),
            audio_codec: Some("opus"),
            hwdec_configured: diag.configured_hwdec.as_deref(),
            hwdec_effective: &diag.effective_decoding,
            speed: 1.25,
            volume_percent: 82.0,
            muted: false,
            buffer_profile: "Padrão",
            buffer_ahead_secs: Some(14.37),
            is_live: false,
            audio_only: false,
            loop_video: true,
            player_backend: "libmpv-opengl",
            failure_category: Some(failure_kind),
        });
        let parsed: serde_json::Value = serde_json::from_str(&safe_json).expect("valid debug json");
        assert_eq!(parsed["app"], "FastTube");
        assert_eq!(parsed["video_id"], "dQw4w9WgXcQ");
        assert_eq!(parsed["video_resolution"], "1920x1080");
        assert_eq!(parsed["viewport_resolution"], "1280x720");
        assert_eq!(parsed["video_codec"], "av1");
        assert_eq!(parsed["audio_codec"], "opus");
        assert_eq!(parsed["hwdec_effective"], "hardware:d3d11va");
        assert_eq!(parsed["dropped_frames"], 2);
        assert_eq!(parsed["loop_video"], true);
        assert_eq!(parsed["failure_category"], "login");

        let tainted_json = player_debug_info_json(&PlayerDebugInfoInput {
            app_version: "0.3.0",
            video_id: Some("https://youtu.be/secret"),
            video_size: None,
            viewport_size: None,
            fps: None,
            dropped_frames: None,
            video_codec: Some(r"C:\Users\alice\cookies.txt"),
            audio_codec: Some("https://example.test/?token=secret"),
            hwdec_configured: Some("cookie=secret"),
            hwdec_effective: &EffectiveDecoding::Software,
            speed: f64::NAN,
            volume_percent: 50.0,
            muted: true,
            buffer_profile: "default",
            buffer_ahead_secs: None,
            is_live: true,
            audio_only: true,
            loop_video: false,
            player_backend: "libmpv-hwnd",
            failure_category: Some(classify_failure(
                r"failed at https://private.example/?cookie=abc C:\secret\path",
            )),
        });
        let parsed_tainted: serde_json::Value =
            serde_json::from_str(&tainted_json).expect("valid tainted json");
        assert!(parsed_tainted["video_id"].is_null());
        assert!(parsed_tainted["video_codec"].is_null());
        assert!(parsed_tainted["audio_codec"].is_null());
        assert!(parsed_tainted["hwdec_configured"].is_null());
        assert!(!tainted_json.contains("secret"));
        assert!(!tainted_json.contains("private.example"));
        assert!(!tainted_json.contains("alice"));
        assert!(!tainted_json.contains("cookies.txt"));
    }
}
