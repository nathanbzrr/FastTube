use crate::history;
use crate::process::Cancellation;
use eframe::egui::Color32;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::io::Read;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

const SPONSORBLOCK_API_URL: &str = "https://sponsor.ajay.app/api/skipSegments";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const SKIP_TOAST_DURATION: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SponsorCategory {
    Sponsor,
    SelfPromo,
    Interaction,
    Intro,
    Outro,
    Preview,
    MusicOfftopic,
    Filler,
}

impl SponsorCategory {
    pub const ALL: [Self; 8] = [
        Self::Sponsor,
        Self::SelfPromo,
        Self::Interaction,
        Self::Intro,
        Self::Outro,
        Self::Preview,
        Self::MusicOfftopic,
        Self::Filler,
    ];

    pub fn api_name(self) -> &'static str {
        match self {
            Self::Sponsor => "sponsor",
            Self::SelfPromo => "selfpromo",
            Self::Interaction => "interaction",
            Self::Intro => "intro",
            Self::Outro => "outro",
            Self::Preview => "preview",
            Self::MusicOfftopic => "music_offtopic",
            Self::Filler => "filler",
        }
    }

    pub fn from_api_name(name: &str) -> Option<Self> {
        match name {
            "sponsor" => Some(Self::Sponsor),
            "selfpromo" => Some(Self::SelfPromo),
            "interaction" => Some(Self::Interaction),
            "intro" => Some(Self::Intro),
            "outro" => Some(Self::Outro),
            "preview" => Some(Self::Preview),
            "music_offtopic" => Some(Self::MusicOfftopic),
            "filler" => Some(Self::Filler),
            _ => None,
        }
    }

    pub fn label_pt(self) -> &'static str {
        match self {
            Self::Sponsor => "Patrocínio",
            Self::SelfPromo => "Autopromoção / Mercadorias",
            Self::Interaction => "Lembrete de inscrição / Like",
            Self::Intro => "Introdução / Vinheta",
            Self::Outro => "Encerramento / Créditos",
            Self::Preview => "Prévia / Recapitulação",
            Self::MusicOfftopic => "Trecho não musical",
            Self::Filler => "Enchimento / Piada paralela",
        }
    }

    #[allow(dead_code)]
    pub fn label_en(self) -> &'static str {
        match self {
            Self::Sponsor => "Sponsor",
            Self::SelfPromo => "Self-Promotion / Merch",
            Self::Interaction => "Subscribe / Like Reminder",
            Self::Intro => "Intro / Intermission",
            Self::Outro => "Outro / Credits",
            Self::Preview => "Preview / Recap",
            Self::MusicOfftopic => "Non-Music Section",
            Self::Filler => "Filler / Tangent",
        }
    }

    #[allow(dead_code)]
    pub fn label(self, pt: bool) -> &'static str {
        if pt { self.label_pt() } else { self.label_en() }
    }

    pub fn description_pt(self) -> &'static str {
        match self {
            Self::Sponsor => {
                "Leituras pagas, inserções comerciais e segmentos patrocinados pelo anunciante."
            }
            Self::SelfPromo => {
                "Divulgação de produtos próprios, cursos, links de afiliado ou outros canais."
            }
            Self::Interaction => {
                "Pedidos explícitos no meio do vídeo para curtir, inscrever-se ou comentar."
            }
            Self::Intro => "Vinhetas animadas ou aberturas repetitivas antes do conteúdo.",
            Self::Outro => "Telas finais, agradecimentos e créditos ao final do vídeo.",
            Self::Preview => "Recaps rápidos ou prévias do que acontece mais tarde no vídeo.",
            Self::MusicOfftopic => {
                "Trechos falados, intros longas ou pausas sem música dentro de videoclipes."
            }
            Self::Filler => "Tangentes e piadas fora do assunto principal do vídeo.",
        }
    }

    #[allow(dead_code)]
    pub fn description_en(self) -> &'static str {
        match self {
            Self::Sponsor => "Paid reads, ad placements, and sponsor-promoted segments.",
            Self::SelfPromo => {
                "Promotion of own products, courses, affiliate links, or other channels."
            }
            Self::Interaction => "Explicit mid-video requests to like, subscribe, or comment.",
            Self::Intro => "Animated title cards or repetitive openings before the main content.",
            Self::Outro => "End screens, thank-yous, and credits at the end of the video.",
            Self::Preview => "Quick recaps or previews of what happens later in the video.",
            Self::MusicOfftopic => {
                "Spoken sections, long intros, or non-music breaks inside music videos."
            }
            Self::Filler => "Tangents and off-topic jokes outside the video's main subject.",
        }
    }

    #[allow(dead_code)]
    pub fn description(self, pt: bool) -> &'static str {
        if pt {
            self.description_pt()
        } else {
            self.description_en()
        }
    }

    pub fn recommended(self) -> bool {
        matches!(
            self,
            Self::Sponsor | Self::SelfPromo | Self::Interaction | Self::Intro | Self::Outro
        )
    }

    pub fn color(self) -> Color32 {
        match self {
            Self::Sponsor => Color32::from_rgb(0, 212, 0),
            Self::SelfPromo => Color32::from_rgb(235, 200, 20),
            Self::Interaction => Color32::from_rgb(204, 0, 255),
            Self::Intro => Color32::from_rgb(0, 200, 220),
            Self::Outro => Color32::from_rgb(66, 135, 245),
            Self::Preview => Color32::from_rgb(0, 143, 214),
            Self::MusicOfftopic => Color32::from_rgb(255, 153, 0),
            Self::Filler => Color32::from_rgb(138, 66, 255),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SponsorSkipMode {
    #[default]
    AutoSkip,
    ManualButton,
    HighlightOnly,
}

impl SponsorSkipMode {
    pub const ALL: [Self; 3] = [Self::AutoSkip, Self::ManualButton, Self::HighlightOnly];

    pub fn label_pt(self) -> &'static str {
        match self {
            Self::AutoSkip => "Pular automaticamente",
            Self::ManualButton => "Mostrar botão para pular",
            Self::HighlightOnly => "Apenas destacar na barra",
        }
    }

    #[allow(dead_code)]
    pub fn label_en(self) -> &'static str {
        match self {
            Self::AutoSkip => "Auto-skip",
            Self::ManualButton => "Show skip button",
            Self::HighlightOnly => "Highlight on bar only",
        }
    }

    #[allow(dead_code)]
    pub fn label(self, pt: bool) -> &'static str {
        if pt { self.label_pt() } else { self.label_en() }
    }

    pub fn description_pt(self) -> &'static str {
        match self {
            Self::AutoSkip => "Avança automaticamente ao entrar em um segmento marcado.",
            Self::ManualButton => {
                "Exibe um botão flutuante sobre o player para você pular se quiser."
            }
            Self::HighlightOnly => {
                "Mostra apenas as marcações coloridas na barra de progresso, sem pular."
            }
        }
    }

    #[allow(dead_code)]
    pub fn description_en(self) -> &'static str {
        match self {
            Self::AutoSkip => "Automatically jumps ahead when entering a marked segment.",
            Self::ManualButton => {
                "Shows a floating button over the player so you can skip manually."
            }
            Self::HighlightOnly => {
                "Only shows colored markers on the progress bar without skipping."
            }
        }
    }

    #[allow(dead_code)]
    pub fn description(self, pt: bool) -> &'static str {
        if pt {
            self.description_pt()
        } else {
            self.description_en()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SponsorCategoryFlags {
    pub sponsor: bool,
    pub selfpromo: bool,
    pub interaction: bool,
    pub intro: bool,
    pub outro: bool,
    pub preview: bool,
    pub music_offtopic: bool,
    pub filler: bool,
}

impl Default for SponsorCategoryFlags {
    fn default() -> Self {
        Self {
            sponsor: true,
            selfpromo: true,
            interaction: true,
            intro: true,
            outro: true,
            preview: false,
            music_offtopic: false,
            filler: false,
        }
    }
}

impl SponsorCategoryFlags {
    pub fn is_enabled(self, category: SponsorCategory) -> bool {
        match category {
            SponsorCategory::Sponsor => self.sponsor,
            SponsorCategory::SelfPromo => self.selfpromo,
            SponsorCategory::Interaction => self.interaction,
            SponsorCategory::Intro => self.intro,
            SponsorCategory::Outro => self.outro,
            SponsorCategory::Preview => self.preview,
            SponsorCategory::MusicOfftopic => self.music_offtopic,
            SponsorCategory::Filler => self.filler,
        }
    }

    pub fn set(&mut self, category: SponsorCategory, enabled: bool) {
        match category {
            SponsorCategory::Sponsor => self.sponsor = enabled,
            SponsorCategory::SelfPromo => self.selfpromo = enabled,
            SponsorCategory::Interaction => self.interaction = enabled,
            SponsorCategory::Intro => self.intro = enabled,
            SponsorCategory::Outro => self.outro = enabled,
            SponsorCategory::Preview => self.preview = enabled,
            SponsorCategory::MusicOfftopic => self.music_offtopic = enabled,
            SponsorCategory::Filler => self.filler = enabled,
        }
    }

    pub fn set_all(&mut self, enabled: bool) {
        for category in SponsorCategory::ALL {
            self.set(category, enabled);
        }
    }

    pub fn active_count(self) -> usize {
        SponsorCategory::ALL
            .into_iter()
            .filter(|category| self.is_enabled(*category))
            .count()
    }

    pub fn enabled_categories(self) -> Vec<SponsorCategory> {
        SponsorCategory::ALL
            .into_iter()
            .filter(|category| self.is_enabled(*category))
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SponsorBlockSettings {
    pub enabled: bool,
    pub skip_mode: SponsorSkipMode,
    pub categories: SponsorCategoryFlags,
    pub min_duration_secs: f64,
    pub show_timeline_markers: bool,
    pub show_skip_toast: bool,
}

impl Default for SponsorBlockSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            skip_mode: SponsorSkipMode::AutoSkip,
            categories: SponsorCategoryFlags::default(),
            min_duration_secs: 1.0,
            show_timeline_markers: true,
            show_skip_toast: true,
        }
    }
}

