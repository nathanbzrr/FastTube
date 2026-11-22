use super::playlist::{extract_playlist_id, is_valid_playlist_id};
use super::query::{collect_yt_search_items, is_playlist_url};
use super::{
    ChannelAboutInfo, ChannelExternalLink, ChannelFeedTab, ChannelFetchOutcome, ChannelHomeShelf,
    ChannelPage, ChannelPlaylist, ChannelShelfKind, ChannelTarget, ChannelVideosBatch,
    QUERY_TIMEOUT, SUBTITLE_USER_AGENT, SearchResult, SearchResultKind, Video, VideoMetadata,
    clean_pt_br_view_label, extract_yt_initial_data, format_duration_seconds, nonempty_str,
    parse_pt_br_compact_count, parse_video_entry, select_video_thumbnail, select_yt_thumbnail,
    valid_http_url, yt_dlp, yt_text,
};
use crate::process::{self, Cancellation};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const DEFAULT_CHANNEL_VIDEOS_LIMIT: usize = 20;

pub const CHANNEL_VIDEOS_PAGE_STEP: usize = 20;

pub const MAX_CHANNEL_VIDEOS_LIMIT: usize = 100;

pub const DEFAULT_CHANNEL_PLAYLISTS_LIMIT: usize = 30;

pub const MAX_CHANNEL_PLAYLISTS_LIMIT: usize = 50;

pub const MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT: usize = 100;

pub const MAX_CHANNEL_DESCRIPTION_CHARS: usize = 8192;

pub const MAX_CHANNEL_TAGS: usize = 30;

pub const MAX_CHANNEL_TAG_CHARS: usize = 64;

pub(super) const CHANNEL_PAGE_CACHE_TTL: Duration = crate::innertube::DEFAULT_CHANNEL_CACHE_TTL;

pub(super) const CHANNEL_PAGE_CACHE_CAPACITY: usize =
    crate::innertube::DEFAULT_CHANNEL_CACHE_CAPACITY;

static CHANNEL_PAGE_CACHE: OnceLock<Mutex<crate::innertube::ChannelIdCache<ChannelPage>>> =
    OnceLock::new();

fn channel_page_cache() -> &'static Mutex<crate::innertube::ChannelIdCache<ChannelPage>> {
    CHANNEL_PAGE_CACHE.get_or_init(|| {
        Mutex::new(crate::innertube::ChannelIdCache::new(
            CHANNEL_PAGE_CACHE_CAPACITY,
            CHANNEL_PAGE_CACHE_TTL,
        ))
    })
}

fn synthetic_channel_id_for_handle(handle: &str) -> Option<String> {
    let clean = handle.trim();
    if !is_valid_channel_handle(clean) {
        return None;
    }
    let digest = crate::innertube::sha1_hex(clean.to_ascii_lowercase().as_bytes());
    Some(format!("UC{}", &digest[..22]))
}

fn store_channel_page_in_cache(
    cache: &mut crate::innertube::ChannelIdCache<ChannelPage>,
    input: &str,
    target: &ChannelTarget,
    page: &ChannelPage,
    now: Instant,
) {
    let fallback_synthetic = match target {
        ChannelTarget::Handle(h) => synthetic_channel_id_for_handle(h),
        ChannelTarget::ChannelId(_) => None,
    }
    .or_else(|| {
        page.handle
            .as_deref()
            .and_then(synthetic_channel_id_for_handle)
    });
    let channel_id = page
        .channel_id
        .as_deref()
        .filter(|id| is_valid_channel_id(id))
        .or_else(|| match target {
            ChannelTarget::ChannelId(id) if is_valid_channel_id(id) => Some(id.as_str()),
            _ => None,
        })
        .or(fallback_synthetic.as_deref());
    let Some(cid) = channel_id else {
        return;
    };
    let mut aliases: Vec<&str> = Vec::with_capacity(4);
    aliases.push(input);
    if let ChannelTarget::Handle(h) = target {
        aliases.push(h.as_str());
    }
    if let Some(h) = page.handle.as_deref() {
        aliases.push(h);
    }
    if !page.canonical_url.is_empty() {
        aliases.push(page.canonical_url.as_str());
    }
    cache.insert(cid, &aliases, page.clone(), now);
}

pub(super) fn fetch_with_cache(
    cache: &Mutex<crate::innertube::ChannelIdCache<ChannelPage>>,
    input: &str,
    target: &ChannelTarget,
    force_refresh: bool,
    fetch: impl FnOnce() -> Result<ChannelPage, String>,
) -> Result<ChannelFetchOutcome, String> {
    if !force_refresh {
        let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let canonical = target.canonical_url();
        let hit = guard
            .get(input, now)
            .cloned()
            .or_else(|| guard.get(&canonical, now).cloned());
        if let Some(page) = hit {
            return Ok(ChannelFetchOutcome {
                page,
                from_cache: true,
            });
        }
    }

    let page = fetch()?;
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    store_channel_page_in_cache(&mut guard, input, target, &page, Instant::now());
    drop(guard);
    Ok(ChannelFetchOutcome {
        page,
        from_cache: false,
    })
}

pub fn is_valid_channel_id(channel_id: &str) -> bool {
    channel_id.len() == 24
        && channel_id.starts_with("UC")
        && channel_id[2..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

pub fn is_valid_channel_handle(handle: &str) -> bool {
    let Some(rest) = handle.strip_prefix('@') else {
        return false;
    };
    (3..=30).contains(&rest.len())
        && rest.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' || byte == b'.'
        })
}

pub fn parse_channel_target(input: &str) -> Option<ChannelTarget> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_valid_channel_id(trimmed) {
        return Some(ChannelTarget::ChannelId(trimmed.to_owned()));
    }
    if is_valid_channel_handle(trimmed) {
        return Some(ChannelTarget::Handle(trimmed.to_owned()));
    }
    parse_youtube_channel_url(trimmed)
}

#[cfg(test)]
pub fn youtube_channel_url(input: &str) -> Option<String> {
    parse_channel_target(input).map(|target| target.canonical_url())
}

#[cfg(test)]
pub fn youtube_channel_videos_url(input: &str) -> Option<String> {
    parse_channel_target(input).map(|target| target.videos_tab_url())
}

pub fn channel_target_for_metadata(metadata: &VideoMetadata) -> Option<ChannelTarget> {
    metadata
        .channel_id
        .as_deref()
        .and_then(parse_channel_target)
        .or_else(|| {
            metadata
                .channel_url
                .as_deref()
                .and_then(parse_channel_target)
        })
}

pub(super) fn parse_youtube_channel_url(url: &str) -> Option<ChannelTarget> {
    let trimmed = url.trim();
    let after_scheme = if let Some(rest) = trimmed.strip_prefix("https://") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        rest
    } else if trimmed
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
    {
        &trimmed[8..]
    } else if trimmed
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
    {
        &trimmed[7..]
    } else {
        return None;
    };

    if after_scheme
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return None;
    }

    let (authority, rest) = match after_scheme.find(['/', '?', '#']) {
        Some(index) => (&after_scheme[..index], &after_scheme[index..]),
        None => (after_scheme, ""),
    };
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let host = if let Some(stripped) = authority.strip_suffix(":443") {
        stripped
    } else if let Some(stripped) = authority.strip_suffix(":80") {
        stripped
    } else if authority.contains(':') {
        return None;
    } else {
        authority
    };
    let host = host.to_ascii_lowercase();
    if !matches!(
        host.as_str(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com"
    ) {
        return None;
    }

    let before_fragment = rest.split('#').next().unwrap_or("");
    let path = match before_fragment.split_once('?') {
        Some((path, _)) => path,
        None => before_fragment,
    };
    let path = path.strip_prefix('/')?;
    let path = path.strip_suffix('/').unwrap_or(path);
    if path.is_empty() {
        return None;
    }

    let is_allowed_tab = |tab: &str| {
        matches!(
            tab,
            "videos" | "featured" | "streams" | "shorts" | "releases" | "playlists" | "about"
        )
    };

    if let Some(after_channel) = path.strip_prefix("channel/") {
        let (candidate, tab) = match after_channel.split_once('/') {
            Some((id, rest)) => {
                if rest.contains('/') || !is_allowed_tab(rest) {
                    return None;
                }
                (id, Some(rest))
            }
            None => (after_channel, None),
        };
        let _ = tab;
        if is_valid_channel_id(candidate) {
            return Some(ChannelTarget::ChannelId(candidate.to_owned()));
        }
        return None;
    }

    let (candidate, tab) = match path.split_once('/') {
        Some((handle, rest)) => {
            if rest.contains('/') || !is_allowed_tab(rest) {
                return None;
            }
            (handle, Some(rest))
        }
        None => (path, None),
    };
    let _ = tab;
    if is_valid_channel_handle(candidate) {
        Some(ChannelTarget::Handle(candidate.to_owned()))
    } else {
        None
    }
}

