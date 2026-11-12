use crate::history::{self, Entry as HistoryEntry, History};
use crate::queue::Queue;
use crate::search::{Video, VideoMetadata};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

/// Filtros estilo chips do YouTube para a página inicial e barra lateral.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FeedFilter {
    #[default]
    All,
    Recommended,
    Music,
    Podcasts,
    Gaming,
    Live,
    Mixes,
    RecentlyUploaded,
    NewToYou,
    Shorts,
    Topic(String),
    Channel(String),
}

impl FeedFilter {
    pub fn label(&self) -> String {
        match self {
            Self::All => "Tudo".into(),
            Self::Recommended => "Recomendados".into(),
            Self::Music => "Música".into(),
            Self::Podcasts => "Podcasts".into(),
            Self::Gaming => "Jogos".into(),
            Self::Live => "Ao vivo".into(),
            Self::Mixes => "Mixes".into(),
            Self::RecentlyUploaded => "Enviados recentemente".into(),
            Self::NewToYou => "Novidades para você".into(),
            Self::Shorts => "Shorts".into(),
            Self::Topic(topic) => topic.clone(),
            Self::Channel(channel) => channel.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum WatchedState {
    InProgress(f64),
    Completed,
}

/// Perfil comportamental aprendido localmente a partir do histórico, fila e sessão atual.
#[derive(Clone, Debug, Default)]
pub struct UserProfile {
    pub channel_weights: HashMap<String, f64>,
    pub token_weights: HashMap<String, f64>,
    pub preferred_duration_secs: Option<f64>,
    watched_states: HashMap<String, WatchedState>,
    pub active_channel: Option<String>,
    pub active_tokens: HashSet<String>,
}

/// Resultado de pontuação de um vídeo pelo motor de recomendações.
#[derive(Clone, Debug)]
pub struct ScoredVideo {
    pub video: Video,
    pub metadata: VideoMetadata,
    pub score: f64,
    pub reason: Option<String>,
    pub is_short: bool,
}

impl UserProfile {
    pub fn build(history: &History, queue: &Queue, current_video: Option<&Video>) -> Self {
        let mut profile = Self::default();
        let mut duration_sum = 0.0;
        let mut duration_weight_sum = 0.0;

        for (index, entry) in history.entries().iter().enumerate() {
            // Peso decai suavemente para itens mais antigos do histórico.
            let recency_weight = 1.0 / (1.0 + (index as f64) * 0.14);
            let completion_ratio = match entry.duration_ms {
                Some(dur) if dur > 0 => (entry.position_ms as f64 / dur as f64).clamp(0.05, 1.0),
                _ if entry.position_ms > 0 => 0.65,
                _ => 0.35,
            };
            let engagement_multiplier = 0.55 + 0.90 * completion_ratio;
            let weight = recency_weight * engagement_multiplier;

            if entry.is_near_end() && entry.position_ms > 0 {
                profile
                    .watched_states
                    .insert(entry.video_id.clone(), WatchedState::Completed);
            } else if entry.resume_position_ms().is_some() {
                profile.watched_states.insert(
                    entry.video_id.clone(),
                    WatchedState::InProgress(completion_ratio),
                );
            }

            let channel_key = normalize_channel_key(&entry.channel);
            if !channel_key.is_empty() {
                *profile.channel_weights.entry(channel_key).or_insert(0.0) += weight * 2.4;
            }

            for token in extract_significant_tokens(&entry.title) {
                *profile.token_weights.entry(token).or_insert(0.0) += weight * 1.35;
            }

            if let Some(dur_ms) = entry.duration_ms {
                let secs = dur_ms as f64 / 1000.0;
                if secs > 10.0 && secs < 14_400.0 {
                    duration_sum += secs * weight;
                    duration_weight_sum += weight;
                }
            }
        }

        // Itens na fila demonstram intenção explícita imediata do usuário.
        for (index, queued) in queue.videos().iter().enumerate() {
            let queue_weight = 1.6 / (1.0 + (index as f64) * 0.2);
            let channel_key = normalize_channel_key(&queued.channel);
            if !channel_key.is_empty() {
                *profile.channel_weights.entry(channel_key).or_insert(0.0) += queue_weight * 2.2;
            }
            for token in extract_significant_tokens(&queued.title) {
                *profile.token_weights.entry(token).or_insert(0.0) += queue_weight * 1.4;
            }
        }

        if let Some(current) = current_video {
            let key = normalize_channel_key(&current.channel);
            if !key.is_empty() {
                profile.active_channel = Some(key);
            }
            profile.active_tokens = extract_significant_tokens(&current.title)
                .into_iter()
                .collect();
        }

        if duration_weight_sum > 0.0 {
            profile.preferred_duration_secs = Some(duration_sum / duration_weight_sum);
        }

        profile
    }

    pub fn has_personal_signals(&self) -> bool {
        !self.channel_weights.is_empty()
            || !self.token_weights.is_empty()
            || self.active_channel.is_some()
    }

    pub fn top_topics(&self, limit: usize) -> Vec<String> {
        let mut items: Vec<(&String, &f64)> = self.token_weights.iter().collect();
        items.sort_by(|a, b| {
            b.1.partial_cmp(a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(b.0))
        });
        items
            .into_iter()
            .take(limit)
            .map(|(token, _)| capitalize_first(token))
            .collect()
    }
}

/// Motor de recomendação próprio do FastTube.
/// Combina afinidade de canal, relevância semântica de tópicos/hashtags,
/// frescor do envio (decay temporal), engajamento (views), adequação de duração,
/// penalidade de vídeos já concluídos e diversificação de canais.
pub struct RecommendationEngine;

impl RecommendationEngine {
    pub fn rank_videos(
        videos: &[Video],
        metadata_map: &HashMap<String, VideoMetadata>,
        profile: &UserProfile,
        filter: &FeedFilter,
    ) -> Vec<ScoredVideo> {
        if videos.is_empty() {
            return Vec::new();
        }

        let reference_epoch = latest_dataset_epoch(videos, metadata_map)
            .or_else(current_unix_epoch)
            .unwrap_or(1_762_728_000);

        let filtered: Vec<&Video> = videos
            .iter()
            .filter(|video| matches_filter(video, metadata_map.get(&video.url), profile, filter))
            .collect();

        if matches!(filter, FeedFilter::RecentlyUploaded) {
            let mut chronological: Vec<(Option<i64>, usize, ScoredVideo)> = filtered
                .into_iter()
                .enumerate()
                .map(|(idx, video)| {
                    let meta = metadata_map.get(&video.url);
                    let epoch = meta
                        .and_then(|m| m.upload_date.as_deref())
                        .and_then(parse_upload_date_epoch);
                    let scored = Self::score_single(video, meta, profile, reference_epoch, filter);
                    (epoch, idx, scored)
                })
                .collect();
            chronological.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            return chronological.into_iter().map(|(_, _, item)| item).collect();
        }

        let mut candidates: Vec<(usize, ScoredVideo)> = filtered
            .into_iter()
            .enumerate()
            .map(|(idx, video)| {
                let meta = metadata_map.get(&video.url);
                let mut scored = Self::score_single(video, meta, profile, reference_epoch, filter);
                // Preserva leve estabilidade de ordem original como desempate suave.
                scored.score -= (idx as f64) * 0.04;
                (idx, scored)
            })
            .collect();

        // Ordena inicialmente pelo score bruto.
        candidates.sort_by(|a, b| {
            b.1.score
                .partial_cmp(&a.1.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });

        // Passagem de diversificação estilo YouTube: evita que um único canal monopolize
        // diversas posições consecutivas no topo do feed.
        let mut ranked = Vec::with_capacity(candidates.len());
        let mut channel_appearances: HashMap<String, usize> = HashMap::new();
        let mut last_channel = String::new();

        while !candidates.is_empty() {
            let mut best_pos = 0;
            let mut best_effective_score = f64::NEG_INFINITY;

            let search_window = candidates.len().min(24);
            for (pos, (_, candidate)) in candidates.iter().enumerate().take(search_window) {
                let ch_key = normalize_channel_key(&candidate.video.channel);
                let count = if ch_key.is_empty() {
                    0
                } else {
                    *channel_appearances.get(&ch_key).unwrap_or(&0)
                };
                let repeat_penalty = (count as f64) * 6.5;
                let consecutive_penalty = if !ch_key.is_empty() && ch_key == last_channel {
                    14.0
                } else {
                    0.0
                };
                let effective = candidate.score - repeat_penalty - consecutive_penalty;
                if effective > best_effective_score {
                    best_effective_score = effective;
                    best_pos = pos;
                }
            }

            let (_, mut chosen) = candidates.remove(best_pos);
            chosen.score = best_effective_score;
            let ch_key = normalize_channel_key(&chosen.video.channel);
            if !ch_key.is_empty() {
                *channel_appearances.entry(ch_key.clone()).or_insert(0) += 1;
                last_channel = ch_key;
            }
            ranked.push(chosen);
        }

        ranked
    }

    pub fn score_single(
        video: &Video,
        metadata: Option<&VideoMetadata>,
        profile: &UserProfile,
        reference_epoch: i64,
        filter: &FeedFilter,
    ) -> ScoredVideo {
        let mut score = 20.0;
        let mut top_reason: Option<(&'static str, f64)> = None;

        let channel_key = normalize_channel_key(&video.channel);
        let video_tokens = extract_significant_tokens(&video.title);
        let is_short = is_short_video(video, metadata);

        // 1. Afinidade com o canal
        let channel_affinity = if !channel_key.is_empty() {
            profile
                .channel_weights
                .get(&channel_key)
                .copied()
                .unwrap_or(0.0)
        } else {
            0.0
        };
        if channel_affinity > 0.0 {
            let boost = (channel_affinity * 7.5).min(34.0);
            score += boost;
            update_reason(&mut top_reason, "Canal que você assiste", boost);
        }

        // 2. Afinidade por tópicos / palavras-chave / hashtags
        let mut token_match_sum = 0.0;
        for token in &video_tokens {
            if let Some(&w) = profile.token_weights.get(token) {
                token_match_sum += w;
            }
        }
        if let Some(desc) = metadata.and_then(|m| m.description.as_deref()) {
            for token in extract_significant_tokens(desc).into_iter().take(18) {
                if let Some(&w) = profile.token_weights.get(&token) {
                    token_match_sum += w * 0.25;
                }
            }
        }
        if token_match_sum > 0.0 {
            let boost = (token_match_sum * 5.2).min(28.0);
            score += boost;
            update_reason(&mut top_reason, "Recomendado para você", boost);
        }

        // 3. Contexto do vídeo em reprodução (para barra lateral / próximos vídeos)
        if let Some(active_ch) = &profile.active_channel {
            if !channel_key.is_empty() && &channel_key == active_ch {
                score += 15.0;
                update_reason(&mut top_reason, "Do mesmo canal", 15.0);
            }
        }
        if !profile.active_tokens.is_empty() {
            let shared = video_tokens
                .iter()
                .filter(|t| profile.active_tokens.contains(*t))
                .count();
            if shared > 0 {
                let boost = (shared as f64 * 6.5).min(20.0);
                score += boost;
                update_reason(&mut top_reason, "Relacionado", boost);
            }
        }

        // 4. Frescor do envio (Recency boost)
        if let Some(upload_raw) = metadata.and_then(|m| m.upload_date.as_deref()) {
            if let Some(upload_epoch) = parse_upload_date_epoch(upload_raw) {
                let age_hours = ((reference_epoch - upload_epoch).max(0) as f64) / 3600.0;
                let freshness = 32.0 * (-age_hours / 84.0).exp();
                score += freshness;
                if age_hours <= 12.0 {
                    update_reason(&mut top_reason, "Novo envio", freshness);
                }
            }
        }

        // 5. Sinal de popularidade / engajamento (logarítmico para não esmagar canais menores)
        if let Some(views) = metadata.and_then(|m| m.view_count) {
            if views > 0 {
                let pop_boost = ((views as f64 + 1.0).log10() * 2.2).min(13.0);
                score += pop_boost;
                if views >= 100_000 && top_reason.is_none() {
                    update_reason(&mut top_reason, "Em alta", pop_boost);
                }
            }
        }

        // 6. Adequação à duração preferida do usuário
        if let (Some(pref_secs), Some(video_secs)) = (
            profile.preferred_duration_secs,
            parse_duration_seconds(&video.duration),
        ) {
            let ratio = if pref_secs > video_secs {
                video_secs / pref_secs.max(1.0)
            } else {
                pref_secs / video_secs.max(1.0)
            };
            score += ratio * 5.0;
        }

        // 7. Estado de visualização no histórico local
        if let Some(video_id) = history::youtube_video_id(&video.url) {
            match profile.watched_states.get(&video_id) {
                Some(WatchedState::Completed) => {
                    // Demove vídeos já concluídos para priorizar conteúdo inédito.
                    score -= 48.0;
                }
                Some(WatchedState::InProgress(_)) => {
                    score += 14.0;
                    top_reason = Some(("Continuar assistindo", 99.0));
                }
                None => {}
            }
        }

        // 8. Ajustes específicos por modo de filtro
        match filter {
            FeedFilter::NewToYou => {
                if channel_affinity == 0.0 {
                    score += 22.0;
                    top_reason = Some(("Novidade para você", 30.0));
                } else {
                    score -= channel_affinity * 5.0;
                }
            }
            FeedFilter::Recommended => {
                if top_reason.is_none() && profile.has_personal_signals() {
                    top_reason = Some(("Para você", 10.0));
                }
            }
            _ => {}
        }

        ScoredVideo {
            video: video.clone(),
            metadata: metadata.cloned().unwrap_or_default(),
            score,
            reason: top_reason.map(|(label, _)| label.to_owned()),
            is_short,
        }
    }

    /// Constrói a lista dinâmica de chips de filtro estilo YouTube para os vídeos carregados.
    pub fn available_chips(
        videos: &[Video],
        metadata_map: &HashMap<String, VideoMetadata>,
        profile: &UserProfile,
    ) -> Vec<FeedFilter> {
        let mut chips = vec![
            FeedFilter::All,
            FeedFilter::Recommended,
            FeedFilter::Music,
            FeedFilter::Podcasts,
            FeedFilter::Gaming,
        ];

        // Tópicos personalizados extraídos do perfil ou dos títulos atuais
        let mut dynamic_topics = profile.top_topics(2);
        if dynamic_topics.is_empty() {
            for topic in extract_trending_feed_topics(videos, 2) {
                if !dynamic_topics.contains(&topic) {
                    dynamic_topics.push(topic);
                }
            }
        }
        for topic in dynamic_topics {
            let filter = FeedFilter::Topic(topic);
            if !chips.contains(&filter) {
                chips.push(filter);
            }
        }

        if videos
            .iter()
            .any(|v| is_live_video(v, metadata_map.get(&v.url)))
        {
            chips.push(FeedFilter::Live);
        }
        chips.push(FeedFilter::Mixes);
        if videos
            .iter()
            .any(|v| is_short_video(v, metadata_map.get(&v.url)))
        {
            chips.push(FeedFilter::Shorts);
        }
        chips.push(FeedFilter::RecentlyUploaded);
        chips.push(FeedFilter::NewToYou);

        chips
    }

    /// Gera uma consulta de descoberta personalizada baseada no histórico e fila do usuário.
    pub fn build_discovery_query(profile: &UserProfile, filter: &FeedFilter) -> String {
        match filter {
            FeedFilter::Music => "música clipe ao vivo oficial".into(),
            FeedFilter::Podcasts => "podcast entrevista episódio completo".into(),
            FeedFilter::Gaming => "gameplay jogos brasil".into(),
            FeedFilter::Live => "ao vivo live stream".into(),
            FeedFilter::Mixes => "mix música playlist".into(),
            FeedFilter::Shorts => "#shorts brasil".into(),
            FeedFilter::Topic(topic) => topic.clone(),
            FeedFilter::Channel(channel) => channel.clone(),
            FeedFilter::All
            | FeedFilter::Recommended
            | FeedFilter::RecentlyUploaded
            | FeedFilter::NewToYou => {
                let topics = profile.top_topics(3);
                if !topics.is_empty() {
                    topics.join(" ")
                } else {
                    "recomendados brasil em alta".into()
                }
            }
        }
    }
}

fn update_reason(current: &mut Option<(&'static str, f64)>, label: &'static str, weight: f64) {
    if current.is_none_or(|(_, best)| weight > best) {
        *current = Some((label, weight));
    }
}

pub fn matches_filter(
    video: &Video,
    metadata: Option<&VideoMetadata>,
    profile: &UserProfile,
    filter: &FeedFilter,
) -> bool {
    match filter {
        FeedFilter::All | FeedFilter::Recommended | FeedFilter::RecentlyUploaded => true,
        FeedFilter::Shorts => is_short_video(video, metadata),
        FeedFilter::Music => is_music_video(video, metadata),
        FeedFilter::Podcasts => is_podcast_video(video, metadata),
        FeedFilter::Gaming => is_gaming_video(video, metadata),
        FeedFilter::Live => is_live_video(video, metadata),
        FeedFilter::Mixes => is_music_video(video, metadata) || is_podcast_video(video, metadata),
        FeedFilter::NewToYou => {
            if let Some(id) = history::youtube_video_id(&video.url) {
                if profile.watched_states.contains_key(&id) {
                    return false;
                }
            }
            let ch_key = normalize_channel_key(&video.channel);
            let ch_weight = profile.channel_weights.get(&ch_key).copied().unwrap_or(0.0);
            ch_weight < 3.5
        }
        FeedFilter::Topic(topic) => {
            let needle = topic.to_lowercase();
            video.title.to_lowercase().contains(&needle)
                || video.channel.to_lowercase().contains(&needle)
                || metadata
                    .and_then(|m| m.description.as_deref())
                    .is_some_and(|d| d.to_lowercase().contains(&needle))
        }
        FeedFilter::Channel(channel) => {
            normalize_channel_key(&video.channel) == normalize_channel_key(channel)
        }
    }
}

/// Detecta se um vídeo deve ser tratado como YouTube Short (formato vertical / curta duração).
pub fn is_short_video(video: &Video, metadata: Option<&VideoMetadata>) -> bool {
    let url_lower = video.url.to_lowercase();
    if url_lower.contains("/shorts/") {
        return true;
    }
    let title_lower = video.title.to_lowercase();
    if title_lower.contains("#shorts")
        || title_lower.contains("#short")
        || title_lower.contains("#meme")
        || title_lower.contains("#humorbr")
        || title_lower.contains("#memeviral")
        || title_lower.contains("#psiutourette")
    {
        return true;
    }
    if let Some(desc) = metadata.and_then(|m| m.description.as_deref()) {
        let desc_lower = desc.to_lowercase();
        if desc_lower.contains("#shorts") || desc_lower.contains("#short") {
            return true;
        }
    }
    if let Some(secs) = parse_duration_seconds(&video.duration) {
        if secs > 0.0 && secs <= 62.0 {
            return true;
        }
    }
    false
}

pub fn is_music_video(video: &Video, metadata: Option<&VideoMetadata>) -> bool {
    let text = combined_searchable_text(video, metadata);
    const KEYWORDS: &[&str] = &[
        "music",
        "música",
        "musica",
        "song",
        "clipe",
        "official video",
        "live from",
        "vevo",
        "acoustic",
        "acústico",
        "violão",
        "guitar",
        "cover",
        "lyrics",
        "letra",
        "album",
        "álbum",
        "feat",
        "ft.",
        "banda",
        "remix",
        "soundtrack",
    ];
    KEYWORDS.iter().any(|kw| text.contains(kw))
}

pub fn is_podcast_video(video: &Video, metadata: Option<&VideoMetadata>) -> bool {
    let text = combined_searchable_text(video, metadata);
    const KEYWORDS: &[&str] = &[
        "podcast",
        "cortes",
        "entrevista",
        "episódio",
        "episodio",
        "papo",
        "conversa",
        "achismos",
        "flow",
        "inteligência ltda",
        "podpah",
        "monark",
        "debate",
    ];
    if KEYWORDS.iter().any(|kw| text.contains(kw)) {
        return true;
    }
    if let Some(secs) = parse_duration_seconds(&video.duration) {
        if secs >= 1500.0 {
            return true;
        }
    }
    false
}

pub fn is_gaming_video(video: &Video, metadata: Option<&VideoMetadata>) -> bool {
    let text = combined_searchable_text(video, metadata);
    const KEYWORDS: &[&str] = &[
        "gameplay",
        "jogo",
        "jogos",
        "game",
        "gaming",
        "dota",
        "minecraft",
        "valorant",
        "cs2",
        "fortnite",
        "gta",
        "elden ring",
        "league of legends",
        "speedrun",
        "boss",
        "playstation",
        "xbox",
        "nintendo",
        "steam",
        "gamer",
    ];
    KEYWORDS.iter().any(|kw| text.contains(kw))
}

pub fn is_live_video(video: &Video, metadata: Option<&VideoMetadata>) -> bool {
    let dur = video.duration.trim().to_lowercase();
    if dur == "live" || dur == "ao vivo" {
        return true;
    }
    let text = combined_searchable_text(video, metadata);
    text.contains("ao vivo") || text.contains("(live") || text.contains("[live")
}

fn combined_searchable_text(video: &Video, metadata: Option<&VideoMetadata>) -> String {
    let mut text = format!(
        "{} {}",
        video.title.to_lowercase(),
        video.channel.to_lowercase()
    );
    if let Some(desc) = metadata.and_then(|m| m.description.as_deref()) {
        text.push(' ');
        text.push_str(&desc.to_lowercase());
    }
    text
}

pub fn normalize_channel_key(channel: &str) -> String {
    channel.trim().to_lowercase()
}

pub fn extract_significant_tokens(text: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "de", "do", "da", "dos", "das", "em", "no", "na", "nos", "nas", "um", "uma", "uns", "umas",
        "para", "por", "com", "sem", "sob", "sobre", "que", "como", "mais", "mas", "ou", "se",
        "ao", "aos", "as", "os", "eu", "tu", "ele", "ela", "nós", "eles", "elas", "meu", "minha",
        "seu", "sua", "isso", "isto", "aquilo", "este", "esta", "esse", "essa", "foi", "ser",
        "está", "esta", "são", "era", "tem", "ter", "vai", "vou", "muito", "todo", "toda", "todos",
        "todas", "não", "nao", "sim", "já", "até", "ate", "quando", "onde", "porque", "porquê",
        "the", "and", "for", "with", "from", "that", "this", "you", "your", "are", "was", "were",
        "have", "has", "had", "not", "but", "what", "all", "can", "out", "about", "who", "get",
        "make", "like", "time", "just", "know", "video", "vídeo", "canal", "oficial", "official",
    ];

    let mut tokens = Vec::new();
    for raw_word in text.split(|c: char| !c.is_alphanumeric() && c != '#' && c != '@') {
        let trimmed = raw_word.trim_matches(|c: char| c == '#' || c == '@');
        let lower = trimmed.to_lowercase();
        if lower.chars().count() < 3 {
            continue;
        }
        if lower.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if STOPWORDS.contains(&lower.as_str()) {
            continue;
        }
        if !tokens.contains(&lower) {
            tokens.push(lower);
        }
    }
    tokens
}

fn extract_trending_feed_topics(videos: &[Video], limit: usize) -> Vec<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for video in videos {
        for token in extract_significant_tokens(&video.title) {
            if token != "shorts" && token != "humorbr" && token != "memeviral" {
                *counts.entry(token).or_insert(0) += 1;
            }
        }
    }
    let mut pairs: Vec<(String, usize)> = counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    pairs
        .into_iter()
        .take(limit)
        .map(|(token, _)| capitalize_first(&token))
        .collect()
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

pub fn parse_duration_seconds(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parts: Vec<&str> = trimmed.split(':').collect();
    match parts.as_slice() {
        [mins, secs] => {
            let m: u64 = mins.parse().ok()?;
            let s: u64 = secs.parse().ok()?;
            Some((m * 60 + s) as f64)
        }
        [hours, mins, secs] => {
            let h: u64 = hours.parse().ok()?;
            let m: u64 = mins.parse().ok()?;
            let s: u64 = secs.parse().ok()?;
            Some((h * 3600 + m * 60 + s) as f64)
        }
        _ => None,
    }
}

/// Converte datas `YYYY-MM-DDTHH:MM:SSZ`, `YYYY-MM-DD` ou `YYYYMMDD` para Unix timestamp (segundos).
pub fn parse_upload_date_epoch(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if trimmed.len() == 8 && trimmed.bytes().all(|b| b.is_ascii_digit()) {
        let year: i32 = trimmed[0..4].parse().ok()?;
        let month: u32 = trimmed[4..6].parse().ok()?;
        let day: u32 = trimmed[6..8].parse().ok()?;
        return ymd_hms_to_epoch(year, month, day, 12, 0, 0);
    }
    if trimmed.len() >= 10 {
        let date_part = &trimmed[0..10];
        let mut date_iter = date_part.split('-');
        let year: i32 = date_iter.next()?.parse().ok()?;
        let month: u32 = date_iter.next()?.parse().ok()?;
        let day: u32 = date_iter.next()?.parse().ok()?;
        let (mut hour, mut min, mut sec) = (0u32, 0u32, 0u32);
        if let Some(time_part) = trimmed.get(11..) {
            let clean_time = time_part
                .trim_end_matches('Z')
                .split(['+', '-', '.'])
                .next()
                .unwrap_or("");
            let mut t_iter = clean_time.split(':');
            if let (Some(h), Some(m)) = (t_iter.next(), t_iter.next()) {
                hour = h.parse().unwrap_or(0);
                min = m.parse().unwrap_or(0);
                sec = t_iter.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            }
        }
        return ymd_hms_to_epoch(year, month, day, hour, min, sec);
    }
    None
}

fn ymd_hms_to_epoch(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 59 {
        return None;
    }
    // Algoritmo civil_from_days (Howard Hinnant)
    let y = year as i64 - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let m = month;
    let d = day;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + (doe as i64) - 719468;
    Some(days * 86_400 + (hour as i64) * 3_600 + (min as i64) * 60 + (sec as i64))
}

fn current_unix_epoch() -> Option<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

pub fn latest_dataset_epoch(
    videos: &[Video],
    metadata_map: &HashMap<String, VideoMetadata>,
) -> Option<i64> {
    videos
        .iter()
        .filter_map(|v| metadata_map.get(&v.url))
        .filter_map(|m| m.upload_date.as_deref())
        .filter_map(parse_upload_date_epoch)
        .max()
}

/// Formata data de publicação no estilo compacto do YouTube Brasil (`há 4 h`, `há 4 dias`, etc.).
pub fn format_youtube_relative_time(raw: &str, reference_epoch: Option<i64>) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let Some(upload_epoch) = parse_upload_date_epoch(trimmed) else {
        return Some(trimmed.to_owned());
    };
    let now_epoch = reference_epoch
        .or_else(current_unix_epoch)
        .unwrap_or(upload_epoch);
    let diff_secs = (now_epoch - upload_epoch).max(0);

    if diff_secs < 60 {
        Some("agora mesmo".into())
    } else if diff_secs < 3_600 {
        let mins = (diff_secs / 60).max(1);
        Some(format!("há {mins} min"))
    } else if diff_secs < 86_400 {
        let hours = (diff_secs / 3_600).max(1);
        Some(format!("há {hours} h"))
    } else if diff_secs < 86_400 * 30 {
        let days = (diff_secs / 86_400).max(1);
        if days == 1 {
            Some("há 1 dia".into())
        } else {
            Some(format!("há {days} dias"))
        }
    } else if diff_secs < 86_400 * 365 {
        let months = (diff_secs / (86_400 * 30)).max(1);
        if months == 1 {
            Some("há 1 mês".into())
        } else {
            Some(format!("há {months} meses"))
        }
    } else {
        let years = (diff_secs / (86_400 * 365)).max(1);
        if years == 1 {
            Some("há 1 ano".into())
        } else {
            Some(format!("há {years} anos"))
        }
    }
}

/// Formata visualizações no estilo compacto do YouTube Brasil (`32 mil`, `1,4 mi`).
pub fn format_youtube_compact_views(count: u64) -> String {
    if count >= 1_000_000 {
        let millions = count as f64 / 1_000_000.0;
        if millions >= 10.0 || (count % 1_000_000) < 100_000 {
            format!("{} mi", count / 1_000_000)
        } else {
            format!("{:.1} mi", millions).replace('.', ",")
        }
    } else if count >= 1_000 {
        format!("{} mil", count / 1_000)
    } else if count == 1 {
        "1 visualização".into()
    } else {
        format!("{count} visualizações")
    }
}

/// Gera uma cor determinística vibrante para o avatar circular de um canal.
pub fn channel_avatar_rgb(channel_name: &str) -> [u8; 3] {
    const PALETTE: &[[u8; 3]] = &[
        [211, 47, 47],
        [25, 118, 210],
        [56, 142, 60],
        [123, 31, 162],
        [245, 124, 0],
        [0, 131, 143],
        [194, 24, 91],
        [69, 90, 100],
        [93, 64, 55],
        [48, 63, 159],
    ];
    let mut hash: usize = 5381;
    for byte in channel_name.trim().as_bytes() {
        hash = hash
            .wrapping_mul(33)
            .wrapping_add(usize::from(byte.to_ascii_lowercase()));
    }
    PALETTE[hash % PALETTE.len()]
}

/// Extrai 1 ou 2 iniciais maiúsculas para o avatar circular do canal.
pub fn channel_avatar_initials(channel_name: &str) -> String {
    let words: Vec<&str> = channel_name
        .trim()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() >= 2 {
        let c1 = words[0].chars().next().unwrap_or('Y');
        let c2 = words[1].chars().next().unwrap_or('T');
        format!("{}{}", c1.to_uppercase(), c2.to_uppercase())
    } else if let Some(first_word) = words.first() {
        first_word
            .chars()
            .take(2)
            .collect::<String>()
            .to_uppercase()
    } else {
        "FT".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_video(id: &str, title: &str, channel: &str, duration: &str) -> Video {
        Video {
            title: title.into(),
            channel: channel.into(),
            duration: duration.into(),
            url: format!("https://www.youtube.com/watch?v={id}"),
            thumbnail: String::new(),
        }
    }

    #[test]
    fn tied_topic_weights_have_stable_alphabetical_order_in_chips() {
        let profile = UserProfile {
            token_weights: HashMap::from([
                ("zebra".to_owned(), 1.0),
                ("alpha".to_owned(), 1.0),
                ("beta".to_owned(), 1.0),
            ]),
            ..UserProfile::default()
        };
        let expected = vec!["Alpha".to_owned(), "Beta".to_owned()];
        for _ in 0..20 {
            assert_eq!(profile.top_topics(2), expected);
            let chips = RecommendationEngine::available_chips(&[], &HashMap::new(), &profile);
            let topics: Vec<String> = chips
                .into_iter()
                .filter_map(|chip| match chip {
                    FeedFilter::Topic(topic) => Some(topic),
                    _ => None,
                })
                .collect();
            assert_eq!(topics, expected);
        }
    }

    #[test]
    fn user_profile_prioritizes_watched_and_queued_channels_and_topics() {
        let mut history = History::default();
        history
            .record(HistoryEntry {
                video_id: "vidwatched1".into(),
                title: "Tutorial Rust Avançado Async Tokio".into(),
                channel: "Canal Rust Brasil".into(),
                position_ms: 540_000,
                duration_ms: Some(600_000),
            })
            .unwrap();
        let mut queue = Queue::new(10);
        let _ = queue.add(sample_video(
            "vidqueued02",
            "Dota 2 Gameplay Pro",
            "AD0LFzZ",
            "12:00",
        ));

        let profile = UserProfile::build(&history, &queue, None);
        assert!(profile.has_personal_signals());
        assert!(
            profile
                .channel_weights
                .get("canal rust brasil")
                .copied()
                .unwrap_or(0.0)
                > 1.0
        );
        assert!(
            profile
                .channel_weights
                .get("ad0lfzz")
                .copied()
                .unwrap_or(0.0)
                > 1.0
        );
        assert!(profile.token_weights.contains_key("rust"));
        assert!(profile.token_weights.contains_key("tokio"));
        assert!(profile.token_weights.contains_key("dota"));
    }

    #[test]
    fn recommendation_engine_demotes_completed_videos_and_boosts_matching_unwatched() {
        let mut history = History::default();
        history
            .record(HistoryEntry {
                video_id: "completed11".into(),
                title: "Aprenda Rust do Zero".into(),
                channel: "RustDev".into(),
                position_ms: 595_000,
                duration_ms: Some(600_000),
            })
            .unwrap();
        let queue = Queue::new(10);
        let profile = UserProfile::build(&history, &queue, None);

        let v_completed = sample_video("completed11", "Aprenda Rust do Zero", "RustDev", "10:00");
        let v_fresh_match = sample_video(
            "freshmatch2",
            "Rust Performance e Memória",
            "RustDev",
            "11:00",
        );
        let v_unrelated = sample_video("unrelated03", "Receita de Bolo Simples", "Cozinha", "8:00");

        let videos = vec![v_completed, v_unrelated, v_fresh_match];
        let metadata = HashMap::new();
        let ranked = RecommendationEngine::rank_videos(
            &videos,
            &metadata,
            &profile,
            &FeedFilter::Recommended,
        );

        assert_eq!(ranked.len(), 3);
        assert_eq!(
            ranked[0].video.url,
            "https://www.youtube.com/watch?v=freshmatch2"
        );
        assert_eq!(
            ranked[2].video.url,
            "https://www.youtube.com/watch?v=completed11"
        );
    }

    #[test]
    fn diversity_pass_prevents_same_channel_from_monopolizing_consecutive_top_slots() {
        let history = History::default();
        let queue = Queue::new(10);
        let profile = UserProfile::build(&history, &queue, None);

        let videos = vec![
            sample_video("ch1_v1____1", "Vídeo 1", "Canal A", "10:00"),
            sample_video("ch1_v2____2", "Vídeo 2", "Canal A", "10:00"),
            sample_video("ch1_v3____3", "Vídeo 3", "Canal A", "10:00"),
            sample_video("ch2_v1____4", "Vídeo B1", "Canal B", "10:00"),
        ];
        let ranked =
            RecommendationEngine::rank_videos(&videos, &HashMap::new(), &profile, &FeedFilter::All);
        assert_eq!(ranked[0].video.channel, "Canal A");
        assert_eq!(ranked[1].video.channel, "Canal B");
    }

    #[test]
    fn relative_time_and_compact_views_format_like_youtube_br() {
        let ref_epoch = parse_upload_date_epoch("2026-11-09T23:30:07Z").unwrap();
        assert_eq!(
            format_youtube_relative_time("2026-11-09T19:30:07Z", Some(ref_epoch)).as_deref(),
            Some("há 4 h")
        );
        assert_eq!(
            format_youtube_relative_time("2026-11-05T23:30:07Z", Some(ref_epoch)).as_deref(),
            Some("há 4 dias")
        );
        assert_eq!(format_youtube_compact_views(32_400), "32 mil");
        assert_eq!(format_youtube_compact_views(1_500_000), "1,5 mi");
    }

    #[test]
    fn shorts_detection_identifies_short_duration_and_hashtags() {
        let short_by_tag = sample_video(
            "shorttag001",
            "O zumbi não esperava #zumbi #humor #animação #shorts",
            "5 Alguma Coisa",
            "0:45",
        );
        let regular = sample_video(
            "regular0002",
            "fui no podcast no monark | luly vlog",
            "Luly",
            "8:01",
        );
        assert!(is_short_video(&short_by_tag, None));
        assert!(!is_short_video(&regular, None));
    }
}