impl SponsorBlockSettings {
    pub fn normalized(mut self) -> Self {
        if !self.min_duration_secs.is_finite() {
            self.min_duration_secs = 1.0;
        }
        self.min_duration_secs = self.min_duration_secs.clamp(0.0, 30.0);
        self
    }

    pub fn valid(&self) -> bool {
        self.min_duration_secs.is_finite() && (0.0..=30.0).contains(&self.min_duration_secs)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SponsorSegment {
    pub uuid: String,
    pub start_secs: f64,
    pub end_secs: f64,
    pub category: SponsorCategory,
}

impl SponsorSegment {
    pub fn duration_secs(&self) -> f64 {
        (self.end_secs - self.start_secs).max(0.0)
    }

    pub fn contains(&self, pos_secs: f64) -> bool {
        pos_secs.is_finite() && pos_secs >= self.start_secs && pos_secs < self.end_secs - 0.15
    }
}

pub struct SponsorBlockFetchMessage {
    pub request_id: u64,
    pub video_id: String,
    pub result: Result<Vec<SponsorSegment>, String>,
}

#[derive(Default)]
pub struct SponsorBlockRuntime {
    pub video_id: Option<String>,
    pub next_request_id: u64,
    pub active_request_id: Option<u64>,
    pub rx: Option<Receiver<SponsorBlockFetchMessage>>,
    pub cancel: Option<Cancellation>,
    pub segments: Vec<SponsorSegment>,
    pub loaded: bool,
    pub error: Option<String>,
    pub auto_skipped_uuids: HashSet<String>,
    pub last_skip_notice: Option<(String, Instant)>,
}

impl SponsorBlockRuntime {
    pub fn cancel_inflight(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.rx = None;
        self.active_request_id = None;
    }

    pub fn reset_for_video(&mut self, video_id: Option<String>) {
        self.cancel_inflight();
        self.video_id = video_id;
        self.segments.clear();
        self.loaded = false;
        self.error = None;
        self.auto_skipped_uuids.clear();
        self.last_skip_notice = None;
    }

    pub fn filtered_segments<'a>(
        &'a self,
        settings: &SponsorBlockSettings,
    ) -> impl Iterator<Item = &'a SponsorSegment> {
        self.segments.iter().filter(|segment| {
            settings.enabled
                && settings.categories.is_enabled(segment.category)
                && segment.duration_secs() >= settings.min_duration_secs
        })
    }

