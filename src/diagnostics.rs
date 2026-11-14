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

pub fn guidance(kind: FailureKind, authenticated: bool, player_active: bool) -> FailureGuidance {
    match kind {
        FailureKind::Login => FailureGuidance {
            summary: if authenticated {
                "A sessão existe, mas o acesso pode ter expirado ou não incluir a permissão necessária."
            } else {
                "Conecte a conta Google para usar inscrições, playlists e comentários."
            },
            steps: [
                "Confirme o oauth-client.json na pasta de dados do FastTube.",
                "Entre novamente e aceite as permissões solicitadas.",
                "Teste uma consulta pública antes de repetir a ação privada.",
            ],
        },
        FailureKind::Quota => FailureGuidance {
            summary: "A cota da YouTube Data API foi atingida; a Home pública e os dados locais continuam disponíveis.",
            steps: [
                "Aguarde a renovação diária da cota do projeto Google.",
                "Evite repetir buscas de inscrições e playlists em sequência.",
                "Use a Home, a biblioteca e a fila até a cota voltar.",
            ],
        },
        FailureKind::Connection => FailureGuidance {
            summary: "O FastTube não conseguiu alcançar o serviço remoto.",
            steps: [
                "Confirme que outros sites abrem no mesmo computador.",
                "Desative temporariamente VPN ou proxy e tente novamente.",
                "Se o erro persistir, copie o relatório redigido para diagnóstico.",
            ],
        },
        FailureKind::Playback => FailureGuidance {
            summary: if player_active {
                "O player está aberto, mas a faixa ou uma dependência não carregou corretamente."
            } else {
                "O player ainda não foi iniciado para esta reprodução."
            },
            steps: [
                "Confirme que libmpv-2.dll está ao lado do executável.",
                "Confirme que yt-dlp está no PATH ou junto do FastTube.",
                "Reabra o vídeo; se falhar, teste outra qualidade ou formato.",
            ],
        },
        FailureKind::Unknown => FailureGuidance {
            summary: "A mensagem não permitiu identificar uma causa específica com segurança.",
            steps: [
                "Repita a ação uma vez para distinguir falha temporária.",
                "Copie o relatório redigido e confira categoria e estado do player.",
                "Reinicie o FastTube se o problema ocorrer em várias telas.",
            ],
        },
    }
}
pub fn classify_failure(message: &str) -> FailureKind {
    let text = message
        .split_whitespace()
        .filter(|part| !part.contains("://"))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if ["token", "login", "sessão", "autoriz", "oauth", "permissão"]
        .iter()
        .any(|s| text.contains(s))
    {
        FailureKind::Login
    } else if ["quota", "cota", "rate limit"]
        .iter()
        .any(|s| text.contains(s))
    {
        FailureKind::Quota
    } else if ["conexão", "connect", "network", "rede", "timeout", "dns"]
        .iter()
        .any(|s| text.contains(s))
    {
        FailureKind::Connection
    } else if [
        "libmpv",
        "yt-dlp",
        "player",
        "reprodução",
        "playback",
        "decoder",
    ]
    .iter()
    .any(|s| text.contains(s))
    {
        FailureKind::Playback
    } else {
        FailureKind::Unknown
    }
}
pub fn safe_report(kind: FailureKind, authenticated: bool, player_active: bool) -> String {
    let guidance = guidance(kind, authenticated, player_active);
    let category = match kind {
        FailureKind::Login => "login",
        FailureKind::Quota => "cota da API",
        FailureKind::Connection => "conexão",
        FailureKind::Playback => "reprodução/dependências",
        FailureKind::Unknown => "não classificada",
    };
    format!(
        "FastTube diagnóstico\nCategoria: {category}\nSessão Google: {}\nPlayer ativo: {}\nPróximos passos:\n- {}\n- {}\n- {}\nDados sensíveis e URLs foram omitidos.",
        if authenticated {
            "conectada"
        } else {
            "desconectada"
        },
        if player_active { "sim" } else { "não" },
        guidance.steps[0],
        guidance.steps[1],
        guidance.steps[2],
    )
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

/// Only explicit known Windows `libmpv` backends count as active hardware decoding;
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

    pub fn short_label(&self) -> String {
        match self {
            Self::HardwareActive(backend) => format!("hardware ativo ({backend})"),
            Self::Software => "software (no)".to_owned(),
            Self::Unknown(Some(raw)) => format!("desconhecido ({raw})"),
            Self::Unknown(None) => "desconhecido".to_owned(),
        }
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
            | "dxva2"
            | "dxva2-copy"
            | "nvdec"
            | "nvdec-copy"
            | "vulkan"
            | "vulkan-copy"
            | "qsv"
            | "qsv-copy"
            | "cuda"
            | "cuda-copy"
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

    pub fn configured_hwdec_label(&self) -> &str {
        self.configured_hwdec.as_deref().unwrap_or("desconhecido")
    }

    pub fn summary_line(&self) -> String {
        let mut parts = vec![
            format!("hwdec-current: {}", self.effective_decoding.short_label()),
            format!("hwdec: {}", self.configured_hwdec_label()),
        ];
        if let Some(codec) = &self.codec {
            parts.push(format!("codec: {codec}"));
        }
        if let Some(size) = &self.size {
            parts.push(format!("tamanho: {size}"));
        }
        parts.join(" • ")
    }
}

pub fn collect_from_property_reader<F>(read_property: F) -> DecodingDiagnostics
where
    F: FnMut(&str) -> Option<String>,
{
    DecodingDiagnostics::from_property_reader(read_property)
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
}
