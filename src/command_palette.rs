use crate::BrowseSection;
use crate::app_state::SettingsTab;
use crate::history;
use crate::library::Bookmark;
use crate::search::{self, ChannelTarget, Video};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CommandPaletteState {
    pub(crate) open: bool,
    pub(crate) query: String,
    pub(crate) selected_index: usize,
    pub(crate) focus_requested: bool,
}

impl CommandPaletteState {
    pub(crate) fn open_palette(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected_index = 0;
        self.focus_requested = true;
    }

    pub(crate) fn close_palette(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected_index = 0;
        self.focus_requested = false;
    }

    pub(crate) fn toggle(&mut self) {
        if self.open {
            self.close_palette();
        } else {
            self.open_palette();
        }
    }

    pub(crate) fn move_selection(&mut self, delta: isize, total: usize) {
        if total == 0 {
            self.selected_index = 0;
            return;
        }
        let current = self.selected_index.min(total - 1) as isize;
        let next = (current + delta).rem_euclid(total as isize) as usize;
        self.selected_index = next;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PaletteCategory {
    Action,
    Section,
    Settings,
    Bookmark,
    Queue,
    Library,
    History,
    Subscription,
}

impl PaletteCategory {
    pub(crate) fn label_pt(self) -> &'static str {
        match self {
            Self::Action => "A\u{00e7}\u{00e3}o r\u{00e1}pida",
            Self::Section => "Navega\u{00e7}\u{00e3}o",
            Self::Settings => "Configura\u{00e7}\u{00f5}es",
            Self::Bookmark => "Marcador",
            Self::Queue => "Fila",
            Self::Library => "Biblioteca",
            Self::History => "Hist\u{00f3}rico",
            Self::Subscription => "Canal inscrito",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DirectYouTubeTarget {
    Video {
        url: String,
        video_id: String,
        start_ms: Option<u64>,
    },
    Channel(ChannelTarget),
    Playlist {
        url: String,
        list_id: String,
    },
}

impl DirectYouTubeTarget {
    pub(crate) fn display_label(&self) -> String {
        match self {
            Self::Video {
                video_id, start_ms, ..
            } => {
                if let Some(ms) = start_ms {
                    format!("V\u{00ed}deo {video_id} ({})", format_timestamp_ms(*ms))
                } else {
                    format!("V\u{00ed}deo {video_id}")
                }
            }
            Self::Channel(target) => match target {
                ChannelTarget::Handle(handle) => format!("Canal {handle}"),
                ChannelTarget::ChannelId(id) => format!("Canal {id}"),
            },
            Self::Playlist { list_id, .. } => format!("Playlist {list_id}"),
        }
    }

    pub(crate) fn canonical_url(&self) -> String {
        match self {
            Self::Video { url, .. } => url.clone(),
            Self::Channel(target) => target.canonical_url(),
            Self::Playlist { url, .. } => url.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PaletteAction {
    Browse(BrowseSection),
    SettingsTab(SettingsTab),
    OpenVideo { video: Video, seek_ms: Option<u64> },
    OpenBookmark { bookmark: Bookmark },
    OpenChannel(ChannelTarget),
    OpenDirectTarget(DirectYouTubeTarget),
    ToggleTheme,
    ToggleTheater,
    ToggleMiniPlayer,
    ClearQueue,
    OpenShortcuts,
}

#[derive(Clone, Debug)]
pub(crate) struct PaletteItem {
    pub(crate) id: String,
    pub(crate) category: PaletteCategory,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) keywords: String,
    pub(crate) shortcut: Option<&'static str>,
    pub(crate) action: PaletteAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RankedPaletteEntry {
    pub(crate) index: usize,
    pub(crate) score: i32,
}

pub(crate) fn format_timestamp_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub(crate) fn normalize_for_search(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.trim().chars() {
        let folded = match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' | 'Å' => {
                'a'
            }
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'ø' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => {
                'o'
            }
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            'ñ' | 'Ñ' => 'n',
            'ý' | 'ÿ' | 'Ý' | 'Ÿ' => 'y',
            other => other,
        };
        for lower in folded.to_lowercase() {
            out.push(lower);
        }
    }
    out
}

pub(crate) fn fuzzy_match_score(query_norm: &str, candidate_norm: &str) -> Option<i32> {
    if query_norm.is_empty() {
        return Some(0);
    }
    if candidate_norm.is_empty() {
        return None;
    }

    if candidate_norm == query_norm {
        return Some(2_000);
    }

    if let Some(byte_idx) = candidate_norm.find(query_norm) {
        let mut score = 1_200;
        if byte_idx == 0 {
            score += 300;
        } else if candidate_norm[..byte_idx]
            .chars()
            .next_back()
            .is_some_and(is_word_separator)
        {
            score += 180;
        }
        let len_penalty = candidate_norm.chars().count().min(120) as i32;
        return Some(score - len_penalty);
    }

    let tokens: Vec<&str> = query_norm.split_whitespace().collect();
    if tokens.len() > 1 {
        let mut total = 400;
        let mut matched_all = true;
        for token in &tokens {
            if let Some(sub_score) = fuzzy_subsequence_only(token, candidate_norm) {
                total += sub_score / 2;
            } else {
                matched_all = false;
                break;
            }
        }
        if matched_all {
            return Some(total);
        }
    }

    fuzzy_subsequence_only(query_norm, candidate_norm)
}

fn fuzzy_subsequence_only(query_norm: &str, candidate_norm: &str) -> Option<i32> {
    let query_chars: Vec<char> = query_norm.chars().filter(|c| !c.is_whitespace()).collect();
    if query_chars.is_empty() {
        return Some(0);
    }
    let cand_chars: Vec<char> = candidate_norm.chars().collect();
    if query_chars.len() > cand_chars.len() {
        return None;
    }

    let mut q_idx = 0usize;
    let mut score = 100i32;
    let mut prev_match: Option<usize> = None;

    for (c_idx, &ch) in cand_chars.iter().enumerate() {
        if q_idx < query_chars.len() && ch == query_chars[q_idx] {
            if c_idx == 0 {
                score += 35;
            } else if is_word_separator(cand_chars[c_idx - 1]) {
                score += 24;
            }
            if let Some(prev) = prev_match {
                if prev + 1 == c_idx {
                    score += 22;
                } else {
                    let gap = (c_idx - prev - 1).min(20) as i32;
                    score -= gap * 2;
                }
            }
            prev_match = Some(c_idx);
            q_idx += 1;
        }
    }

    if q_idx == query_chars.len() {
        Some(score - (cand_chars.len().min(100) as i32) / 2)
    } else {
        None
    }
}

fn is_word_separator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '/' | '-' | '_' | ':' | '.' | '(' | '[' | '|')
}

pub(crate) fn score_palette_item(query_norm: &str, item: &PaletteItem) -> Option<i32> {
    if query_norm.is_empty() {
        return Some(0);
    }
    let title_norm = normalize_for_search(&item.title);
    let subtitle_norm = normalize_for_search(&item.subtitle);
    let keywords_norm = normalize_for_search(&item.keywords);
    let category_norm = normalize_for_search(item.category.label_pt());

    let title_score = fuzzy_match_score(query_norm, &title_norm).map(|s| s + 1_000);
    let subtitle_score = fuzzy_match_score(query_norm, &subtitle_norm).map(|s| s + 350);
    let keywords_score = fuzzy_match_score(query_norm, &keywords_norm).map(|s| s + 150);
    let category_score = fuzzy_match_score(query_norm, &category_norm).map(|s| s + 50);

    let best = [title_score, subtitle_score, keywords_score, category_score]
        .into_iter()
        .flatten()
        .max();

    if best.is_some() {
        return best;
    }

    let combined = format!("{category_norm} {title_norm} {subtitle_norm} {keywords_norm}");
    fuzzy_match_score(query_norm, &combined)
}

pub(crate) fn filter_and_rank(
    query: &str,
    items: &[PaletteItem],
    limit: usize,
) -> Vec<RankedPaletteEntry> {
    let query_norm = normalize_for_search(query);
    if query_norm.is_empty() {
        return items
            .iter()
            .enumerate()
            .take(limit)
            .map(|(index, _)| RankedPaletteEntry { index, score: 0 })
            .collect();
    }

    let mut ranked: Vec<RankedPaletteEntry> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            score_palette_item(&query_norm, item).map(|score| RankedPaletteEntry { index, score })
        })
        .collect();

    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| items[a.index].category.cmp(&items[b.index].category))
            .then_with(|| a.index.cmp(&b.index))
    });
    ranked.truncate(limit);
    ranked
}

