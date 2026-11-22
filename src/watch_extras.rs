use crate::{comment_actions, history, oauth, process, search};
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

static SUBTITLE_TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);
const SUBTITLE_VTT_CACHE_LIMIT: usize = 4;
pub(crate) const MAX_WATCH_COMMENTS: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadySubtitle {
    pub url: String,
    pub path: PathBuf,
    pub label: String,
    pub language: String,
}

struct DownloadedSubtitle {
    url: String,
    label: String,
    language: String,
    vtt: String,
    cues: Vec<crate::transcript::Cue>,
}

struct TranscriptPayload {
    url: String,
    vtt: String,
    cues: Vec<crate::transcript::Cue>,
}

#[derive(Default)]
pub struct TranscriptDisplayCache {
    pub valid: bool,
    pub revision: u64,
    pub raw_len: usize,
    pub first_cue_sig: Option<(u64, u64, usize)>,
    pub last_cue_sig: Option<(u64, u64, usize)>,
    pub chapters_sig: u64,
    pub segments: Vec<crate::transcript::Cue>,
    pub items: Vec<crate::transcript::TranscriptDisplayItem>,
    pub item_heights: Vec<Option<(i32, bool, f32)>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidebarTab {
    #[default]
    All,
    FromChannel,
    Related,
    ForYou,
    Transcript,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Results,
    Comments,
}

pub use crate::comment_actions::CommentRating;

struct CommentRatingJob {
    comment_id: String,
    rx: Receiver<comment_actions::RemoteCommentRatingOutcome>,
    cancel: process::Cancellation,
}

struct CommentStateSyncResult {
    video_id: String,
    order: oauth::CommentOrder,
    web_session: Option<comment_actions::YouTubeWebSession>,
    auth_expired: bool,
    result: Result<comment_actions::VideoCommentTokens, String>,
}

#[derive(Default)]
pub struct CommentTranslation {
    pub translated_text: Option<String>,
    pub translated_lines: Vec<CachedTimestampLine>,
    pub showing_translated: bool,
    pub loading: bool,
    pub error: Option<String>,
    rx: Option<Receiver<Result<String, String>>>,
    cancel: Option<process::Cancellation>,
}

fn parse_translate_response(json: &serde_json::Value) -> Result<String, String> {
    let segments = json
        .get(0)
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "Resposta de tradu\u{00e7}\u{00e3}o inv\u{00e1}lida.".to_owned())?;
    let mut out = String::new();
    for segment in segments {
        if let Some(piece) = segment.get(0).and_then(serde_json::Value::as_str) {
            out.push_str(piece);
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return Err("N\u{00e3}o foi poss\u{00ed}vel traduzir este coment\u{00e1}rio.".into());
    }
    Ok(out)
}

fn translate_text_to_pt(text: &str, cancel: &process::Cancellation) -> Result<String, String> {
    if cancel.cancelled() {
        return Err("Tradu\u{00e7}\u{00e3}o cancelada.".into());
    }
    let clipped: String = text.trim().chars().take(4000).collect();
    if clipped.is_empty() {
        return Err("Coment\u{00e1}rio vazio.".into());
    }
    let response = crate::http_client::no_redirect()
        .get("https://translate.googleapis.com/translate_a/single")
        .timeout(std::time::Duration::from_secs(10))
        .query(&[
            ("client", "gtx"),
            ("sl", "auto"),
            ("tl", "pt"),
            ("dt", "t"),
            ("q", clipped.as_str()),
        ])
        .send()
        .map_err(|_| "Falha de conex\u{00e3}o ao traduzir coment\u{00e1}rio.".to_owned())?;
    if cancel.cancelled() {
        return Err("Tradu\u{00e7}\u{00e3}o cancelada.".into());
    }
    if !response.status().is_success() {
        return Err(
            "Servi\u{00e7}o de tradu\u{00e7}\u{00e3}o indispon\u{00ed}vel no momento.".into(),
        );
    }
    let json: serde_json::Value = response
        .json()
        .map_err(|_| "Resposta de tradu\u{00e7}\u{00e3}o inv\u{00e1}lida.".to_owned())?;
    parse_translate_response(&json)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextTimestamp {
    pub label: String,
    pub seconds: u64,
    pub byte_start: usize,
    pub byte_end: usize,
}

pub fn extract_timestamps(text: &str) -> Vec<TextTimestamp> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let prev_ok = i == 0
            || text[..i]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != ':');
        if !prev_ok {
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            continue;
        }
        let start = i;
        let mut parts = [0u64; 3];
        let mut part_count = 0usize;
        let mut cursor = i;
        let mut valid = true;

        for slot in 0..3 {
            let d_start = cursor;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
            }
            let digit_len = cursor - d_start;
            if digit_len == 0 || digit_len > 2 {
                valid = false;
                break;
            }
            if slot > 0 && digit_len != 2 {
                valid = false;
                break;
            }
            let Ok(num) = text[d_start..cursor].parse::<u64>() else {
                valid = false;
                break;
            };
            parts[part_count] = num;
            part_count += 1;
            if cursor < bytes.len() && bytes[cursor] == b':' {
                if slot == 2 {
                    valid = false;
                    break;
                }
                cursor += 1;
            } else {
                break;
            }
        }

        let end_ok = cursor == bytes.len()
            || text[cursor..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != ':');

        if valid && end_ok && (part_count == 2 || part_count == 3) {
            let seconds = if part_count == 2 {
                let (m, s) = (parts[0], parts[1]);
                if s < 60 { Some(m * 60 + s) } else { None }
            } else {
                let (h, m, s) = (parts[0], parts[1], parts[2]);
                if m < 60 && s < 60 {
                    Some(h * 3600 + m * 60 + s)
                } else {
                    None
                }
            };
            if let Some(secs) = seconds {
                found.push(TextTimestamp {
                    label: text[start..cursor].to_owned(),
                    seconds: secs,
                    byte_start: start,
                    byte_end: cursor,
                });
                i = cursor;
                continue;
            }
        }

        i = start + 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    found
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedTimestampLine {
    pub text: String,
    pub timestamps: Vec<TextTimestamp>,
}

pub fn parse_timestamp_lines(text: &str) -> Vec<CachedTimestampLine> {
    text.lines()
        .map(|line| CachedTimestampLine {
            text: line.to_owned(),
            timestamps: extract_timestamps(line),
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub enum DescriptionToken<'a> {
    Text(&'a str),
    Timestamp { raw: &'a str, secs: f64 },
    Url(&'a str),
    Mention(&'a str),
    Hashtag(&'a str),
}

fn is_url_body_byte(b: u8) -> bool {
    !b.is_ascii_whitespace() && !matches!(b, b'"' | b'\'' | b'<' | b'>')
}

fn trim_url_trailing_punctuation(raw: &str) -> &str {
    let mut end = raw.len();
    loop {
        let slice = &raw[..end];
        let Some(last) = slice.chars().next_back() else {
            break;
        };
        let strip = match last {
            '.' | ',' | ';' | '!' | '?' | ':' => true,
            ')' => slice.matches(')').count() > slice.matches('(').count(),
            ']' => slice.matches(']').count() > slice.matches('[').count(),
            '}' => slice.matches('}').count() > slice.matches('{').count(),
            _ => false,
        };
        if strip {
            end -= last.len_utf8();
        } else {
            break;
        }
    }
    &raw[..end]
}

fn parse_url_at(line: &str, start: usize) -> Option<usize> {
    let rest = line.get(start..)?;
    let scheme_len = if rest.starts_with("https://") {
        8
    } else if rest.starts_with("http://") {
        7
    } else {
        return None;
    };
    let bytes = line.as_bytes();
    let mut cursor = start + scheme_len;
    while cursor < bytes.len() && is_url_body_byte(bytes[cursor]) {
        cursor += 1;
    }
    let trimmed = trim_url_trailing_punctuation(&line[start..cursor]);
    if trimmed.len() <= scheme_len {
        return None;
    }
    Some(start + trimmed.len())
}

fn parse_mention_at(line: &str, start: usize) -> Option<usize> {
    if line.as_bytes().get(start).copied() != Some(b'@') {
        return None;
    }
    let prev_ok = start == 0
        || line[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && !matches!(c, '_' | '.' | '-'));
    if !prev_ok {
        return None;
    }
    let rest = &line[start + 1..];
    let mut len = 0usize;
    for ch in rest.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-') {
            len += ch.len_utf8();
        } else {
            break;
        }
    }
    let mut end = start + 1 + len;
    while end > start + 1 && matches!(line.as_bytes()[end - 1], b'.' | b'-') {
        end -= 1;
    }
    (end > start + 1).then_some(end)
}

fn parse_hashtag_at(line: &str, start: usize) -> Option<usize> {
    if line.as_bytes().get(start).copied() != Some(b'#') {
        return None;
    }
    let prev_ok = start == 0
        || line[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && !matches!(c, '_' | '/'));
    if !prev_ok {
        return None;
    }
    let rest = &line[start + 1..];
    let mut len = 0usize;
    let mut has_alpha_or_digit = false;
    for ch in rest.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            if ch.is_alphanumeric() {
                has_alpha_or_digit = true;
            }
            len += ch.len_utf8();
        } else {
            break;
        }
    }
    (len > 0 && has_alpha_or_digit).then_some(start + 1 + len)
}

fn parse_timestamp_at(line: &str, start: usize) -> Option<(usize, f64)> {
    let bytes = line.as_bytes();
    if !bytes.get(start).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let prev_ok = start == 0
        || line[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != ':');
    if !prev_ok {
        return None;
    }
    let mut parts = [0u64; 3];
    let mut part_count = 0usize;
    let mut cursor = start;
    for slot in 0..3 {
        let d_start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        let digit_len = cursor - d_start;
        if digit_len == 0 || digit_len > 2 || (slot > 0 && digit_len != 2) {
            return None;
        }
        parts[part_count] = line[d_start..cursor].parse::<u64>().ok()?;
        part_count += 1;
        if cursor < bytes.len() && bytes[cursor] == b':' {
            if slot == 2 {
                return None;
            }
            cursor += 1;
        } else {
            break;
        }
    }
    let end_ok = cursor == bytes.len()
        || line[cursor..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != ':');
    if !end_ok {
        return None;
    }
    let secs = match part_count {
        2 if parts[1] < 60 => parts[0] * 60 + parts[1],
        3 if parts[1] < 60 && parts[2] < 60 => parts[0] * 3600 + parts[1] * 60 + parts[2],
        _ => return None,
    };
    Some((cursor, secs as f64))
}

/// Tokenizes a single description line into plain text, timestamps, `http(s)://` URLs,
/// `@mentions`, and `#hashtags` while preserving exact lossless byte spans.
pub fn tokenize_description_line(line: &str) -> Vec<DescriptionToken<'_>> {
    if line.is_empty() {
        return Vec::new();
    }
    let mut tokens = Vec::new();
    let mut text_start = 0usize;
    let mut i = 0usize;
    let bytes = line.as_bytes();

    while i < bytes.len() {
        let b = bytes[i];
        let matched = if b == b'h' {
            parse_url_at(line, i).map(|end| (end, DescriptionToken::Url(&line[i..end])))
        } else if b == b'@' {
            parse_mention_at(line, i).map(|end| (end, DescriptionToken::Mention(&line[i..end])))
        } else if b == b'#' {
            parse_hashtag_at(line, i).map(|end| (end, DescriptionToken::Hashtag(&line[i..end])))
        } else if b.is_ascii_digit() {
            parse_timestamp_at(line, i).map(|(end, secs)| {
                (
                    end,
                    DescriptionToken::Timestamp {
                        raw: &line[i..end],
                        secs,
                    },
                )
            })
        } else {
            None
        };

        if let Some((end, token)) = matched {
            if i > text_start {
                tokens.push(DescriptionToken::Text(&line[text_start..i]));
            }
            tokens.push(token);
            i = end;
            text_start = end;
        } else {
            let ch_len = line[i..].chars().next().map_or(1, char::len_utf8);
            i += ch_len;
        }
    }

    if text_start < bytes.len() {
        tokens.push(DescriptionToken::Text(&line[text_start..]));
    }
    tokens
}

/// Returns the like ratio in `0.0..=1.0` for the compact RYD ratio bar, or `None` if total is 0.
pub fn ryd_like_ratio(likes: u64, dislikes: u64) -> Option<f32> {
    let total = likes.saturating_add(dislikes);
    if total == 0 {
        return None;
    }
    Some((likes as f64 / total as f64).clamp(0.0, 1.0) as f32)
}

fn normalize_comment_search_str(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars().flat_map(char::to_lowercase) {
        let mapped = match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            'ý' | 'ÿ' => 'y',
            '\u{0300}'..='\u{036f}' => continue,
            other => other,
        };
        out.push(mapped);
    }
    out
}

/// Filters comments locally by query (case- and accent-insensitive across author and body).
/// Multi-word queries require every term to match either the author or comment text.
pub fn filter_comments_by_query(comments: &[oauth::Comment], query: &str) -> Vec<usize> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return (0..comments.len()).collect();
    }
    let terms: Vec<String> = trimmed
        .split_whitespace()
        .map(normalize_comment_search_str)
        .filter(|t| !t.is_empty())
        .collect();
    if terms.is_empty() {
        return (0..comments.len()).collect();
    }
    comments
        .iter()
        .enumerate()
        .filter_map(|(idx, comment)| {
            let author_norm = normalize_comment_search_str(&comment.author);
            let text_norm = normalize_comment_search_str(&comment.text);
            let matches_all = terms.iter().all(|term| {
                author_norm.contains(term.as_str()) || text_norm.contains(term.as_str())
            });
            matches_all.then_some(idx)
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DescriptionChapter {
    pub title: String,
    pub label: String,
    pub seconds: u64,
}

fn clean_chapter_piece(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c: char| {
            matches!(
                c,
                '-' | '–' | '—' | ':' | '|' | '•' | '*' | '[' | ']' | '(' | ')' | '/' | '\\'
            ) || c.is_whitespace()
        })
        .trim()
        .to_owned()
}

pub fn extract_description_chapters(description: &str) -> Vec<DescriptionChapter> {
    let mut chapters = Vec::new();
    let mut seen_seconds = HashSet::new();
    for raw_line in description.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let timestamps = extract_timestamps(trimmed);
        let Some(first) = timestamps.first() else {
            continue;
        };
        let before_raw = trimmed[..first.byte_start].trim();
        let before_clean = clean_chapter_piece(before_raw);
        let before_num_trimmed = before_clean.trim_end_matches(['.', ')']);
        let before_clean = if !before_num_trimmed.is_empty()
            && before_num_trimmed.bytes().all(|b| b.is_ascii_digit())
        {
            String::new()
        } else {
            before_clean
        };
        let prefix_words = before_clean.split_whitespace().count();
        if prefix_words > 4 || before_clean.len() > 36 {
            continue;
        }
        let after_start = if timestamps.len() >= 2 {
            let between = clean_chapter_piece(&trimmed[first.byte_end..timestamps[1].byte_start]);
            if between.is_empty() || matches!(between.to_lowercase().as_str(), "a" | "to" | "até")
            {
                timestamps[1].byte_end
            } else {
                first.byte_end
            }
        } else {
            first.byte_end
        };
        let after_clean = clean_chapter_piece(&trimmed[after_start..]);
        let title = match (before_clean.is_empty(), after_clean.is_empty()) {
            (false, false) => format!("{before_clean} — {after_clean}"),
            (false, true) => before_clean,
            (true, false) => after_clean,
            (true, true) => continue,
        };
        if title.is_empty() || !seen_seconds.insert(first.seconds) {
            continue;
        }
        chapters.push(DescriptionChapter {
            title,
            label: first.label.clone(),
            seconds: first.seconds,
        });
    }
    chapters.sort_by_key(|c| c.seconds);
    chapters
}

