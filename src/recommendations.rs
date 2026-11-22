use crate::history::{self, History};
use crate::queue::Queue;
use crate::search::{Video, VideoMetadata};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

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

#[derive(Clone, Debug, Default)]
pub struct UserProfile {
    pub channel_weights: HashMap<String, f64>,
    pub token_weights: HashMap<String, f64>,
    pub preferred_duration_secs: Option<f64>,
    watched_states: HashMap<String, WatchedState>,
    pub active_channel: Option<String>,
    pub active_tokens: HashSet<String>,
    pub muted_topics: HashSet<String>,
    pub muted_channels: HashSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecommendationReason {
    ContinueWatching,
    FrequentChannel,
    SimilarTopic(String),
    RelatedToCurrent,
    RecentFeed,
    NewToYou,
    Popular,
    PlaylistMix,
}

#[cfg(test)]
impl RecommendationReason {
    pub fn badge_label(&self) -> &'static str {
        match self {
            Self::ContinueWatching => "Continuar assistindo",
            Self::FrequentChannel => "Canal frequente",
            Self::SimilarTopic(_) => "Tópico semelhante",
            Self::RelatedToCurrent => "Relacionado ao atual",
            Self::RecentFeed => "Novo no feed",
            Self::NewToYou => "Novo canal para você",
            Self::Popular => "Em alta",
            Self::PlaylistMix => "Mix recomendado",
        }
    }