    pub fn segment_at(
        &self,
        pos_secs: f64,
        settings: &SponsorBlockSettings,
    ) -> Option<&SponsorSegment> {
        self.filtered_segments(settings)
            .find(|segment| segment.contains(pos_secs))
    }

    #[allow(dead_code)]
    pub fn coalesced_skip_target(
        &self,
        pos_secs: f64,
        settings: &SponsorBlockSettings,
    ) -> Option<(f64, Vec<String>, SponsorCategory)> {
        let first = self
            .filtered_segments(settings)
            .find(|seg| seg.contains(pos_secs) && !self.auto_skipped_uuids.contains(&seg.uuid))?;
        let mut target_end = first.end_secs;
        let mut uuids = vec![first.uuid.clone()];
        let primary_category = first.category;
        loop {
            let mut extended = false;
            for seg in self.filtered_segments(settings) {
                if !uuids.contains(&seg.uuid)
                    && !self.auto_skipped_uuids.contains(&seg.uuid)
                    && seg.start_secs <= target_end + 0.35
                    && seg.end_secs > target_end
                {
                    target_end = seg.end_secs;
                    uuids.push(seg.uuid.clone());
                    extended = true;
                }
            }
            if !extended {
                break;
            }
        }
        Some((target_end, uuids, primary_category))
    }