fn normalize_url_scheme(trimmed: &str) -> String {
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        trimmed.to_owned()
    } else if [
        "youtube.com/",
        "www.youtube.com/",
        "m.youtube.com/",
        "music.youtube.com/",
        "youtu.be/",
        "youtube-nocookie.com/",
        "www.youtube-nocookie.com/",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix))
    {
        format!("https://{trimmed}")
    } else {
        trimmed.to_owned()
    }
}

fn is_valid_playlist_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub(crate) fn parse_youtube_timestamp_ms(url: &str) -> Option<u64> {
    let parsed = url::Url::parse(url).ok()?;
    for key in ["t", "start", "time_continue"] {
        if let Some((_, val)) = parsed.query_pairs().find(|(k, _)| k == key)
            && let Some(ms) = parse_timestamp_token_ms(val.as_ref())
        {
            return Some(ms);
        }
    }
    if let Some(frag) = parsed.fragment() {
        let stripped = frag.strip_prefix("t=").unwrap_or(frag);
        if let Some(ms) = parse_timestamp_token_ms(stripped) {
            return Some(ms);
        }
    }
    None
}

pub(crate) fn parse_timestamp_token_ms(raw: &str) -> Option<u64> {
    let token = raw.trim();
    if token.is_empty() {
        return None;
    }
    if let Ok(secs) = token.parse::<u64>() {
        return secs.checked_mul(1000);
    }
    if token.contains(':') {
        let parts: Vec<&str> = token.split(':').collect();
        if !(2..=3).contains(&parts.len()) {
            return None;
        }
        let nums: Option<Vec<u64>> = parts.iter().map(|p| p.parse::<u64>().ok()).collect();
        let nums = nums?;
        let secs = match nums.as_slice() {
            [m, s] if *s < 60 => m.checked_mul(60)?.checked_add(*s)?,
            [h, m, s] if *m < 60 && *s < 60 => h
                .checked_mul(3600)?
                .checked_add(m.checked_mul(60)?)?
                .checked_add(*s)?,
            _ => return None,
        };
        return secs.checked_mul(1000);
    }
    let lower = token.to_ascii_lowercase();
    let mut total_secs = 0u64;
    let mut num_buf = String::new();
    let mut saw_unit = false;
    for ch in lower.chars() {
        if ch.is_ascii_digit() {
            num_buf.push(ch);
        } else {
            let val = num_buf.parse::<u64>().ok()?;
            num_buf.clear();
            saw_unit = true;
            let mult = match ch {
                'h' => 3600,
                'm' => 60,
                's' => 1,
                _ => return None,
            };
            total_secs = total_secs.checked_add(val.checked_mul(mult)?)?;
        }
    }
    if !num_buf.is_empty() {
        if saw_unit {
            let val = num_buf.parse::<u64>().ok()?;
            total_secs = total_secs.checked_add(val)?;
        } else {
            return None;
        }
    }
    saw_unit.then(|| total_secs.saturating_mul(1000))
}