    pub fn detail_label(&self) -> String {
        match self {
            Self::SimilarTopic(topic) if !topic.is_empty() => {
                format!("Tópico semelhante: {topic}")
            }
            _ => self.badge_label().to_owned(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScoredVideo {
    pub video: Video,
    pub metadata: VideoMetadata,
    pub score: f64,
    pub is_short: bool,
    #[allow(dead_code)]
    pub reasons: Vec<RecommendationReason>,
    pub matched_topic: Option<String>,
}

impl ScoredVideo {
    #[cfg(test)]
    pub fn primary_reason_label(&self) -> Option<&'static str> {
        self.reasons.first().map(RecommendationReason::badge_label)
    }

    #[cfg(test)]
    pub fn explanation_tooltip(&self) -> String {
        if self.reasons.is_empty() {
            return "Por que este vídeo?\n• Sugestão geral do feed".into();
        }
        let mut lines = Vec::with_capacity(self.reasons.len() + 1);
        lines.push("Por que este vídeo?".to_owned());
        for reason in self.reasons.iter().take(4) {
            lines.push(format!("• {}", reason.detail_label()));
        }
        lines.join("\n")
    }

    pub fn primary_topic_hint(&self) -> Option<String> {
        if let Some(topic) = &self.matched_topic
            && !topic.trim().is_empty()
        {
            return Some(topic.clone());
        }
        extract_significant_tokens(&self.video.title)
            .into_iter()
            .find(|tok| tok != "shorts")
            .map(|tok| capitalize_first(&tok))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HomeQueryPlan {
    pub video_queries: Vec<String>,
    pub channel_targets: Vec<crate::search::ChannelTarget>,
    pub playlist_query: Option<String>,
    pub shorts_query: Option<String>,
}

impl UserProfile {
    pub fn build(history: &History, queue: &Queue, current_video: Option<&Video>) -> Self {
        let mut profile = Self::default();
        let mut duration_sum = 0.0;
        let mut duration_weight_sum = 0.0;

        for (index, entry) in history.entries().iter().enumerate() {
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

    #[cfg(test)]
    pub fn has_personal_signals(&self) -> bool {
        !self.channel_weights.is_empty()
            || !self.token_weights.is_empty()
            || self.active_channel.is_some()
    }

    pub fn top_topics(&self, limit: usize) -> Vec<String> {
        let mut items: Vec<(&String, &f64)> = self
            .token_weights
            .iter()
            .filter(|(token, _)| !self.muted_topics.contains(token.as_str()))
            .collect();
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

    pub fn apply_muted_topics(&mut self, muted_topics: &[String]) {
        for raw in muted_topics {
            let key = raw.trim().to_lowercase();
            if key.is_empty() {
                continue;
            }
            self.muted_topics.insert(key.clone());
            self.token_weights.remove(&key);
            self.active_tokens.remove(&key);
        }
    }

    pub fn apply_muted_channels(&mut self, muted_channels: &[String]) {
        for raw in muted_channels {
            let key = normalize_channel_key(raw);
            if key.is_empty() {
                continue;
            }
            self.muted_channels.insert(key.clone());
            self.channel_weights.remove(&key);
            if self.active_channel.as_deref() == Some(key.as_str()) {
                self.active_channel = None;
            }
        }
    }
}

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
                scored.score -= (idx as f64) * 0.04;
                (idx, scored)
            })
            .collect();

        candidates.sort_by(|a, b| {
            b.1.score
                .partial_cmp(&a.1.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });

        // Diversify top slots so a single channel does not dominate consecutive entries.
        let mut channel_interner: HashMap<String, usize> = HashMap::new();
        let mut slots: Vec<Option<(ScoredVideo, Option<usize>)>> = candidates
            .into_iter()
            .map(|(_, scored)| {
                let ch_key = normalize_channel_key(&scored.video.channel);
                let ch_id = if ch_key.is_empty() {
                    None
                } else if let Some(&id) = channel_interner.get(&ch_key) {
                    Some(id)
                } else {
                    let id = channel_interner.len();
                    channel_interner.insert(ch_key, id);
                    Some(id)
                };
                Some((scored, ch_id))
            })
            .collect();

        let order =
            diversify_windowed_indices(slots.len(), channel_interner.len(), 6.5, 14.0, |idx| {
                let (scored, ch_id) = slots[idx].as_ref().expect("unconsumed candidate slot");
                (scored.score, *ch_id)
            });

        let mut ranked = Vec::with_capacity(order.len());
        for (idx, effective_score) in order {
            let (mut chosen, _) = slots[idx].take().expect("chosen candidate slot");
            chosen.score = effective_score;
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
        let (score, is_short, reasons, matched_topic) =
            Self::score_single_parts(video, metadata, profile, reference_epoch, filter);
        ScoredVideo {
            video: video.clone(),
            metadata: metadata.cloned().unwrap_or_default(),
            score,
            is_short,
            reasons,
            matched_topic,
        }
    }

    fn score_single_parts(
        video: &Video,
        metadata: Option<&VideoMetadata>,
        profile: &UserProfile,
        reference_epoch: i64,
        filter: &FeedFilter,
    ) -> (f64, bool, Vec<RecommendationReason>, Option<String>) {
        let mut score = 20.0;
        let mut reasons: Vec<RecommendationReason> = Vec::new();

        let channel_key = normalize_channel_key(&video.channel);
        let video_tokens = extract_significant_tokens(&video.title);
        let is_short = is_short_video(video, metadata);

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
        }

        let mut token_match_sum = 0.0;
        let mut best_matched_token: Option<(String, f64)> = None;
        let mut matches_muted_topic = false;
        for token in &video_tokens {
            if profile.muted_topics.contains(token.as_str()) {
                matches_muted_topic = true;
            } else if let Some(&w) = profile.token_weights.get(token) {
                token_match_sum += w;
                if best_matched_token.as_ref().is_none_or(|(_, bw)| w > *bw) {
                    best_matched_token = Some((token.clone(), w));
                }
            }
        }
        if !matches_muted_topic && !profile.muted_topics.is_empty() {
            matches_muted_topic = profile.muted_topics.iter().any(|muted| {
                contains_ignore_case(&video.title, muted)
                    || contains_ignore_case(&video.channel, muted)
            });
        }
        if let Some(desc) = metadata.and_then(|m| m.description.as_deref()) {
            for token in extract_significant_tokens(desc).into_iter().take(18) {
                if !profile.muted_topics.contains(token.as_str())
                    && let Some(&w) = profile.token_weights.get(&token)
                {
                    token_match_sum += w * 0.25;
                    if best_matched_token
                        .as_ref()
                        .is_none_or(|(_, bw)| w * 0.25 > *bw)
                    {
                        best_matched_token = Some((token, w * 0.25));
                    }
                }
            }
        }
        if token_match_sum > 0.0 {
            let boost = (token_match_sum * 5.2).min(28.0);
            score += boost;
        }

        let matched_topic = best_matched_token
            .as_ref()
            .map(|(tok, _)| capitalize_first(tok))
            .or_else(|| {
                video_tokens
                    .iter()
                    .find(|tok| {
                        tok.as_str() != "shorts" && !profile.muted_topics.contains(tok.as_str())
                    })
                    .map(|tok| capitalize_first(tok))
            });

        let active_channel_match = profile
            .active_channel
            .as_ref()
            .is_some_and(|active_ch| !channel_key.is_empty() && &channel_key == active_ch);
        if active_channel_match {
            score += 15.0;
        }
        let mut active_shared = 0usize;
        if !profile.active_tokens.is_empty() {
            active_shared = video_tokens
                .iter()
                .filter(|t| profile.active_tokens.contains(*t))
                .count();
            if active_shared > 0 {
                let boost = (active_shared as f64 * 6.5).min(20.0);
                score += boost;
            }
        }

        let mut freshness_boost = 0.0;
        if let Some(upload_raw) = metadata.and_then(|m| m.upload_date.as_deref())
            && let Some(upload_epoch) = parse_upload_date_epoch(upload_raw)
        {
            let age_hours = ((reference_epoch - upload_epoch).max(0) as f64) / 3600.0;
            freshness_boost = 32.0 * (-age_hours / 84.0).exp();
            score += freshness_boost;
        }

        let mut pop_boost = 0.0;
        if let Some(views) = metadata.and_then(|m| m.view_count)
            && views > 0
        {
            pop_boost = ((views as f64 + 1.0).log10() * 2.2).min(13.0);
            score += pop_boost;
        }

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

        let mut in_progress = false;
        if let Some(video_id) = history::youtube_video_id(&video.url) {
            match profile.watched_states.get(&video_id) {
                Some(WatchedState::Completed) => {
                    score -= 48.0;
                }
                Some(WatchedState::InProgress(_)) => {
                    score += 14.0;
                    in_progress = true;
                }
                None => {}
            }
        }

        if matches!(filter, FeedFilter::NewToYou) {
            if channel_affinity == 0.0 {
                score += 22.0;
            } else {
                score -= channel_affinity * 5.0;
            }
        }
        if matches!(filter, FeedFilter::All) && video.url.contains("playlist?list=") {
            score += 6.5;
        }
        if matches_muted_topic && !matches!(filter, FeedFilter::Topic(_)) {
            score -= 120.0;
        }

        if in_progress {
            reasons.push(RecommendationReason::ContinueWatching);
        }
        if channel_affinity > 0.0 {
            reasons.push(RecommendationReason::FrequentChannel);
        }
        if let Some((topic_tok, _)) = &best_matched_token
            && token_match_sum > 0.0
        {
            reasons.push(RecommendationReason::SimilarTopic(capitalize_first(
                topic_tok,
            )));
        }
        if active_channel_match || active_shared > 0 {
            reasons.push(RecommendationReason::RelatedToCurrent);
        }
        if freshness_boost >= 10.0 {
            reasons.push(RecommendationReason::RecentFeed);
        }
        if matches!(filter, FeedFilter::NewToYou) && channel_affinity == 0.0 {
            reasons.push(RecommendationReason::NewToYou);
        }
        if video.url.contains("playlist?list=") {
            reasons.push(RecommendationReason::PlaylistMix);
        }
        if pop_boost >= 10.0 {
            reasons.push(RecommendationReason::Popular);
        }
        if reasons.is_empty() {
            reasons.push(RecommendationReason::RecentFeed);
        }

        (score, is_short, reasons, matched_topic)
    }

    #[allow(dead_code)]
    pub fn available_chips(
        videos: &[Video],
        metadata_map: &HashMap<String, VideoMetadata>,
        profile: &UserProfile,
    ) -> Vec<FeedFilter> {
        Self::available_chips_with_pinned(videos, metadata_map, profile, &[])
    }

    pub fn available_chips_with_pinned(
        videos: &[Video],
        metadata_map: &HashMap<String, VideoMetadata>,
        profile: &UserProfile,
        pinned_topics: &[String],
    ) -> Vec<FeedFilter> {
        let mut chips = vec![FeedFilter::All];
        let mut seen_topic_keys: HashSet<String> = HashSet::new();

        for raw in pinned_topics {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let key = trimmed.to_lowercase();
            if seen_topic_keys.insert(key) {
                chips.push(FeedFilter::Topic(trimmed.to_owned()));
            }
        }

        for base in [
            FeedFilter::Gaming,
            FeedFilter::Podcasts,
            FeedFilter::Music,
            FeedFilter::Mixes,
        ] {
            chips.push(base);
        }

        let mut dynamic_topics = profile.top_topics(2);
        if dynamic_topics.is_empty() {
            for topic in extract_trending_feed_topics(videos, 2) {
                if !dynamic_topics.contains(&topic) {
                    dynamic_topics.push(topic);
                }
            }
        }
        for topic in dynamic_topics {
            let key = topic.trim().to_lowercase();
            if !seen_topic_keys.insert(key) {
                continue;
            }
            let filter = FeedFilter::Topic(topic);
            if !chips.contains(&filter) {
                chips.push(filter);
            }
        }

        chips.push(FeedFilter::Live);
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

    pub fn build_home_query_plan(
        profile: &UserProfile,
        subscribed_channels: &[crate::search::HomeChannel],
        filter: &FeedFilter,
        page: usize,
    ) -> HomeQueryPlan {
        const GENERAL_DISCOVERY_POOL: &[&str] = &[
            "brasil jogos tecnologia podcast",
            "entretenimento brasil música cortes",
            "gameplay humor brasil novidades",
            "ciência história cinema análise",
            "programação engenharia dev brasil",
            "esportes futebol highlights brasil",
            "música ao vivo acústico brasil",
            "documentário curiosidades brasil",
        ];
        const PLAYLIST_POOL: &[&str] = &[
            "mix rock indie brasil",
            "mix música brasil playlist",
            "mix lofi hip hop estudo",
            "playlist podcast tecnologia",
        ];
        const SHORTS_POOL: &[&str] = &[
            "#shorts brasil humor",
            "#shorts gaming memes",
            "#shorts tecnologia curiosidades",
            "#shorts música highlights",
        ];

        let mut plan = HomeQueryPlan::default();
        let top_topics = profile.top_topics(6);

        let mut ranked_channels: Vec<(&String, &f64)> = profile.channel_weights.iter().collect();
        ranked_channels.sort_by(|a, b| {
            b.1.partial_cmp(a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(b.0))
        });

        match filter {
            FeedFilter::All
            | FeedFilter::Recommended
            | FeedFilter::RecentlyUploaded
            | FeedFilter::NewToYou => {
                if !top_topics.is_empty() {
                    let start = (page * 2) % top_topics.len();
                    let q1 = (0..top_topics.len().min(2))
                        .map(|offset| top_topics[(start + offset) % top_topics.len()].as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !q1.trim().is_empty() {
                        plan.video_queries.push(q1);
                    }
                }
                if !ranked_channels.is_empty() {
                    let (ch, _) = ranked_channels[page % ranked_channels.len()];
                    plan.video_queries.push(ch.clone());
                } else if !subscribed_channels.is_empty() {
                    let ch = &subscribed_channels[page % subscribed_channels.len()];
                    plan.video_queries.push(ch.title.clone());
                }
                let fallback_idx = page % GENERAL_DISCOVERY_POOL.len();
                let fallback_query = GENERAL_DISCOVERY_POOL[fallback_idx].to_owned();
                if plan.video_queries.len() < 2 {
                    plan.video_queries.push(fallback_query);
                }
                if page > 0 || plan.video_queries.len() < 2 {
                    let extra_idx = (page + 3) % GENERAL_DISCOVERY_POOL.len();
                    let extra = GENERAL_DISCOVERY_POOL[extra_idx].to_owned();
                    if !plan.video_queries.contains(&extra) {
                        plan.video_queries.push(extra);
                    }
                }

                if !subscribed_channels.is_empty() {
                    let max_targets = 2.min(subscribed_channels.len());
                    let base = (page * max_targets) % subscribed_channels.len();
                    for offset in 0..max_targets {
                        let ch = &subscribed_channels[(base + offset) % subscribed_channels.len()];
                        if ch.channel_id.starts_with("UC") && ch.channel_id.len() >= 12 {
                            plan.channel_targets
                                .push(crate::search::ChannelTarget::ChannelId(
                                    ch.channel_id.clone(),
                                ));
                        }
                    }
                }

                if page.is_multiple_of(2) {
                    let playlist_q = if !top_topics.is_empty() && page == 0 {
                        format!("{} mix", top_topics[0])
                    } else {
                        PLAYLIST_POOL[(page / 2) % PLAYLIST_POOL.len()].to_owned()
                    };
                    plan.playlist_query = Some(playlist_q);
                }

                if page.is_multiple_of(2) {
                    let shorts_q = if !top_topics.is_empty() && page == 0 {
                        format!("{} #shorts", top_topics[0])
                    } else {
                        SHORTS_POOL[(page / 2) % SHORTS_POOL.len()].to_owned()
                    };
                    plan.shorts_query = Some(shorts_q);
                }
            }
            FeedFilter::Music => {
                let variants = [
                    "música clipe ao vivo oficial",
                    "álbum acústico música brasil",
                    "show ao vivo rock pop brasil",
                ];
                plan.video_queries
                    .push(variants[page % variants.len()].to_owned());
                plan.playlist_query = Some("mix música brasil playlist".into());
                if page == 0 {
                    plan.shorts_query = Some("música #shorts".into());
                }
            }
            FeedFilter::Podcasts => {
                let variants = [
                    "podcast entrevista episódio completo",
                    "cortes podcast brasil debate",
                    "podcast tecnologia história completo",
                ];
                plan.video_queries
                    .push(variants[page % variants.len()].to_owned());
                plan.playlist_query = Some("podcast episódios completos playlist".into());
            }
            FeedFilter::Gaming => {
                let variants = [
                    "gameplay jogos brasil",
                    "league of legends gta minecraft brasil",
                    "review análise jogos lançamento",
                ];
                plan.video_queries
                    .push(variants[page % variants.len()].to_owned());
                if page == 0 {
                    plan.shorts_query = Some("gaming #shorts".into());
                }
            }
            FeedFilter::Live => {
                let variants = ["ao vivo live stream brasil", "transmissão ao vivo agora"];
                plan.video_queries
                    .push(variants[page % variants.len()].to_owned());
            }
            FeedFilter::Mixes => {
                let mix_q = if !top_topics.is_empty() {
                    format!("{} mix playlist", top_topics[page % top_topics.len()])
                } else {
                    PLAYLIST_POOL[page % PLAYLIST_POOL.len()].to_owned()
                };
                plan.video_queries.push("mix música ao vivo".into());
                plan.playlist_query = Some(mix_q);
            }
            FeedFilter::Shorts => {
                let shorts_q = SHORTS_POOL[page % SHORTS_POOL.len()].to_owned();
                plan.shorts_query = Some(shorts_q);
                plan.video_queries.push(if page == 0 {
                    "#shorts brasil".into()
                } else {
                    format!("#shorts {}", SHORTS_POOL[page % SHORTS_POOL.len()])
                });
            }
            FeedFilter::Topic(topic) => {
                plan.video_queries.push(if page == 0 {
                    topic.clone()
                } else {
                    format!("{topic} novidades {}", page + 1)
                });
                if page == 0 {
                    plan.shorts_query = Some(format!("{topic} #shorts"));
                }
            }
            FeedFilter::Channel(channel) => {
                plan.video_queries.push(channel.clone());
            }
        }

        plan
    }

    pub fn rank_sidebar_video_indices(
        videos: &[&Video],
        metadata_map: &HashMap<String, VideoMetadata>,
        profile: &UserProfile,
        current_video: Option<&Video>,
        current_metadata: Option<&VideoMetadata>,
        tab: crate::watch_extras::SidebarTab,
    ) -> Vec<(usize, f64, bool)> {
        if videos.is_empty() {
            return Vec::new();
        }

        let reference_epoch = latest_dataset_epoch_refs(videos, metadata_map)
            .or_else(current_unix_epoch)
            .unwrap_or(1_762_728_000);

        let current_url = current_video.map(|v| v.url.as_str()).unwrap_or("");
        let current_id = current_video.and_then(|v| history::youtube_video_id(&v.url));
        let current_ch_key = current_video
            .map(|v| normalize_channel_key(&v.channel))
            .unwrap_or_default();
        let current_tokens = current_video
            .map(|v| {
                let mut toks = extract_significant_tokens(&v.title);
                if let Some(desc) = current_metadata.and_then(|m| m.description.as_deref()) {
                    for tok in extract_significant_tokens(desc).into_iter().take(16) {
                        if !toks.contains(&tok) {
                            toks.push(tok);
                        }
                    }
                }
                toks
            })
            .unwrap_or_default();
        let current_is_music = current_video.is_some_and(|v| is_music_video(v, current_metadata));
        let current_is_gaming = current_video.is_some_and(|v| is_gaming_video(v, current_metadata));
        let current_is_podcast =
            current_video.is_some_and(|v| is_podcast_video(v, current_metadata));

        let non_self: Vec<usize> = videos
            .iter()
            .enumerate()
            .filter_map(|(orig_idx, v)| {
                if !current_url.is_empty() && v.url == current_url {
                    return None;
                }
                if let (Some(cid), Some(vid)) = (&current_id, history::youtube_video_id(&v.url))
                    && cid == &vid
                {
                    return None;
                }
                Some(orig_idx)
            })
            .collect();

        if matches!(tab, crate::watch_extras::SidebarTab::FromChannel) {
            let same_channel: Vec<usize> = non_self
                .iter()
                .copied()
                .filter(|&orig_idx| {
                    !current_ch_key.is_empty()
                        && normalize_channel_key(&videos[orig_idx].channel) == current_ch_key
                })
                .collect();
            let pool = if !same_channel.is_empty() {
                same_channel
            } else {
                non_self
            };
            let mut scored_list: Vec<(usize, usize, f64, bool)> = pool
                .into_iter()
                .enumerate()
                .map(|(idx, orig_idx)| {
                    let video = videos[orig_idx];
                    let meta = metadata_map.get(&video.url);
                    let (mut score, is_short, _, _) = Self::score_single_parts(
                        video,
                        meta,
                        profile,
                        reference_epoch,
                        &FeedFilter::All,
                    );
                    if !current_ch_key.is_empty()
                        && normalize_channel_key(&video.channel) == current_ch_key
                    {
                        score += 100.0;
                    }
                    score -= (idx as f64) * 0.05;
                    (idx, orig_idx, score, is_short)
                })
                .collect();
            scored_list.sort_by(|a, b| {
                b.2.partial_cmp(&a.2)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.0.cmp(&b.0))
            });
            return scored_list
                .into_iter()
                .map(|(_, orig_idx, score, is_short)| (orig_idx, score, is_short))
                .collect();
        }

        struct CandidateEntry {
            pos_idx: usize,
            orig_idx: usize,
            score: f64,
            is_short: bool,
            ch_key: String,
        }

        let mut candidates: Vec<CandidateEntry> = non_self
            .into_iter()
            .enumerate()
            .map(|(idx, orig_idx)| {
                let video = videos[orig_idx];
                let meta = metadata_map.get(&video.url);
                let (mut score, is_short, _, _) = Self::score_single_parts(
                    video,
                    meta,
                    profile,
                    reference_epoch,
                    &FeedFilter::Recommended,
                );
                let ch_key = normalize_channel_key(&video.channel);
                let v_tokens = extract_significant_tokens(&video.title);
                let shared_with_current = v_tokens
                    .iter()
                    .filter(|t| current_tokens.contains(*t))
                    .count();
                let same_category = (current_is_music && is_music_video(video, meta))
                    || (current_is_gaming && is_gaming_video(video, meta))
                    || (current_is_podcast && is_podcast_video(video, meta));

                match tab {
                    crate::watch_extras::SidebarTab::All
                    | crate::watch_extras::SidebarTab::Transcript => {
                        score -= (idx as f64) * 0.04;
                    }
                    crate::watch_extras::SidebarTab::FromChannel => {}
                    crate::watch_extras::SidebarTab::Related => {
                        score += (shared_with_current as f64) * 16.0;
                        if same_category {
                            score += 18.0;
                        }
                        if !current_ch_key.is_empty() && ch_key == current_ch_key {
                            score += 10.0;
                        }
                        score -= (idx as f64) * 0.03;
                    }
                    crate::watch_extras::SidebarTab::ForYou => {
                        let ch_affinity =
                            profile.channel_weights.get(&ch_key).copied().unwrap_or(0.0);
                        let mut personal_topic_score = 0.0;
                        for tok in &v_tokens {
                            if let Some(&w) = profile.token_weights.get(tok) {
                                personal_topic_score += w;
                            }
                        }
                        score += (ch_affinity * 9.0).min(42.0);
                        score += (personal_topic_score * 7.5).min(38.0);

                        let is_unwatched = history::youtube_video_id(&video.url)
                            .is_none_or(|id| !profile.watched_states.contains_key(&id));
                        if is_unwatched {
                            score += 16.0;
                        }
                        if !current_ch_key.is_empty() && ch_key != current_ch_key {
                            score += 9.0;
                        }
                        if is_recent_upload(meta, Some(reference_epoch)) {
                            score += 8.0;
                        }
                    }
                }
                CandidateEntry {
                    pos_idx: idx,
                    orig_idx,
                    score,
                    is_short,
                    ch_key,
                }
            })
            .collect();

        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.pos_idx.cmp(&b.pos_idx))
        });

        let repeat_weight = match tab {
            crate::watch_extras::SidebarTab::ForYou => 9.5,
            crate::watch_extras::SidebarTab::Related => 5.0,
            _ => 6.5,
        };

        let mut channel_interner: HashMap<String, usize> = HashMap::new();
        let candidate_ch_ids: Vec<Option<usize>> = candidates
            .iter()
            .map(|candidate| {
                if candidate.ch_key.is_empty() {
                    None
                } else if let Some(&id) = channel_interner.get(&candidate.ch_key) {
                    Some(id)
                } else {
                    let id = channel_interner.len();
                    channel_interner.insert(candidate.ch_key.clone(), id);
                    Some(id)
                }
            })
            .collect();

        let order = diversify_windowed_indices(
            candidates.len(),
            channel_interner.len(),
            repeat_weight,
            14.0,
            |idx| (candidates[idx].score, candidate_ch_ids[idx]),
        );

        order
            .into_iter()
            .map(|(idx, effective_score)| {
                let chosen = &candidates[idx];
                (chosen.orig_idx, effective_score, chosen.is_short)
            })
            .collect()
    }

    #[cfg(test)]
    pub fn rank_sidebar_videos(
        videos: &[Video],
        metadata_map: &HashMap<String, VideoMetadata>,
        profile: &UserProfile,
        current_video: Option<&Video>,
        current_metadata: Option<&VideoMetadata>,
        tab: crate::watch_extras::SidebarTab,
    ) -> Vec<ScoredVideo> {
        let refs: Vec<&Video> = videos.iter().collect();
        Self::rank_sidebar_video_indices(
            &refs,
            metadata_map,
            profile,
            current_video,
            current_metadata,
            tab,
        )
        .into_iter()
        .map(|(orig_idx, score, is_short)| {
            let video = &videos[orig_idx];
            ScoredVideo {
                video: video.clone(),
                metadata: metadata_map.get(&video.url).cloned().unwrap_or_default(),
                score,
                is_short,
                reasons: Vec::new(),
                matched_topic: None,
            }
        })
        .collect()
    }
}

/// O(N * W) windowed diversification (W = 24) using an active index window and
/// interned channel IDs. Produces the exact same selection order and effective
/// scores as removing `best_pos` from a sorted `Vec`, without O(N^2) shifts or
/// per-iteration String allocations.
fn diversify_windowed_indices<F>(
    len: usize,
    channel_count: usize,
    repeat_weight: f64,
    consecutive_penalty_val: f64,
    mut score_and_channel: F,
) -> Vec<(usize, f64)>
where
    F: FnMut(usize) -> (f64, Option<usize>),
{
    const WINDOW_SIZE: usize = 24;
    let mut ranked = Vec::with_capacity(len);
    if len == 0 {
        return ranked;
    }
    let mut channel_appearances = vec![0usize; channel_count];
    let mut last_channel: Option<usize> = None;
    let initial_window = len.min(WINDOW_SIZE);
    let mut window: Vec<usize> = (0..initial_window).collect();
    let mut next_unwindowed = initial_window;

    while !window.is_empty() {
        let mut best_w_pos = 0usize;
        let mut best_effective_score = f64::NEG_INFINITY;
        let mut best_channel: Option<usize> = None;

        for (w_pos, &cand_idx) in window.iter().enumerate() {
            let (base_score, ch_id) = score_and_channel(cand_idx);
            let repeat_penalty =
                ch_id.map_or(0.0, |id| (channel_appearances[id] as f64) * repeat_weight);
            let consecutive_penalty = if ch_id.is_some() && ch_id == last_channel {
                consecutive_penalty_val
            } else {
                0.0
            };
            let effective = base_score - repeat_penalty - consecutive_penalty;
            if effective > best_effective_score {
                best_effective_score = effective;
                best_w_pos = w_pos;
                best_channel = ch_id;
            }
        }

        let chosen_idx = window.remove(best_w_pos);
        if next_unwindowed < len {
            window.push(next_unwindowed);
            next_unwindowed += 1;
        }
        if let Some(id) = best_channel {
            channel_appearances[id] += 1;
            last_channel = Some(id);
        }
        ranked.push((chosen_idx, best_effective_score));
    }

    ranked
}

pub fn is_recent_upload(metadata: Option<&VideoMetadata>, reference_epoch: Option<i64>) -> bool {
    let Some(upload_raw) = metadata.and_then(|m| m.upload_date.as_deref()) else {
        return false;
    };
    let Some(upload_epoch) = parse_upload_date_epoch(upload_raw) else {
        return false;
    };
    let now_epoch = reference_epoch
        .or_else(current_unix_epoch)
        .unwrap_or(upload_epoch);
    let diff_secs = (now_epoch - upload_epoch).max(0);
    diff_secs <= 86_400 * 7
}

pub fn matches_filter(
    video: &Video,
    metadata: Option<&VideoMetadata>,
    profile: &UserProfile,
    filter: &FeedFilter,
) -> bool {
    if !matches!(filter, FeedFilter::Channel(_)) && !profile.muted_channels.is_empty() {
        let ch_key = normalize_channel_key(&video.channel);
        if !ch_key.is_empty() && profile.muted_channels.contains(&ch_key) {
            return false;
        }
    }
    match filter {
        FeedFilter::All | FeedFilter::Recommended | FeedFilter::RecentlyUploaded => true,
        FeedFilter::Shorts => is_short_video(video, metadata),
        FeedFilter::Music => is_music_video(video, metadata),
        FeedFilter::Podcasts => is_podcast_video(video, metadata),
        FeedFilter::Gaming => is_gaming_video(video, metadata),
        FeedFilter::Live => is_live_video(video, metadata),
        FeedFilter::Mixes => {
            video.url.contains("playlist?list=")
                || contains_ignore_case(&video.title, "mix")
                || is_music_video(video, metadata)
                || is_podcast_video(video, metadata)
        }
        FeedFilter::NewToYou => {
            if let Some(id) = history::youtube_video_id(&video.url)
                && profile.watched_states.contains_key(&id)
            {
                return false;
            }
            let ch_key = normalize_channel_key(&video.channel);
            let ch_weight = profile.channel_weights.get(&ch_key).copied().unwrap_or(0.0);
            ch_weight < 3.5
        }
        FeedFilter::Topic(topic) => {
            let needle = topic.to_lowercase();
            contains_ignore_case(&video.title, &needle)
                || contains_ignore_case(&video.channel, &needle)
                || metadata
                    .and_then(|m| m.description.as_deref())
                    .is_some_and(|d| contains_ignore_case(d, &needle))
        }
        FeedFilter::Channel(channel) => {
            normalize_channel_key(&video.channel) == normalize_channel_key(channel)
        }
    }
}

pub fn is_short_video(video: &Video, metadata: Option<&VideoMetadata>) -> bool {
    if contains_ignore_case(&video.url, "playlist?list=") {
        return false;
    }
    if metadata.is_some_and(|m| m.kind == crate::search::SearchResultKind::Short) {
        return true;
    }
    if contains_ignore_case(&video.url, "/shorts/") {
        return true;
    }
    if contains_ignore_case(&video.title, "#shorts")
        || contains_ignore_case(&video.title, "#short")
        || contains_ignore_case(&video.title, "#meme")
        || contains_ignore_case(&video.title, "#humorbr")
        || contains_ignore_case(&video.title, "#memeviral")
        || contains_ignore_case(&video.title, "#psiutourette")
    {
        return true;
    }
    if let Some(desc) = metadata.and_then(|m| m.description.as_deref())
        && (contains_ignore_case(desc, "#shorts") || contains_ignore_case(desc, "#short"))
    {
        return true;
    }
    if let Some(secs) = parse_duration_seconds(&video.duration)
        && secs > 0.0
        && secs <= 62.0
    {
        return true;
    }
    false
}

pub fn is_music_video(video: &Video, metadata: Option<&VideoMetadata>) -> bool {
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
    video_matches_keywords(video, metadata, KEYWORDS)
}

pub fn is_podcast_video(video: &Video, metadata: Option<&VideoMetadata>) -> bool {
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
    if video_matches_keywords(video, metadata, KEYWORDS) {
        return true;
    }
    if let Some(secs) = parse_duration_seconds(&video.duration)
        && secs >= 1500.0
    {
        return true;
    }
    false
}

pub fn is_gaming_video(video: &Video, metadata: Option<&VideoMetadata>) -> bool {
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
    video_matches_keywords(video, metadata, KEYWORDS)
}

pub fn is_live_video(video: &Video, metadata: Option<&VideoMetadata>) -> bool {
    if metadata.is_some_and(|meta| meta.is_live) {
        return true;
    }
    let dur = video.duration.trim();
    if dur.eq_ignore_ascii_case("live") || dur.eq_ignore_ascii_case("ao vivo") {
        return true;
    }
    const KEYWORDS: &[&str] = &["ao vivo", "(live", "[live"];
    video_matches_keywords(video, metadata, KEYWORDS)
}

fn contains_ignore_case(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    if needle_lower.is_ascii() {
        let needle_bytes = needle_lower.as_bytes();
        return haystack.len() >= needle_bytes.len()
            && haystack
                .as_bytes()
                .windows(needle_bytes.len())
                .any(|window| window.eq_ignore_ascii_case(needle_bytes));
    }
    if haystack.is_ascii() {
        return false;
    }
    haystack.to_lowercase().contains(needle_lower)
}

fn field_matches_keywords(field: &str, keywords_lower: &[&str]) -> bool {
    if field.is_empty() {
        return false;
    }
    let mut has_non_ascii_kw = false;
    for &kw in keywords_lower {
        if kw.is_ascii() {
            if contains_ignore_case(field, kw) {
                return true;
            }
        } else {
            has_non_ascii_kw = true;
        }
    }
    if has_non_ascii_kw && !field.is_ascii() {
        let lower = field.to_lowercase();
        return keywords_lower
            .iter()
            .any(|&kw| !kw.is_ascii() && lower.contains(kw));
    }
    false
}

fn video_matches_keywords(
    video: &Video,
    metadata: Option<&VideoMetadata>,
    keywords_lower: &[&str],
) -> bool {
    field_matches_keywords(&video.title, keywords_lower)
        || field_matches_keywords(&video.channel, keywords_lower)
        || metadata
            .and_then(|m| m.description.as_deref())
            .is_some_and(|desc| field_matches_keywords(desc, keywords_lower))
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
    // Howard Hinnant's civil_from_days
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

pub(crate) fn current_unix_epoch() -> Option<i64> {
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

fn latest_dataset_epoch_refs(
    videos: &[&Video],
    metadata_map: &HashMap<String, VideoMetadata>,
) -> Option<i64> {
    videos
        .iter()
        .filter_map(|v| metadata_map.get(&v.url))
        .filter_map(|m| m.upload_date.as_deref())
        .filter_map(parse_upload_date_epoch)
        .max()
}

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
    use crate::history::Entry as HistoryEntry;

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
                watched_at_ms: None,
                ..Default::default()
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
                watched_at_ms: None,
                ..Default::default()
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

        let short_by_meta = Video {
            title: "Regular Looking Title".into(),
            channel: "Creator".into(),
            url: "https://www.youtube.com/watch?v=shortmeta01".into(),
            thumbnail: String::new(),
            duration: String::new(),
        };
        let short_meta = VideoMetadata {
            kind: crate::search::SearchResultKind::Short,
            ..Default::default()
        };
        assert!(is_short_video(&short_by_meta, Some(&short_meta)));
    }

    #[test]
    fn sidebar_tabs_filter_and_rank_videos_for_channel_related_and_for_you() {
        let mut history = History::default();
        history
            .record(HistoryEntry {
                video_id: "hist_rust01".into(),
                title: "Rust Async Tokio Masterclass".into(),
                channel: "RustLab".into(),
                position_ms: 400_000,
                duration_ms: Some(500_000),
                watched_at_ms: None,
                ..Default::default()
            })
            .unwrap();
        let queue = Queue::new(10);
        let current = sample_video("curr_klara1", "Chatting and tapping ASMR", "klara", "18:04");
        let profile = UserProfile::build(&history, &queue, Some(&current));

        let v_same_ch = sample_video(
            "klara_vid02",
            "Designing you a custom outfit asmr",
            "klara",
            "24:09",
        );
        let v_related_asmr = sample_video(
            "asmr_rel003",
            "ASMR tapping e massagenzinha relax",
            "brellyna asmr",
            "1:19:19",
        );
        let v_for_you_rust = sample_video(
            "rust_fy_004",
            "Tokio Async Runtime Internals em Rust",
            "RustLab",
            "22:00",
        );

        let videos = vec![
            current.clone(),
            v_same_ch.clone(),
            v_related_asmr.clone(),
            v_for_you_rust.clone(),
        ];
        let meta = HashMap::new();

        let from_channel = RecommendationEngine::rank_sidebar_videos(
            &videos,
            &meta,
            &profile,
            Some(&current),
            None,
            crate::watch_extras::SidebarTab::FromChannel,
        );
        assert_eq!(from_channel.len(), 1);
        assert_eq!(from_channel[0].video.channel, "klara");
        assert_eq!(from_channel[0].video.url, v_same_ch.url);

        let related = RecommendationEngine::rank_sidebar_videos(
            &videos,
            &meta,
            &profile,
            Some(&current),
            None,
            crate::watch_extras::SidebarTab::Related,
        );
        assert!(!related.is_empty());
        assert!(related.iter().all(|v| v.video.url != current.url));

        let for_you = RecommendationEngine::rank_sidebar_videos(
            &videos,
            &meta,
            &profile,
            Some(&current),
            None,
            crate::watch_extras::SidebarTab::ForYou,
        );
        assert_eq!(for_you[0].video.url, v_for_you_rust.url);
    }

    #[test]
    fn home_query_plan_rotates_queries_and_channels_across_pages() {
        let mut profile = UserProfile::default();
        profile.token_weights.insert("strokes".into(), 5.0);
        profile.token_weights.insert("rust".into(), 4.0);
        profile.channel_weights.insert("almanaque sos".into(), 3.0);

        let subs = vec![
            crate::search::HomeChannel {
                title: "5 Alguma Coisa".into(),
                channel_id: "UC0000000000000000000001".into(),
                avatar_url: None,
            },
            crate::search::HomeChannel {
                title: "1155 do ET".into(),
                channel_id: "UC0000000000000000000002".into(),
                avatar_url: None,
            },
            crate::search::HomeChannel {
                title: "AchismosTV".into(),
                channel_id: "UC0000000000000000000003".into(),
                avatar_url: None,
            },
        ];

        let p0 = RecommendationEngine::build_home_query_plan(&profile, &subs, &FeedFilter::All, 0);
        let p1 = RecommendationEngine::build_home_query_plan(&profile, &subs, &FeedFilter::All, 1);
        let p2 = RecommendationEngine::build_home_query_plan(&profile, &subs, &FeedFilter::All, 2);

        assert!(!p0.video_queries.is_empty());
        assert_eq!(p0.channel_targets.len(), 2);
        assert!(p0.playlist_query.is_some());
        assert!(p0.shorts_query.is_some());
        assert_ne!(p0.channel_targets, p1.channel_targets);
        assert_ne!(p0.video_queries, p1.video_queries);
        assert!(p2.playlist_query.is_some());
    }

    #[test]
    fn playlist_mix_matches_mixes_filter_and_is_not_classified_as_short() {
        let mix_playlist = Video {
            title: "Mix de One way trigger (mellow version) - The Strokes".into(),
            channel: "The Strokes e outros".into(),
            url: "https://www.youtube.com/playlist?list=RDEM12345".into(),
            thumbnail: String::new(),
            duration: String::new(),
        };
        assert!(!is_short_video(&mix_playlist, None));
        assert!(matches_filter(
            &mix_playlist,
            None,
            &UserProfile::default(),
            &FeedFilter::Mixes
        ));
    }

    #[test]
    fn zero_alloc_keyword_matchers_handle_ascii_and_accented_portuguese_terms() {
        let music = sample_video(
            "m1",
            "Banda X - MÚSICA NOVA (Clipe Oficial)",
            "VEVO Brasil",
            "3:45",
        );
        assert!(is_music_video(&music, None));

        let podcast = sample_video(
            "p1",
            "Conversa Longa - EPISÓDIO #42",
            "Canal Geral",
            "1:15:00",
        );
        assert!(is_podcast_video(&podcast, None));

        let gaming = sample_video("g1", "Ranked no VALORANT com amigos", "Streamer", "22:10");
        assert!(is_gaming_video(&gaming, None));

        let live = sample_video("l1", "TRANSMISSÃO AO VIVO DE SÁBADO", "Canal Live", "45:00");
        assert!(is_live_video(&live, None));
    }

    #[test]
    fn apply_muted_topics_excludes_muted_tokens_from_top_topics_and_chips() {
        let mut profile = UserProfile {
            token_weights: HashMap::from([
                ("rust".to_owned(), 5.0),
                ("gaming".to_owned(), 4.0),
                ("tokio".to_owned(), 3.0),
            ]),
            active_tokens: HashSet::from(["rust".to_owned(), "tokio".to_owned()]),
            ..UserProfile::default()
        };
        profile.apply_muted_topics(&["  RUST ".to_owned()]);
        assert_eq!(profile.top_topics(4), vec!["Gaming", "Tokio"]);
        assert!(!profile.active_tokens.contains("rust"));
        let chips = RecommendationEngine::available_chips(&[], &HashMap::new(), &profile);
        assert!(
            !chips
                .iter()
                .any(|c| matches!(c, FeedFilter::Topic(t) if t.eq_ignore_ascii_case("rust")))
        );
    }

    #[test]
    fn muted_channels_filter_out_videos_and_muted_topics_demote_candidates() {
        let mut profile = UserProfile {
            channel_weights: HashMap::from([
                ("canal chato".to_owned(), 4.0),
                ("canal legal".to_owned(), 2.0),
            ]),
            token_weights: HashMap::from([("rust".to_owned(), 4.0)]),
            ..UserProfile::default()
        };
        profile.apply_muted_channels(&["  Canal Chato ".to_owned()]);
        profile.apply_muted_topics(&["cripto".to_owned()]);

        let videos = vec![
            sample_video("m_ch", "Vídeo qualquer", "Canal Chato", "8:00"),
            sample_video("m_top", "Cripto moedas hoje", "Canal Legal", "9:00"),
            sample_video("keep1", "Rust Async na Prática", "Canal Legal", "12:00"),
        ];
        let ranked =
            RecommendationEngine::rank_videos(&videos, &HashMap::new(), &profile, &FeedFilter::All);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].video.title, "Rust Async na Prática");
        assert_eq!(ranked[1].video.title, "Cripto moedas hoje");
        assert!(ranked[0].primary_reason_label().is_some());
        assert!(
            ranked[0]
                .reasons
                .contains(&RecommendationReason::FrequentChannel)
        );
        assert!(
            ranked[0]
                .reasons
                .iter()
                .any(|r| matches!(r, RecommendationReason::SimilarTopic(_)))
        );
        assert!(ranked[0].explanation_tooltip().contains("Canal frequente"));
    }

    #[test]
    fn available_chips_with_pinned_places_pinned_topics_without_duplicates() {
        let profile = UserProfile {
            token_weights: HashMap::from([("rust".to_owned(), 5.0), ("linux".to_owned(), 4.0)]),
            ..UserProfile::default()
        };
        let pinned = vec!["Rust".to_owned(), "  Música Indie ".to_owned()];
        let chips = RecommendationEngine::available_chips_with_pinned(
            &[],
            &HashMap::new(),
            &profile,
            &pinned,
        );
        assert_eq!(chips[0], FeedFilter::All);
        assert_eq!(chips[1], FeedFilter::Topic("Rust".to_owned()));
        assert_eq!(chips[2], FeedFilter::Topic("Música Indie".to_owned()));
        let rust_count = chips
            .iter()
            .filter(|c| matches!(c, FeedFilter::Topic(t) if t.eq_ignore_ascii_case("rust")))
            .count();
        assert_eq!(rust_count, 1);
    }

    #[test]
    fn windowed_diversification_matches_naive_order_and_scales_linearly_for_large_n() {
        // Verify exact bit-for-bit parity against a naive O(N^2) Vec::remove reference.
        let profile = UserProfile::default();
        let channels = ["Alpha", "Beta", "Gamma", "Delta", "", "Alpha", "Beta"];
        let small_videos: Vec<Video> = (0..64)
            .map(|i| {
                sample_video(
                    &format!("vidrank{i:04}"),
                    &format!("Rust video topic {i}"),
                    channels[i % channels.len()],
                    "10:00",
                )
            })
            .collect();
        let ranked = RecommendationEngine::rank_videos(
            &small_videos,
            &HashMap::new(),
            &profile,
            &FeedFilter::All,
        );
        assert_eq!(ranked.len(), small_videos.len());

        // Independent reference check on the pre-diversified candidate scores.
        let ref_epoch = current_unix_epoch().unwrap_or(1_762_728_000);
        let mut ref_candidates: Vec<(usize, String, f64)> = small_videos
            .iter()
            .enumerate()
            .map(|(idx, video)| {
                let mut scored = RecommendationEngine::score_single(
                    video,
                    None,
                    &profile,
                    ref_epoch,
                    &FeedFilter::All,
                );
                scored.score -= (idx as f64) * 0.04;
                (idx, video.url.clone(), scored.score)
            })
            .collect();
        ref_candidates.sort_by(|a, b| {
            b.2.partial_cmp(&a.2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        let mut ref_appearances: HashMap<String, usize> = HashMap::new();
        let mut ref_last = String::new();
        let mut ref_order = Vec::with_capacity(ref_candidates.len());
        while !ref_candidates.is_empty() {
            let mut best_pos = 0;
            let mut best_eff = f64::NEG_INFINITY;
            for (pos, (orig_idx, _, score)) in ref_candidates
                .iter()
                .enumerate()
                .take(ref_candidates.len().min(24))
            {
                let ch = normalize_channel_key(&small_videos[*orig_idx].channel);
                let count = if ch.is_empty() {
                    0
                } else {
                    *ref_appearances.get(&ch).unwrap_or(&0)
                };
                let eff = *score
                    - (count as f64) * 6.5
                    - if !ch.is_empty() && ch == ref_last {
                        14.0
                    } else {
                        0.0
                    };
                if eff > best_eff {
                    best_eff = eff;
                    best_pos = pos;
                }
            }
            let (orig_idx, url, _) = ref_candidates.remove(best_pos);
            let ch = normalize_channel_key(&small_videos[orig_idx].channel);
            if !ch.is_empty() {
                *ref_appearances.entry(ch.clone()).or_insert(0) += 1;
                ref_last = ch;
            }
            ref_order.push((url, best_eff));
        }
        let actual_order: Vec<(String, f64)> = ranked
            .iter()
            .map(|s| (s.video.url.clone(), s.score))
            .collect();
        assert_eq!(actual_order, ref_order);

        // Large-N sanity: 1,500 items rank cleanly without quadratic blowup.
        let large_videos: Vec<Video> = (0..1_500)
            .map(|i| {
                sample_video(
                    &format!("bigvid{i:05}"),
                    &format!("Large feed video {i}"),
                    &format!("Channel {}", i % 30),
                    "8:30",
                )
            })
            .collect();
        let large_ranked = RecommendationEngine::rank_videos(
            &large_videos,
            &HashMap::new(),
            &profile,
            &FeedFilter::All,
        );
        assert_eq!(large_ranked.len(), 1_500);
        assert_ne!(large_ranked[0].video.channel, large_ranked[1].video.channel);
    }
}