#[derive(Clone, Debug)]
pub struct CachedCommentMeta {
    pub id: String,
    pub text_len: usize,
    pub published: String,
    pub avatar_name: String,
    pub handle: String,
    pub relative_time: String,
    pub lines: Vec<CachedTimestampLine>,
    pub measured_height: Option<(i32, u64, f32)>,
}

impl CachedCommentMeta {
    fn from_comment(comment: &oauth::Comment) -> Self {
        let avatar_raw = comment.author.trim_start_matches('@');
        let avatar_name = if avatar_raw.is_empty() {
            "YT".to_owned()
        } else {
            avatar_raw.to_owned()
        };
        let handle = if comment.author.starts_with('@') {
            comment.author.clone()
        } else {
            format!("@{}", comment.author)
        };
        let relative_time =
            crate::recommendations::format_youtube_relative_time(&comment.published, None)
                .unwrap_or_else(|| comment.published.clone());
        let lines = parse_timestamp_lines(&comment.text);
        Self {
            id: comment.id.clone(),
            text_len: comment.text.len(),
            published: comment.published.clone(),
            avatar_name,
            handle,
            relative_time,
            lines,
            measured_height: None,
        }
    }
}

pub fn sync_cached_comment_meta(comments: &[oauth::Comment], cache: &mut Vec<CachedCommentMeta>) {
    let up_to_date = cache.len() == comments.len()
        && cache.iter().zip(comments.iter()).all(|(cached, item)| {
            cached.id == item.id
                && cached.text_len == item.text.len()
                && cached.published == item.published
        });
    if up_to_date {
        return;
    }
    let mut previous_heights: HashMap<String, (i32, u64, f32)> = cache
        .drain(..)
        .filter_map(|entry| entry.measured_height.map(|h| (entry.id, h)))
        .collect();
    cache.reserve(comments.len());
    for comment in comments {
        let mut entry = CachedCommentMeta::from_comment(comment);
        if let Some(height) = previous_heights.remove(&entry.id) {
            entry.measured_height = Some(height);
        }
        cache.push(entry);
    }
}

#[derive(Default)]
pub struct CommentReplies {
    pub comments: Vec<oauth::Comment>,
    pub render_cache: Vec<CachedCommentMeta>,
    pub next: Option<String>,
    pub error: Option<String>,
    pub loaded: bool,
    loading: Option<Receiver<Result<oauth::CommentsPage, String>>>,
    cancel: Option<process::Cancellation>,
    pub reply_open: bool,
    pub reply_draft: String,
    pub reply_post_error: Option<String>,
    reply_post_rx: Option<Receiver<Result<oauth::Comment, String>>>,
    reply_post_cancel: Option<process::Cancellation>,
}

impl CommentReplies {
    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    pub fn is_posting_reply(&self) -> bool {
        self.reply_post_rx.is_some()
    }

    pub fn retry_uses_next_page(&self) -> bool {
        self.next.is_some()
    }
}

#[derive(Default)]
struct TempSubtitleFiles(HashMap<String, PathBuf>);

impl Drop for TempSubtitleFiles {
    fn drop(&mut self) {
        for (_, path) in self.0.drain() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InnerTubeCommentMeta {
    pub is_pinned: bool,
    pub is_hearted: bool,
    pub is_owner: bool,
    pub is_member: bool,
    pub like_count_text: Option<String>,
    pub replies_continuation: Option<String>,
}

fn parse_compact_comment_count(raw: &str) -> Option<u64> {
    if let Some(val) = search::parse_pt_br_compact_count(raw) {
        return Some(val);
    }
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u64>().ok()
    }
}

#[allow(dead_code)]
pub type WatchExtrasState = WatchExtras;

pub struct WatchExtras {
    pub video_url: Option<String>,
    pub like_count_text: Option<String>,
    pub view_count_text: Option<String>,
    pub published_date_text: Option<String>,
    pub tab: Tab,
    pub sidebar_tab: SidebarTab,
    pub related_videos: Vec<crate::search::Video>,
    pub related_metadata: Vec<crate::search::VideoMetadata>,
    pub related_chips: Vec<crate::innertube::RelatedFilterChip>,
    pub selected_related_chip: Option<usize>,
    pub related_continuation: Option<String>,
    pub innertube_comments_continuation: Option<String>,
    pub innertube_comments_top_continuation: Option<String>,
    pub innertube_comments_newest_continuation: Option<String>,
    pub innertube_comments_loading: bool,
    pub innertube_comment_meta: HashMap<String, InnerTubeCommentMeta>,
    pub heatmap: Vec<crate::overlay::HeatmapPoint>,
    pub(crate) cached_heatmap_key: Option<(u64, usize, bool)>,
    pub(crate) cached_heatmap_points: Vec<crate::overlay::HeatmapPoint>,
    pub(crate) cached_sidebar_related_key: Option<(usize, Option<String>, Option<usize>)>,
    pub comments: Vec<oauth::Comment>,
    pub comment_render_cache: Vec<CachedCommentMeta>,
    pub comments_next: Option<String>,
    pub comments_total: Option<u64>,
    pub comments_error: Option<String>,
    pub comments_loaded: bool,
    pub comments_order: oauth::CommentOrder,
    pub comments_order_menu_open: bool,
    pub comments_loading: Option<Receiver<Result<oauth::CommentsPage, String>>>,
    comments_cancel: Option<process::Cancellation>,
    pub comment_draft: String,
    pub comment_composer_active: bool,
    pub comment_emoji_picker_open: bool,
    pub reply_emoji_picker_for: Option<String>,
    pub comment_search_query: String,
    pub editing_comment_id: Option<String>,
    pub editing_comment_draft: String,
    pub report_modal_open: bool,
    pub report_reason: String,
    pub report_details: String,
    pub comment_post_error: Option<String>,
    comment_post_rx: Option<Receiver<Result<oauth::Comment, String>>>,
    comment_post_cancel: Option<process::Cancellation>,
    pub comment_replies: HashMap<String, CommentReplies>,
    pub comment_ratings: HashMap<String, CommentRating>,
    pub initial_comment_ratings: HashMap<String, CommentRating>,
    user_modified_comment_ratings: HashSet<String>,
    pub comment_rating_errors: HashMap<String, String>,
    comment_action_tokens: Option<comment_actions::VideoCommentTokens>,
    comment_rating_job: Option<CommentRatingJob>,
    comment_state_sync_rx: Option<Receiver<CommentStateSyncResult>>,
    comment_state_sync_cancel: Option<process::Cancellation>,
    pub comment_state_sync_started: bool,
    pub comment_translations: HashMap<String, CommentTranslation>,
    pub subtitles: Vec<search::SubtitleTrack>,
    pub subtitles_error: Option<String>,
    pub subtitles_loaded: bool,
    pub subtitles_loading: Option<Receiver<Result<Vec<search::SubtitleTrack>, String>>>,
    subtitles_cancel: Option<process::Cancellation>,
    pub active_subtitle_url: Option<String>,
    pub active_subtitle_cues: Vec<crate::transcript::Cue>,
    vtt_cache: HashMap<String, String>,
    temp_subtitle_files: TempSubtitleFiles,
    subtitle_apply_rx: Option<Receiver<Result<DownloadedSubtitle, String>>>,
    subtitle_apply_cancel: Option<process::Cancellation>,
    ready_subtitle: Option<ReadySubtitle>,
    pub transcript: Vec<crate::transcript::Cue>,
    pub transcript_open: bool,
    pub transcript_query: String,
    pub transcript_error: Option<String>,
    pub transcript_loading: bool,
    pub transcript_follow: bool,
    pub transcript_show_timestamps: bool,
    pub transcript_font_size: f32,
    pub transcript_high_contrast: bool,
    pub transcript_track_url: Option<String>,
    pub transcript_auto_attempted: bool,
    pub transcript_user_scrolled: bool,
    pub transcript_last_scrolled_segment: Option<usize>,
    pub transcript_last_scroll_offset: f32,
    pub transcript_revision: u64,
    transcript_request_id: u64,
    transcript_rx: Option<Receiver<(u64, Result<TranscriptPayload, String>)>>,
    transcript_cancel: Option<process::Cancellation>,
    pub transcript_display_cache: TranscriptDisplayCache,
    pub transcript_export_status: Option<(bool, String)>,
    transcript_export_rx: Option<Receiver<Result<Option<PathBuf>, String>>>,
}

impl Default for WatchExtras {
    fn default() -> Self {
        Self {
            video_url: None,
            like_count_text: None,
            view_count_text: None,
            published_date_text: None,
            tab: Tab::default(),
            sidebar_tab: SidebarTab::default(),
            related_videos: Vec::new(),
            related_metadata: Vec::new(),
            related_chips: Vec::new(),
            selected_related_chip: None,
            related_continuation: None,
            innertube_comments_continuation: None,
            innertube_comments_top_continuation: None,
            innertube_comments_newest_continuation: None,
            innertube_comments_loading: false,
            innertube_comment_meta: HashMap::new(),
            heatmap: Vec::new(),
            cached_heatmap_key: None,
            cached_heatmap_points: Vec::new(),
            cached_sidebar_related_key: None,
            comments: Vec::new(),
            comment_render_cache: Vec::new(),
            comments_next: None,
            comments_total: None,
            comments_error: None,
            comments_loaded: false,
            comments_order: oauth::CommentOrder::default(),
            comments_order_menu_open: false,
            comments_loading: None,
            comments_cancel: None,
            comment_draft: String::new(),
            comment_composer_active: false,
            comment_emoji_picker_open: false,
            reply_emoji_picker_for: None,
            comment_search_query: String::new(),
            editing_comment_id: None,
            editing_comment_draft: String::new(),
            report_modal_open: false,
            report_reason: String::new(),
            report_details: String::new(),
            comment_post_error: None,
            comment_post_rx: None,
            comment_post_cancel: None,
            comment_replies: HashMap::new(),
            comment_ratings: HashMap::new(),
            initial_comment_ratings: HashMap::new(),
            user_modified_comment_ratings: HashSet::new(),
            comment_rating_errors: HashMap::new(),
            comment_action_tokens: None,
            comment_rating_job: None,
            comment_state_sync_rx: None,
            comment_state_sync_cancel: None,
            comment_state_sync_started: false,
            comment_translations: HashMap::new(),
            subtitles: Vec::new(),
            subtitles_error: None,
            subtitles_loaded: false,
            subtitles_loading: None,
            subtitles_cancel: None,
            active_subtitle_url: None,
            active_subtitle_cues: Vec::new(),
            vtt_cache: HashMap::new(),
            temp_subtitle_files: TempSubtitleFiles::default(),
            subtitle_apply_rx: None,
            subtitle_apply_cancel: None,
            ready_subtitle: None,
            transcript: Vec::new(),
            transcript_open: false,
            transcript_query: String::new(),
            transcript_error: None,
            transcript_loading: false,
            transcript_follow: true,
            transcript_show_timestamps: true,
            transcript_font_size: 14.0,
            transcript_high_contrast: false,
            transcript_track_url: None,
            transcript_auto_attempted: false,
            transcript_user_scrolled: false,
            transcript_last_scrolled_segment: None,
            transcript_last_scroll_offset: 0.0,
            transcript_revision: 0,
            transcript_request_id: 0,
            transcript_rx: None,
            transcript_cancel: None,
            transcript_display_cache: TranscriptDisplayCache::default(),
            transcript_export_status: None,
            transcript_export_rx: None,
        }
    }
}

impl WatchExtras {
    pub fn is_posting_comment(&self) -> bool {
        self.comment_post_rx.is_some()
    }