    pub fn active_skip_toast(&self, now: Instant) -> Option<&str> {
        self.last_skip_notice.as_ref().and_then(|(label, at)| {
            (now.saturating_duration_since(*at) < SKIP_TOAST_DURATION).then_some(label.as_str())
        })
    }
}

pub fn parse_segments_json(bytes: &[u8]) -> Result<Vec<SponsorSegment>, String> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err("Resposta do SponsorBlock excedeu o limite permitido.".into());
    }
    let root: Value = serde_json::from_slice(bytes)
        .map_err(|_| "Resposta JSON inválida do SponsorBlock.".to_owned())?;
    let items = root
        .as_array()
        .ok_or_else(|| "Formato inesperado na resposta do SponsorBlock.".to_owned())?;
    let mut segments = Vec::new();
    for item in items.iter().take(256) {
        let Some(category_str) = item.get("category").and_then(Value::as_str) else {
            continue;
        };
        let Some(category) = SponsorCategory::from_api_name(category_str) else {
            continue;
        };
        if let Some(action_type) = item.get("actionType").and_then(Value::as_str)
            && action_type != "skip"
        {
            continue;
        }
        let Some(uuid) = item
            .get("UUID")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.len() <= 128)
        else {
            continue;
        };
        let Some(bounds) = item.get("segment").and_then(Value::as_array) else {
            continue;
        };
        if bounds.len() < 2 {
            continue;
        }
        let Some(start_secs) = bounds[0].as_f64() else {
            continue;
        };
        let Some(end_secs) = bounds[1].as_f64() else {
            continue;
        };
        if !start_secs.is_finite()
            || !end_secs.is_finite()
            || start_secs < 0.0
            || end_secs <= start_secs + 0.1
        {
            continue;
        }
        segments.push(SponsorSegment {
            uuid: uuid.to_owned(),
            start_secs,
            end_secs,
            category,
        });
    }
    segments.sort_by(|a, b| {
        a.start_secs
            .total_cmp(&b.start_secs)
            .then_with(|| a.end_secs.total_cmp(&b.end_secs))
    });
    Ok(segments)
}