pub fn fetch_channel_tab_videos_with_cancel(
    input: &str,
    tab: ChannelFeedTab,
    limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let (target, effective_limit) = parse_and_clamp_channel_request(input, limit)?;
    let tab_url = match tab {
        ChannelFeedTab::Videos => target.videos_tab_url(),
        ChannelFeedTab::Shorts => target.shorts_tab_url(),
        ChannelFeedTab::Streams => target.streams_tab_url(),
    };
    if let Ok(items) = fetch_channel_tab_videos_html(&tab_url, tab, effective_limit, cancellation)
        && !items.is_empty()
    {
        return Ok(items);
    }
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let limit_arg = effective_limit.to_string();
    let output = process::output(
        yt_dlp().args([
            "--flat-playlist",
            "--extractor-args",
            "youtubetab:approximate_date",
            "--playlist-end",
            &limit_arg,
            "--dump-single-json",
            "--skip-download",
            "--no-warnings",
            "--",
            &tab_url,
        ]),
        cancellation,
        QUERY_TIMEOUT,
    )
    .map_err(|error| {
        format!("Não foi possível executar yt-dlp: {error}. Verifique se ele está ao lado do app ou no PATH.")
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let trimmed = stderr.trim();
        if trimmed.contains("This channel does not have a")
            || trimmed.contains("streams tab")
            || trimmed.contains("shorts tab")
        {
            return Ok(Vec::new());
        }
        return Err(format!(
            "Não foi possível consultar esta aba do canal: {trimmed}"
        ));
    }
    parse_channel_page(&output.stdout, &target, effective_limit).map(|page| page.videos)
}

#[cfg(test)]
pub fn fetch_channel_page(input: &str) -> Result<ChannelPage, String> {
    fetch_channel_page_with_limit(input, DEFAULT_CHANNEL_VIDEOS_LIMIT)
}

#[cfg(test)]
pub fn fetch_channel_page_with_limit(input: &str, limit: usize) -> Result<ChannelPage, String> {
    let (target, effective_limit) = parse_and_clamp_channel_request(input, limit)?;
    fetch_channel_page_for_target(&target, effective_limit)
}

#[cfg(test)]
pub fn fetch_channel_page_with_cache(
    input: &str,
    limit: usize,
    force_refresh: bool,
) -> Result<ChannelFetchOutcome, String> {
    fetch_channel_page_with_cancel(input, limit, force_refresh, &Cancellation::default())
}

pub fn fetch_channel_page_with_cancel(
    input: &str,
    limit: usize,
    force_refresh: bool,
    cancellation: &Cancellation,
) -> Result<ChannelFetchOutcome, String> {
    let cache = channel_page_cache();
    fetch_channel_page_cached_with(cache, input, limit, force_refresh, |target, limit| {
        fetch_channel_page_for_target_cancel(target, limit, cancellation)
    })
}

fn parse_and_clamp_channel_request(
    input: &str,
    limit: usize,
) -> Result<(ChannelTarget, usize), String> {
    let target = parse_channel_target(input).ok_or_else(|| {
        "Identificador ou URL de canal do YouTube inválido ou não suportado.".to_owned()
    })?;
    let effective_limit = limit.clamp(1, MAX_CHANNEL_VIDEOS_LIMIT);
    Ok((target, effective_limit))
}

pub(super) fn fetch_channel_page_cached_with(
    cache: &Mutex<crate::innertube::ChannelIdCache<ChannelPage>>,
    input: &str,
    limit: usize,
    force_refresh: bool,
    fetch: impl FnOnce(&ChannelTarget, usize) -> Result<ChannelPage, String>,
) -> Result<ChannelFetchOutcome, String> {
    let (target, effective_limit) = parse_and_clamp_channel_request(input, limit)?;
    fetch_with_cache(cache, input, &target, force_refresh, || {
        fetch(&target, effective_limit)
    })
}

#[derive(Clone, Debug, Default, PartialEq)]
#[allow(dead_code)]
pub struct ChannelTabPage {
    pub videos: Vec<SearchResult>,
    pub sort_chips: Vec<crate::innertube::ChannelSortChip>,
    pub next_continuation: Option<String>,
    pub tab_endpoints: Vec<crate::innertube::ChannelTabEndpoint>,
    pub community_posts: Vec<crate::innertube::ChannelCommunityPost>,
}

pub fn is_playlist_search_result(item: &SearchResult) -> bool {
    item.metadata.kind == SearchResultKind::Playlist
        || is_playlist_url(&item.video.url)
        || item.video.url.contains("playlist?list=")
}

pub fn playlist_privacy_from_badges(badges: &[String]) -> crate::playlist_sync::PlaylistPrivacy {
    for badge in badges {
        for part in badge.split(['·', '•', '|']) {
            let norm = part.trim().to_lowercase();
            match norm.as_str() {
                "private" | "privado" | "privada" | "particular" => {
                    return crate::playlist_sync::PlaylistPrivacy::Private;
                }
                "unlisted" | "não listado" | "não listada" | "nao listado" | "nao listada" => {
                    return crate::playlist_sync::PlaylistPrivacy::Unlisted;
                }
                "public" | "público" | "pública" | "publico" | "publica" => {
                    return crate::playlist_sync::PlaylistPrivacy::Public;
                }
                _ => {}
            }
        }
    }
    crate::playlist_sync::PlaylistPrivacy::Unknown
}

pub fn channel_playlist_from_search_result(item: &SearchResult) -> Option<ChannelPlaylist> {
    if !is_playlist_search_result(item) {
        return None;
    }
    let id = item
        .video
        .url
        .strip_prefix("https://www.youtube.com/playlist?list=")
        .and_then(|rest| rest.split(['&', '#', '/']).next())
        .map(str::to_owned)
        .or_else(|| {
            url::Url::parse(&item.video.url).ok().and_then(|parsed| {
                parsed
                    .query_pairs()
                    .find_map(|(k, v)| (k == "list").then(|| v.into_owned()))
            })
        })
        .filter(|id| is_valid_playlist_id(id))?;
    let item_count = item
        .metadata
        .playlist_count
        .as_deref()
        .and_then(parse_pt_br_compact_count);
    Some(ChannelPlaylist {
        id,
        title: item.video.title.clone(),
        url: item.video.url.clone(),
        thumbnail: item.video.thumbnail.clone(),
        item_count,
        description: item.metadata.description.clone(),
        privacy: playlist_privacy_from_badges(&item.metadata.badges),
    })
}

pub fn merge_channel_playlists(
    primary: Vec<ChannelPlaylist>,
    extra: Vec<ChannelPlaylist>,
    limit: usize,
) -> Vec<ChannelPlaylist> {
    let effective_limit = limit.clamp(1, MAX_CHANNEL_PLAYLISTS_LIMIT);
    let mut out: Vec<ChannelPlaylist> = Vec::new();
    let mut index_by_id: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for pl in primary.into_iter().chain(extra) {
        if let Some(&idx) = index_by_id.get(&pl.id) {
            let existing = &mut out[idx];
            if existing.thumbnail.trim().is_empty() && !pl.thumbnail.trim().is_empty() {
                existing.thumbnail = pl.thumbnail;
            }
            if existing.item_count.is_none() && pl.item_count.is_some() {
                existing.item_count = pl.item_count;
            }
            if existing.description.is_none() && pl.description.is_some() {
                existing.description = pl.description;
            }
            if existing.privacy == crate::playlist_sync::PlaylistPrivacy::Unknown
                && pl.privacy != crate::playlist_sync::PlaylistPrivacy::Unknown
            {
                existing.privacy = pl.privacy;
            }
        } else if out.len() < effective_limit {
            index_by_id.insert(pl.id.clone(), out.len());
            out.push(pl);
        }
    }
    out
}

fn innertube_page_has_real_videos(it_page: &crate::innertube::InnerTubeChannelPage) -> bool {
    it_page
        .items
        .iter()
        .zip(it_page.metadata.iter())
        .any(|(v, m)| {
            m.kind != SearchResultKind::Playlist
                && !is_playlist_url(&v.url)
                && !v.url.contains("playlist?list=")
        })
}

fn backfill_videos_tab_if_needed(
    it_page: &mut crate::innertube::InnerTubeChannelPage,
    ctx: &crate::innertube::InnerTubeContext,
    session: Option<&mut crate::innertube::YouTubeWebSession>,
    cancel: &Cancellation,
) {
    if innertube_page_has_real_videos(it_page) {
        return;
    }
    let Some(videos_tab) = it_page
        .tabs
        .iter()
        .find(|t| t.kind == crate::innertube::ChannelTabKind::Videos && !t.selected)
        .cloned()
    else {
        return;
    };
    if let Ok(videos_page) = crate::innertube::fetch_channel_browse(
        Some(&videos_tab.browse_id),
        videos_tab.params.as_deref(),
        None,
        ctx,
        session,
        cancel,
    ) {
        let fetched_real_videos = innertube_page_has_real_videos(&videos_page);
        if fetched_real_videos {
            for tab in &mut it_page.tabs {
                tab.selected = tab.kind == crate::innertube::ChannelTabKind::Videos;
            }
        }
        it_page.items.extend(videos_page.items);
        it_page.metadata.extend(videos_page.metadata);
        if !videos_page.sort_chips.is_empty() {
            it_page.sort_chips = videos_page.sort_chips;
        }
        if videos_page.continuation.is_some() {
            it_page.continuation = videos_page.continuation;
        }
    }
}

/// Converts a parsed InnerTube channel browse response into a `ChannelPage`
/// without imposing any 100-video cap, preserving dynamic tabs, sort chips,
/// and continuation tokens.
pub fn channel_page_from_innertube(
    it: crate::innertube::InnerTubeChannelPage,
    requested_target: &ChannelTarget,
) -> ChannelPage {
    let channel_id = it.channel_id.or_else(|| match requested_target {
        ChannelTarget::ChannelId(id) => Some(id.clone()),
        ChannelTarget::Handle(_) => None,
    });
    let handle = it.handle.or_else(|| match requested_target {
        ChannelTarget::Handle(h) => Some(h.clone()),
        ChannelTarget::ChannelId(_) => None,
    });
    let resolved_target = if let Some(id) = &channel_id {
        ChannelTarget::ChannelId(id.clone())
    } else if let Some(h) = &handle {
        ChannelTarget::Handle(h.clone())
    } else {
        requested_target.clone()
    };
    let canonical_url = resolved_target.canonical_url();
    let subscriber_count = it
        .subscriber_count_text
        .as_deref()
        .and_then(parse_pt_br_compact_count);
    let selected_tab_kind = it.tabs.iter().find(|t| t.selected).map(|t| t.kind.clone());
    let is_playlists_tab = selected_tab_kind == Some(crate::innertube::ChannelTabKind::Playlists);
    let has_home_tab = it
        .tabs
        .iter()
        .any(|t| t.kind == crate::innertube::ChannelTabKind::Home);
    let has_shorts_tab = it
        .tabs
        .iter()
        .any(|t| t.kind == crate::innertube::ChannelTabKind::Shorts);
    let has_streams_tab = it
        .tabs
        .iter()
        .any(|t| t.kind == crate::innertube::ChannelTabKind::Live);
    let has_playlists_tab = it
        .tabs
        .iter()
        .any(|t| t.kind == crate::innertube::ChannelTabKind::Playlists);

    let mut videos: Vec<SearchResult> = Vec::new();
    let mut initial_playlists: Vec<ChannelPlaylist> = Vec::new();
    let mut seen_playlist_ids = std::collections::HashSet::new();
    for (video, metadata) in it.items.into_iter().zip(it.metadata) {
        let res = SearchResult { video, metadata };
        if is_playlist_search_result(&res) {
            if let Some(pl) = channel_playlist_from_search_result(&res)
                && seen_playlist_ids.insert(pl.id.clone())
            {
                initial_playlists.push(pl);
            }
        } else if !is_playlists_tab {
            videos.push(res);
        }
    }
    let has_videos_tab = !videos.is_empty();
    let (videos_popular_continuation, videos_oldest_continuation, sort_chips, next_continuation) =
        if is_playlists_tab {
            (None, None, Vec::new(), None)
        } else {
            (
                it.sort_chips.get(1).map(|c| c.continuation.clone()),
                it.sort_chips.get(2).map(|c| c.continuation.clone()),
                it.sort_chips,
                it.continuation,
            )
        };

    ChannelPage {
        target: resolved_target,
        channel_id,
        title: it.title,
        handle,
        canonical_url,
        subscriber_count,
        subscriber_count_text: it.subscriber_count_text,
        video_count_text: it.video_count_text,
        videos_popular_continuation,
        videos_oldest_continuation,
        has_home_tab,
        has_videos_tab,
        has_shorts_tab,
        has_streams_tab,
        has_playlists_tab: has_playlists_tab || !initial_playlists.is_empty(),
        avatar_url: it.avatar_url,
        banner_url: it.banner_url,
        verified: Some(it.verified || it.official_artist),
        videos,
        initial_playlists,
        tab_endpoints: it.tabs,
        sort_chips,
        next_continuation,
        community_posts: it.community_posts,
        ..ChannelPage::default()
    }
}

/// Fetches a channel page via InnerTube `youtubei/v1/browse` with dynamic `tabRenderer.endpoint`,
/// sort chips, continuation pagination (no 100-video cap), and canonical `UC...` LRU caching.
/// Falls back to HTML/yt-dlp strictly when InnerTube fails.
#[allow(dead_code)]
pub fn fetch_channel_page_innertube(
    input: &str,
    tab_params: Option<&str>,
    force_refresh: bool,
    language: crate::i18n::UiLanguage,
    country: Option<&str>,
    session: Option<&crate::innertube::YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<ChannelFetchOutcome, String> {
    if cancel.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let target = parse_channel_target(input).ok_or_else(|| {
        "Identificador ou URL de canal do YouTube inválido ou não suportado.".to_owned()
    })?;
    let cache = channel_page_cache();
    let has_tab_params = tab_params.is_some_and(|p| !p.trim().is_empty());

    if !has_tab_params && !force_refresh {
        let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let canonical = target.canonical_url();
        let hit = guard
            .get(input, now)
            .cloned()
            .or_else(|| guard.get(&canonical, now).cloned());
        if let Some(page) = hit {
            return Ok(ChannelFetchOutcome {
                page,
                from_cache: true,
            });
        }
    }

    let known_browse_id = match &target {
        ChannelTarget::ChannelId(id) => Some(id.clone()),
        ChannelTarget::Handle(_) => {
            let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
            guard
                .resolve_key(input)
                .or_else(|| guard.resolve_key(&target.canonical_url()))
                .map(|key| key.as_str().to_owned())
        }
    };

    let ctx = crate::innertube::InnerTubeContext::from_language(language, country);
    let mut owned_session = session.cloned();

    let mut page_opt: Option<ChannelPage> = None;
    if let Some(browse_id) = known_browse_id.as_deref()
        && let Ok(mut it_page) = crate::innertube::fetch_channel_browse(
            Some(browse_id),
            tab_params,
            None,
            &ctx,
            owned_session.as_mut(),
            cancel,
        )
    {
        if !has_tab_params {
            backfill_videos_tab_if_needed(&mut it_page, &ctx, owned_session.as_mut(), cancel);
        }
        page_opt = Some(channel_page_from_innertube(it_page, &target));
    }

    if cancel.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }

    let page = match page_opt {
        Some(page) => page,
        None => {
            // Resolve `@handle` or fall back when direct browse failed: HTML first, then yt-dlp.
            let html_or_fallback = fetch_channel_page_for_target_with_session(
                &target,
                MAX_CHANNEL_VIDEOS_LIMIT,
                &ctx,
                owned_session.as_mut(),
                cancel,
            )?;
            if has_tab_params
                && let Some(cid) = html_or_fallback.channel_id.as_deref()
                && let Ok(tab_page) = crate::innertube::fetch_channel_browse(
                    Some(cid),
                    tab_params,
                    None,
                    &ctx,
                    owned_session.as_mut(),
                    cancel,
                )
            {
                let mut merged = html_or_fallback;
                let mut videos = Vec::new();
                let mut playlists = Vec::new();
                let mut seen_pl = std::collections::HashSet::new();
                for (video, metadata) in tab_page.items.into_iter().zip(tab_page.metadata) {
                    let res = SearchResult { video, metadata };
                    if is_playlist_search_result(&res) {
                        if let Some(pl) = channel_playlist_from_search_result(&res)
                            && seen_pl.insert(pl.id.clone())
                        {
                            playlists.push(pl);
                        }
                    } else {
                        videos.push(res);
                    }
                }
                merged.videos = videos;
                merged.has_videos_tab = !merged.videos.is_empty();
                if !playlists.is_empty() {
                    merged.initial_playlists = merge_channel_playlists(
                        merged.initial_playlists,
                        playlists,
                        MAX_CHANNEL_PLAYLISTS_LIMIT,
                    );
                    merged.has_playlists_tab = true;
                }
                if !tab_page.tabs.is_empty() {
                    merged.tab_endpoints = tab_page.tabs;
                }
                if !tab_page.sort_chips.is_empty() {
                    merged.sort_chips = tab_page.sort_chips;
                }
                merged.next_continuation = tab_page.continuation;
                merged
            } else {
                html_or_fallback
            }
        }
    };

    if !has_tab_params {
        let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        store_channel_page_in_cache(&mut guard, input, &target, &page, Instant::now());
    }

    Ok(ChannelFetchOutcome {
        page,
        from_cache: false,
    })
}

/// Fetches a channel tab (`browse_id` + `params`) or continuation page (`continuation`)
/// via InnerTube `youtubei/v1/browse` with no 100-video cap.
#[allow(dead_code)]
pub fn fetch_channel_tab_continuation(
    browse_id: Option<&str>,
    params: Option<&str>,
    continuation: Option<&str>,
    language: crate::i18n::UiLanguage,
    country: Option<&str>,
    session: Option<&crate::innertube::YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<ChannelTabPage, String> {
    if cancel.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let ctx = crate::innertube::InnerTubeContext::from_language(language, country);
    let mut owned_session = session.cloned();
    let it = crate::innertube::fetch_channel_browse(
        browse_id,
        params,
        continuation,
        &ctx,
        owned_session.as_mut(),
        cancel,
    )
    .map_err(|err| err.into_message())?;

    let videos = it
        .items
        .into_iter()
        .zip(it.metadata)
        .map(|(video, metadata)| SearchResult { video, metadata })
        .collect();
    Ok(ChannelTabPage {
        videos,
        sort_chips: it.sort_chips,
        next_continuation: it.continuation,
        tab_endpoints: it.tabs,
        community_posts: it.community_posts,
    })
}

#[cfg(test)]
fn fetch_channel_page_for_target(
    target: &ChannelTarget,
    effective_limit: usize,
) -> Result<ChannelPage, String> {
    fetch_channel_page_for_target_cancel(target, effective_limit, &Cancellation::default())
}

pub fn empty_channel_page_for_target(target: &ChannelTarget) -> ChannelPage {
    let handle = match target {
        ChannelTarget::Handle(h) => Some(h.clone()),
        ChannelTarget::ChannelId(_) => None,
    };
    let channel_id = match target {
        ChannelTarget::ChannelId(id) => Some(id.clone()),
        ChannelTarget::Handle(_) => None,
    };
    ChannelPage {
        target: target.clone(),
        channel_id,
        title: None,
        handle,
        canonical_url: target.canonical_url(),
        ..ChannelPage::default()
    }
}

fn fetch_channel_page_for_target_cancel(
    target: &ChannelTarget,
    effective_limit: usize,
    cancellation: &Cancellation,
) -> Result<ChannelPage, String> {
    let ctx = crate::innertube::InnerTubeContext::default();
    fetch_channel_page_for_target_with_session(target, effective_limit, &ctx, None, cancellation)
}

fn fetch_channel_page_for_target_with_session(
    target: &ChannelTarget,
    effective_limit: usize,
    ctx: &crate::innertube::InnerTubeContext,
    mut session: Option<&mut crate::innertube::YouTubeWebSession>,
    cancellation: &Cancellation,
) -> Result<ChannelPage, String> {
    let html_page = fetch_channel_page_html(target, effective_limit, cancellation).ok();
    if session.is_none()
        && let Some(page) = html_page.as_ref()
        && (!page.videos.is_empty()
            || !page.home_shelves.is_empty()
            || page.featured_video.is_some())
    {
        return Ok(page.clone());
    }
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let browse_id_opt = match target {
        ChannelTarget::ChannelId(id) => Some(id.clone()),
        ChannelTarget::Handle(_) => html_page.as_ref().and_then(|p| p.channel_id.clone()),
    };
    if let Some(browse_id) = browse_id_opt.as_deref() {
        if let Ok(mut it_page) = crate::innertube::fetch_channel_browse(
            Some(browse_id),
            None,
            None,
            ctx,
            session.as_deref_mut(),
            cancellation,
        ) {
            backfill_videos_tab_if_needed(&mut it_page, ctx, session, cancellation);
            let page = channel_page_from_innertube(it_page, target);
            if !page.videos.is_empty()
                || !page.tab_endpoints.is_empty()
                || !page.initial_playlists.is_empty()
            {
                return Ok(page);
            }
        }
        if cancellation.cancelled() {
            return Err("Operação cancelada.".to_owned());
        }
    }
    if let Some(page) = html_page.as_ref()
        && (!page.videos.is_empty()
            || !page.home_shelves.is_empty()
            || page.featured_video.is_some()
            || !page.tab_endpoints.is_empty()
            || !page.initial_playlists.is_empty())
    {
        return Ok(page.clone());
    }
    let limit_arg = effective_limit.to_string();
    let videos_url = target.videos_tab_url();
    let output = process::output(
        yt_dlp().args([
            "--flat-playlist",
            "--extractor-args",
            "youtubetab:approximate_date",
            "--playlist-end",
            &limit_arg,
            "--dump-single-json",
            "--skip-download",
            "--no-warnings",
            "--",
            &videos_url,
        ]),
        cancellation,
        QUERY_TIMEOUT,
    )
    .map_err(|error| {
        format!("Não foi possível executar yt-dlp: {error}. Verifique se ele está ao lado do app ou no PATH.")
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if stderr.contains("does not have a") || stderr.contains("has no videos") {
            return Ok(html_page.unwrap_or_else(|| empty_channel_page_for_target(target)));
        }
        return Err(format!("Não foi possível consultar o canal: {stderr}"));
    }
    match parse_channel_page(&output.stdout, target, effective_limit) {
        Ok(page) => Ok(page),
        Err(_) if html_page.is_some() => {
            Ok(html_page.unwrap_or_else(|| empty_channel_page_for_target(target)))
        }
        Err(err) => Err(err),
    }
}

fn fetch_youtube_initial_data_url(url: &str, cancellation: &Cancellation) -> Result<Value, String> {
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let response = crate::http_client::shared()
        .get(url)
        .timeout(Duration::from_secs(12))
        .header(reqwest::header::USER_AGENT, SUBTITLE_USER_AGENT)
        .header(
            reqwest::header::ACCEPT_LANGUAGE,
            "pt-BR,pt;q=0.9,en-US;q=0.8,en;q=0.7",
        )
        .send()
        .map_err(|error| format!("Falha na consulta ao YouTube: {error}"))?;
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    if !response.status().is_success() {
        return Err(format!("YouTube retornou HTTP {}", response.status()));
    }
    let body = response
        .text()
        .map_err(|error| format!("Falha ao ler resposta do YouTube: {error}"))?;
    extract_yt_initial_data(&body)
        .ok_or_else(|| "Não foi possível ler os dados iniciais da página do YouTube.".to_owned())
}

fn fetch_youtube_innertube_browse(
    continuation: &str,
    cancellation: &Cancellation,
) -> Result<Value, String> {
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let payload = serde_json::json!({
        "context": {
            "client": {
                "hl": "pt",
                "gl": "BR",
                "clientName": "WEB",
                "clientVersion": "2.20260101.00.00"
            }
        },
        "continuation": continuation
    });
    let response = crate::http_client::shared()
        .post("https://www.youtube.com/youtubei/v1/browse?prettyPrint=false")
        .timeout(Duration::from_secs(10))
        .header(reqwest::header::USER_AGENT, SUBTITLE_USER_AGENT)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(
            reqwest::header::ACCEPT_LANGUAGE,
            "pt-BR,pt;q=0.9,en-US;q=0.8,en;q=0.7",
        )
        .json(&payload)
        .send()
        .map_err(|error| format!("Falha na consulta browse ao YouTube: {error}"))?;
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    if !response.status().is_success() {
        return Err(format!(
            "YouTube browse retornou HTTP {}",
            response.status()
        ));
    }
    response
        .json::<Value>()
        .map_err(|error| format!("Falha ao decodificar resposta browse do YouTube: {error}"))
}

fn fetch_youtube_innertube_player_duration(
    video_id: &str,
    cancellation: &Cancellation,
) -> Option<String> {
    if cancellation.cancelled() || !crate::history::is_valid_video_id(video_id) {
        return None;
    }
    let payload = serde_json::json!({
        "context": {
            "client": {
                "hl": "pt",
                "gl": "BR",
                "clientName": "WEB",
                "clientVersion": "2.20260101.00.00"
            }
        },
        "videoId": video_id
    });
    let response = crate::http_client::shared()
        .post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false")
        .timeout(Duration::from_secs(6))
        .header(reqwest::header::USER_AGENT, SUBTITLE_USER_AGENT)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&payload)
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let val = response.json::<Value>().ok()?;
    let secs = val
        .pointer("/videoDetails/lengthSeconds")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<u64>().ok())?;
    let formatted = format_duration_seconds(secs);
    (!formatted.is_empty()).then_some(formatted)
}

pub fn decode_youtube_redirect_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let normalized = if trimmed.starts_with("/redirect") {
        format!("https://www.youtube.com{trimmed}")
    } else {
        trimmed.to_owned()
    };
    if (normalized.starts_with("https://www.youtube.com/redirect")
        || normalized.starts_with("http://www.youtube.com/redirect"))
        && let Ok(parsed) = url::Url::parse(&normalized)
    {
        for (k, v) in parsed.query_pairs() {
            if k == "q" && !v.trim().is_empty() {
                let target = v.trim();
                if target.starts_with("http://") || target.starts_with("https://") {
                    return target.to_owned();
                }
                return format!("https://{}", target.trim_start_matches('/'));
            }
        }
    }
    if normalized.starts_with("http://") || normalized.starts_with("https://") {
        normalized
    } else {
        format!("https://{}", normalized.trim_start_matches('/'))
    }
}