    pub fn is_rating_comment(&self, comment_id: &str) -> bool {
        self.comment_rating_job
            .as_ref()
            .is_some_and(|job| job.comment_id == comment_id)
    }

    pub fn is_rating_any_comment(&self) -> bool {
        self.comment_rating_job.is_some()
    }

    pub fn start_comment_state_sync(
        &mut self,
        web_session: Option<&comment_actions::YouTubeWebSession>,
        ctx: &egui::Context,
    ) {
        if self.comment_state_sync_started || self.comment_state_sync_rx.is_some() {
            return;
        }
        let Some(video_id) = self
            .video_url
            .as_deref()
            .and_then(history::youtube_video_id)
        else {
            return;
        };
        self.comment_state_sync_started = true;
        let order = self.comments_order;
        let cached = web_session.cloned();
        let cancel = process::Cancellation::default();
        self.comment_state_sync_cancel = Some(cancel.clone());
        let (tx, rx) = mpsc::channel();
        self.comment_state_sync_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let (session, auth_expired, result) =
                comment_actions::sync_comment_states_remote(cached, &video_id, order, &cancel);
            let _ = tx.send(CommentStateSyncResult {
                video_id,
                order,
                web_session: session,
                auth_expired,
                result,
            });
            ctx.request_repaint();
        });
    }

    pub fn start_comment_rating(
        &mut self,
        comment_id: &str,
        parent_comment_id: Option<&str>,
        clicked: CommentRating,
        web_session: Option<&comment_actions::YouTubeWebSession>,
        ctx: &egui::Context,
    ) {
        if self.comment_rating_job.is_some() {
            return;
        }
        let Some(video_id) = self
            .video_url
            .as_deref()
            .and_then(history::youtube_video_id)
        else {
            self.comment_rating_errors.insert(
                comment_id.to_owned(),
                "Avalia\u{00e7}\u{00e3}o de coment\u{00e1}rios dispon\u{00ed}vel somente para v\u{00ed}deos do YouTube."
                    .into(),
            );
            return;
        };
        self.comment_rating_errors.remove(comment_id);
        let current = self.comment_ratings.get(comment_id).copied();
        let target = if current == Some(clicked) {
            None
        } else {
            Some(clicked)
        };
        let cache = self
            .comment_action_tokens
            .as_ref()
            .filter(|c| c.video_id == video_id && c.order == self.comments_order)
            .cloned()
            .unwrap_or_else(|| {
                comment_actions::VideoCommentTokens::for_video(&video_id, self.comments_order)
            });
        let cached_session = web_session.cloned();
        let comment_id_owned = comment_id.to_owned();
        let parent_id_owned = parent_comment_id.map(str::to_owned);
        let cancel = process::Cancellation::default();
        let (tx, rx) = mpsc::channel();
        self.comment_rating_job = Some(CommentRatingJob {
            comment_id: comment_id_owned.clone(),
            rx,
            cancel: cancel.clone(),
        });
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let outcome = comment_actions::apply_comment_rating_remote(
                cached_session,
                cache,
                &comment_id_owned,
                parent_id_owned.as_deref(),
                current,
                target,
                &cancel,
            );
            let _ = tx.send(outcome);
            ctx.request_repaint();
        });
    }

    pub fn comment_displayed_likes(&self, comment_id: &str, base_likes: u64) -> u64 {
        let initial_like =
            self.initial_comment_ratings.get(comment_id).copied() == Some(CommentRating::Like);
        let current_like =
            self.comment_ratings.get(comment_id).copied() == Some(CommentRating::Like);
        match (initial_like, current_like) {
            (false, true) => base_likes.saturating_add(1),
            (true, false) => base_likes.saturating_sub(1),
            _ => base_likes,
        }
    }

    pub fn toggle_comment_translation(
        &mut self,
        comment_id: &str,
        text: &str,
        ctx: &egui::Context,
    ) {
        let entry = self
            .comment_translations
            .entry(comment_id.to_owned())
            .or_default();
        if entry.loading {
            return;
        }
        if entry.translated_text.is_some() {
            entry.showing_translated = !entry.showing_translated;
            entry.error = None;
            return;
        }
        entry.loading = true;
        entry.error = None;
        let cancel = process::Cancellation::default();
        entry.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        entry.rx = Some(receiver);
        let source_text = text.to_owned();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = translate_text_to_pt(&source_text, &cancel);
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    pub fn sync_video(&mut self, video_url: Option<&str>) {
        if self.video_url.as_deref() == video_url {
            return;
        }
        if let Some(cancel) = self.subtitles_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.subtitle_apply_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.comments_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.comment_post_cancel.take() {
            cancel.cancel();
        }
        if let Some(job) = self.comment_rating_job.take() {
            job.cancel.cancel();
        }
        if let Some(cancel) = self.comment_state_sync_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.transcript_cancel.take() {
            cancel.cancel();
        }
        for replies in self.comment_replies.values_mut() {
            if let Some(cancel) = replies.cancel.take() {
                cancel.cancel();
            }
            if let Some(cancel) = replies.reply_post_cancel.take() {
                cancel.cancel();
            }
        }
        for translation in self.comment_translations.values_mut() {
            if let Some(cancel) = translation.cancel.take() {
                cancel.cancel();
            }
        }
        let transcript_follow = self.transcript_follow;
        let transcript_show_timestamps = self.transcript_show_timestamps;
        let transcript_font_size = self.transcript_font_size;
        let transcript_high_contrast = self.transcript_high_contrast;
        let comments_order = self.comments_order;
        *self = Self {
            video_url: video_url.map(str::to_owned),
            comments_order,
            transcript_follow,
            transcript_show_timestamps,
            transcript_font_size,
            transcript_high_contrast,
            ..Self::default()
        };
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.sync_video(None);
    }

    pub fn apply_watch_next(&mut self, next: crate::innertube::InnerTubeWatchNext) {
        if next.like_count_text.is_some() {
            self.like_count_text = next.like_count_text;
        }
        if next.view_count_text.is_some() {
            self.view_count_text = next.view_count_text;
        }
        if next.published_date_text.is_some() {
            self.published_date_text = next.published_date_text;
        }
        if !next.related_videos.is_empty() || self.related_videos.is_empty() {
            self.related_videos = next.related_videos;
            self.related_metadata = next.related_metadata;
        }
        if !next.related_chips.is_empty() || self.related_chips.is_empty() {
            self.related_chips = next.related_chips;
            self.selected_related_chip = self.related_chips.iter().position(|c| c.selected);
        }
        self.related_continuation = next.related_continuation;
        if next.comments_continuation.is_some()
            && (!self.comments_loaded || self.innertube_comments_continuation.is_none())
        {
            self.innertube_comments_continuation = next.comments_continuation;
        }
        if !next.heatmap.is_empty() || self.heatmap.is_empty() {
            self.heatmap = next
                .heatmap
                .into_iter()
                .filter(|b| b.duration_ms > 0 && b.intensity.is_finite())
                .map(|b| crate::overlay::HeatmapPoint {
                    start_secs: b.start_ms as f64 / 1000.0,
                    duration_secs: b.duration_ms as f64 / 1000.0,
                    intensity: b.intensity.clamp(0.0, 1.0),
                })
                .collect();
            self.cached_heatmap_key = None;
        }
        self.cached_sidebar_related_key = None;
    }

    pub fn apply_innertube_comments_page(
        &mut self,
        page: crate::innertube::InnerTubeCommentsPage,
        append: bool,
    ) {
        self.innertube_comments_loading = false;
        self.comments_loaded = true;
        self.comments_error = None;
        if !append {
            self.comments.clear();
            self.comment_render_cache.clear();
            self.innertube_comment_meta.clear();
            self.comments_next = None;
        }
        if page.top_sort_continuation.is_some() {
            self.innertube_comments_top_continuation = page.top_sort_continuation;
        }
        if page.newest_sort_continuation.is_some() {
            self.innertube_comments_newest_continuation = page.newest_sort_continuation;
        }
        if let Some(total_raw) = page.total_count_text.as_deref()
            && let Some(parsed_total) = parse_compact_comment_count(total_raw)
        {
            self.comments_total = Some(parsed_total);
        }
        let mut seen_ids: HashSet<String> = self.comments.iter().map(|c| c.id.clone()).collect();
        for item in page.comments {
            if self.comments.len() >= MAX_WATCH_COMMENTS {
                break;
            }
            if item.id.trim().is_empty() || !seen_ids.insert(item.id.clone()) {
                continue;
            }
            let likes = item
                .like_count_text
                .as_deref()
                .and_then(parse_compact_comment_count)
                .unwrap_or(0);
            self.innertube_comment_meta.insert(
                item.id.clone(),
                InnerTubeCommentMeta {
                    is_pinned: item.is_pinned,
                    is_hearted: item.is_hearted,
                    is_owner: item.is_owner,
                    is_member: item.is_member,
                    like_count_text: item.like_count_text,
                    replies_continuation: item.replies_continuation,
                },
            );
            self.comments.push(oauth::Comment {
                id: item.id,
                author: item.author,
                author_avatar: item.author_avatar,
                text: item.text,
                published: item.published_text,
                likes,
                replies: item.reply_count,
            });
        }
        if let Some(total) = self.comments_total {
            self.comments_total = Some(total.max(self.comments.len() as u64));
        }
        self.innertube_comments_continuation = if self.comments.len() >= MAX_WATCH_COMMENTS {
            None
        } else {
            page.continuation
        };
        sync_cached_comment_meta(&self.comments, &mut self.comment_render_cache);
    }

    pub fn apply_preloaded_manifest_extras(
        &mut self,
        manifest: &crate::stream_manifest::StreamManifest,
    ) {
        self.apply_preloaded_subtitles(manifest.to_subtitle_tracks());
    }

    pub fn is_transcript_open(&self) -> bool {
        self.transcript_open || self.sidebar_tab == SidebarTab::Transcript
    }

    pub fn open_transcript(&mut self) {
        self.transcript_open = true;
        self.transcript_user_scrolled = false;
        self.transcript_last_scrolled_segment = None;
    }

    pub fn close_transcript(&mut self) {
        self.transcript_open = false;
        if self.sidebar_tab == SidebarTab::Transcript {
            self.sidebar_tab = SidebarTab::All;
        }
    }

    pub fn preferred_transcript_track(&self, prefer_pt: bool) -> Option<&search::SubtitleTrack> {
        let is_preferred_lang = |lang: &str| {
            let lower = lang.trim().to_ascii_lowercase();
            if prefer_pt {
                lower == "pt" || lower.starts_with("pt-") || lower.starts_with("pt_")
            } else {
                lower == "en" || lower.starts_with("en-") || lower.starts_with("en_")
            }
        };
        if let Some(selected_url) = self.transcript_track_url.as_deref()
            && let Some(track) = self.subtitles.iter().find(|t| t.url == selected_url)
        {
            return Some(track);
        }
        if let Some(active_url) = self.active_subtitle_url.as_deref()
            && let Some(track) = self.subtitles.iter().find(|t| t.url == active_url)
        {
            return Some(track);
        }
        self.subtitles
            .iter()
            .find(|t| !t.automatic && is_preferred_lang(&t.language))
            .or_else(|| self.subtitles.iter().find(|t| !t.automatic))
            .or_else(|| {
                self.subtitles
                    .iter()
                    .find(|t| t.automatic && is_preferred_lang(&t.language))
            })
            .or_else(|| self.subtitles.first())
    }

    pub fn reload_transcript(&mut self, prefer_pt: bool, ctx: &egui::Context) {
        if let Some(cancel) = self.transcript_cancel.take() {
            cancel.cancel();
        }
        self.transcript_rx = None;
        self.transcript_loading = false;
        self.transcript_error = None;
        self.transcript.clear();
        self.transcript_user_scrolled = false;
        self.transcript_last_scrolled_segment = None;
        self.transcript_revision = self.transcript_revision.wrapping_add(1);
        self.transcript_display_cache.valid = false;
        if let Some(track) = self.preferred_transcript_track(prefer_pt).cloned() {
            self.vtt_cache.remove(&track.url);
            self.start_transcript(&track, ctx);
        } else {
            self.transcript_track_url = None;
            self.transcript_auto_attempted = false;
            self.refresh_subtitles(ctx);
        }
    }

    pub fn reset_comments(&mut self) {
        if let Some(cancel) = self.comments_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.comment_post_cancel.take() {
            cancel.cancel();
        }
        if let Some(job) = self.comment_rating_job.take() {
            job.cancel.cancel();
        }
        if let Some(cancel) = self.comment_state_sync_cancel.take() {
            cancel.cancel();
        }
        for replies in self.comment_replies.values_mut() {
            if let Some(cancel) = replies.cancel.take() {
                cancel.cancel();
            }
            if let Some(cancel) = replies.reply_post_cancel.take() {
                cancel.cancel();
            }
        }
        for translation in self.comment_translations.values_mut() {
            if let Some(cancel) = translation.cancel.take() {
                cancel.cancel();
            }
        }
        self.comments.clear();
        self.comment_render_cache.clear();
        self.comments_next = None;
        self.comments_total = None;
        self.comments_error = None;
        self.comments_loaded = false;
        self.comments_loading = None;
        self.comments_order_menu_open = false;
        self.comment_composer_active = false;
        self.comment_emoji_picker_open = false;
        self.reply_emoji_picker_for = None;
        self.comment_search_query.clear();
        self.editing_comment_id = None;
        self.editing_comment_draft.clear();
        self.comment_post_rx = None;
        self.comment_post_error = None;
        self.comment_replies.clear();
        self.comment_ratings.clear();
        self.initial_comment_ratings.clear();
        self.user_modified_comment_ratings.clear();
        self.comment_rating_errors.clear();
        self.comment_action_tokens = None;
        self.comment_state_sync_rx = None;
        self.comment_state_sync_started = false;
        self.comment_translations.clear();
        self.innertube_comments_loading = false;
        self.innertube_comment_meta.clear();
    }

    pub fn set_comments_order(
        &mut self,
        order: oauth::CommentOrder,
        session: Option<&oauth::TokenSession>,
        ctx: &egui::Context,
    ) {
        self.comments_order_menu_open = false;
        if self.comments_order == order {
            return;
        }
        self.comments_order = order;
        self.reset_comments();
        self.start_comments(session, false, ctx);
    }

    pub fn start_comments(
        &mut self,
        session: Option<&oauth::TokenSession>,
        next: bool,
        ctx: &egui::Context,
    ) {
        if self.comments_loading.is_some() {
            return;
        }
        let Some(mut session) = session.cloned() else {
            self.comments_error = Some("Entre novamente para consultar comentários.".into());
            return;
        };
        if !session.has_comment_scope() {
            self.comments_error = Some(
                "A sessão conectada não tem permissão para ler comentários. Use \"Autorizar comentários\".".to_owned(),
            );
            return;
        }
        let Some(video_id) = self
            .video_url
            .as_deref()
            .and_then(history::youtube_video_id)
        else {
            self.comments_error =
                Some("Comentários disponíveis somente para vídeos do YouTube.".into());
            return;
        };
        let page = if next {
            self.comments_next.clone()
        } else {
            None
        };
        if next && page.is_none() {
            return;
        }
        if !next {
            self.comments.clear();
            self.comments_next = None;
        }
        self.comments_error = None;
        let order = self.comments_order;
        let cancel = process::Cancellation::default();
        self.comments_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.comments_loading = Some(receiver);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = session.refresh_if_needed(&cancel).and_then(|()| {
                oauth::comments_page_ordered(
                    &session.access_token,
                    &video_id,
                    order,
                    page.as_deref(),
                    &cancel,
                )
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    pub fn start_post_comment(
        &mut self,
        session: Option<&oauth::TokenSession>,
        ctx: &egui::Context,
    ) {
        if self.comment_post_rx.is_some() {
            return;
        }
        let Some(mut session) = session.cloned() else {
            self.comment_post_error = Some("Entre com sua conta Google para comentar.".into());
            return;
        };
        if !session.has_comment_scope() {
            self.comment_post_error = Some(
                "A sessão atual não possui permissão de escrita. Use \"Autorizar comentários\"."
                    .into(),
            );
            return;
        }
        let Some(video_id) = self
            .video_url
            .as_deref()
            .and_then(history::youtube_video_id)
        else {
            self.comment_post_error =
                Some("Comentários disponíveis somente para vídeos do YouTube.".into());
            return;
        };
        let text = self.comment_draft.trim().to_owned();
        if text.is_empty() {
            self.comment_post_error = Some("Escreva um comentário antes de enviar.".into());
            return;
        }
        self.comment_post_error = None;
        let cancel = process::Cancellation::default();
        self.comment_post_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.comment_post_rx = Some(receiver);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = session.refresh_if_needed(&cancel).and_then(|()| {
                oauth::post_comment(&session.access_token, &video_id, &text, &cancel)
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    pub fn start_post_reply(
        &mut self,
        comment_id: &str,
        session: Option<&oauth::TokenSession>,
        ctx: &egui::Context,
    ) {
        let Some(mut session) = session.cloned() else {
            self.comment_replies
                .entry(comment_id.to_owned())
                .or_default()
                .reply_post_error = Some("Entre com sua conta Google para responder.".into());
            return;
        };
        if !session.has_comment_scope() {
            self.comment_replies
                .entry(comment_id.to_owned())
                .or_default()
                .reply_post_error =
                Some("A sessão atual não possui permissão de escrita para respostas.".into());
            return;
        }
        let state = self
            .comment_replies
            .entry(comment_id.to_owned())
            .or_default();
        if state.reply_post_rx.is_some() {
            return;
        }
        let text = state.reply_draft.trim().to_owned();
        if text.is_empty() {
            state.reply_post_error = Some("Escreva uma resposta antes de enviar.".into());
            return;
        }
        state.reply_post_error = None;
        let cancel = process::Cancellation::default();
        state.reply_post_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        state.reply_post_rx = Some(receiver);
        let parent_id = comment_id.to_owned();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = session.refresh_if_needed(&cancel).and_then(|()| {
                oauth::post_comment_reply(&session.access_token, &parent_id, &text, &cancel)
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    pub fn start_replies(
        &mut self,
        comment_id: &str,
        next: bool,
        session: Option<&oauth::TokenSession>,
        ctx: &egui::Context,
    ) {
        let Some(mut session) = session.cloned() else {
            self.comment_replies
                .entry(comment_id.to_owned())
                .or_default()
                .error = Some("Entre novamente para consultar respostas.".into());
            return;
        };
        let state = self
            .comment_replies
            .entry(comment_id.to_owned())
            .or_default();
        if state.loading.is_some() {
            return;
        }
        let page = if next { state.next.clone() } else { None };
        if next && page.is_none() {
            return;
        }
        if !next {
            state.comments.clear();
            state.next = None;
        }
        state.error = None;
        let cancel = process::Cancellation::default();
        state.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        state.loading = Some(receiver);
        let parent_id = comment_id.to_owned();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = session.refresh_if_needed(&cancel).and_then(|()| {
                oauth::comment_replies_page(
                    &session.access_token,
                    &parent_id,
                    page.as_deref(),
                    &cancel,
                )
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }
    pub fn start_subtitles(&mut self, ctx: &egui::Context) {
        self.start_subtitles_inner(false, ctx);
    }

    pub fn refresh_subtitles(&mut self, ctx: &egui::Context) {
        self.start_subtitles_inner(true, ctx);
    }

    fn start_subtitles_inner(&mut self, force_refresh: bool, ctx: &egui::Context) {
        if !force_refresh && self.subtitles_loading.is_some() {
            return;
        }
        let Some(url) = self.video_url.clone() else {
            return;
        };
        if force_refresh {
            if let Some(cancel) = self.subtitles_cancel.take() {
                cancel.cancel();
            }
            self.subtitles_loading = None;
            self.vtt_cache.clear();
        }
        let cancel = process::Cancellation::default();
        self.subtitles_cancel = Some(cancel.clone());
        self.subtitles_error = None;
        let (sender, receiver) = mpsc::channel();
        self.subtitles_loading = Some(receiver);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = if force_refresh {
                search::youtube_subtitles_fresh(&url, &cancel)
            } else {
                search::youtube_subtitles(&url, &cancel)
            };
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    pub fn apply_preloaded_subtitles(&mut self, tracks: Vec<search::SubtitleTrack>) {
        if self.subtitles_loaded || self.subtitles_loading.is_some() {
            return;
        }
        self.subtitles = search::sanitize_subtitle_tracks(tracks);
        self.subtitles_loaded = true;
        self.subtitles_error = None;
    }

    pub fn cancel_active_subtitle(&mut self) {
        if let Some(cancel) = self.subtitle_apply_cancel.take() {
            cancel.cancel();
        }
        self.subtitle_apply_rx = None;
        self.ready_subtitle = None;
        self.active_subtitle_url = None;
        self.active_subtitle_cues.clear();
        self.subtitles_error = None;
    }

    fn cache_vtt(&mut self, url: &str, vtt: String) {
        if self.vtt_cache.len() >= SUBTITLE_VTT_CACHE_LIMIT && !self.vtt_cache.contains_key(url) {
            self.vtt_cache.clear();
        }
        self.vtt_cache.insert(url.to_owned(), vtt);
    }

    fn ensure_temp_subtitle_file(&mut self, url: &str, vtt: &str) -> Result<PathBuf, String> {
        if let Some(existing) = self.temp_subtitle_files.0.get(url)
            && existing.exists()
        {
            return Ok(existing.clone());
        }
        let dir = std::env::temp_dir();
        for _ in 0..8 {
            let seq = SUBTITLE_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!("fasttube-sub-{}-{seq}.vtt", std::process::id()));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    if file.write_all(vtt.as_bytes()).is_err() {
                        let _ = std::fs::remove_file(&path);
                        return Err(
                            "Não foi possível gravar o arquivo temporário de legenda.".into()
                        );
                    }
                    self.temp_subtitle_files
                        .0
                        .insert(url.to_owned(), path.clone());
                    return Ok(path);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => {
                    return Err("Não foi possível criar o arquivo temporário de legenda.".into());
                }
            }
        }
        Err("Não foi possível preparar o arquivo temporário de legenda.".into())
    }

    pub fn is_subtitle_applying(&self) -> bool {
        self.subtitle_apply_rx.is_some()
    }

    pub fn take_ready_subtitle(&mut self) -> Option<ReadySubtitle> {
        self.ready_subtitle.take()
    }

    pub fn start_apply_subtitle(
        &mut self,
        track: &search::SubtitleTrack,
        label: String,
        ctx: &egui::Context,
    ) {
        if let Some(cancel) = self.subtitle_apply_cancel.take() {
            cancel.cancel();
        }
        self.subtitle_apply_rx = None;
        self.ready_subtitle = None;
        self.subtitles_error = None;
        self.active_subtitle_cues.clear();
        self.active_subtitle_url = Some(track.url.clone());
        if let Some(vtt) = self.vtt_cache.get(&track.url).cloned() {
            match crate::transcript::parse_vtt(&vtt) {
                Ok(cues) if !cues.is_empty() => {
                    self.active_subtitle_cues = cues;
                    match self.ensure_temp_subtitle_file(&track.url, &vtt) {
                        Ok(path) => {
                            self.ready_subtitle = Some(ReadySubtitle {
                                url: track.url.clone(),
                                path,
                                label,
                                language: track.language.clone(),
                            });
                        }
                        Err(error) => {
                            self.active_subtitle_url = None;
                            self.active_subtitle_cues.clear();
                            self.subtitles_error = Some(error);
                        }
                    }
                }
                _ => {
                    self.vtt_cache.remove(&track.url);
                    self.active_subtitle_url = None;
                    self.active_subtitle_cues.clear();
                    self.subtitles_error =
                        Some("A faixa selecionada não contém texto de legenda.".into());
                }
            }
            return;
        }
        let video_url = self.video_url.clone();
        let track_owned = track.clone();
        let cancel = process::Cancellation::default();
        self.subtitle_apply_cancel = Some(cancel.clone());
        let (tx, rx) = mpsc::channel();
        self.subtitle_apply_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = if cancel.cancelled() {
                Err("Download da legenda cancelado.".into())
            } else {
                search::download_vtt_for_track(video_url.as_deref(), &track_owned, &cancel)
                    .and_then(|vtt| {
                        let cues = crate::transcript::parse_vtt(&vtt).map_err(str::to_owned)?;
                        if cues.is_empty() {
                            return Err("A faixa selecionada não contém texto de legenda.".into());
                        }
                        Ok(DownloadedSubtitle {
                            url: track_owned.url,
                            label,
                            language: track_owned.language,
                            vtt,
                            cues,
                        })
                    })
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    pub fn start_transcript(&mut self, track: &search::SubtitleTrack, ctx: &egui::Context) {
        if let Some(cancel) = self.transcript_cancel.take() {
            cancel.cancel();
        }
        self.transcript_rx = None;
        self.transcript_loading = false;
        self.transcript_request_id = self.transcript_request_id.wrapping_add(1);
        let request_id = self.transcript_request_id;
        self.transcript_track_url = Some(track.url.clone());
        self.transcript_auto_attempted = true;
        self.transcript_error = None;
        self.transcript.clear();
        self.transcript_query.clear();
        self.transcript_user_scrolled = false;
        self.transcript_last_scrolled_segment = None;
        self.transcript_revision = self.transcript_revision.wrapping_add(1);
        self.transcript_display_cache.valid = false;
        if self.active_subtitle_url.as_deref() == Some(track.url.as_str())
            && !self.active_subtitle_cues.is_empty()
        {
            self.transcript = self.active_subtitle_cues.clone();
            self.transcript_revision = self.transcript_revision.wrapping_add(1);
            return;
        }
        if let Some(vtt) = self.vtt_cache.get(&track.url) {
            match crate::transcript::parse_vtt(vtt).map_err(str::to_owned) {
                Ok(cues) if cues.is_empty() => {
                    self.transcript_error =
                        Some("A faixa selecionada não contém trechos de legenda.".into());
                }
                Ok(cues) => {
                    self.transcript = cues;
                    self.transcript_revision = self.transcript_revision.wrapping_add(1);
                }
                Err(error) => self.transcript_error = Some(error),
            }
            return;
        }
        self.transcript_loading = true;
        let video_url = self.video_url.clone();
        let track_owned = track.clone();
        let cancel = process::Cancellation::default();
        self.transcript_cancel = Some(cancel.clone());
        let (tx, rx) = mpsc::channel();
        self.transcript_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = if cancel.cancelled() {
                Err("Consulta cancelada.".into())
            } else {
                search::download_vtt_for_track(video_url.as_deref(), &track_owned, &cancel)
                    .and_then(|vtt| {
                        let cues = crate::transcript::parse_vtt(&vtt).map_err(str::to_owned)?;
                        if cues.is_empty() {
                            return Err("A faixa selecionada não contém trechos de legenda.".into());
                        }
                        Ok(TranscriptPayload {
                            url: track_owned.url,
                            vtt,
                            cues,
                        })
                    })
            };
            let _ = tx.send((request_id, result));
            ctx.request_repaint();
        });
    }

    pub fn is_exporting_transcript(&self) -> bool {
        self.transcript_export_rx.is_some()
    }

    pub fn start_transcript_export(
        &mut self,
        video_title: &str,
        cues: &[crate::transcript::Cue],
        ctx: &egui::Context,
    ) {
        if self.transcript_export_rx.is_some() || cues.is_empty() {
            return;
        }
        self.transcript_export_status = None;
        let title_owned = video_title.trim().to_owned();
        let safe_stem: String = title_owned
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_') {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .trim()
            .chars()
            .take(48)
            .collect();
        let default_name = if safe_stem.is_empty() {
            "transcricao.txt".to_owned()
        } else {
            format!("{safe_stem}-transcricao.txt")
        };
        let cues_owned = cues.to_vec();
        let (tx, rx) = mpsc::channel();
        self.transcript_export_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result =
                crate::settings::pick_text_export_dialog("Exportar transcrição", &default_name)
                    .and_then(|maybe_path| {
                        let Some(path) = maybe_path else {
                            return Ok(None);
                        };
                        let ext = path.extension().and_then(|ext| ext.to_str());
                        let body = if ext.is_some_and(|ext| ext.eq_ignore_ascii_case("md")) {
                            crate::transcript::format_markdown(&title_owned, &cues_owned)
                        } else if ext.is_some_and(|ext| ext.eq_ignore_ascii_case("srt")) {
                            crate::transcript::format_srt(&cues_owned)
                        } else {
                            crate::transcript::format_txt(&cues_owned)
                        };
                        history::write_atomic(&path, body.as_bytes()).map_err(|error| {
                            format!(
                                "Não foi possível salvar a transcrição em '{}': {error}",
                                path.display()
                            )
                        })?;
                        Ok(Some(path))
                    });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    pub fn poll(&mut self) {
        let subtitle_apply_result = self.subtitle_apply_rx.as_ref().map(Receiver::try_recv);
        match subtitle_apply_result {
            Some(Ok(Ok(downloaded))) => {
                self.active_subtitle_cues = downloaded.cues;
                self.cache_vtt(&downloaded.url, downloaded.vtt.clone());
                match self.ensure_temp_subtitle_file(&downloaded.url, &downloaded.vtt) {
                    Ok(path) => {
                        self.active_subtitle_url = Some(downloaded.url.clone());
                        self.ready_subtitle = Some(ReadySubtitle {
                            url: downloaded.url,
                            path,
                            label: downloaded.label,
                            language: downloaded.language,
                        });
                    }
                    Err(error) => {
                        self.active_subtitle_url = None;
                        self.active_subtitle_cues.clear();
                        self.subtitles_error = Some(error);
                    }
                }
                self.subtitle_apply_rx = None;
                self.subtitle_apply_cancel = None;
            }
            Some(Ok(Err(error))) => {
                self.active_subtitle_url = None;
                self.active_subtitle_cues.clear();
                self.subtitles_error = Some(error);
                self.subtitle_apply_rx = None;
                self.subtitle_apply_cancel = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.active_subtitle_url = None;
                self.active_subtitle_cues.clear();
                self.subtitles_error = Some("Download da legenda interrompido.".into());
                self.subtitle_apply_rx = None;
                self.subtitle_apply_cancel = None;
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }
        let transcript_result = self.transcript_rx.as_ref().map(Receiver::try_recv);
        match transcript_result {
            Some(Ok((recv_id, Ok(payload)))) => {
                if recv_id == self.transcript_request_id {
                    self.cache_vtt(&payload.url, payload.vtt);
                    self.transcript_track_url = Some(payload.url);
                    self.transcript = payload.cues;
                    self.transcript_revision = self.transcript_revision.wrapping_add(1);
                    self.transcript_loading = false;
                    self.transcript_rx = None;
                    self.transcript_cancel = None;
                }
            }
            Some(Ok((recv_id, Err(error)))) => {
                if recv_id == self.transcript_request_id {
                    self.transcript_error = Some(error);
                    self.transcript_loading = false;
                    self.transcript_rx = None;
                    self.transcript_cancel = None;
                }
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.transcript_error = Some("Consulta de transcrição interrompida.".into());
                self.transcript_loading = false;
                self.transcript_rx = None;
                self.transcript_cancel = None;
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }
        match self.comments_loading.as_ref().map(Receiver::try_recv) {
            Some(Ok(Ok(page))) => {
                self.comments_loaded = true;
                let remaining = MAX_WATCH_COMMENTS.saturating_sub(self.comments.len());
                if remaining > 0 {
                    self.comments
                        .extend(page.comments.into_iter().take(remaining));
                }
                if let Some(total) = page.total_results {
                    self.comments_total = Some(total.max(self.comments.len() as u64));
                }
                self.comments_next = if self.comments.len() >= MAX_WATCH_COMMENTS {
                    None
                } else {
                    page.next_page
                };
                self.comments_loading = None;
                self.comments_cancel = None;
            }
            Some(Ok(Err(error))) => {
                self.comments_loaded = true;
                self.comments_error = Some(error);
                self.comments_loading = None;
                self.comments_cancel = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.comments_loaded = true;
                self.comments_error = Some("Consulta de comentários interrompida.".into());
                self.comments_loading = None;
                self.comments_cancel = None;
            }
            _ => {}
        }
        match self.comment_post_rx.as_ref().map(Receiver::try_recv) {
            Some(Ok(Ok(comment))) => {
                self.comments_loaded = true;
                self.comments.insert(0, comment);
                if self.comments.len() > MAX_WATCH_COMMENTS {
                    self.comments.truncate(MAX_WATCH_COMMENTS);
                }
                if let Some(total) = &mut self.comments_total {
                    *total = total.saturating_add(1);
                }
                self.comment_draft.clear();
                self.comment_composer_active = false;
                self.comment_emoji_picker_open = false;
                self.comment_post_error = None;
                self.comment_post_rx = None;
                self.comment_post_cancel = None;
            }
            Some(Ok(Err(error))) => {
                self.comment_post_error = Some(error);
                self.comment_post_rx = None;
                self.comment_post_cancel = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.comment_post_error = Some("Envio de comentário interrompido.".into());
                self.comment_post_rx = None;
                self.comment_post_cancel = None;
            }
            _ => {}
        }
        for translation in self.comment_translations.values_mut() {
            match translation.rx.as_ref().map(Receiver::try_recv) {
                Some(Ok(Ok(translated))) => {
                    translation.translated_lines = parse_timestamp_lines(&translated);
                    translation.translated_text = Some(translated);
                    translation.showing_translated = true;
                    translation.loading = false;
                    translation.error = None;
                    translation.rx = None;
                    translation.cancel = None;
                }
                Some(Ok(Err(error))) => {
                    translation.loading = false;
                    translation.error = Some(error);
                    translation.rx = None;
                    translation.cancel = None;
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    translation.loading = false;
                    translation.error = Some("Tradu\u{00e7}\u{00e3}o interrompida.".into());
                    translation.rx = None;
                    translation.cancel = None;
                }
                _ => {}
            }
        }
        let mut incremented_parents: Vec<String> = Vec::new();
        for (parent_id, replies) in self.comment_replies.iter_mut() {
            match replies.loading.as_ref().map(Receiver::try_recv) {
                Some(Ok(Ok(page))) => {
                    replies.loaded = true;
                    replies.comments.extend(page.comments);
                    replies.next = page.next_page;
                    replies.loading = None;
                    replies.cancel = None;
                }
                Some(Ok(Err(error))) => {
                    replies.loaded = true;
                    replies.error = Some(error);
                    replies.loading = None;
                    replies.cancel = None;
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    replies.loaded = true;
                    replies.error = Some("Consulta de respostas interrompida.".into());
                    replies.loading = None;
                    replies.cancel = None;
                }
                _ => {}
            }
            match replies.reply_post_rx.as_ref().map(Receiver::try_recv) {
                Some(Ok(Ok(reply))) => {
                    replies.loaded = true;
                    replies.comments.push(reply);
                    replies.reply_draft.clear();
                    replies.reply_open = false;
                    replies.reply_post_error = None;
                    replies.reply_post_rx = None;
                    replies.reply_post_cancel = None;
                    incremented_parents.push(parent_id.clone());
                }
                Some(Ok(Err(error))) => {
                    replies.reply_post_error = Some(error);
                    replies.reply_post_rx = None;
                    replies.reply_post_cancel = None;
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    replies.reply_post_error = Some("Envio de resposta interrompido.".into());
                    replies.reply_post_rx = None;
                    replies.reply_post_cancel = None;
                }
                _ => {}
            }
        }
        for parent_id in incremented_parents {
            if let Some(comment) = self.comments.iter_mut().find(|c| c.id == parent_id) {
                comment.replies = comment.replies.saturating_add(1);
            }
        }
        match self.subtitles_loading.as_ref().map(Receiver::try_recv) {
            Some(Ok(Ok(tracks))) => {
                self.subtitles = search::sanitize_subtitle_tracks(tracks);
                self.subtitles_loaded = true;
                self.subtitles_error = None;
                self.subtitles_loading = None;
                self.subtitles_cancel = None;
            }
            Some(Ok(Err(error))) => {
                self.subtitles_loaded = true;
                self.subtitles_error = Some(error);
                self.subtitles_loading = None;
                self.subtitles_cancel = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.subtitles_loaded = true;
                self.subtitles_error = Some("Consulta de legendas interrompida.".into());
                self.subtitles_loading = None;
                self.subtitles_cancel = None;
            }
            _ => {}
        }
        match self.transcript_export_rx.as_ref().map(Receiver::try_recv) {
            Some(Ok(Ok(Some(path)))) => {
                self.transcript_export_status =
                    Some((true, format!("Transcrição salva em '{}'.", path.display())));
                self.transcript_export_rx = None;
            }
            Some(Ok(Ok(None))) => {
                self.transcript_export_rx = None;
            }
            Some(Ok(Err(error))) => {
                self.transcript_export_status = Some((false, error));
                self.transcript_export_rx = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.transcript_export_status =
                    Some((false, "Exportação de transcrição interrompida.".into()));
                self.transcript_export_rx = None;
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }
    }

    pub fn poll_with_session(
        &mut self,
        web_session: &mut Option<comment_actions::YouTubeWebSession>,
        web_session_checked: &mut bool,
    ) -> Option<String> {
        self.poll();

        let current_video_id = self
            .video_url
            .as_deref()
            .and_then(history::youtube_video_id);

        match self.comment_state_sync_rx.as_ref().map(Receiver::try_recv) {
            Some(Ok(sync_res)) => {
                if current_video_id.as_deref() == Some(sync_res.video_id.as_str())
                    && self.comments_order == sync_res.order
                {
                    *web_session_checked = true;
                    if sync_res.auth_expired {
                        *web_session = None;
                    } else if let Some(sess) = sync_res.web_session {
                        *web_session = Some(sess);
                    }
                    if let Ok(tokens) = sync_res.result {
                        let collected = tokens.collected_states();
                        match &mut self.comment_action_tokens {
                            Some(existing)
                                if existing.video_id == tokens.video_id
                                    && existing.order == tokens.order =>
                            {
                                existing.merge_from(tokens);
                            }
                            slot => *slot = Some(tokens),
                        }
                        for (id, state) in collected {
                            self.initial_comment_ratings.insert(id.clone(), state);
                            if !self.user_modified_comment_ratings.contains(&id) {
                                self.comment_ratings.insert(id, state);
                            }
                        }
                    }
                }
                self.comment_state_sync_rx = None;
                self.comment_state_sync_cancel = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.comment_state_sync_rx = None;
                self.comment_state_sync_cancel = None;
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }

        let rating_recv = self
            .comment_rating_job
            .as_ref()
            .map(|job| (job.comment_id.clone(), job.rx.try_recv()));
        match rating_recv {
            Some((comment_id, Ok(outcome))) => {
                *web_session_checked = true;
                *web_session = outcome.web_session;
                if current_video_id.as_deref() == Some(outcome.cache.video_id.as_str())
                    && self.comments_order == outcome.cache.order
                {
                    match &mut self.comment_action_tokens {
                        Some(existing)
                            if existing.video_id == outcome.cache.video_id
                                && existing.order == outcome.cache.order =>
                        {
                            existing.merge_from(outcome.cache);
                        }
                        slot => *slot = Some(outcome.cache),
                    }
                }
                self.comment_rating_job = None;
                match outcome.result {
                    Ok(applied) => {
                        self.user_modified_comment_ratings
                            .insert(comment_id.clone());
                        self.comment_rating_errors.remove(&comment_id);
                        match applied {
                            Some(rating) => {
                                self.comment_ratings.insert(comment_id, rating);
                            }
                            None => {
                                self.comment_ratings.remove(&comment_id);
                            }
                        }
                        return Some(match applied {
                            Some(CommentRating::Like) => {
                                "Gostei registrado no coment\u{00e1}rio.".into()
                            }
                            Some(CommentRating::Dislike) => {
                                "N\u{00e3}o gostei registrado no coment\u{00e1}rio.".into()
                            }
                            None => "Avalia\u{00e7}\u{00e3}o do coment\u{00e1}rio removida.".into(),
                        });
                    }
                    Err(err) => {
                        self.comment_rating_errors.insert(comment_id, err.clone());
                        return Some(err);
                    }
                }
            }
            Some((comment_id, Err(TryRecvError::Disconnected))) => {
                self.comment_rating_job = None;
                let err = "Envio da avalia\u{00e7}\u{00e3}o de coment\u{00e1}rio interrompido."
                    .to_owned();
                self.comment_rating_errors.insert(comment_id, err.clone());
                return Some(err);
            }
            Some((_, Err(TryRecvError::Empty))) | None => {}
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_reply(id: &str) -> oauth::Comment {
        oauth::Comment {
            id: id.into(),
            author: "Ana".into(),
            author_avatar: None,
            text: "Resposta".into(),
            published: String::new(),
            likes: 0,
            replies: 0,
        }
    }

    #[test]
    fn changing_video_discards_pending_responses() {
        let mut state = WatchExtras::default();
        state.sync_video(Some("https://www.youtube.com/watch?v=one"));
        let (sender, receiver) = mpsc::channel();
        state.comments_loading = Some(receiver);
        let cancel = process::Cancellation::default();
        state.comments_cancel = Some(cancel.clone());
        state.sync_video(Some("https://www.youtube.com/watch?v=two"));
        assert!(cancel.cancelled());
        assert!(
            sender
                .send(Ok(oauth::CommentsPage {
                    comments: vec![],
                    next_page: None,
                    total_results: None,
                }))
                .is_err()
        );
        assert!(state.comments_loading.is_none());
        assert_eq!(
            state.video_url.as_deref(),
            Some("https://www.youtube.com/watch?v=two")
        );
    }

    #[test]
    fn missing_comment_scope_sets_error_without_loading() {
        let mut state = WatchExtras::default();
        state.sync_video(Some("https://www.youtube.com/watch?v=video000001"));
        state.start_comments(
            Some(&oauth::TokenSession::test_session_without_comment_scope(
                "old",
            )),
            false,
            &egui::Context::default(),
        );
        assert!(state.comments_loading.is_none());
        assert!(!state.comments_loaded);
        assert!(
            state
                .comments_error
                .as_deref()
                .unwrap()
                .contains("Autorizar coment\u{00e1}rios")
        );
    }

    #[test]
    fn reset_comments_clears_state_and_cancels_pending_requests() {
        let mut state = WatchExtras {
            comments: vec![oauth::Comment {
                id: "comment-1".into(),
                author: "Ana".into(),
                author_avatar: None,
                text: "texto".into(),
                published: String::new(),
                likes: 0,
                replies: 1,
            }],
            comments_next: Some("next".into()),
            comments_error: Some("erro".into()),
            comments_loaded: true,
            ..WatchExtras::default()
        };
        let comments_cancel = process::Cancellation::default();
        state.comments_cancel = Some(comments_cancel.clone());
        let (sender, receiver) = mpsc::channel();
        state.comments_loading = Some(receiver);
        let replies_cancel = process::Cancellation::default();
        let replies = state.comment_replies.entry("comment-1".into()).or_default();
        let (reply_sender, reply_receiver) = mpsc::channel();
        replies.loading = Some(reply_receiver);
        replies.cancel = Some(replies_cancel.clone());

        state.reset_comments();

        assert!(comments_cancel.cancelled());
        assert!(replies_cancel.cancelled());
        assert!(
            sender
                .send(Ok(oauth::CommentsPage {
                    comments: vec![],
                    next_page: None,
                    total_results: None,
                }))
                .is_err()
        );
        assert!(
            reply_sender
                .send(Ok(oauth::CommentsPage {
                    comments: vec![],
                    next_page: None,
                    total_results: None,
                }))
                .is_err()
        );
        assert!(state.comments.is_empty());
        assert!(state.comments_next.is_none());
        assert!(state.comments_error.is_none());
        assert!(!state.comments_loaded);
        assert!(state.comments_loading.is_none());
        assert!(state.comment_replies.is_empty());
    }

    #[test]
    fn comments_pages_append_without_truncating_limit() {
        let mut state = WatchExtras::default();
        let comment = oauth::Comment {
            id: "comment-1".into(),
            author: "Ana".into(),
            author_avatar: None,
            text: "Olá".into(),
            published: String::new(),
            likes: 0,
            replies: 0,
        };
        state.comments = vec![comment.clone(); 100];
        let (sender, receiver) = mpsc::channel();
        state.comments_loading = Some(receiver);
        assert!(
            sender
                .send(Ok(oauth::CommentsPage {
                    comments: vec![comment; 50],
                    next_page: Some("next-page-token".into()),
                    total_results: Some(203),
                }))
                .is_ok()
        );
        state.poll();
        assert_eq!(state.comments.len(), 150);
        assert_eq!(state.comments_next.as_deref(), Some("next-page-token"));
        assert_eq!(state.comments_total, Some(203));
        assert!(state.comments_loaded);
    }

    #[test]
    fn failed_first_reply_page_can_retry_from_the_first_page() {
        let mut extras = WatchExtras::default();
        let (sender, receiver) = mpsc::channel();
        let state = extras
            .comment_replies
            .entry("comment-1".into())
            .or_default();
        state.loading = Some(receiver);
        sender.send(Err("falha temporária".into())).unwrap();

        extras.poll();

        let state = extras.comment_replies.get("comment-1").unwrap();
        assert!(state.loaded);
        assert_eq!(state.error.as_deref(), Some("falha temporária"));
        assert!(!state.retry_uses_next_page());
    }

    #[test]
    fn failed_later_reply_page_keeps_replies_and_retries_same_page() {
        let mut extras = WatchExtras::default();
        let (sender, receiver) = mpsc::channel();
        let state = extras
            .comment_replies
            .entry("comment-1".into())
            .or_default();
        state.comments = vec![sample_reply("reply-1")];
        state.next = Some("page-2".into());
        state.loaded = true;
        state.loading = Some(receiver);
        sender.send(Err("falha temporária".into())).unwrap();

        extras.poll();

        let state = extras.comment_replies.get("comment-1").unwrap();
        assert_eq!(state.comments.len(), 1);
        assert_eq!(state.comments[0].id, "reply-1");
        assert_eq!(state.next.as_deref(), Some("page-2"));
        assert_eq!(state.error.as_deref(), Some("falha temporária"));
        assert!(state.retry_uses_next_page());
    }

    #[test]
    fn changing_video_cancels_reply_request_and_discards_obsolete_result() {
        let mut state = WatchExtras::default();
        state.sync_video(Some("https://www.youtube.com/watch?v=one"));
        let cancel = process::Cancellation::default();
        let (sender, receiver) = mpsc::channel();
        let replies = state.comment_replies.entry("comment-1".into()).or_default();
        replies.loading = Some(receiver);
        replies.cancel = Some(cancel.clone());

        state.sync_video(Some("https://www.youtube.com/watch?v=two"));

        assert!(cancel.cancelled());
        assert!(
            sender
                .send(Ok(oauth::CommentsPage {
                    comments: vec![sample_reply("obsolete-reply")],
                    next_page: None,
                    total_results: None,
                }))
                .is_err()
        );
        state.poll();
        assert!(!state.comment_replies.contains_key("comment-1"));
        assert_eq!(
            state.video_url.as_deref(),
            Some("https://www.youtube.com/watch?v=two")
        );
    }

    #[test]
    fn subtitle_cache_prepares_temp_file_and_cleans_up_on_video_change() {
        let mut extras = WatchExtras::default();
        extras.sync_video(Some("https://www.youtube.com/watch?v=one"));
        let track = search::SubtitleTrack {
            language: "pt-BR".into(),
            automatic: false,
            url: "https://www.youtube.com/api/timedtext?v=one&lang=pt-BR".into(),
        };
        let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:03.000\nOlá mundo\n".to_owned();
        let cues = crate::transcript::parse_vtt(&vtt).unwrap();
        let (tx, rx) = mpsc::channel();
        extras.subtitle_apply_rx = Some(rx);
        tx.send(Ok(DownloadedSubtitle {
            url: track.url.clone(),
            label: "pt-BR".into(),
            language: "pt-BR".into(),
            vtt,
            cues,
        }))
        .unwrap();

        extras.poll();

        let ready = extras
            .take_ready_subtitle()
            .expect("ready subtitle should be produced");
        assert!(ready.path.exists());
        let saved = std::fs::read_to_string(&ready.path).unwrap();
        assert!(saved.contains("Olá mundo"));

        // Reusing cache for transcript avoids network and parses cues immediately.
        extras.start_transcript(&track, &egui::Context::default());
        assert!(!extras.transcript_loading);
        assert_eq!(extras.transcript.len(), 1);
        assert_eq!(extras.transcript[0].text, "Olá mundo");

        // Switching video cleans up the temporary .vtt file.
        extras.sync_video(Some("https://www.youtube.com/watch?v=two"));
        assert!(!ready.path.exists());
    }

    #[test]
    fn extract_timestamps_parses_mm_ss_and_h_mm_ss_with_boundaries() {
        let text =
            "Intro 0:00, Setup (1:25), Deep dive [1:02:03]! Ignored: 99:60, a1:20, 1:2:3, 123:45.";
        let found = extract_timestamps(text);
        assert_eq!(found.len(), 3);
        assert_eq!((found[0].label.as_str(), found[0].seconds), ("0:00", 0));
        assert_eq!((found[1].label.as_str(), found[1].seconds), ("1:25", 85));
        assert_eq!(
            (found[2].label.as_str(), found[2].seconds),
            ("1:02:03", 3723)
        );
    }

    #[test]
    fn posted_comments_and_replies_update_state_on_poll() {
        let mut extras = WatchExtras::default();
        extras.sync_video(Some("https://www.youtube.com/watch?v=video000001"));
        extras.comment_draft = "   ".into();
        extras.start_post_comment(
            Some(&oauth::TokenSession::test_session("tok")),
            &egui::Context::default(),
        );
        assert!(extras.comment_post_error.is_some());
        assert!(!extras.is_posting_comment());

        extras.comment_draft = "Novo comentário".into();
        let (tx, rx) = mpsc::channel();
        extras.comment_post_rx = Some(rx);
        tx.send(Ok(oauth::Comment {
            id: "c1".into(),
            author: "Eu".into(),
            author_avatar: None,
            text: "Novo comentário".into(),
            published: "agora".into(),
            likes: 0,
            replies: 0,
        }))
        .unwrap();
        extras.poll();
        assert_eq!(extras.comments.len(), 1);
        assert_eq!(extras.comments[0].id, "c1");
        assert!(extras.comment_draft.is_empty());

        let (rtx, rrx) = mpsc::channel();
        let entry = extras.comment_replies.entry("c1".into()).or_default();
        entry.reply_open = true;
        entry.reply_draft = "Obrigado!".into();
        entry.reply_post_rx = Some(rrx);
        rtx.send(Ok(sample_reply("r1"))).unwrap();
        extras.poll();

        assert_eq!(extras.comments[0].replies, 1);
        let entry = extras.comment_replies.get("c1").unwrap();
        assert!(!entry.reply_open);
        assert!(entry.reply_draft.is_empty());
        assert_eq!(entry.comments.len(), 1);
    }

    #[test]
    fn comment_ratings_and_translations_poll_remotely_and_preserve_state_on_error() {
        let mut extras = WatchExtras::default();
        extras.sync_video(Some("https://www.youtube.com/watch?v=video000001"));
        assert_eq!(extras.comment_displayed_likes("c1", 66), 66);

        // When comment loaded already liked, displayed likes must not double-count.
        extras
            .initial_comment_ratings
            .insert("c1".into(), CommentRating::Like);
        extras
            .comment_ratings
            .insert("c1".into(), CommentRating::Like);
        assert_eq!(extras.comment_displayed_likes("c1", 66), 66);

        // Remote unlike decrements displayed likes by 1.
        let mut web_session = None;
        let mut checked = false;
        let (tx, rx) = mpsc::channel();
        extras.comment_rating_job = Some(CommentRatingJob {
            comment_id: "c1".into(),
            rx,
            cancel: process::Cancellation::default(),
        });
        tx.send(comment_actions::RemoteCommentRatingOutcome {
            web_session: Some(comment_actions::YouTubeWebSession::test_session()),
            cache: comment_actions::VideoCommentTokens::for_video(
                "video000001",
                oauth::CommentOrder::Relevance,
            ),
            result: Ok(None),
        })
        .unwrap();
        let status = extras.poll_with_session(&mut web_session, &mut checked);
        assert!(checked);
        assert!(web_session.is_some());
        assert!(status.is_some());
        assert!(!extras.comment_ratings.contains_key("c1"));
        assert_eq!(extras.comment_displayed_likes("c1", 66), 65);

        // Remote failure keeps existing state and records error without local fallback.
        let (tx, rx) = mpsc::channel();
        extras.comment_rating_job = Some(CommentRatingJob {
            comment_id: "c1".into(),
            rx,
            cancel: process::Cancellation::default(),
        });
        tx.send(comment_actions::RemoteCommentRatingOutcome {
            web_session: None,
            cache: comment_actions::VideoCommentTokens::for_video(
                "video000001",
                oauth::CommentOrder::Relevance,
            ),
            result: Err("Sessão expirada".into()),
        })
        .unwrap();
        let err_msg = extras.poll_with_session(&mut web_session, &mut checked);
        assert_eq!(err_msg.as_deref(), Some("Sessão expirada"));
        assert!(!extras.comment_ratings.contains_key("c1"));
        assert_eq!(
            extras.comment_rating_errors.get("c1").map(String::as_str),
            Some("Sessão expirada")
        );

        // Remote like on a previously unrated comment increments displayed likes by 1.
        let (tx, rx) = mpsc::channel();
        extras.comment_rating_job = Some(CommentRatingJob {
            comment_id: "c2".into(),
            rx,
            cancel: process::Cancellation::default(),
        });
        tx.send(comment_actions::RemoteCommentRatingOutcome {
            web_session: Some(comment_actions::YouTubeWebSession::test_session()),
            cache: comment_actions::VideoCommentTokens::for_video(
                "video000001",
                oauth::CommentOrder::Relevance,
            ),
            result: Ok(Some(CommentRating::Like)),
        })
        .unwrap();
        extras.poll_with_session(&mut web_session, &mut checked);
        assert_eq!(extras.comment_displayed_likes("c2", 10), 11);
        assert!(!extras.comment_rating_errors.contains_key("c2"));

        let parsed = parse_translate_response(&serde_json::json!([
            [["Olá ", "Hello "], ["mundo", "world"]],
            null,
            "en"
        ]))
        .unwrap();
        assert_eq!(parsed, "Olá mundo");

        let (tx, rx) = mpsc::channel();
        let cancel = process::Cancellation::default();
        let tr = extras.comment_translations.entry("c1".into()).or_default();
        tr.loading = true;
        tr.rx = Some(rx);
        tr.cancel = Some(cancel.clone());
        tx.send(Ok("Texto traduzido".into())).unwrap();
        extras.poll();

        let tr = extras.comment_translations.get("c1").unwrap();
        assert!(!tr.loading);
        assert!(tr.showing_translated);
        assert_eq!(tr.translated_text.as_deref(), Some("Texto traduzido"));

        // Toggling cached translation switches between translated and original without network.
        extras.toggle_comment_translation("c1", "Hello", &egui::Context::default());
        assert!(!extras.comment_translations["c1"].showing_translated);
        extras.toggle_comment_translation("c1", "Hello", &egui::Context::default());
        assert!(extras.comment_translations["c1"].showing_translated);

        // Resetting comments cancels pending rating and state sync jobs cleanly.
        let rating_cancel = process::Cancellation::default();
        let (_rtx, rrx) = mpsc::channel();
        extras.comment_rating_job = Some(CommentRatingJob {
            comment_id: "c1".into(),
            rx: rrx,
            cancel: rating_cancel.clone(),
        });
        let sync_cancel = process::Cancellation::default();
        extras.comment_state_sync_cancel = Some(sync_cancel.clone());
        extras.reset_comments();
        assert!(rating_cancel.cancelled());
        assert!(sync_cancel.cancelled());
        assert!(extras.comment_ratings.is_empty());
        assert!(extras.initial_comment_ratings.is_empty());
        assert!(extras.comment_rating_errors.is_empty());
        assert!(extras.comment_translations.is_empty());
    }

    #[test]
    fn apply_preloaded_subtitles_populates_state_until_video_changes() {
        let mut extras = WatchExtras::default();
        extras.sync_video(Some("https://www.youtube.com/watch?v=video000001"));
        extras.subtitles_error = Some("Erro anterior".into());

        let tracks = vec![search::SubtitleTrack {
            language: "en".into(),
            url: "https://example.com/en.vtt".into(),
            automatic: true,
        }];
        extras.apply_preloaded_subtitles(tracks.clone());
        assert!(extras.subtitles_loaded);
        assert!(extras.subtitles_error.is_none());
        assert_eq!(extras.subtitles, tracks);

        // Should not overwrite once already loaded for the current video.
        extras.apply_preloaded_subtitles(Vec::new());
        assert_eq!(extras.subtitles.len(), 1);

        // Switching videos resets subtitle state for the next preload.
        extras.sync_video(Some("https://www.youtube.com/watch?v=video000002"));
        assert!(!extras.subtitles_loaded);
        assert!(extras.subtitles.is_empty());
    }

    #[test]
    fn parse_timestamp_lines_and_sync_cached_comment_meta_preserve_measured_heights() {
        let lines = parse_timestamp_lines("Intro 0:00\nParte 1:25 meio e 2:30 fim");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].timestamps.len(), 1);
        assert_eq!(lines[0].timestamps[0].seconds, 0);
        assert_eq!(lines[1].timestamps.len(), 2);
        assert_eq!(lines[1].timestamps[0].seconds, 85);
        assert_eq!(lines[1].timestamps[1].seconds, 150);

        let mut c1 = sample_reply("c1");
        c1.text = "Veja 1:25 agora".into();
        c1.published = "há 2h".into();
        let mut cache = Vec::new();
        sync_cached_comment_meta(std::slice::from_ref(&c1), &mut cache);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache[0].lines.len(), 1);
        assert_eq!(cache[0].lines[0].timestamps[0].seconds, 85);

        cache[0].measured_height = Some((640, 42, 92.0));
        // Unchanged comments preserve measured height.
        sync_cached_comment_meta(std::slice::from_ref(&c1), &mut cache);
        assert_eq!(cache[0].measured_height, Some((640, 42, 92.0)));

        // Editing comment text triggers re-parse of timestamp lines; replacing id clears height.
        c1.text = "Mudou para 0:45!".into();
        sync_cached_comment_meta(std::slice::from_ref(&c1), &mut cache);
        assert_eq!(cache[0].lines[0].timestamps[0].seconds, 45);

        c1.id = "c2".into();
        sync_cached_comment_meta(&[c1], &mut cache);
        assert_eq!(cache[0].measured_height, None);
    }

    #[test]
    fn extract_description_chapters_parses_standard_formats_and_skips_prose() {
        let description = "\
Inscreva-se no canal!
0:00 Introdução
01:25 - Configuração inicial
[1:02:03] Conclusão e próximos passos
Parte 2: 05:10 Detalhes avançados
02:00 - 03:15 Intervalo técnico
No vídeo anterior que gravamos lá pelas 12:30 falamos bastante sobre isso.
0:00
";
        let chapters = extract_description_chapters(description);
        assert_eq!(chapters.len(), 5);
        assert_eq!(chapters[0].label, "0:00");
        assert_eq!(chapters[0].seconds, 0);
        assert_eq!(chapters[0].title, "Introdução");

        assert_eq!(chapters[1].label, "01:25");
        assert_eq!(chapters[1].seconds, 85);
        assert_eq!(chapters[1].title, "Configuração inicial");

        assert_eq!(chapters[2].label, "02:00");
        assert_eq!(chapters[2].seconds, 120);
        assert_eq!(chapters[2].title, "Intervalo técnico");

        assert_eq!(chapters[3].label, "05:10");
        assert_eq!(chapters[3].seconds, 310);
        assert_eq!(chapters[3].title, "Parte 2 — Detalhes avançados");

        assert_eq!(chapters[4].label, "1:02:03");
        assert_eq!(chapters[4].seconds, 3723);
        assert_eq!(chapters[4].title, "Conclusão e próximos passos");
    }

    #[test]
    fn extract_description_chapters_strips_numbered_prefixes_and_sorts_chronologically() {
        let description = "\
2) 01:30 Setup
1. 00:00 Intro
03) 00:45 Middle
";
        let chapters = extract_description_chapters(description);
        assert_eq!(
            chapters,
            vec![
                DescriptionChapter {
                    title: "Intro".into(),
                    label: "00:00".into(),
                    seconds: 0,
                },
                DescriptionChapter {
                    title: "Middle".into(),
                    label: "00:45".into(),
                    seconds: 45,
                },
                DescriptionChapter {
                    title: "Setup".into(),
                    label: "01:30".into(),
                    seconds: 90,
                },
            ]
        );
    }

    #[test]
    fn start_transcript_reuses_active_subtitle_cues_without_vtt_cache() {
        let mut extras = WatchExtras::default();
        let track = search::SubtitleTrack {
            language: "pt-BR".into(),
            automatic: false,
            url: "https://www.youtube.com/api/timedtext?v=one&lang=pt-BR".into(),
        };
        let cue = crate::transcript::Cue {
            start_ms: 1000,
            end_ms: 3000,
            text: "Legenda ativa".into(),
        };
        extras.active_subtitle_url = Some(track.url.clone());
        extras.active_subtitle_cues = vec![cue.clone()];

        extras.start_transcript(&track, &egui::Context::default());
        assert!(!extras.transcript_loading);
        assert_eq!(extras.transcript, vec![cue]);
    }

    #[test]
    fn switching_or_failing_subtitle_clears_stale_cues_and_active_url() {
        let mut extras = WatchExtras::default();
        extras.sync_video(Some("https://www.youtube.com/watch?v=video000001"));
        let en_url = "https://www.youtube.com/api/timedtext?v=video000001&lang=en&fmt=vtt";
        let pt_url = "https://www.youtube.com/api/timedtext?v=video000001&lang=pt&fmt=vtt";
        extras.active_subtitle_url = Some(en_url.into());
        extras.active_subtitle_cues = vec![crate::transcript::Cue {
            start_ms: 0,
            end_ms: 2_000,
            text: "English line".into(),
        }];

        // Selecting a non-cached track (simulated here via invalid/unresolvable URL) clears old cues immediately.
        let pt_track = search::SubtitleTrack {
            language: "pt".into(),
            automatic: true,
            url: pt_url.into(),
        };
        let (tx, rx) = mpsc::channel();
        extras.start_apply_subtitle(
            &pt_track,
            "pt (automática)".into(),
            &egui::Context::default(),
        );
        assert!(extras.active_subtitle_cues.is_empty());
        assert_eq!(extras.active_subtitle_url.as_deref(), Some(pt_url));

        // Cancel real worker and inject simulated failure to verify failure cleanup deterministically.
        if let Some(cancel) = extras.subtitle_apply_cancel.take() {
            cancel.cancel();
        }
        extras.subtitle_apply_rx = Some(rx);
        tx.send(Err("HTTP 429".into())).unwrap();
        extras.poll();

        assert!(extras.active_subtitle_url.is_none());
        assert!(extras.active_subtitle_cues.is_empty());
        assert_eq!(extras.subtitles_error.as_deref(), Some("HTTP 429"));

        // Disconnected channel also clears active_subtitle_url and active_subtitle_cues.
        extras.active_subtitle_url = Some(pt_url.into());
        extras.active_subtitle_cues = vec![crate::transcript::Cue {
            start_ms: 0,
            end_ms: 1_000,
            text: "Stale".into(),
        }];
        let (tx_drop, rx_drop) = mpsc::channel::<Result<DownloadedSubtitle, String>>();
        drop(tx_drop);
        extras.subtitle_apply_rx = Some(rx_drop);
        extras.poll();
        assert!(extras.active_subtitle_url.is_none());
        assert!(extras.active_subtitle_cues.is_empty());
        assert!(extras.subtitles_error.is_some());
    }

    #[test]
    fn cancel_active_subtitle_cancels_inflight_download_and_clears_state() {
        let mut extras = WatchExtras::default();
        extras.sync_video(Some("https://www.youtube.com/watch?v=video000001"));
        let cancel = process::Cancellation::default();
        let (tx, rx) = mpsc::channel();
        extras.subtitle_apply_cancel = Some(cancel.clone());
        extras.subtitle_apply_rx = Some(rx);
        extras.active_subtitle_url =
            Some("https://www.youtube.com/api/timedtext?v=video000001&lang=en&fmt=vtt".into());
        extras.active_subtitle_cues = vec![crate::transcript::Cue {
            start_ms: 0,
            end_ms: 2_000,
            text: "Old cue".into(),
        }];
        extras.ready_subtitle = Some(ReadySubtitle {
            url: "https://www.youtube.com/api/timedtext?v=video000001&lang=en&fmt=vtt".into(),
            path: PathBuf::from("nonexistent.vtt"),
            label: "en".into(),
            language: "en".into(),
        });
        extras.subtitles_error = Some("Erro".into());

        extras.cancel_active_subtitle();

        assert!(cancel.cancelled());
        assert!(extras.subtitle_apply_rx.is_none());
        assert!(extras.take_ready_subtitle().is_none());
        assert!(extras.active_subtitle_url.is_none());
        assert!(extras.active_subtitle_cues.is_empty());
        assert!(extras.subtitles_error.is_none());

        let send_res = tx.send(Ok(DownloadedSubtitle {
            url: "https://www.youtube.com/api/timedtext?v=video000001&lang=en&fmt=vtt".into(),
            label: "en".into(),
            language: "en".into(),
            vtt: "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nLate\n".into(),
            cues: vec![],
        }));
        assert!(send_res.is_err());
        extras.poll();
        assert!(extras.active_subtitle_url.is_none());
        assert!(extras.active_subtitle_cues.is_empty());
    }

    #[test]
    fn apply_preloaded_subtitles_deduplicates_orig_and_duplicate_urls() {
        let mut extras = WatchExtras::default();
        extras.sync_video(Some("https://www.youtube.com/watch?v=video000001"));
        let pt_url = "https://www.youtube.com/api/timedtext?v=video000001&kind=asr&lang=pt&fmt=vtt";
        extras.apply_preloaded_subtitles(vec![
            search::SubtitleTrack {
                language: "pt-orig".into(),
                automatic: true,
                url: pt_url.into(),
            },
            search::SubtitleTrack {
                language: "pt".into(),
                automatic: true,
                url: pt_url.into(),
            },
        ]);
        assert_eq!(
            extras.subtitles,
            vec![search::SubtitleTrack {
                language: "pt".into(),
                automatic: true,
                url: pt_url.into(),
            }]
        );
    }

    #[test]
    fn preferred_transcript_track_follows_priority_order() {
        let mut extras = WatchExtras {
            subtitles: vec![
                search::SubtitleTrack {
                    language: "es".into(),
                    automatic: true,
                    url: "https://www.youtube.com/api/timedtext?v=1&lang=es&kind=asr".into(),
                },
                search::SubtitleTrack {
                    language: "en".into(),
                    automatic: true,
                    url: "https://www.youtube.com/api/timedtext?v=1&lang=en&kind=asr".into(),
                },
                search::SubtitleTrack {
                    language: "fr".into(),
                    automatic: false,
                    url: "https://www.youtube.com/api/timedtext?v=1&lang=fr".into(),
                },
                search::SubtitleTrack {
                    language: "pt-BR".into(),
                    automatic: false,
                    url: "https://www.youtube.com/api/timedtext?v=1&lang=pt-BR".into(),
                },
            ],
            ..Default::default()
        };

        assert_eq!(
            extras
                .preferred_transcript_track(true)
                .map(|t| t.language.as_str()),
            Some("pt-BR")
        );
        // When UI is English, manual English isn't present so first manual track ('fr') wins over ASR.
        assert_eq!(
            extras
                .preferred_transcript_track(false)
                .map(|t| t.language.as_str()),
            Some("fr")
        );

        extras.active_subtitle_url =
            Some("https://www.youtube.com/api/timedtext?v=1&lang=en&kind=asr".into());
        assert_eq!(
            extras
                .preferred_transcript_track(true)
                .map(|t| t.language.as_str()),
            Some("en")
        );

        extras.transcript_track_url =
            Some("https://www.youtube.com/api/timedtext?v=1&lang=es&kind=asr".into());
        assert_eq!(
            extras
                .preferred_transcript_track(true)
                .map(|t| t.language.as_str()),
            Some("es")
        );
    }

    #[test]
    fn poll_discards_stale_transcript_request_ids() {
        let (tx, rx) = mpsc::channel();
        let mut extras = WatchExtras {
            transcript_request_id: 5,
            transcript_loading: true,
            transcript_rx: Some(rx),
            ..Default::default()
        };
        tx.send((
            4,
            Ok(TranscriptPayload {
                url: "https://www.youtube.com/api/timedtext?v=1&lang=en".into(),
                vtt: "WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nStale".into(),
                cues: vec![crate::transcript::Cue {
                    start_ms: 0,
                    end_ms: 2_000,
                    text: "Stale".into(),
                }],
            }),
        ))
        .unwrap();

        extras.poll();
        assert!(extras.transcript.is_empty());
        assert!(extras.transcript_loading);
        assert!(extras.transcript_rx.is_some());
    }

    #[test]
    fn apply_watch_next_populates_related_videos_chips_heatmap_and_comments_continuation() {
        let mut extras = WatchExtras::default();
        extras.sync_video(Some("https://www.youtube.com/watch?v=video000001"));

        let next = crate::innertube::InnerTubeWatchNext {
            related_videos: vec![
                search::Video {
                    title: "Related 1".into(),
                    channel: "Creator A".into(),
                    url: "https://www.youtube.com/watch?v=rel00000001".into(),
                    thumbnail: "https://i.ytimg.com/vi/rel00000001/hqdefault.jpg".into(),
                    duration: "10:00".into(),
                },
                search::Video {
                    title: "Related 2".into(),
                    channel: "Creator B".into(),
                    url: "https://www.youtube.com/watch?v=rel00000002".into(),
                    thumbnail: String::new(),
                    duration: "04:15".into(),
                },
            ],
            related_metadata: vec![
                search::VideoMetadata {
                    channel_id: Some("UC1111111111111111111111".into()),
                    channel_verified: true,
                    ..Default::default()
                },
                search::VideoMetadata::default(),
            ],
            related_chips: vec![
                crate::innertube::RelatedFilterChip {
                    title: "All".into(),
                    continuation: None,
                    selected: false,
                },
                crate::innertube::RelatedFilterChip {
                    title: "Rust".into(),
                    continuation: Some("chip_cont_rust".into()),
                    selected: true,
                },
            ],
            related_continuation: Some("rel_next_token".into()),
            comments_continuation: Some("comments_init_token".into()),
            heatmap: vec![
                crate::innertube::HeatmapBucket {
                    start_ms: 0,
                    duration_ms: 5_000,
                    intensity: 0.25,
                },
                crate::innertube::HeatmapBucket {
                    start_ms: 5_000,
                    duration_ms: 5_000,
                    intensity: 1.4,
                },
                crate::innertube::HeatmapBucket {
                    start_ms: 10_000,
                    duration_ms: 0,
                    intensity: 0.5,
                },
            ],
            channel_id: Some("UC1111111111111111111111".into()),
            channel_verified: true,
            like_count_text: Some("12 mil".into()),
            subscriber_count_text: Some("500 mil inscritos".into()),
            description_runs: Vec::new(),
            ..Default::default()
        };

        extras.apply_watch_next(next);

        assert_eq!(extras.related_videos.len(), 2);
        assert_eq!(extras.related_videos[0].title, "Related 1");
        assert_eq!(extras.related_chips.len(), 2);
        assert_eq!(extras.selected_related_chip, Some(1));
        assert_eq!(
            extras.related_continuation.as_deref(),
            Some("rel_next_token")
        );
        assert_eq!(
            extras.innertube_comments_continuation.as_deref(),
            Some("comments_init_token")
        );
        assert_eq!(extras.heatmap.len(), 2);
        assert!((extras.heatmap[0].start_secs - 0.0).abs() < 1e-6);
        assert!((extras.heatmap[0].duration_secs - 5.0).abs() < 1e-6);
        assert!((extras.heatmap[0].intensity - 0.25).abs() < 1e-6);
        assert!((extras.heatmap[1].start_secs - 5.0).abs() < 1e-6);
        assert!((extras.heatmap[1].intensity - 1.0).abs() < 1e-6);

        // Switching videos resets all watch-next state cleanly.
        extras.sync_video(Some("https://www.youtube.com/watch?v=video000002"));
        assert!(extras.related_videos.is_empty());
        assert!(extras.related_chips.is_empty());
        assert!(extras.selected_related_chip.is_none());
        assert!(extras.related_continuation.is_none());
        assert!(extras.innertube_comments_continuation.is_none());
        assert!(extras.heatmap.is_empty());
    }

    #[test]
    fn apply_innertube_comments_page_supports_oauth_free_append_and_enforces_max_cap() {
        let mut extras = WatchExtras {
            innertube_comments_loading: true,
            ..Default::default()
        };

        let first_page = crate::innertube::InnerTubeCommentsPage {
            comments: vec![crate::innertube::InnerTubeComment {
                id: "it_c1".into(),
                author: "@alice".into(),
                author_avatar: Some("https://yt3.ggpht.com/a1.jpg".into()),
                text: "Great video at 1:20".into(),
                published_text: "há 3 horas".into(),
                like_count_text: Some("1,5 mil".into()),
                is_pinned: true,
                is_hearted: true,
                is_owner: true,
                is_member: false,
                reply_count: 4,
                replies_continuation: Some("replies_c1".into()),
            }],
            continuation: Some("page2_token".into()),
            top_sort_continuation: Some("sort_top_token".into()),
            newest_sort_continuation: Some("sort_new_token".into()),
            total_count_text: Some("842 comentários".into()),
        };

        extras.apply_innertube_comments_page(first_page, false);
        assert!(!extras.innertube_comments_loading);
        assert!(extras.comments_loaded);
        assert_eq!(extras.comments.len(), 1);
        assert_eq!(extras.comments[0].id, "it_c1");
        assert_eq!(extras.comments[0].likes, 1_500);
        assert_eq!(extras.comments[0].replies, 4);
        assert_eq!(extras.comments_total, Some(842));
        assert_eq!(
            extras.innertube_comments_continuation.as_deref(),
            Some("page2_token")
        );
        let meta = extras
            .innertube_comment_meta
            .get("it_c1")
            .expect("metadata stored");
        assert!(meta.is_pinned);
        assert!(meta.is_hearted);
        assert!(meta.is_owner);
        assert!(!meta.is_member);
        assert_eq!(meta.replies_continuation.as_deref(), Some("replies_c1"));

        // Append up to MAX_WATCH_COMMENTS and verify cap + continuation clearing.
        let mut many_comments = Vec::new();
        for idx in 0..(MAX_WATCH_COMMENTS + 10) {
            many_comments.push(crate::innertube::InnerTubeComment {
                id: format!("it_bulk_{idx}"),
                author: format!("@user_{idx}"),
                author_avatar: None,
                text: format!("Comment body {idx}"),
                published_text: "há 1 dia".into(),
                like_count_text: Some("3".into()),
                is_pinned: false,
                is_hearted: false,
                is_owner: false,
                is_member: idx % 2 == 0,
                reply_count: 0,
                replies_continuation: None,
            });
        }
        extras.apply_innertube_comments_page(
            crate::innertube::InnerTubeCommentsPage {
                comments: many_comments,
                continuation: Some("page3_should_clear_at_cap".into()),
                top_sort_continuation: None,
                newest_sort_continuation: None,
                total_count_text: None,
            },
            true,
        );
        assert_eq!(extras.comments.len(), MAX_WATCH_COMMENTS);
        assert!(extras.innertube_comments_continuation.is_none());
    }

    #[test]
    fn apply_preloaded_manifest_extras_hydrates_subtitles_from_manifest() {
        let mut extras = WatchExtras::default();
        extras.sync_video(Some("https://www.youtube.com/watch?v=video000001"));

        let manifest = crate::stream_manifest::StreamManifest {
            video_id: "video000001".into(),
            title: "Demo".into(),
            channel: "Channel".into(),
            channel_id: Some("UC1111111111111111111111".into()),
            channel_verified: true,
            duration_secs: Some(120.0),
            is_live: false,
            upload_date: None,
            video_formats: Vec::new(),
            audio_formats: Vec::new(),
            muxed_formats: Vec::new(),
            chapters: Vec::new(),
            captions: vec![
                crate::stream_manifest::ManifestCaptionTrack {
                    language_code: "pt-BR".into(),
                    label: "Português (Brasil)".into(),
                    url: "https://www.youtube.com/api/timedtext?v=video000001&lang=pt-BR&fmt=vtt"
                        .into(),
                    is_auto_generated: false,
                },
                crate::stream_manifest::ManifestCaptionTrack {
                    language_code: "en".into(),
                    label: "English (auto)".into(),
                    url: "https://www.youtube.com/api/timedtext?v=video000001&lang=en&kind=asr&fmt=vtt"
                        .into(),
                    is_auto_generated: true,
                },
            ],
            storyboard: None,
            fetched_at: std::time::Instant::now(),
            expires_at: std::time::Instant::now() + std::time::Duration::from_secs(3600),
        };

        extras.apply_preloaded_manifest_extras(&manifest);
        assert!(extras.subtitles_loaded);
        assert_eq!(extras.subtitles.len(), 2);
        assert_eq!(extras.subtitles[0].language, "pt-BR");
        assert!(!extras.subtitles[0].automatic);
        assert!(extras.subtitles[0].url.contains("fmt=vtt"));
        assert_eq!(extras.subtitles[1].language, "en");
        assert!(extras.subtitles[1].automatic);
    }

    #[test]
    fn tokenize_description_line_parses_urls_mentions_hashtags_and_timestamps() {
        let line =
            "Check 01:25 with @rustlang at https://rust-lang.org/learn, #programação and 1:02:03!";
        let tokens = tokenize_description_line(line);
        assert_eq!(
            tokens,
            vec![
                DescriptionToken::Text("Check "),
                DescriptionToken::Timestamp {
                    raw: "01:25",
                    secs: 85.0,
                },
                DescriptionToken::Text(" with "),
                DescriptionToken::Mention("@rustlang"),
                DescriptionToken::Text(" at "),
                DescriptionToken::Url("https://rust-lang.org/learn"),
                DescriptionToken::Text(", "),
                DescriptionToken::Hashtag("#programação"),
                DescriptionToken::Text(" and "),
                DescriptionToken::Timestamp {
                    raw: "1:02:03",
                    secs: 3723.0,
                },
                DescriptionToken::Text("!"),
            ]
        );
    }

    #[test]
    fn tokenize_description_line_ignores_emails_and_url_internal_tokens() {
        let line = "Contact dev@example.com or visit http://example.com:8080/01:20#section (ok)";
        let tokens = tokenize_description_line(line);
        assert_eq!(
            tokens,
            vec![
                DescriptionToken::Text("Contact dev@example.com or visit "),
                DescriptionToken::Url("http://example.com:8080/01:20#section"),
                DescriptionToken::Text(" (ok)"),
            ]
        );
    }

    #[test]
    fn ryd_like_ratio_computes_bounds_and_handles_zero_total() {
        assert_eq!(ryd_like_ratio(0, 0), None);
        assert_eq!(ryd_like_ratio(100, 0), Some(1.0));
        assert_eq!(ryd_like_ratio(0, 50), Some(0.0));
        let half = ryd_like_ratio(75, 25).expect("non-zero total");
        assert!((half - 0.75).abs() < 1e-6);
    }

    #[test]
    fn filter_comments_by_query_matches_author_and_text_case_and_accent_insensitively() {
        let comments = vec![
            oauth::Comment {
                id: "1".into(),
                author: "@JoãoSilva".into(),
                author_avatar: None,
                text: "Ótima explicação sobre programação em Rust!".into(),
                published: "1d".into(),
                likes: 12,
                replies: 0,
            },
            oauth::Comment {
                id: "2".into(),
                author: "@Alice".into(),
                author_avatar: None,
                text: "Audio quality is crisp".into(),
                published: "2h".into(),
                likes: 3,
                replies: 0,
            },
        ];

        assert_eq!(filter_comments_by_query(&comments, "   "), vec![0, 1]);
        assert_eq!(filter_comments_by_query(&comments, "programacao"), vec![0]);
        assert_eq!(filter_comments_by_query(&comments, "joao rust"), vec![0]);
        assert_eq!(filter_comments_by_query(&comments, "ALICE audio"), vec![1]);
        assert!(filter_comments_by_query(&comments, "python").is_empty());
    }

    #[test]
    fn watch_extras_reset_clears_comment_search_edit_and_report_modal_state() {
        let mut extras = WatchExtras::default();
        extras.sync_video(Some("https://www.youtube.com/watch?v=video000001"));
        extras.comment_search_query = "rust".into();
        extras.editing_comment_id = Some("c1".into());
        extras.editing_comment_draft = "Updated body".into();
        extras.report_modal_open = true;
        extras.report_reason = "Spam".into();
        extras.report_details = "Bot links".into();

        extras.reset();

        assert!(extras.comment_search_query.is_empty());
        assert!(extras.editing_comment_id.is_none());
        assert!(extras.editing_comment_draft.is_empty());
        assert!(!extras.report_modal_open);
        assert!(extras.report_reason.is_empty());
        assert!(extras.report_details.is_empty());
    }
}