fn sanitize_watch_url_for_history(raw_url: &str) -> Option<(String, Option<u64>)> {
    let start_ms = parse_youtube_timestamp_ms(raw_url);
    if let Some(id) = history::youtube_video_id(raw_url)
        && let Some(canonical) = history::youtube_watch_url(&id)
    {
        return Some((canonical, start_ms));
    }
    let parsed = url::Url::parse(raw_url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    if matches!(
        host.as_str(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com"
    ) && (parsed.path() == "/watch" || parsed.path() == "/watch/")
        && let Some((_, v)) = parsed.query_pairs().find(|(k, _)| k == "v")
        && history::is_valid_video_id(v.as_ref())
        && let Some(canonical) = history::youtube_watch_url(v.as_ref())
    {
        return Some((canonical, start_ms));
    }
    None
}

fn parse_playlist_target(raw_url: &str) -> Option<DirectYouTubeTarget> {
    let parsed = url::Url::parse(raw_url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    if !matches!(
        host.as_str(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com"
    ) {
        return None;
    }
    if parsed.path() != "/playlist" && parsed.path() != "/playlist/" {
        return None;
    }
    let (_, list_id) = parsed.query_pairs().find(|(k, _)| k == "list")?;
    if !is_valid_playlist_id(list_id.as_ref()) {
        return None;
    }
    let list_id = list_id.into_owned();
    Some(DirectYouTubeTarget::Playlist {
        url: format!("https://www.youtube.com/playlist?list={list_id}"),
        list_id,
    })
}

/// Parses a YouTube URL strictly (no bare `@handle`), suitable for clipboard detection.
pub(crate) fn parse_direct_youtube_url(input: &str) -> Option<DirectYouTubeTarget> {
    let trimmed = input.trim();
    if trimmed.is_empty()
        || trimmed.len() > 2048
        || trimmed.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return None;
    }
    let normalized = normalize_url_scheme(trimmed);
    let lower = normalized.to_ascii_lowercase();
    if !lower.starts_with("https://") && !lower.starts_with("http://") {
        return None;
    }
    if let Some((canonical_url, start_ms)) = sanitize_watch_url_for_history(&normalized)
        && let Some(video_id) = history::youtube_video_id(&canonical_url)
    {
        return Some(DirectYouTubeTarget::Video {
            url: canonical_url,
            video_id,
            start_ms,
        });
    }
    if let Some(playlist) = parse_playlist_target(&normalized) {
        return Some(playlist);
    }
    search::parse_channel_target(&normalized).map(DirectYouTubeTarget::Channel)
}

/// Parses a YouTube URL or `@handle` / channel ID entered in the search bar or palette.
pub(crate) fn parse_direct_youtube_target(input: &str) -> Option<DirectYouTubeTarget> {
    let trimmed = input.trim();
    if trimmed.is_empty()
        || trimmed.len() > 2048
        || trimmed.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return None;
    }
    if let Some(url_target) = parse_direct_youtube_url(trimmed) {
        return Some(url_target);
    }
    search::parse_channel_target(trimmed).map(DirectYouTubeTarget::Channel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_item(
        id: &str,
        category: PaletteCategory,
        title: &str,
        subtitle: &str,
    ) -> PaletteItem {
        PaletteItem {
            id: id.to_owned(),
            category,
            title: title.to_owned(),
            subtitle: subtitle.to_owned(),
            keywords: String::new(),
            shortcut: None,
            action: PaletteAction::Browse(BrowseSection::Home),
        }
    }

    #[test]
    fn normalize_and_fuzzy_match_handles_accents_and_subsequences() {
        assert_eq!(normalize_for_search("Configurações"), "configuracoes");
        assert_eq!(normalize_for_search("Início"), "inicio");
        assert!(fuzzy_match_score("inicio", &normalize_for_search("Início")).is_some());
        assert!(fuzzy_match_score("config", &normalize_for_search("Configurações")).is_some());
        assert!(fuzzy_match_score("sb", &normalize_for_search("SponsorBlock")).is_some());
        assert!(fuzzy_match_score("xyz", &normalize_for_search("Histórico")).is_none());
    }

    #[test]
    fn filter_and_rank_prioritizes_title_matches_and_respects_limit() {
        let items = vec![
            sample_item("1", PaletteCategory::History, "Rust tutorial", "Channel A"),
            sample_item(
                "2",
                PaletteCategory::Settings,
                "Reprodu\u{00e7}\u{00e3}o e desempenho",
                "Configura\u{00e7}\u{00f5}es",
            ),
            sample_item("3", PaletteCategory::Section, "In\u{00ed}cio", "Abrir feed"),
        ];

        let ranked = filter_and_rank("reproducao", &items, 10);
        assert_eq!(ranked.len(), 1);
        assert_eq!(items[ranked[0].index].id, "2");

        let all_limited = filter_and_rank("", &items, 2);
        assert_eq!(all_limited.len(), 2);
        assert_eq!(all_limited[0].index, 0);
        assert_eq!(all_limited[1].index, 1);
    }

    #[test]
    fn parse_direct_youtube_url_recognizes_video_timestamps_playlists_and_channels() {
        assert_eq!(
            parse_direct_youtube_url("https://youtu.be/dQw4w9WgXcQ?t=1m30s"),
            Some(DirectYouTubeTarget::Video {
                url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned(),
                video_id: "dQw4w9WgXcQ".to_owned(),
                start_ms: Some(90_000),
            })
        );
        assert_eq!(
            parse_direct_youtube_url("youtube.com/watch?v=dQw4w9WgXcQ&t=45"),
            Some(DirectYouTubeTarget::Video {
                url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned(),
                video_id: "dQw4w9WgXcQ".to_owned(),
                start_ms: Some(45_000),
            })
        );
        assert_eq!(
            parse_direct_youtube_url("https://www.youtube.com/shorts/abcdefghijk"),
            Some(DirectYouTubeTarget::Video {
                url: "https://www.youtube.com/watch?v=abcdefghijk".to_owned(),
                video_id: "abcdefghijk".to_owned(),
                start_ms: None,
            })
        );
        assert_eq!(
            parse_direct_youtube_url("https://www.youtube.com/playlist?list=PL1234567890"),
            Some(DirectYouTubeTarget::Playlist {
                url: "https://www.youtube.com/playlist?list=PL1234567890".to_owned(),
                list_id: "PL1234567890".to_owned(),
            })
        );
        assert_eq!(
            parse_direct_youtube_url("https://www.youtube.com/@rustlang/videos"),
            Some(DirectYouTubeTarget::Channel(ChannelTarget::Handle(
                "@rustlang".to_owned()
            )))
        );
        assert!(parse_direct_youtube_url("@rustlang").is_none());
        assert!(parse_direct_youtube_url("rust tutorial").is_none());
        assert!(parse_direct_youtube_url("https://example.com/watch?v=dQw4w9WgXcQ").is_none());
    }

    #[test]
    fn parse_direct_youtube_target_accepts_handles_and_channel_ids() {
        assert_eq!(
            parse_direct_youtube_target("@rustlang"),
            Some(DirectYouTubeTarget::Channel(ChannelTarget::Handle(
                "@rustlang".to_owned()
            )))
        );
        assert_eq!(
            parse_direct_youtube_target("UC_x5XG1OV2P6uZZ5FSM9Ttw"),
            Some(DirectYouTubeTarget::Channel(ChannelTarget::ChannelId(
                "UC_x5XG1OV2P6uZZ5FSM9Ttw".to_owned()
            )))
        );
        assert!(parse_direct_youtube_target("rust programming").is_none());
    }
}