fn infer_external_link_title(url: &str) -> String {
    let lower = url.to_lowercase();
    if lower.contains("twitch.tv") {
        "Twitch".to_owned()
    } else if lower.contains("twitter.com") || lower.contains("x.com") {
        "Twitter".to_owned()
    } else if lower.contains("discord.gg") || lower.contains("discord.com") {
        "Discord".to_owned()
    } else if lower.contains("instagram.com") {
        "Instagram".to_owned()
    } else if lower.contains("tiktok.com") {
        "TikTok".to_owned()
    } else if lower.contains("facebook.com") {
        "Facebook".to_owned()
    } else if lower.contains("github.com") {
        "GitHub".to_owned()
    } else if lower.contains("reddit.com") {
        "Reddit".to_owned()
    } else if let Ok(parsed) = url::Url::parse(url)
        && let Some(host) = parsed.host_str()
    {
        host.strip_prefix("www.").unwrap_or(host).to_owned()
    } else {
        url.trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("www.")
            .to_owned()
    }
}

fn find_first_continuation_token(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(token) = map
                .get("continuationCommand")
                .and_then(|c| c.get("token"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                return Some(token.to_owned());
            }
            for child in map.values() {
                if let Some(found) = find_first_continuation_token(child) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(find_first_continuation_token),
        _ => None,
    }
}

fn find_about_channel_view_model(value: &Value) -> Option<&Value> {
    match value {
        Value::Object(map) => {
            if let Some(vm) = map.get("aboutChannelViewModel") {
                return Some(vm);
            }
            for child in map.values() {
                if let Some(found) = find_about_channel_view_model(child) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(find_about_channel_view_model),
        _ => None,
    }
}

fn apply_about_continuation_data(page: &mut ChannelPage, about_resp: &Value) {
    let Some(vm) = find_about_channel_view_model(about_resp) else {
        return;
    };
    if let Some(desc) = nonempty_str(vm, "description") {
        page.description = sanitize_channel_description(Some(desc));
    }
    if let Some(country) = nonempty_str(vm, "country") {
        page.country = Some(country.to_owned());
    }
    let joined = yt_text(vm.get("joinedDateText"));
    if !joined.is_empty() {
        page.joined_date_text = Some(joined);
    }
    if let Some(subs) = nonempty_str(vm, "subscriberCountText") {
        if page.subscriber_count.is_none() {
            page.subscriber_count = parse_pt_br_compact_count(subs);
        }
        if page.subscriber_count_text.is_none() {
            page.subscriber_count_text = Some(subs.to_owned());
        }
    }
    if let Some(videos_txt) = nonempty_str(vm, "videoCountText")
        && page.video_count_text.is_none()
    {
        page.video_count_text = Some(videos_txt.to_owned());
    }
    if let Some(views_txt) = nonempty_str(vm, "viewCountText") {
        page.view_count_text = Some(views_txt.to_owned());
        if page.view_count.is_none() {
            page.view_count = parse_pt_br_compact_count(views_txt);
        }
    }
    if let Some(links_arr) = vm.get("links").and_then(Value::as_array) {
        let mut parsed_links = Vec::new();
        for item in links_arr {
            let link_vm = item.get("channelExternalLinkViewModel").unwrap_or(item);
            let title = link_vm
                .pointer("/title/content")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            let raw_url = link_vm
                .pointer("/link/commandRuns/0/onTap/innertubeCommand/urlEndpoint/url")
                .or_else(|| link_vm.pointer("/link/content"))
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if !raw_url.is_empty() {
                let decoded = decode_youtube_redirect_url(raw_url);
                let label = if title.is_empty() {
                    infer_external_link_title(&decoded)
                } else {
                    title.to_owned()
                };
                parsed_links.push(ChannelExternalLink {
                    title: label,
                    url: decoded,
                });
            }
        }
        if !parsed_links.is_empty() {
            page.links = parsed_links;
        }
    }
}

fn fill_channel_defaults_on_result(
    res: &mut SearchResult,
    channel_title: Option<&str>,
    channel_id: Option<&str>,
    canonical_url: &str,
    verified: bool,
) {
    if res.video.channel.is_empty()
        && let Some(title) = channel_title
    {
        res.video.channel = title.to_owned();
    }
    if res.metadata.channel_id.is_none()
        && let Some(cid) = channel_id
    {
        res.metadata.channel_id = Some(cid.to_owned());
    }
    if res.metadata.channel_url.is_none() && !canonical_url.is_empty() {
        res.metadata.channel_url = Some(canonical_url.to_owned());
    }
    if verified {
        res.metadata.channel_verified = true;
    }
}

pub fn parse_channel_initial_data(
    featured_data: &Value,
    videos_data: Option<&Value>,
    requested_target: &ChannelTarget,
    limit: usize,
) -> Result<ChannelPage, String> {
    let effective_limit = limit.clamp(1, MAX_CHANNEL_VIDEOS_LIMIT);
    let data = featured_data;
    let meta = data.pointer("/metadata/channelMetadataRenderer");
    let page_header = data.pointer("/header/pageHeaderRenderer");
    let header_vm = page_header.and_then(|h| h.pointer("/content/pageHeaderViewModel"));

    let channel_id = meta
        .and_then(|m| nonempty_str(m, "externalId"))
        .filter(|id| is_valid_channel_id(id))
        .map(str::to_owned)
        .or_else(|| match requested_target {
            ChannelTarget::ChannelId(id) => Some(id.clone()),
            ChannelTarget::Handle(_) => None,
        });

    let mut handle = meta
        .and_then(|m| nonempty_str(m, "vanityChannelUrl"))
        .and_then(parse_youtube_channel_url)
        .and_then(|t| match t {
            ChannelTarget::Handle(h) => Some(h),
            ChannelTarget::ChannelId(_) => None,
        })
        .or_else(|| match requested_target {
            ChannelTarget::Handle(h) => Some(h.clone()),
            ChannelTarget::ChannelId(_) => None,
        });

    let mut subscriber_count = None;
    let mut subscriber_count_text = None;
    let mut video_count_text = None;

    if let Some(rows) = header_vm
        .and_then(|vm| vm.pointer("/metadata/contentMetadataViewModel/metadataRows"))
        .and_then(Value::as_array)
    {
        for row in rows {
            let Some(parts) = row.get("metadataParts").and_then(Value::as_array) else {
                continue;
            };
            for part in parts {
                let text = part
                    .pointer("/text/content")
                    .or_else(|| part.get("content"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("");
                if text.is_empty() {
                    continue;
                }
                if text.starts_with('@') && is_valid_channel_handle(text) {
                    handle = Some(text.to_owned());
                } else {
                    let lower = text.to_lowercase();
                    if lower.contains("inscrito") || lower.contains("subscriber") {
                        subscriber_count_text = Some(text.to_owned());
                        subscriber_count = parse_pt_br_compact_count(text);
                    } else if lower.contains("vídeo") || lower.contains("video") {
                        video_count_text = Some(text.to_owned());
                    }
                }
            }
        }
    }

    let resolved_target = if let Some(id) = &channel_id {
        ChannelTarget::ChannelId(id.clone())
    } else if let Some(h) = &handle {
        ChannelTarget::Handle(h.clone())
    } else {
        requested_target.clone()
    };
    let canonical_url = resolved_target.canonical_url();

    let title = page_header
        .and_then(|h| nonempty_str(h, "pageTitle"))
        .or_else(|| {
            header_vm
                .and_then(|vm| vm.pointer("/title/dynamicTextViewModel/text/content"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .or_else(|| meta.and_then(|m| nonempty_str(m, "title")))
        .map(str::to_owned);

    let verified = header_vm
        .and_then(|vm| vm.pointer("/title/dynamicTextViewModel/text/attachmentRuns"))
        .and_then(Value::as_array)
        .is_some_and(|runs| !runs.is_empty());

    let banner_url = header_vm
        .map(|vm| {
            select_yt_thumbnail(
                vm.pointer("/banner/imageBannerViewModel/image/sources"),
                2560,
            )
        })
        .filter(|s| !s.is_empty());

    let avatar_url = header_vm
        .map(|vm| {
            select_yt_thumbnail(
                vm.pointer("/image/decoratedAvatarViewModel/avatar/avatarViewModel/image/sources"),
                160,
            )
        })
        .filter(|s| !s.is_empty())
        .or_else(|| {
            meta.map(|m| select_yt_thumbnail(m.pointer("/avatar/thumbnails"), 160))
                .filter(|s| !s.is_empty())
        });

    let description_preview = header_vm
        .and_then(|vm| {
            vm.pointer("/description/descriptionPreviewViewModel/description/content")
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let description = meta
        .and_then(|m| nonempty_str(m, "description"))
        .and_then(|d| sanitize_channel_description(Some(d)))
        .or_else(|| description_preview.clone());

    let about_continuation = header_vm.and_then(|vm| {
        vm.get("description")
            .and_then(find_first_continuation_token)
            .or_else(|| {
                vm.get("attribution")
                    .and_then(find_first_continuation_token)
            })
    });

    let mut links = Vec::new();
    let mut more_links_label = None;
    if let Some(attr) = header_vm.and_then(|vm| vm.get("attribution")) {
        let attr_vm = attr.get("attributionViewModel").unwrap_or(attr);
        let raw_url = attr_vm
            .pointer("/text/commandRuns/0/onTap/innertubeCommand/urlEndpoint/url")
            .or_else(|| attr_vm.pointer("/text/content"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if !raw_url.is_empty() {
            let decoded = decode_youtube_redirect_url(raw_url);
            let title = infer_external_link_title(&decoded);
            links.push(ChannelExternalLink {
                title,
                url: decoded,
            });
        }
        if let Some(suffix) = attr_vm
            .pointer("/suffix/content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            more_links_label = Some(suffix.trim_start_matches("...").trim().to_owned());
        }
    }
    if let Some(same_as) = data
        .pointer("/microformat/microformatDataRenderer/channelProfileMicroformatDetails/profilePage/mainEntity/sameAs")
        .and_then(Value::as_array)
    {
        for item in same_as {
            if let Some(raw) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                let decoded = decode_youtube_redirect_url(raw);
                if !links.iter().any(|existing| existing.url.eq_ignore_ascii_case(&decoded)) {
                    links.push(ChannelExternalLink {
                        title: infer_external_link_title(&decoded),
                        url: decoded,
                    });
                }
            }
        }
    }

    let mut has_home_tab = false;
    let mut has_shorts_tab = false;
    let mut has_streams_tab = false;
    let mut has_playlists_tab = false;
    let mut featured_video: Option<SearchResult> = None;
    let mut home_shelves: Vec<ChannelHomeShelf> = Vec::new();
    let mut videos: Vec<SearchResult> = Vec::new();
    let mut initial_playlists: Vec<ChannelPlaylist> = Vec::new();
    let mut seen_initial_playlists = std::collections::HashSet::new();
    let mut videos_popular_continuation = None;
    let mut videos_oldest_continuation = None;

    if let Some(tabs) = data
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for tab_entry in tabs {
            let Some(tab) = tab_entry.get("tabRenderer") else {
                continue;
            };
            let tab_url = tab
                .pointer("/endpoint/commandMetadata/webCommandMetadata/url")
                .and_then(Value::as_str)
                .unwrap_or("");
            if tab_url.ends_with("/featured") || tab_url.ends_with("/home") {
                has_home_tab = true;
            } else if tab_url.ends_with("/shorts") {
                has_shorts_tab = true;
            } else if tab_url.ends_with("/streams") {
                has_streams_tab = true;
            } else if tab_url.ends_with("/playlists") {
                has_playlists_tab = true;
            }

            if tab_url.ends_with("/playlists") {
                if tab.get("selected").and_then(Value::as_bool) == Some(true)
                    && let Some(content) = tab.get("content")
                {
                    let mut raw = Vec::new();
                    let mut seen = std::collections::HashSet::new();
                    collect_yt_search_items(content, &mut raw, &mut seen);
                    for item in raw {
                        if let Some(pl) = channel_playlist_from_search_result(&item)
                            && seen_initial_playlists.insert(pl.id.clone())
                        {
                            initial_playlists.push(pl);
                        }
                    }
                }
                continue;
            }

            let is_home_tab =
                tab_url.is_empty() || tab_url.ends_with("/featured") || tab_url.ends_with("/home");
            if is_home_tab
                && let Some(sections) = tab
                    .pointer("/content/sectionListRenderer/contents")
                    .and_then(Value::as_array)
            {
                for sec in sections {
                    let Some(item_sec) = sec
                        .pointer("/itemSectionRenderer/contents")
                        .and_then(Value::as_array)
                    else {
                        continue;
                    };
                    for item in item_sec {
                        if let Some(cvp) = item.get("channelVideoPlayerRenderer") {
                            if let Some(vid) = nonempty_str(cvp, "videoId") {
                                let v_title = yt_text(cvp.get("title"));
                                let v_desc = {
                                    let d = yt_text(cvp.get("description"));
                                    (!d.is_empty()).then_some(d)
                                };
                                let views_raw = yt_text(cvp.get("viewCountText"));
                                let view_count = parse_pt_br_compact_count(&views_raw);
                                let view_count_text = clean_pt_br_view_label(&views_raw);
                                let pub_raw = yt_text(cvp.get("publishedTimeText"));
                                let published_time_text = (!pub_raw.is_empty()).then_some(pub_raw);
                                featured_video = Some(SearchResult {
                                    video: Video {
                                        title: v_title,
                                        channel: title.clone().unwrap_or_default(),
                                        url: format!("https://www.youtube.com/watch?v={vid}"),
                                        thumbnail: format!(
                                            "https://i.ytimg.com/vi/{vid}/hqdefault.jpg"
                                        ),
                                        duration: String::new(),
                                    },
                                    metadata: VideoMetadata {
                                        view_count,
                                        description: v_desc,
                                        channel_id: channel_id.clone(),
                                        channel_url: Some(canonical_url.clone()),
                                        kind: SearchResultKind::Video,
                                        published_time_text,
                                        view_count_text,
                                        channel_verified: verified,
                                        ..Default::default()
                                    },
                                });
                            }
                        } else if let Some(shelf) = item.get("shelfRenderer") {
                            let shelf_title = yt_text(shelf.get("title"));
                            let subtitle = {
                                let sub = yt_text(shelf.get("subtitle"));
                                (!sub.is_empty()).then_some(sub)
                            };
                            let playlist_id = shelf
                                .pointer(
                                    "/playAllButton/buttonRenderer/navigationEndpoint/watchEndpoint/playlistId",
                                )
                                .and_then(Value::as_str)
                                .or_else(|| {
                                    shelf
                                        .pointer("/endpoint/browseEndpoint/browseId")
                                        .and_then(Value::as_str)
                                        .and_then(|b| b.strip_prefix("VL"))
                                })
                                .map(str::trim)
                                .filter(|id| is_valid_playlist_id(id) && !id.starts_with("UU"))
                                .map(str::to_owned);
                            let mut shelf_items = Vec::new();
                            let mut seen = std::collections::HashSet::new();
                            if let Some(content) = shelf.get("content") {
                                collect_yt_search_items(content, &mut shelf_items, &mut seen);
                            }
                            for entry in &mut shelf_items {
                                fill_channel_defaults_on_result(
                                    entry,
                                    title.as_deref(),
                                    channel_id.as_deref(),
                                    &canonical_url,
                                    verified,
                                );
                            }
                            if !shelf_items.is_empty() {
                                home_shelves.push(ChannelHomeShelf {
                                    title: shelf_title,
                                    subtitle,
                                    kind: ChannelShelfKind::Videos,
                                    playlist_id,
                                    items: shelf_items,
                                });
                            }
                        } else if let Some(reel) = item.get("reelShelfRenderer") {
                            let mut shelf_title = yt_text(reel.get("title"));
                            if shelf_title.is_empty() {
                                shelf_title = "Shorts".to_owned();
                            }
                            let mut shelf_items = Vec::new();
                            let mut seen = std::collections::HashSet::new();
                            if let Some(items_val) = reel.get("items") {
                                collect_yt_search_items(items_val, &mut shelf_items, &mut seen);
                            }
                            for entry in &mut shelf_items {
                                fill_channel_defaults_on_result(
                                    entry,
                                    title.as_deref(),
                                    channel_id.as_deref(),
                                    &canonical_url,
                                    verified,
                                );
                            }
                            if !shelf_items.is_empty() {
                                has_shorts_tab = true;
                                home_shelves.push(ChannelHomeShelf {
                                    title: shelf_title,
                                    subtitle: None,
                                    kind: ChannelShelfKind::Shorts,
                                    playlist_id: None,
                                    items: shelf_items,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(v_data) = videos_data
        && let Some(tabs) = v_data
            .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
            .and_then(Value::as_array)
    {
        for tab_entry in tabs {
            let Some(tab_r) = tab_entry.get("tabRenderer") else {
                continue;
            };
            let tab_url = tab_r
                .pointer("/endpoint/commandMetadata/webCommandMetadata/url")
                .and_then(Value::as_str)
                .unwrap_or("");
            if tab_url.ends_with("/playlists") {
                continue;
            }
            let Some(grid) = tab_r.pointer("/content/richGridRenderer") else {
                continue;
            };
            let mut seen = std::collections::HashSet::new();
            if let Some(contents) = grid.get("contents") {
                collect_yt_search_items(contents, &mut videos, &mut seen);
            }
            videos.retain(|item| !is_playlist_search_result(item));
            if let Some(chips) = grid
                .pointer("/header/chipBarViewModel/chips")
                .and_then(Value::as_array)
            {
                if let Some(chip_pop) = chips.get(1) {
                    videos_popular_continuation = find_first_continuation_token(chip_pop);
                }
                if let Some(chip_old) = chips.get(2) {
                    videos_oldest_continuation = find_first_continuation_token(chip_old);
                }
            }
            break;
        }
    }

    if videos.is_empty() {
        for shelf in &home_shelves {
            if shelf.kind == ChannelShelfKind::Videos {
                let real_videos: Vec<SearchResult> = shelf
                    .items
                    .iter()
                    .filter(|item| !is_playlist_search_result(item))
                    .cloned()
                    .collect();
                if !real_videos.is_empty() {
                    videos = real_videos;
                    break;
                }
            }
        }
    }

    let parsed_featured_it = crate::innertube::parse_channel_browse_response(featured_data);
    let parsed_videos_it = videos_data.map(crate::innertube::parse_channel_browse_response);
    let tab_endpoints = if !parsed_featured_it.tabs.is_empty() {
        parsed_featured_it.tabs
    } else {
        parsed_videos_it
            .as_ref()
            .map(|v| v.tabs.clone())
            .unwrap_or_default()
    };
    let has_home_tab = has_home_tab
        || tab_endpoints
            .iter()
            .any(|t| t.kind == crate::innertube::ChannelTabKind::Home);
    let (sort_chips, next_continuation) = if videos.is_empty() {
        (Vec::new(), None)
    } else {
        let sort_chips = parsed_videos_it
            .as_ref()
            .filter(|v| !v.sort_chips.is_empty())
            .map(|v| v.sort_chips.clone())
            .unwrap_or(parsed_featured_it.sort_chips);
        let next_continuation = parsed_videos_it
            .and_then(|v| v.continuation)
            .or(parsed_featured_it.continuation);
        (sort_chips, next_continuation)
    };

    for v in &mut videos {
        fill_channel_defaults_on_result(
            v,
            title.as_deref(),
            channel_id.as_deref(),
            &canonical_url,
            verified,
        );
    }
    videos.truncate(effective_limit);
    let has_videos_tab = !videos.is_empty();

    Ok(ChannelPage {
        target: resolved_target,
        channel_id,
        title,
        handle,
        canonical_url,
        description,
        description_preview,
        subscriber_count,
        view_count: None,
        subscriber_count_text,
        video_count_text,
        view_count_text: None,
        country: None,
        joined_date_text: None,
        links,
        more_links_label,
        about_continuation,
        featured_video,
        home_shelves,
        videos_popular_continuation,
        videos_oldest_continuation,
        has_home_tab,
        has_videos_tab,
        has_shorts_tab,
        has_streams_tab,
        has_playlists_tab: has_playlists_tab || !initial_playlists.is_empty(),
        avatar_url,
        banner_url,
        verified: Some(verified),
        tags: Vec::new(),
        videos,
        initial_playlists,
        tab_endpoints,
        sort_chips,
        next_continuation,
        community_posts: Vec::new(),
    })
}

fn fetch_channel_page_html(
    target: &ChannelTarget,
    effective_limit: usize,
    cancellation: &Cancellation,
) -> Result<ChannelPage, String> {
    let featured_url = target.featured_tab_url();
    let videos_url = target.videos_tab_url();

    let (featured_res, videos_res) = std::thread::scope(|s| {
        let f_handle = s.spawn(|| fetch_youtube_initial_data_url(&featured_url, cancellation));
        let v_handle = s.spawn(|| fetch_youtube_initial_data_url(&videos_url, cancellation));
        (
            f_handle
                .join()
                .unwrap_or_else(|_| Err("Worker panic".to_owned())),
            v_handle
                .join()
                .unwrap_or_else(|_| Err("Worker panic".to_owned())),
        )
    });

    let featured_data = match featured_res {
        Ok(val) => val,
        Err(err) => {
            if let Ok(v_val) = videos_res {
                return parse_channel_initial_data(&v_val, Some(&v_val), target, effective_limit);
            }
            return Err(err);
        }
    };

    let mut page = parse_channel_initial_data(
        &featured_data,
        videos_res.as_ref().ok(),
        target,
        effective_limit,
    )?;

    let featured_vid_id = page
        .featured_video
        .as_ref()
        .filter(|v| v.video.duration.is_empty())
        .and_then(|v| crate::history::youtube_video_id(&v.video.url));
    let about_token = page.about_continuation.clone();

    if featured_vid_id.is_some() || about_token.is_some() {
        let (dur_opt, about_opt) = std::thread::scope(|s| {
            let d_handle = s.spawn(|| {
                featured_vid_id
                    .as_deref()
                    .and_then(|vid| fetch_youtube_innertube_player_duration(vid, cancellation))
            });
            let a_handle = s.spawn(|| {
                about_token
                    .as_deref()
                    .and_then(|tok| fetch_youtube_innertube_browse(tok, cancellation).ok())
            });
            (
                d_handle.join().ok().flatten(),
                a_handle.join().ok().flatten(),
            )
        });
        if let (Some(dur), Some(fv)) = (dur_opt, page.featured_video.as_mut()) {
            fv.video.duration = dur;
        }
        if let Some(about_resp) = about_opt {
            apply_about_continuation_data(&mut page, &about_resp);
        }
    }

    Ok(page)
}

fn fetch_channel_tab_videos_html(
    tab_url: &str,
    tab: ChannelFeedTab,
    effective_limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let data = fetch_youtube_initial_data_url(tab_url, cancellation)?;
    let meta = data.pointer("/metadata/channelMetadataRenderer");
    let channel_title = meta.and_then(|m| nonempty_str(m, "title"));
    let channel_id = meta
        .and_then(|m| nonempty_str(m, "externalId"))
        .filter(|id| is_valid_channel_id(id));
    let canonical_url = channel_id
        .map(|id| format!("https://www.youtube.com/channel/{id}"))
        .unwrap_or_default();
    let verified = data
        .pointer("/header/pageHeaderRenderer/content/pageHeaderViewModel/title/dynamicTextViewModel/text/attachmentRuns")
        .and_then(Value::as_array)
        .is_some_and(|runs| !runs.is_empty());

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(tabs) = data
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for tab_entry in tabs {
            let Some(tr) = tab_entry.get("tabRenderer") else {
                continue;
            };
            if tr.get("selected").and_then(Value::as_bool) != Some(true)
                && tr.get("content").is_none()
            {
                continue;
            }
            if let Some(content) = tr.get("content") {
                collect_yt_search_items(content, &mut out, &mut seen);
            }
        }
    }
    for item in &mut out {
        if tab == ChannelFeedTab::Shorts {
            item.metadata.kind = SearchResultKind::Short;
        }
        fill_channel_defaults_on_result(item, channel_title, channel_id, &canonical_url, verified);
    }
    out.truncate(effective_limit);
    Ok(out)
}

pub fn fetch_channel_sorted_videos_with_cancel(
    continuation_token: &str,
    channel_title: Option<&str>,
    channel_id: Option<&str>,
    canonical_url: &str,
    verified: bool,
    limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let effective_limit = limit.clamp(1, MAX_CHANNEL_VIDEOS_LIMIT);
    let resp = fetch_youtube_innertube_browse(continuation_token, cancellation)?;
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    collect_yt_search_items(&resp, &mut out, &mut seen);
    for item in &mut out {
        fill_channel_defaults_on_result(item, channel_title, channel_id, canonical_url, verified);
    }
    out.truncate(effective_limit);
    Ok(out)
}

pub fn fetch_channel_search_with_cancel(
    input: &str,
    query: &str,
    limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let (target, effective_limit) = parse_and_clamp_channel_request(input, limit)?;
    let trimmed_query = query.trim();
    if trimmed_query.is_empty() {
        return Ok(Vec::new());
    }
    let search_url = target
        .search_url(trimmed_query)
        .ok_or_else(|| "URL de busca do canal inválida.".to_owned())?;
    let data = fetch_youtube_initial_data_url(&search_url, cancellation)?;
    let meta = data.pointer("/metadata/channelMetadataRenderer");
    let channel_title = meta.and_then(|m| nonempty_str(m, "title"));
    let channel_id = meta
        .and_then(|m| nonempty_str(m, "externalId"))
        .filter(|id| is_valid_channel_id(id));
    let canonical_url = target.canonical_url();
    let verified = data
        .pointer("/header/pageHeaderRenderer/content/pageHeaderViewModel/title/dynamicTextViewModel/text/attachmentRuns")
        .and_then(Value::as_array)
        .is_some_and(|runs| !runs.is_empty());

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(tabs) = data
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for tab_entry in tabs {
            if let Some(expandable) = tab_entry.pointer("/expandableTabRenderer/content") {
                collect_yt_search_items(expandable, &mut out, &mut seen);
            } else if let Some(tr) = tab_entry.get("tabRenderer")
                && tr.get("selected").and_then(Value::as_bool) == Some(true)
                && let Some(content) = tr.get("content")
            {
                collect_yt_search_items(content, &mut out, &mut seen);
            }
        }
    }
    for item in &mut out {
        fill_channel_defaults_on_result(item, channel_title, channel_id, &canonical_url, verified);
    }
    out.truncate(effective_limit);
    Ok(out)
}

pub(super) fn clean_channel_title(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    for suffix in [
        " - Videos",
        " - Vídeos",
        " - Playlists",
        " - About",
        " - Sobre",
    ] {
        if let Some(stripped) = trimmed.strip_suffix(suffix) {
            let stripped = stripped.trim();
            if !stripped.is_empty() {
                return Some(stripped.to_owned());
            }
        }
    }
    Some(trimmed.to_owned())
}

fn sanitize_channel_description(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(
            trimmed
                .chars()
                .take(MAX_CHANNEL_DESCRIPTION_CHARS)
                .collect(),
        )
    }
}

fn parse_channel_tags(value: &Value) -> Vec<String> {
    let Some(items) = value.get("tags").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut tags = Vec::new();
    for item in items {
        if tags.len() >= MAX_CHANNEL_TAGS {
            break;
        }
        let Some(raw) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        let sanitized: String = raw.chars().take(MAX_CHANNEL_TAG_CHARS).collect();
        if !sanitized.is_empty() && !tags.contains(&sanitized) {
            tags.push(sanitized);
        }
    }
    tags
}

fn select_channel_thumbnails(value: &Value) -> (Option<String>, Option<String>) {
    let Some(items) = value.get("thumbnails").and_then(Value::as_array) else {
        return (None, None);
    };

    let mut avatar_url: Option<String> = None;
    let mut banner_url: Option<String> = None;

    for item in items {
        let Some(url) = item
            .get("url")
            .and_then(Value::as_str)
            .and_then(valid_http_url)
        else {
            continue;
        };
        let id = item.get("id").and_then(Value::as_str).unwrap_or("");
        let width = item.get("width").and_then(Value::as_u64);
        let height = item.get("height").and_then(Value::as_u64);

        if id.contains("avatar") || (width.is_some() && width == height) {
            avatar_url = Some(url.to_owned());
        } else if id.contains("banner")
            || matches!((width, height), (Some(w), Some(h)) if h > 0 && w >= h * 2)
        {
            banner_url = Some(url.to_owned());
        }
    }

    (avatar_url, banner_url)
}

pub(super) fn parse_channel_page(
    output: &[u8],
    requested_target: &ChannelTarget,
    limit: usize,
) -> Result<ChannelPage, String> {
    let effective_limit = limit.clamp(1, MAX_CHANNEL_VIDEOS_LIMIT);
    if output.iter().all(u8::is_ascii_whitespace) {
        return Err("Resposta vazia do yt-dlp ao consultar o canal.".to_owned());
    }

    if let Ok(single_value) = serde_json::from_slice::<Value>(output) {
        if single_value.get("entries").is_some()
            || single_value
                .get("_type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "playlist")
        {
            return parse_channel_playlist_json(&single_value, requested_target, effective_limit);
        }
        return parse_channel_json_lines(&[output], requested_target, effective_limit);
    }

    let trimmed_lines: Vec<&[u8]> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
        .collect();
    if trimmed_lines.is_empty() {
        return Err("Resposta vazia do yt-dlp ao consultar o canal.".to_owned());
    }

    parse_channel_json_lines(&trimmed_lines, requested_target, effective_limit)
}

fn parse_channel_playlist_json(
    root: &Value,
    requested_target: &ChannelTarget,
    limit: usize,
) -> Result<ChannelPage, String> {
    let entries = root
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| "Resposta inválida do yt-dlp: lista de vídeos ausente.".to_owned())?;
    let channel_id = ["channel_id", "id", "playlist_channel_id", "playlist_id"]
        .into_iter()
        .filter_map(|key| nonempty_str(root, key))
        .find(|candidate| is_valid_channel_id(candidate))
        .map(str::to_owned)
        .or_else(|| match requested_target {
            ChannelTarget::ChannelId(id) => Some(id.clone()),
            ChannelTarget::Handle(_) => None,
        });

    let handle = ["uploader_id", "playlist_uploader_id"]
        .into_iter()
        .filter_map(|key| nonempty_str(root, key))
        .find(|candidate| is_valid_channel_handle(candidate))
        .map(str::to_owned)
        .or_else(|| {
            ["uploader_url", "channel_url", "webpage_url"]
                .into_iter()
                .filter_map(|key| nonempty_str(root, key))
                .find_map(|url| match parse_youtube_channel_url(url) {
                    Some(ChannelTarget::Handle(handle)) => Some(handle),
                    _ => None,
                })
        })
        .or_else(|| match requested_target {
            ChannelTarget::Handle(handle) => Some(handle.clone()),
            ChannelTarget::ChannelId(_) => None,
        });

    let resolved_target = if let Some(id) = &channel_id {
        ChannelTarget::ChannelId(id.clone())
    } else if let Some(handle) = &handle {
        ChannelTarget::Handle(handle.clone())
    } else {
        requested_target.clone()
    };
    let canonical_url = resolved_target.canonical_url();

    let title = [
        "channel",
        "uploader",
        "playlist_channel",
        "playlist_uploader",
    ]
    .into_iter()
    .filter_map(|key| nonempty_str(root, key))
    .next()
    .map(str::to_owned)
    .or_else(|| nonempty_str(root, "title").and_then(clean_channel_title));

    let description = sanitize_channel_description(nonempty_str(root, "description"));
    let subscriber_count = root.get("channel_follower_count").and_then(Value::as_u64);
    let view_count = root.get("view_count").and_then(Value::as_u64);
    let verified = root.get("channel_is_verified").and_then(Value::as_bool);
    let (avatar_url, banner_url) = select_channel_thumbnails(root);
    let tags = parse_channel_tags(root);

    let mut videos = Vec::new();
    {
        let mut flat_entries: Vec<&Value> = Vec::new();
        for entry in entries {
            if let Some(nested) = entry.get("entries").and_then(Value::as_array) {
                flat_entries.extend(nested.iter());
            } else {
                flat_entries.push(entry);
            }
        }
        for entry in flat_entries {
            if videos.len() >= limit {
                break;
            }
            if let Some(video) = parse_video_entry(
                entry,
                true,
                title.as_deref(),
                channel_id.as_deref(),
                Some(&canonical_url),
            ) && !is_playlist_search_result(&video)
            {
                videos.push(video);
            }
        }
    }
    let has_videos_tab = !videos.is_empty();

    Ok(ChannelPage {
        target: resolved_target,
        channel_id,
        title,
        handle,
        canonical_url,
        description,
        subscriber_count,
        view_count,
        avatar_url,
        banner_url,
        verified,
        tags,
        has_videos_tab,
        videos,
        ..Default::default()
    })
}

fn parse_channel_json_lines(
    lines: &[&[u8]],
    requested_target: &ChannelTarget,
    limit: usize,
) -> Result<ChannelPage, String> {
    let mut parsed_values = Vec::with_capacity(lines.len().min(limit));
    for line in lines {
        let value: Value = serde_json::from_slice(line)
            .map_err(|error| format!("Resposta inválida do yt-dlp: {error}"))?;
        parsed_values.push(value);
    }

    let first = &parsed_values[0];
    let channel_id = ["playlist_channel_id", "playlist_id"]
        .into_iter()
        .filter_map(|key| nonempty_str(first, key))
        .find(|candidate| is_valid_channel_id(candidate))
        .map(str::to_owned)
        .or_else(|| match requested_target {
            ChannelTarget::ChannelId(id) => Some(id.clone()),
            ChannelTarget::Handle(_) => None,
        });

    let handle = ["playlist_uploader_id"]
        .into_iter()
        .filter_map(|key| nonempty_str(first, key))
        .find(|candidate| is_valid_channel_handle(candidate))
        .map(str::to_owned)
        .or_else(|| match requested_target {
            ChannelTarget::Handle(handle) => Some(handle.clone()),
            ChannelTarget::ChannelId(_) => None,
        });

    let resolved_target = if let Some(id) = &channel_id {
        ChannelTarget::ChannelId(id.clone())
    } else if let Some(handle) = &handle {
        ChannelTarget::Handle(handle.clone())
    } else {
        requested_target.clone()
    };
    let canonical_url = resolved_target.canonical_url();

    let title = ["playlist_channel", "playlist_uploader"]
        .into_iter()
        .filter_map(|key| nonempty_str(first, key))
        .next()
        .map(str::to_owned)
        .or_else(|| nonempty_str(first, "playlist_title").and_then(clean_channel_title))
        .or_else(|| nonempty_str(first, "playlist").and_then(clean_channel_title));

    let mut videos = Vec::new();
    for value in &parsed_values {
        if videos.len() >= limit {
            break;
        }
        if let Some(video) = parse_video_entry(
            value,
            true,
            title.as_deref(),
            channel_id.as_deref(),
            Some(&canonical_url),
        ) && !is_playlist_search_result(&video)
        {
            videos.push(video);
        }
    }
    let has_videos_tab = !videos.is_empty();

    Ok(ChannelPage {
        target: resolved_target,
        channel_id,
        title,
        handle,
        canonical_url,
        description: None,
        subscriber_count: None,
        view_count: None,
        avatar_url: None,
        banner_url: None,
        verified: None,
        tags: Vec::new(),
        has_videos_tab,
        videos,
        ..Default::default()
    })
}

pub fn update_cached_channel_videos(
    target: &ChannelTarget,
    _effective_limit: usize,
    videos: &[SearchResult],
) {
    let Some(cache) = CHANNEL_PAGE_CACHE.get() else {
        return;
    };
    let canonical = target.canonical_url();
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if let Some(entry) = guard.get_mut(&canonical, now) {
        entry.videos = videos.to_vec();
    }
}

pub fn fetch_channel_videos_batch_with_cancel(
    input: &str,
    start: usize,
    count: usize,
    cancellation: &Cancellation,
) -> Result<ChannelVideosBatch, String> {
    let target = parse_channel_target(input).ok_or_else(|| {
        "Identificador ou URL de canal do YouTube inválido ou não suportado.".to_owned()
    })?;
    if start == 0 || start > MAX_CHANNEL_VIDEOS_LIMIT {
        return Err(format!(
            "Limite máximo de {MAX_CHANNEL_VIDEOS_LIMIT} vídeos por canal atingido."
        ));
    }
    let effective_count = count.clamp(1, CHANNEL_VIDEOS_PAGE_STEP.max(MAX_CHANNEL_VIDEOS_LIMIT));
    let end = start
        .saturating_add(effective_count.saturating_sub(1))
        .min(MAX_CHANNEL_VIDEOS_LIMIT);
    let expected_slice_len = end.saturating_sub(start).saturating_add(1);
    let start_arg = start.to_string();
    let end_arg = end.to_string();
    let videos_url = target.videos_tab_url();
    let output = process::output(
        yt_dlp().args([
            "--flat-playlist",
            "--extractor-args",
            "youtubetab:approximate_date",
            "--playlist-start",
            &start_arg,
            "--playlist-end",
            &end_arg,
            "--dump-single-json",
            "--skip-download",
            "--no-warnings",
            "--",
            &videos_url,
        ]),
        cancellation,
        QUERY_TIMEOUT,
    )
    .map_err(|error| {
        format!("Não foi possível executar yt-dlp: {error}. Verifique se ele está ao lado do app ou no PATH.")
    })?;
    if !output.status.success() {
        return Err(format!(
            "Não foi possível carregar mais vídeos do canal: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_channel_videos_batch(&output.stdout, &target, start, end, expected_slice_len)
}

pub(super) fn parse_channel_videos_batch(
    output: &[u8],
    requested_target: &ChannelTarget,
    start: usize,
    end: usize,
    expected_slice_len: usize,
) -> Result<ChannelVideosBatch, String> {
    let page = parse_channel_page(output, requested_target, expected_slice_len)?;
    let reached_max_limit = end >= MAX_CHANNEL_VIDEOS_LIMIT;
    let has_more = !reached_max_limit && page.videos.len() >= expected_slice_len;
    Ok(ChannelVideosBatch {
        videos: page.videos,
        start,
        end,
        has_more,
        reached_max_limit,
    })
}

fn parse_channel_playlist_entry(value: &Value) -> Option<ChannelPlaylist> {
    let id = extract_playlist_id(value)?;
    let url = format!("https://www.youtube.com/playlist?list={id}");
    if !is_playlist_url(&url) {
        return None;
    }
    let title: String = nonempty_str(value, "title")
        .unwrap_or(&id)
        .chars()
        .take(512)
        .collect();
    let thumbnail = select_video_thumbnail(value);
    let item_count = value
        .get("playlist_count")
        .or_else(|| value.get("n_entries"))
        .or_else(|| value.get("entry_count"))
        .and_then(Value::as_u64);
    let description = sanitize_channel_description(nonempty_str(value, "description"));
    Some(ChannelPlaylist {
        id,
        title,
        url,
        thumbnail,
        item_count,
        description,
        privacy: crate::playlist_sync::PlaylistPrivacy::Unknown,
    })
}

fn fetch_channel_playlists_html(
    playlists_url: &str,
    effective_limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<ChannelPlaylist>, String> {
    let data = fetch_youtube_initial_data_url(playlists_url, cancellation)?;
    let mut raw_items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(tabs) = data
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs")
        .and_then(Value::as_array)
    {
        for tab_entry in tabs {
            let Some(tr) = tab_entry.get("tabRenderer") else {
                continue;
            };
            if tr.get("selected").and_then(Value::as_bool) != Some(true)
                && tr.get("content").is_none()
            {
                continue;
            }
            if let Some(content) = tr.get("content") {
                collect_yt_search_items(content, &mut raw_items, &mut seen);
            }
        }
    }
    let mut playlists = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    for item in raw_items {
        if playlists.len() >= effective_limit {
            break;
        }
        if item.metadata.kind != SearchResultKind::Playlist {
            continue;
        }
        let Some(id) = item
            .video
            .url
            .strip_prefix("https://www.youtube.com/playlist?list=")
            .and_then(|rest| rest.split(['&', '#', '/']).next())
            .filter(|id| is_valid_playlist_id(id))
            .map(str::to_owned)
        else {
            continue;
        };
        if !seen_ids.insert(id.clone()) {
            continue;
        }
        let item_count = item
            .metadata
            .playlist_count
            .as_deref()
            .and_then(parse_pt_br_compact_count);
        playlists.push(ChannelPlaylist {
            id,
            title: item.video.title,
            url: item.video.url,
            thumbnail: item.video.thumbnail,
            item_count,
            description: item.metadata.description,
            privacy: playlist_privacy_from_badges(&item.metadata.badges),
        });
    }
    Ok(playlists)
}

pub fn fetch_channel_playlists_innertube(
    input: &str,
    browse_id: Option<&str>,
    tab_params: Option<&str>,
    limit: usize,
    language: crate::i18n::UiLanguage,
    session: Option<&crate::innertube::YouTubeWebSession>,
    cancellation: &Cancellation,
) -> Result<Vec<ChannelPlaylist>, String> {
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let target = parse_channel_target(input).ok_or_else(|| {
        "Identificador ou URL de canal do YouTube inválido ou não suportado.".to_owned()
    })?;
    let effective_limit = limit.clamp(1, MAX_CHANNEL_PLAYLISTS_LIMIT);

    let mut resolved_browse_id = browse_id
        .map(str::trim)
        .filter(|id| is_valid_channel_id(id))
        .map(str::to_owned)
        .or_else(|| match &target {
            ChannelTarget::ChannelId(id) => Some(id.clone()),
            ChannelTarget::Handle(_) => {
                let guard = channel_page_cache()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                guard
                    .resolve_key(input)
                    .or_else(|| guard.resolve_key(&target.canonical_url()))
                    .map(|k| k.as_str().to_owned())
            }
        });

    if resolved_browse_id.is_none()
        && session.is_some()
        && let Ok(data) = fetch_youtube_initial_data_url(&target.playlists_tab_url(), cancellation)
    {
        resolved_browse_id = data
            .pointer("/metadata/channelMetadataRenderer/externalId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| is_valid_channel_id(id))
            .map(str::to_owned);
    }

    if let Some(cid) = resolved_browse_id.as_deref() {
        let ctx = crate::innertube::InnerTubeContext::from_language(language, None);
        let mut owned_session = session.cloned();
        let initial_params = tab_params.map(str::trim).filter(|p| !p.is_empty());
        if let Ok(mut it_page) = crate::innertube::fetch_channel_browse(
            Some(cid),
            initial_params,
            None,
            &ctx,
            owned_session.as_mut(),
            cancellation,
        ) {
            if initial_params.is_none()
                && let Some(pl_tab) = it_page
                    .tabs
                    .iter()
                    .find(|t| t.kind == crate::innertube::ChannelTabKind::Playlists && !t.selected)
                    .cloned()
                && let Ok(pl_page) = crate::innertube::fetch_channel_browse(
                    Some(&pl_tab.browse_id),
                    pl_tab.params.as_deref(),
                    None,
                    &ctx,
                    owned_session.as_mut(),
                    cancellation,
                )
            {
                it_page = pl_page;
            }

            let mut playlists = Vec::new();
            let mut seen_ids = std::collections::HashSet::new();
            let collect_page =
                |page: crate::innertube::InnerTubeChannelPage,
                 dest: &mut Vec<ChannelPlaylist>,
                 seen: &mut std::collections::HashSet<String>| {
                    for (video, metadata) in page.items.into_iter().zip(page.metadata) {
                        let res = SearchResult { video, metadata };
                        if let Some(pl) = channel_playlist_from_search_result(&res)
                            && seen.insert(pl.id.clone())
                        {
                            dest.push(pl);
                            if dest.len() >= effective_limit {
                                break;
                            }
                        }
                    }
                };
            let mut next_cont = it_page.continuation.clone();
            collect_page(it_page, &mut playlists, &mut seen_ids);

            let mut seen_tokens = std::collections::HashSet::new();
            for _ in 0..4 {
                if cancellation.cancelled() || playlists.len() >= effective_limit {
                    break;
                }
                let Some(token) = next_cont
                    .take()
                    .filter(|t| !t.trim().is_empty() && seen_tokens.insert(t.clone()))
                else {
                    break;
                };
                match crate::innertube::fetch_channel_browse(
                    None,
                    None,
                    Some(&token),
                    &ctx,
                    owned_session.as_mut(),
                    cancellation,
                ) {
                    Ok(cont_page) => {
                        next_cont = cont_page.continuation.clone();
                        collect_page(cont_page, &mut playlists, &mut seen_ids);
                    }
                    Err(_) => break,
                }
            }
            if cancellation.cancelled() {
                return Err("Operação cancelada.".to_owned());
            }
            if !playlists.is_empty() || session.is_some() {
                return Ok(playlists);
            }
        }
        if cancellation.cancelled() {
            return Err("Operação cancelada.".to_owned());
        }
    }

    fetch_channel_playlists_with_cancel(input, effective_limit, cancellation)
}

pub fn fetch_channel_playlists_with_cancel(
    input: &str,
    limit: usize,
    cancellation: &Cancellation,
) -> Result<Vec<ChannelPlaylist>, String> {
    let target = parse_channel_target(input).ok_or_else(|| {
        "Identificador ou URL de canal do YouTube inválido ou não suportado.".to_owned()
    })?;
    let effective_limit = limit.clamp(1, MAX_CHANNEL_PLAYLISTS_LIMIT);
    let playlists_url = target.playlists_tab_url();
    if let Ok(playlists) =
        fetch_channel_playlists_html(&playlists_url, effective_limit, cancellation)
        && !playlists.is_empty()
    {
        return Ok(playlists);
    }
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let limit_arg = effective_limit.to_string();
    let output = process::output(
        yt_dlp().args([
            "--flat-playlist",
            "--playlist-end",
            &limit_arg,
            "--dump-single-json",
            "--skip-download",
            "--no-warnings",
            "--",
            &playlists_url,
        ]),
        cancellation,
        QUERY_TIMEOUT,
    )
    .map_err(|error| {
        format!("Não foi possível executar yt-dlp: {error}. Verifique se ele está ao lado do app ou no PATH.")
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let trimmed = stderr.trim();
        if trimmed.contains("This channel does not have a")
            || trimmed.contains("playlists tab")
            || trimmed.contains("does not have any playlists")
        {
            return Ok(Vec::new());
        }
        return Err(format!(
            "Não foi possível consultar as playlists públicas do canal: {trimmed}"
        ));
    }
    parse_channel_playlists(&output.stdout, effective_limit)
}

pub(super) fn parse_channel_playlists(
    output: &[u8],
    limit: usize,
) -> Result<Vec<ChannelPlaylist>, String> {
    let effective_limit = limit.clamp(1, MAX_CHANNEL_PLAYLISTS_LIMIT);
    if output.iter().all(u8::is_ascii_whitespace) {
        return Ok(Vec::new());
    }

    let mut playlists = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    if let Ok(single_value) = serde_json::from_slice::<Value>(output) {
        if let Some(entries) = single_value.get("entries").and_then(Value::as_array) {
            let mut flat_entries: Vec<&Value> = Vec::new();
            for entry in entries {
                if let Some(nested) = entry.get("entries").and_then(Value::as_array) {
                    flat_entries.extend(nested.iter());
                } else {
                    flat_entries.push(entry);
                }
            }
            for entry in flat_entries {
                if playlists.len() >= effective_limit {
                    break;
                }
                if let Some(playlist) = parse_channel_playlist_entry(entry)
                    && seen_ids.insert(playlist.id.clone())
                {
                    playlists.push(playlist);
                }
            }
            return Ok(playlists);
        }
        if let Some(playlist) = parse_channel_playlist_entry(&single_value) {
            playlists.push(playlist);
        }
        return Ok(playlists);
    }

    for line in output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
    {
        if playlists.len() >= effective_limit {
            break;
        }
        let value: Value = serde_json::from_slice(line)
            .map_err(|error| format!("Resposta inválida do yt-dlp: {error}"))?;
        if let Some(playlist) = parse_channel_playlist_entry(&value)
            && seen_ids.insert(playlist.id.clone())
        {
            playlists.push(playlist);
        }
    }
    Ok(playlists)
}

fn fetch_channel_about_html(
    target: &ChannelTarget,
    cancellation: &Cancellation,
) -> Result<ChannelAboutInfo, String> {
    let featured_url = target.featured_tab_url();
    let data = fetch_youtube_initial_data_url(&featured_url, cancellation)?;
    let mut page = parse_channel_initial_data(&data, None, target, 1)?;
    if let Some(token) = page.about_continuation.clone()
        && let Ok(about_resp) = fetch_youtube_innertube_browse(&token, cancellation)
    {
        apply_about_continuation_data(&mut page, &about_resp);
    }
    let tags = data
        .pointer("/microformat/microformatDataRenderer/tags")
        .and_then(Value::as_array)
        .map(|items| {
            let mut out = Vec::new();
            for item in items.iter().take(MAX_CHANNEL_TAGS) {
                if let Some(raw) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    let s: String = raw.chars().take(MAX_CHANNEL_TAG_CHARS).collect();
                    if !s.is_empty() && !out.contains(&s) {
                        out.push(s);
                    }
                }
            }
            out
        })
        .unwrap_or_default();
    Ok(ChannelAboutInfo {
        channel_id: page.channel_id,
        title: page.title,
        handle: page.handle,
        canonical_url: Some(page.canonical_url),
        description: page.description,
        subscriber_count: page.subscriber_count,
        view_count: page.view_count,
        subscriber_count_text: page.subscriber_count_text,
        video_count_text: page.video_count_text,
        view_count_text: page.view_count_text,
        country: page.country,
        joined_date_text: page.joined_date_text,
        links: page.links,
        avatar_url: page.avatar_url,
        banner_url: page.banner_url,
        verified: page.verified,
        tags,
    })
}

pub fn fetch_channel_about_with_cancel(
    input: &str,
    cancellation: &Cancellation,
) -> Result<ChannelAboutInfo, String> {
    let target = parse_channel_target(input).ok_or_else(|| {
        "Identificador ou URL de canal do YouTube inválido ou não suportado.".to_owned()
    })?;
    if let Ok(about) = fetch_channel_about_html(&target, cancellation) {
        return Ok(about);
    }
    if cancellation.cancelled() {
        return Err("Operação cancelada.".to_owned());
    }
    let about_url = target.about_tab_url();
    let output = process::output(
        yt_dlp().args([
            "--flat-playlist",
            "--playlist-items",
            "0",
            "--dump-single-json",
            "--skip-download",
            "--no-warnings",
            "--",
            &about_url,
        ]),
        cancellation,
        QUERY_TIMEOUT,
    )
    .map_err(|error| {
        format!("Não foi possível executar yt-dlp: {error}. Verifique se ele está ao lado do app ou no PATH.")
    })?;
    if !output.status.success() {
        return Err(format!(
            "Não foi possível consultar as informações do canal: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_channel_about(&output.stdout, &target)
}

pub(super) fn parse_channel_about(
    output: &[u8],
    requested_target: &ChannelTarget,
) -> Result<ChannelAboutInfo, String> {
    if output.iter().all(u8::is_ascii_whitespace) {
        return Err("Resposta vazia do yt-dlp ao consultar informações do canal.".to_owned());
    }
    let root: Value = serde_json::from_slice(output)
        .map_err(|error| format!("Resposta inválida do yt-dlp: {error}"))?;

    let channel_id = ["channel_id", "id", "playlist_channel_id", "playlist_id"]
        .into_iter()
        .filter_map(|key| nonempty_str(&root, key))
        .find(|candidate| is_valid_channel_id(candidate))
        .map(str::to_owned)
        .or_else(|| match requested_target {
            ChannelTarget::ChannelId(id) => Some(id.clone()),
            ChannelTarget::Handle(_) => None,
        });

    let handle = ["uploader_id", "playlist_uploader_id"]
        .into_iter()
        .filter_map(|key| nonempty_str(&root, key))
        .find(|candidate| is_valid_channel_handle(candidate))
        .map(str::to_owned)
        .or_else(|| {
            ["uploader_url", "channel_url", "webpage_url"]
                .into_iter()
                .filter_map(|key| nonempty_str(&root, key))
                .find_map(|url| match parse_youtube_channel_url(url) {
                    Some(ChannelTarget::Handle(handle)) => Some(handle),
                    _ => None,
                })
        })
        .or_else(|| match requested_target {
            ChannelTarget::Handle(handle) => Some(handle.clone()),
            ChannelTarget::ChannelId(_) => None,
        });

    let resolved_target = if let Some(id) = &channel_id {
        ChannelTarget::ChannelId(id.clone())
    } else if let Some(handle) = &handle {
        ChannelTarget::Handle(handle.clone())
    } else {
        requested_target.clone()
    };
    let canonical_url = Some(resolved_target.canonical_url());

    let title = [
        "channel",
        "uploader",
        "playlist_channel",
        "playlist_uploader",
    ]
    .into_iter()
    .filter_map(|key| nonempty_str(&root, key))
    .next()
    .map(str::to_owned)
    .or_else(|| nonempty_str(&root, "title").and_then(clean_channel_title));

    let description = sanitize_channel_description(nonempty_str(&root, "description"));
    let subscriber_count = root.get("channel_follower_count").and_then(Value::as_u64);
    let view_count = root.get("view_count").and_then(Value::as_u64);
    let verified = root.get("channel_is_verified").and_then(Value::as_bool);
    let (avatar_url, banner_url) = select_channel_thumbnails(&root);
    let tags = parse_channel_tags(&root);

    Ok(ChannelAboutInfo {
        channel_id,
        title,
        handle,
        canonical_url,
        description,
        subscriber_count,
        view_count,
        avatar_url,
        banner_url,
        verified,
        tags,
        ..Default::default()
    })
}

pub(super) fn clear_channel_page_cache() {
    if let Some(lock) = CHANNEL_PAGE_CACHE.get() {
        let mut cache = lock.lock().unwrap_or_else(|error| error.into_inner());
        cache.clear();
    }
}