pub fn build_segments_url(
    video_id: &str,
    categories: &[SponsorCategory],
) -> Result<String, String> {
    if !history::is_valid_video_id(video_id) {
        return Err("ID de vídeo inválido para consulta ao SponsorBlock.".into());
    }
    let category_names: Vec<&str> = categories.iter().map(|c| c.api_name()).collect();
    let categories_json = serde_json::to_string(&category_names)
        .map_err(|_| "Falha ao serializar categorias do SponsorBlock.".to_owned())?;
    let mut parsed = url::Url::parse(SPONSORBLOCK_API_URL)
        .map_err(|_| "URL do SponsorBlock inválida.".to_owned())?;
    parsed
        .query_pairs_mut()
        .append_pair("videoID", video_id)
        .append_pair("categories", &categories_json)
        .append_pair("actionType", "skip");
    Ok(parsed.into())
}

pub fn fetch_segments_with_cancel(
    video_id: &str,
    categories: &[SponsorCategory],
    cancel: &Cancellation,
) -> Result<Vec<SponsorSegment>, String> {
    if cancel.cancelled() {
        return Err("Consulta ao SponsorBlock cancelada.".into());
    }
    if categories.is_empty() {
        return Ok(Vec::new());
    }
    let request_url = build_segments_url(video_id, categories)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent("FastTube/0.1 (SponsorBlock integration)")
        .build()
        .map_err(|error| {
            format!("Não foi possível iniciar cliente HTTP do SponsorBlock: {error}")
        })?;
    if cancel.cancelled() {
        return Err("Consulta ao SponsorBlock cancelada.".into());
    }
    let response = client
        .get(&request_url)
        .send()
        .map_err(|error| format!("Falha ao consultar SponsorBlock: {error}"))?;
    if cancel.cancelled() {
        return Err("Consulta ao SponsorBlock cancelada.".into());
    }
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Vec::new());
    }
    if !response.status().is_success() {
        return Err(format!(
            "SponsorBlock respondeu com status HTTP {}.",
            response.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    response
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Erro ao ler resposta do SponsorBlock: {error}"))?;
    parse_segments_json(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_sorts_valid_sponsorblock_segments() {
        let payload = br#"[
            {"UUID":"seg-2","segment":[120.5,145.0],"category":"outro","actionType":"skip"},
            {"UUID":"seg-1","segment":[12.0,32.5],"category":"sponsor","actionType":"skip"},
            {"UUID":"ignore-mute","segment":[40.0,50.0],"category":"sponsor","actionType":"mute"},
            {"UUID":"ignore-bad-bounds","segment":[80.0,70.0],"category":"intro","actionType":"skip"}
        ]"#;
        let parsed = parse_segments_json(payload).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].uuid, "seg-1");
        assert_eq!(parsed[0].category, SponsorCategory::Sponsor);
        assert_eq!(parsed[1].uuid, "seg-2");
        assert_eq!(parsed[1].category, SponsorCategory::Outro);
    }

    #[test]
    fn builds_validated_api_url_and_filters_active_segments() {
        let url = build_segments_url(
            "dQw4w9WgXcQ",
            &[SponsorCategory::Sponsor, SponsorCategory::SelfPromo],
        )
        .unwrap();
        assert!(url.contains("videoID=dQw4w9WgXcQ"));
        assert!(url.contains("sponsor"));
        assert!(url.contains("selfpromo"));
        assert!(build_segments_url("bad", &[SponsorCategory::Sponsor]).is_err());

        let runtime = SponsorBlockRuntime {
            segments: vec![
                SponsorSegment {
                    uuid: "short".into(),
                    start_secs: 5.0,
                    end_secs: 5.5,
                    category: SponsorCategory::Sponsor,
                },
                SponsorSegment {
                    uuid: "real".into(),
                    start_secs: 20.0,
                    end_secs: 45.0,
                    category: SponsorCategory::Sponsor,
                },
            ],
            ..Default::default()
        };
        let settings = SponsorBlockSettings::default();
        assert!(runtime.segment_at(5.2, &settings).is_none());
        assert_eq!(
            runtime.segment_at(25.0, &settings).map(|s| s.uuid.as_str()),
            Some("real")
        );
        assert!(runtime.segment_at(44.95, &settings).is_none());
    }

    #[test]
    fn coalesces_adjacent_sponsor_segments_into_single_jump() {
        let settings = SponsorBlockSettings::default();
        let mut runtime = SponsorBlockRuntime {
            segments: vec![
                SponsorSegment {
                    uuid: "intro".into(),
                    start_secs: 5.0,
                    end_secs: 15.0,
                    category: SponsorCategory::Intro,
                },
                SponsorSegment {
                    uuid: "sponsor".into(),
                    start_secs: 15.2,
                    end_secs: 40.0,
                    category: SponsorCategory::Sponsor,
                },
                SponsorSegment {
                    uuid: "far".into(),
                    start_secs: 40.5,
                    end_secs: 50.0,
                    category: SponsorCategory::SelfPromo,
                },
            ],
            ..Default::default()
        };

        assert_eq!(
            runtime.coalesced_skip_target(6.0, &settings),
            Some((
                40.0,
                vec!["intro".into(), "sponsor".into()],
                SponsorCategory::Intro
            ))
        );

        // Segments already marked as skipped are ignored during coalescing.
        runtime.auto_skipped_uuids.insert("sponsor".into());
        assert_eq!(
            runtime.coalesced_skip_target(6.0, &settings),
            Some((15.0, vec!["intro".into()], SponsorCategory::Intro))
        );

        runtime.auto_skipped_uuids.insert("intro".into());
        assert_eq!(runtime.coalesced_skip_target(6.0, &settings), None);
    }

    #[test]
    fn bilingual_labels_and_descriptions_are_non_empty() {
        let mut cat_labels_en = HashSet::new();
        let mut cat_labels_pt = HashSet::new();
        let mut cat_descs_en = HashSet::new();
        let mut cat_descs_pt = HashSet::new();
        for cat in SponsorCategory::ALL {
            assert!(!cat.label_en().trim().is_empty());
            assert!(!cat.label_pt().trim().is_empty());
            assert!(!cat.description_en().trim().is_empty());
            assert!(!cat.description_pt().trim().is_empty());
            assert_eq!(cat.label(false), cat.label_en());
            assert_eq!(cat.label(true), cat.label_pt());
            assert_eq!(cat.description(false), cat.description_en());
            assert_eq!(cat.description(true), cat.description_pt());
            assert!(cat_labels_en.insert(cat.label_en()));
            assert!(cat_labels_pt.insert(cat.label_pt()));
            assert!(cat_descs_en.insert(cat.description_en()));
            assert!(cat_descs_pt.insert(cat.description_pt()));
        }

        let mut mode_labels_en = HashSet::new();
        let mut mode_labels_pt = HashSet::new();
        let mut mode_descs_en = HashSet::new();
        let mut mode_descs_pt = HashSet::new();
        for mode in SponsorSkipMode::ALL {
            assert!(!mode.label_en().trim().is_empty());
            assert!(!mode.label_pt().trim().is_empty());
            assert!(!mode.description_en().trim().is_empty());
            assert!(!mode.description_pt().trim().is_empty());
            assert_eq!(mode.label(false), mode.label_en());
            assert_eq!(mode.label(true), mode.label_pt());
            assert_eq!(mode.description(false), mode.description_en());
            assert_eq!(mode.description(true), mode.description_pt());
            assert!(mode_labels_en.insert(mode.label_en()));
            assert!(mode_labels_pt.insert(mode.label_pt()));
            assert!(mode_descs_en.insert(mode.description_en()));
            assert!(mode_descs_pt.insert(mode.description_pt()));
        }
    }
}
