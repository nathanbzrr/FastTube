use super::channel::clean_channel_title;
use super::filters::{
    SearchFilters, SearchSort, SearchTypeFilter, apply_search_filters, encode_search_sp,
};
use super::{
    BROWSER_FEED_TIMEOUT, ChannelTarget, HomeFeed, QUERY_TIMEOUT, SUBTITLE_USER_AGENT,
    SearchResult, SearchResultKind, Video, VideoMetadata, channel_url_from_browse_endpoint,
    clean_pt_br_view_label, clean_published_time_text, extract_yt_initial_data,
    has_verified_owner_badge, is_valid_channel_handle, is_valid_channel_id, nonempty_str,
    parse_lockup_metadata_rows, parse_pt_br_compact_count, parse_video_entry,
    percent_encode_query_component, select_yt_thumbnail, yt_dlp, yt_text,
};
use crate::process::{self, Cancellation};
use serde_json::Value;
use std::io::Read;
use std::time::Duration;

pub(super) fn is_playlist_url(url: &str) -> bool {
    url.strip_prefix("https://www.youtube.com/playlist?list=")
        .is_some_and(|id| {
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
}

pub fn session_feed(
    cookies: &[Value],
    target: &str,
    cancel: &Cancellation,
) -> Result<HomeFeed, String> {
    use std::io::Write;
    if target != ":ytrec" && !is_playlist_url(target) {
        return Err("Endereço de playlist inválido.".into());
    }
    let profile = crate::session::profile_dir()?;
    let path = profile.join(format!(".fasttube-cookies-{}.txt", std::process::id()));
    struct TemporaryCookies(std::path::PathBuf);
    impl Drop for TemporaryCookies {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| "Não foi possível preparar a sessão do YouTube.")?;
    let _cleanup = TemporaryCookies(path.clone());
    writeln!(file, "# Netscape HTTP Cookie File")
        .map_err(|_| "Não foi possível preparar a sessão.")?;
    for cookie in cookies {
        let domain = cookie.get("domain").and_then(Value::as_str).unwrap_or("");
        let name = cookie.get("name").and_then(Value::as_str).unwrap_or("");
        let value = cookie.get("value").and_then(Value::as_str).unwrap_or("");
        let cookie_path = cookie.get("path").and_then(Value::as_str).unwrap_or("/");
        if !["youtube.com", ".youtube.com", "www.youtube.com"].contains(&domain)
            || name.is_empty()
            || [domain, name, value, cookie_path]
                .iter()
                .any(|s| s.contains(['\t', '\r', '\n']))
        {
            continue;
        }
        let expiry = cookie
            .get("expires")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .max(0.0) as u64;
        let secure = if cookie.get("secure").and_then(Value::as_bool) == Some(true) {
            "TRUE"
        } else {
            "FALSE"
        };
        let prefix = if cookie.get("httpOnly").and_then(Value::as_bool) == Some(true) {
            "#HttpOnly_"
        } else {
            ""
        };
        writeln!(
            file,
            "{prefix}{domain}\t{}\t{cookie_path}\t{secure}\t{expiry}\t{name}\t{value}",
            if domain.starts_with('.') {
                "TRUE"
            } else {
                "FALSE"
            }
        )
        .map_err(|_| "Não foi possível preparar a sessão.")?;
    }
    drop(file);
    let mut cmd = yt_dlp();
    cmd.args(["--ignore-config", "--cookies"]).arg(&path).args([
        "--flat-playlist",
        "--extractor-args",
        "youtubetab:approximate_date",
        "--playlist-end",
        "50",
        "--dump-json",
        "--no-warnings",
        "--",
        target,
    ]);
    let output = process::output(&mut cmd, cancel, BROWSER_FEED_TIMEOUT)
        .map_err(|e| format!("Não foi possível consultar o YouTube: {e}"))?;
    if !output.status.success() {
        return Err("Não foi possível carregar o YouTube. Sua sessão pode ter expirado.".into());
    }
    parse_home_feed(&output.stdout)
}

pub(super) fn parse_home_feed(output: &[u8]) -> Result<HomeFeed, String> {
    let mut feed = HomeFeed {
        items: Vec::new(),
        playlists: Vec::new(),
        channels: Vec::new(),
    };
    for line in output
        .split(|b| *b == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
    {
        let value: Value =
            serde_json::from_slice(line).map_err(|_| "Resposta inválida do YouTube.".to_owned())?;
        let playlist = value
            .get("ie_key")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case("YoutubeTab"));
        if playlist {
            let id = value.get("id").and_then(Value::as_str).unwrap_or("");
            let url = format!("https://www.youtube.com/playlist?list={id}");
            if !is_playlist_url(&url) {
                continue;
            }
            let mut entry = match parse_video_entry(&value, false, None, None, None) {
                Some(v) => v,
                None => continue,
            };
            entry.video.url = url.clone();
            entry.video.duration.clear();
            feed.playlists.push(url);
            feed.items.push(entry);
        } else if value
            .get("ie_key")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case("Youtube"))
        {
            let fallback_name = nonempty_str(&value, "playlist_channel")
                .or_else(|| nonempty_str(&value, "playlist_uploader"))
                .or_else(|| nonempty_str(&value, "playlist_title"))
                .and_then(clean_channel_title);
            let fallback_id = nonempty_str(&value, "playlist_channel_id")
                .or_else(|| nonempty_str(&value, "playlist_id"))
                .filter(|candidate| is_valid_channel_id(candidate));
            let fallback_url = fallback_id
                .map(|id| ChannelTarget::ChannelId(id.to_owned()).canonical_url())
                .or_else(|| {
                    nonempty_str(&value, "playlist_uploader_id")
                        .filter(|handle| is_valid_channel_handle(handle))
                        .map(|handle| ChannelTarget::Handle(handle.to_owned()).canonical_url())
                });
            if let Some(entry) = parse_video_entry(
                &value,
                true,
                fallback_name.as_deref(),
                fallback_id,
                fallback_url.as_deref(),
            ) && crate::history::youtube_video_id(&entry.video.url).is_some()
            {
                feed.items.push(entry);
            }
        }
    }
    Ok(feed)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HomeBatchRequest {
    pub video_queries: Vec<String>,
    pub channel_targets: Vec<ChannelTarget>,
    pub playlist_query: Option<String>,
    pub shorts_query: Option<String>,
}

pub fn build_playlist_search_url(query: &str) -> String {
    let trimmed = query.trim();
    let encoded = percent_encode_query_component(if trimmed.is_empty() {
        "mix musica playlist brasil"
    } else {
        trimmed
    });
    format!("https://www.youtube.com/results?search_query={encoded}&sp=EgIQAw%3D%3D")
}

fn fetch_home_lane(
    target: &str,
    playlist_end: Option<&str>,
    cancellation: &Cancellation,
) -> Result<HomeFeed, String> {
    if cancellation.cancelled() {
        return Err("Consulta cancelada.".into());
    }
    let mut cmd = yt_dlp();
    cmd.args([
        "--flat-playlist",
        "--extractor-args",
        "youtubetab:approximate_date",
    ]);
    if let Some(end) = playlist_end {
        cmd.args(["--playlist-end", end]);
    }
    cmd.args(["--dump-json", "--no-warnings", "--", target]);
    let output = process::output(&mut cmd, cancellation, QUERY_TIMEOUT)
        .map_err(|error| format!("Não foi possível executar yt-dlp: {error}."))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            "Não foi possível carregar o feed do YouTube.".into()
        } else {
            format!("Não foi possível carregar o feed: {stderr}")
        });
    }
    parse_home_feed(&output.stdout)
}

pub(super) fn merge_home_feeds(feeds: Vec<HomeFeed>) -> HomeFeed {
    let mut merged = HomeFeed {
        items: Vec::new(),
        playlists: Vec::new(),
        channels: Vec::new(),
    };
    let mut seen_keys = std::collections::HashSet::new();
    let mut seen_playlists = std::collections::HashSet::new();
    for feed in feeds {
        for playlist in feed.playlists {
            if seen_playlists.insert(playlist.clone()) {
                merged.playlists.push(playlist);
            }
        }
        for item in feed.items {
            let key = crate::history::youtube_video_id(&item.video.url)
                .unwrap_or_else(|| item.video.url.clone());
            if seen_keys.insert(key) {
                merged.items.push(item);
            }
        }
    }
    merged
}

pub fn fetch_home_batch(
    request: &HomeBatchRequest,
    cancellation: &Cancellation,
) -> Result<HomeFeed, String> {
    if cancellation.cancelled() {
        return Err("Consulta cancelada.".into());
    }

    let mut targets: Vec<(String, Option<&'static str>)> = Vec::new();
    for query in &request.video_queries {
        let trimmed = query.trim();
        if !trimmed.is_empty() {
            targets.push((format!("ytsearch25:{trimmed}"), None));
        }
    }
    for target in &request.channel_targets {
        targets.push((target.videos_tab_url(), Some("10")));
    }
    if let Some(playlist_query) = request
        .playlist_query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
    {
        targets.push((build_playlist_search_url(playlist_query), Some("12")));
    }
    if let Some(shorts_query) = request
        .shorts_query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
    {
        targets.push((format!("ytsearch18:{shorts_query}"), None));
    }
    if targets.is_empty() {
        return Ok(HomeFeed {
            items: Vec::new(),
            playlists: Vec::new(),
            channels: Vec::new(),
        });
    }

    let results: Vec<Result<HomeFeed, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = targets
            .iter()
            .map(|(target, playlist_end)| {
                let target = target.as_str();
                let playlist_end = *playlist_end;
                scope.spawn(move || fetch_home_lane(target, playlist_end, cancellation))
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err("Falha interna ao carregar o feed.".into()))
            })
            .collect()
    });

    let mut feeds = Vec::new();
    let mut first_error = None;
    for result in results {
        match result {
            Ok(feed) => feeds.push(feed),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }

    if cancellation.cancelled() {
        return Err("Consulta cancelada.".into());
    }

    let merged = merge_home_feeds(feeds);
    if merged.items.is_empty()
        && let Some(error) = first_error
    {
        return Err(error);
    }
    Ok(merged)
}

#[cfg(test)]
pub fn find(query: &str) -> Result<Vec<Video>, String> {
    find_with_metadata(query)
        .map(|results| results.into_iter().map(|result| result.video).collect())
}

#[cfg(test)]
pub fn find_with_metadata(query: &str) -> Result<Vec<SearchResult>, String> {
    find_with_cancel(query, &Cancellation::default())
}

#[cfg(test)]
pub fn find_with_cancel(
    query: &str,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    find_filtered_with_cancel(query, SearchFilters::default(), cancellation)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub struct SearchPageOutput {
    pub videos: Vec<Video>,
    pub metadata: Vec<VideoMetadata>,
    pub continuation: Option<String>,
    pub spelling: Option<crate::innertube::SearchSpellingCorrection>,
    pub estimated_results: Option<u64>,
}

#[allow(dead_code)]
impl SearchPageOutput {
    pub fn into_search_results(self) -> Vec<SearchResult> {
        self.videos
            .into_iter()
            .zip(self.metadata)
            .map(|(video, metadata)| SearchResult { video, metadata })
            .collect()
    }
}

/// Fetches a page of search results via InnerTube `youtubei/v1/search` with pagination,
/// multi-select filters, and spelling correction, gracefully falling back to `yt-dlp`
/// on page 0 when InnerTube fails.
#[allow(dead_code)]
pub fn search_videos_innertube(
    query: &str,
    filters: &SearchFilters,
    continuation: Option<&str>,
    language: crate::i18n::UiLanguage,
    country: Option<&str>,
    session: Option<&crate::innertube::YouTubeWebSession>,
    cancel: &Cancellation,
) -> Result<SearchPageOutput, String> {
    if cancel.cancelled() {
        return Err("Pesquisa cancelada.".to_owned());
    }
    let has_continuation = continuation.is_some_and(|t| !t.trim().is_empty());
    let is_url = query.starts_with("https://") || query.starts_with("http://");
    if !has_continuation && is_url {
        let results = find_filtered_with_cancel_yt_dlp(query, filters, cancel)?;
        let (videos, metadata): (Vec<Video>, Vec<VideoMetadata>) =
            results.into_iter().map(|r| (r.video, r.metadata)).unzip();
        return Ok(SearchPageOutput {
            videos,
            metadata,
            continuation: None,
            spelling: None,
            estimated_results: None,
        });
    }

    let ctx = crate::innertube::InnerTubeContext::from_language(language, country);
    let it_filters = filters.to_innertube();
    let mut owned_session = session.cloned();

    match crate::innertube::fetch_search_page(
        query,
        &it_filters,
        continuation,
        &ctx,
        owned_session.as_mut(),
        cancel,
    ) {
        Ok(page) => Ok(SearchPageOutput {
            videos: page.items,
            metadata: page.metadata,
            continuation: page.continuation,
            spelling: page.spelling,
            estimated_results: page.estimated_results,
        }),
        Err(err) => {
            if err.is_cancelled() || cancel.cancelled() {
                return Err("Pesquisa cancelada.".to_owned());
            }
            if has_continuation {
                return Err(err.into_message());
            }
            let fallback = find_filtered_with_cancel_yt_dlp(query, filters, cancel)?;
            let (videos, metadata): (Vec<Video>, Vec<VideoMetadata>) =
                fallback.into_iter().map(|r| (r.video, r.metadata)).unzip();
            Ok(SearchPageOutput {
                videos,
                metadata,
                continuation: None,
                spelling: None,
                estimated_results: None,
            })
        }
    }
}

#[allow(dead_code)]
pub fn find_filtered_with_cancel<F: std::borrow::Borrow<SearchFilters>>(
    query: &str,
    filters: F,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let filters = filters.borrow();
    let is_url = query.starts_with("https://") || query.starts_with("http://");
    if !is_url && filters.kind != SearchTypeFilter::Shorts {
        let ctx = crate::innertube::InnerTubeContext::default();
        let it_filters = filters.to_innertube();
        if let Ok(page) =
            crate::innertube::fetch_search_page(query, &it_filters, None, &ctx, None, cancellation)
            && !page.items.is_empty()
        {
            return Ok(page
                .items
                .into_iter()
                .zip(page.metadata)
                .map(|(video, metadata)| SearchResult { video, metadata })
                .collect());
        }
        if cancellation.cancelled() {
            return Err("Pesquisa cancelada.".to_owned());
        }
    }
    if !is_url
        && let Ok(html_results) = fetch_youtube_search_html(query, filters, cancellation)
        && !html_results.is_empty()
    {
        return Ok(html_results);
    }
    if cancellation.cancelled() {
        return Err("Pesquisa cancelada.".to_owned());
    }
    find_filtered_with_cancel_yt_dlp(query, filters, cancellation)
}

fn find_filtered_with_cancel_yt_dlp(
    query: &str,
    filters: &SearchFilters,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    let is_url = query.starts_with("https://") || query.starts_with("http://");
    let target = if is_url {
        query.to_owned()
    } else if filters.sort == SearchSort::Date {
        format!("ytsearchdate20:{query}")
    } else if !filters.is_default() {
        format!("ytsearch30:{query}")
    } else {
        format!("ytsearch20:{query}")
    };
    let mut command = yt_dlp();
    command.args([
        "--flat-playlist",
        "--extractor-args",
        "youtubetab:approximate_date",
        "--dump-json",
        "--no-warnings",
        "--",
        &target,
    ]);
    let output = process::output(&mut command, cancellation, QUERY_TIMEOUT).map_err(|error| {
        format!("Não foi possível executar yt-dlp: {error}. Verifique se ele está ao lado do app ou no PATH.")
    })?;
    if !output.status.success() {
        return Err(format!(
            "A pesquisa falhou: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let parsed = parse_results(&output.stdout, !is_url)?;
    Ok(if is_url {
        parsed
    } else {
        apply_search_filters(parsed, filters)
    })
}

#[allow(dead_code)]
fn fetch_youtube_search_html(
    query: &str,
    filters: &SearchFilters,
    cancellation: &Cancellation,
) -> Result<Vec<SearchResult>, String> {
    if cancellation.cancelled() {
        return Err("Pesquisa cancelada.".to_owned());
    }
    let mut url = url::Url::parse("https://www.youtube.com/results")
        .map_err(|_| "URL de pesquisa inválida.".to_owned())?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("search_query", query);
        if let Some(sp) = encode_search_sp(filters) {
            pairs.append_pair("sp", &sp);
        }
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
        return Err("Pesquisa cancelada.".to_owned());
    }
    if !response.status().is_success() {
        return Err(format!("YouTube retornou HTTP {}", response.status()));
    }
    let body = response
        .text()
        .map_err(|error| format!("Falha ao ler resposta do YouTube: {error}"))?;
    let data = extract_yt_initial_data(&body)
        .ok_or_else(|| "Não foi possível ler os dados iniciais de busca do YouTube.".to_owned())?;
    Ok(parse_yt_initial_data_search_results(&data))
}

fn parse_yt_video_renderer(v: &Value) -> Option<SearchResult> {
    let video_id = v
        .get("videoId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    let title = yt_text(v.get("title"));
    if title.is_empty() {
        return None;
    }
    let nav_url = v
        .pointer("/navigationEndpoint/commandMetadata/webCommandMetadata/url")
        .and_then(Value::as_str)
        .unwrap_or("");
    let is_short = nav_url.starts_with("/shorts/");
    let url = if is_short {
        format!("https://www.youtube.com/shorts/{video_id}")
    } else {
        format!("https://www.youtube.com/watch?v={video_id}")
    };
    let duration = yt_text(v.get("lengthText"));
    let thumbnail = select_yt_thumbnail(v.pointer("/thumbnail/thumbnails"), 480);

    let channel_text = yt_text(
        v.get("ownerText")
            .or_else(|| v.get("longBylineText"))
            .or_else(|| v.get("shortBylineText")),
    );

    let (channel_id, channel_url) = channel_url_from_browse_endpoint(
        v.pointer("/channelThumbnailSupportedRenderers/channelThumbnailWithLinkRenderer/navigationEndpoint")
            .or_else(|| v.pointer("/ownerText/runs/0/navigationEndpoint"))
            .or_else(|| v.pointer("/longBylineText/runs/0/navigationEndpoint")),
    );
    let avatar_url = select_yt_thumbnail(
        v.pointer("/channelThumbnailSupportedRenderers/channelThumbnailWithLinkRenderer/thumbnail/thumbnails"),
        68,
    );
    let channel_avatar_url = (!avatar_url.is_empty()).then_some(avatar_url);
    let channel_verified = has_verified_owner_badge(v.get("ownerBadges"));

    let mut is_live = false;
    let mut badges = Vec::new();
    if let Some(badge_arr) = v.get("badges").and_then(Value::as_array) {
        for badge in badge_arr {
            let renderer = badge.get("metadataBadgeRenderer").unwrap_or(badge);
            let style = renderer.get("style").and_then(Value::as_str).unwrap_or("");
            let icon = renderer
                .pointer("/icon/iconType")
                .and_then(Value::as_str)
                .unwrap_or("");
            let label = renderer
                .get("label")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if style.contains("LIVE_NOW") || icon == "LIVE" || label.eq_ignore_ascii_case("ao vivo")
            {
                is_live = true;
            } else if !label.is_empty() && !badges.iter().any(|existing: &String| existing == label)
            {
                badges.push(label.to_owned());
            }
        }
    }
    if duration.is_empty()
        && let Some(overlays) = v.get("thumbnailOverlays").and_then(Value::as_array)
        && overlays.iter().any(|ov| {
            ov.pointer("/thumbnailOverlayTimeStatusRenderer/style")
                .and_then(Value::as_str)
                == Some("LIVE")
        })
    {
        is_live = true;
    }

    let short_views_raw = yt_text(v.get("shortViewCountText"));
    let full_views_raw = yt_text(v.get("viewCountText"));
    let view_count = parse_pt_br_compact_count(&full_views_raw)
        .or_else(|| parse_pt_br_compact_count(&short_views_raw));
    let view_count_text = clean_pt_br_view_label(&short_views_raw)
        .or_else(|| clean_pt_br_view_label(&full_views_raw));
    let published_time_raw = yt_text(v.get("publishedTimeText"));
    let published_time_text = clean_published_time_text(&published_time_raw);

    let description = v
        .pointer("/detailedMetadataSnippets/0/snippetText")
        .or_else(|| v.get("descriptionSnippet"))
        .map(|val| yt_text(Some(val)))
        .filter(|text| !text.is_empty());

    Some(SearchResult {
        video: Video {
            title,
            channel: channel_text,
            url,
            thumbnail,
            duration: if is_live { String::new() } else { duration },
        },
        metadata: VideoMetadata {
            view_count,
            upload_date: None,
            description,
            channel_id,
            channel_url,
            kind: if is_short {
                SearchResultKind::Short
            } else {
                SearchResultKind::Video
            },
            is_live,
            published_time_text,
            view_count_text,
            channel_avatar_url,
            channel_verified,
            badges,
            playlist_count: None,
            subscriber_count_text: None,
            upcoming_start_unix: None,
        },
    })
}

fn parse_yt_shorts_lockup(s: &Value) -> Option<SearchResult> {
    let watch_ep = s.pointer("/onTap/innertubeCommand/reelWatchEndpoint");
    let video_id = watch_ep
        .and_then(|ep| ep.get("videoId"))
        .and_then(Value::as_str)
        .or_else(|| {
            s.get("entityId")
                .and_then(Value::as_str)
                .and_then(|id| id.strip_prefix("shorts-shelf-item-"))
        })?
        .trim();
    if video_id.is_empty() {
        return None;
    }
    let title = s
        .pointer("/overlayMetadata/primaryText/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .or_else(|| {
            s.get("accessibilityText")
                .and_then(Value::as_str)
                .map(str::trim)
        })?
        .to_owned();
    let views_raw = s
        .pointer("/overlayMetadata/secondaryText/content")
        .and_then(Value::as_str)
        .unwrap_or("");
    let view_count = parse_pt_br_compact_count(views_raw);
    let view_count_text = clean_pt_br_view_label(views_raw);
    let thumbnail = select_yt_thumbnail(
        s.pointer("/thumbnailViewModel/thumbnailViewModel/image/sources")
            .or_else(|| watch_ep.and_then(|ep| ep.pointer("/thumbnail/thumbnails")))
            .or_else(|| s.pointer("/thumbnail/sources")),
        480,
    );
    Some(SearchResult {
        video: Video {
            title,
            channel: String::new(),
            url: format!("https://www.youtube.com/shorts/{video_id}"),
            thumbnail,
            duration: "0:59".to_owned(),
        },
        metadata: VideoMetadata {
            view_count,
            view_count_text,
            kind: SearchResultKind::Short,
            ..Default::default()
        },
    })
}

fn parse_yt_channel_renderer(c: &Value) -> Option<SearchResult> {
    let channel_id = c
        .get("channelId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| is_valid_channel_id(id))?
        .to_owned();
    let title = yt_text(c.get("title"));
    if title.is_empty() {
        return None;
    }
    let (_, resolved_url) = channel_url_from_browse_endpoint(c.get("navigationEndpoint"));
    let canonical_url =
        resolved_url.unwrap_or_else(|| format!("https://www.youtube.com/channel/{channel_id}"));
    let avatar = select_yt_thumbnail(c.pointer("/thumbnail/thumbnails"), 176);
    let handle_part = yt_text(c.get("subscriberCountText"));
    let subs_part = yt_text(c.get("videoCountText"));
    let combined_sub = match (handle_part.is_empty(), subs_part.is_empty()) {
        (false, false) => Some(format!("{handle_part} \u{2022} {subs_part}")),
        (false, true) => Some(handle_part),
        (true, false) => Some(subs_part),
        (true, true) => None,
    };
    let description = {
        let text = yt_text(c.get("descriptionSnippet"));
        (!text.is_empty()).then_some(text)
    };
    let verified = has_verified_owner_badge(c.get("ownerBadges"));
    Some(SearchResult {
        video: Video {
            title: title.clone(),
            channel: title,
            url: canonical_url.clone(),
            thumbnail: avatar.clone(),
            duration: String::new(),
        },
        metadata: VideoMetadata {
            description,
            channel_id: Some(channel_id),
            channel_url: Some(canonical_url),
            kind: SearchResultKind::Channel,
            channel_avatar_url: (!avatar.is_empty()).then_some(avatar),
            channel_verified: verified,
            subscriber_count_text: combined_sub,
            ..Default::default()
        },
    })
}

pub(super) fn parse_yt_lockup_video(l: &Value) -> Option<SearchResult> {
    let content_type = l.get("contentType").and_then(Value::as_str).unwrap_or("");
    let content_id = l
        .get("contentId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    if content_type != "LOCKUP_CONTENT_TYPE_VIDEO"
        && (!content_type.is_empty() || !crate::history::is_valid_video_id(content_id))
    {
        return None;
    }
    let title = l
        .pointer("/metadata/lockupMetadataViewModel/title/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())?
        .to_owned();
    let thumbnail = select_yt_thumbnail(
        l.pointer("/contentImage/thumbnailViewModel/image/sources"),
        480,
    );
    let mut duration = String::new();
    let mut is_live = false;
    if let Some(overlays) = l
        .pointer("/contentImage/thumbnailViewModel/overlays")
        .and_then(Value::as_array)
    {
        for ov in overlays {
            if let Some(badges) = ov
                .pointer("/thumbnailOverlayBadgeViewModel/thumbnailBadges")
                .and_then(Value::as_array)
            {
                for badge_entry in badges {
                    let badge = badge_entry
                        .get("thumbnailBadgeViewModel")
                        .unwrap_or(badge_entry);
                    let style = badge
                        .get("badgeStyle")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let text = badge
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or("");
                    if style.contains("LIVE") || text.eq_ignore_ascii_case("ao vivo") {
                        is_live = true;
                    } else if !text.is_empty() && duration.is_empty() {
                        duration = text.to_owned();
                    }
                }
            }
        }
    }

    let parsed_rows = parse_lockup_metadata_rows(l);
    is_live |= parsed_rows.is_live;
    let channel = parsed_rows.channel;
    let channel_id = parsed_rows.channel_id;
    let channel_url = parsed_rows.channel_url;
    let channel_verified = parsed_rows.channel_verified;
    let view_count = parsed_rows.view_count;
    let view_count_text = parsed_rows.view_count_text;
    let published_time_text = parsed_rows.published_time_text;

    let avatar_url = select_yt_thumbnail(
        l.pointer(
            "/metadata/lockupMetadataViewModel/image/decoratedAvatarViewModel/avatar/avatarViewModel/image/sources",
        ),
        68,
    );
    let channel_avatar_url = (!avatar_url.is_empty()).then_some(avatar_url);

    Some(SearchResult {
        video: Video {
            title,
            channel,
            url: format!("https://www.youtube.com/watch?v={content_id}"),
            thumbnail,
            duration: if is_live { String::new() } else { duration },
        },
        metadata: VideoMetadata {
            view_count,
            upload_date: None,
            description: None,
            channel_id,
            channel_url,
            kind: SearchResultKind::Video,
            is_live,
            published_time_text,
            view_count_text,
            channel_avatar_url,
            channel_verified,
            ..Default::default()
        },
    })
}

fn parse_yt_lockup_playlist(l: &Value) -> Option<SearchResult> {
    let content_type = l.get("contentType").and_then(Value::as_str).unwrap_or("");
    let content_id = l
        .get("contentId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    if content_type != "LOCKUP_CONTENT_TYPE_PLAYLIST"
        && !content_id.starts_with("PL")
        && !content_id.starts_with("RD")
        && !content_id.starts_with("OL")
    {
        return None;
    }
    let title = l
        .pointer("/metadata/lockupMetadataViewModel/title/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())?
        .to_owned();
    let first_part = l.pointer(
        "/metadata/lockupMetadataViewModel/metadata/contentMetadataViewModel/metadataRows/0/metadataParts/0/text",
    );
    let channel = first_part
        .and_then(|p| p.get("content"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_owned();
    let (channel_id, channel_url) = channel_url_from_browse_endpoint(
        first_part.and_then(|p| p.pointer("/commandRuns/0/onTap/innertubeCommand")),
    );
    let playlist_count = l
        .pointer("/contentImage/collectionThumbnailViewModel/primaryThumbnail/thumbnailViewModel/overlays")
        .or_else(|| l.pointer("/contentImage/thumbnailViewModel/overlays"))
        .and_then(Value::as_array)
        .and_then(|ovs| {
            ovs.iter().find_map(|ov| {
                ov.pointer("/thumbnailOverlayBadgeViewModel/thumbnailBadges/0/thumbnailBadgeViewModel/text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            })
        });
    let thumbnail = select_yt_thumbnail(
        l.pointer("/contentImage/collectionThumbnailViewModel/primaryThumbnail/thumbnailViewModel/image/sources")
            .or_else(|| l.pointer("/contentImage/thumbnailViewModel/image/sources")),
        480,
    );
    let badges = crate::innertube::extract_lockup_playlist_privacy_badges(l);
    Some(SearchResult {
        video: Video {
            title,
            channel,
            url: format!("https://www.youtube.com/playlist?list={content_id}"),
            thumbnail,
            duration: String::new(),
        },
        metadata: VideoMetadata {
            channel_id,
            channel_url,
            kind: SearchResultKind::Playlist,
            badges,
            playlist_count,
            ..Default::default()
        },
    })
}

fn parse_yt_playlist_renderer(p: &Value) -> Option<SearchResult> {
    let playlist_id = p
        .get("playlistId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    let title = yt_text(p.get("title"));
    if title.is_empty() {
        return None;
    }
    let channel = yt_text(p.get("shortBylineText").or_else(|| p.get("longBylineText")));
    let (channel_id, channel_url) = channel_url_from_browse_endpoint(
        p.pointer("/shortBylineText/runs/0/navigationEndpoint")
            .or_else(|| p.pointer("/longBylineText/runs/0/navigationEndpoint")),
    );
    let thumbnail = select_yt_thumbnail(p.pointer("/thumbnails/0/thumbnails"), 480);
    let count_num = p.get("videoCount").and_then(Value::as_str).map(str::trim);
    let playlist_count = count_num
        .filter(|c| !c.is_empty())
        .map(|c| format!("{c} v\u{00ed}deos"));
    Some(SearchResult {
        video: Video {
            title,
            channel,
            url: format!("https://www.youtube.com/playlist?list={playlist_id}"),
            thumbnail,
            duration: String::new(),
        },
        metadata: VideoMetadata {
            channel_id,
            channel_url,
            kind: SearchResultKind::Playlist,
            playlist_count,
            ..Default::default()
        },
    })
}

pub(super) fn collect_yt_search_items(
    value: &Value,
    out: &mut Vec<SearchResult>,
    seen: &mut std::collections::HashSet<String>,
) {
    let Some(obj) = value.as_object() else {
        if let Some(arr) = value.as_array() {
            for item in arr {
                collect_yt_search_items(item, out, seen);
            }
        }
        return;
    };
    if let Some(v) = obj
        .get("videoRenderer")
        .or_else(|| obj.get("gridVideoRenderer"))
        .or_else(|| obj.get("compactVideoRenderer"))
    {
        if let Some(res) = parse_yt_video_renderer(v)
            && seen.insert(res.video.url.clone())
        {
            out.push(res);
        }
        return;
    }
    if let Some(s) = obj.get("shortsLockupViewModel") {
        if let Some(res) = parse_yt_shorts_lockup(s)
            && seen.insert(res.video.url.clone())
        {
            out.push(res);
        }
        return;
    }
    if let Some(c) = obj.get("channelRenderer") {
        if let Some(res) = parse_yt_channel_renderer(c)
            && seen.insert(res.video.url.clone())
        {
            out.push(res);
        }
        return;
    }
    if let Some(l) = obj.get("lockupViewModel") {
        if let Some(res) = parse_yt_lockup_video(l).or_else(|| parse_yt_lockup_playlist(l))
            && seen.insert(res.video.url.clone())
        {
            out.push(res);
        }
        return;
    }
    if let Some(p) = obj
        .get("playlistRenderer")
        .or_else(|| obj.get("gridPlaylistRenderer"))
    {
        if let Some(res) = parse_yt_playlist_renderer(p)
            && seen.insert(res.video.url.clone())
        {
            out.push(res);
        }
        return;
    }
    for (key, child) in obj {
        // Skip secondary side rails (e.g., ads or watch-card playlists) when primary results exist.
        if key == "secondaryContents" {
            continue;
        }
        collect_yt_search_items(child, out, seen);
    }
}

#[allow(dead_code)]
pub fn parse_yt_initial_data_search_results(data: &Value) -> Vec<SearchResult> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let root = data
        .pointer("/contents/twoColumnSearchResultsRenderer/primaryContents")
        .unwrap_or(data);
    collect_yt_search_items(root, &mut out, &mut seen);
    out
}

pub(super) fn parse_results(
    output: &[u8],
    youtube_search: bool,
) -> Result<Vec<SearchResult>, String> {
    let mut videos = Vec::new();
    for line in output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
    {
        let value: Value = serde_json::from_slice(line)
            .map_err(|error| format!("Resposta inválida do yt-dlp: {error}"))?;
        if let Some(result) = parse_video_entry(&value, youtube_search, None, None, None) {
            videos.push(result);
        }
    }
    Ok(videos)
}

pub const YOUTUBE_SUGGEST_ENDPOINT: &str = "https://suggestqueries.google.com/complete/search";

const SEARCH_SUGGEST_TIMEOUT: Duration = Duration::from_secs(4);

const MAX_SEARCH_SUGGEST_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchSuggestionItem {
    pub text: String,
    pub is_recent: bool,
}

pub fn parse_youtube_suggest_json(body: &str) -> Result<Vec<String>, String> {
    let parsed: Value = serde_json::from_str(body)
        .map_err(|err| format!("Resposta inválida de sugestões de busca: {err}"))?;
    let suggestions = parsed
        .as_array()
        .and_then(|arr| arr.get(1))
        .and_then(Value::as_array)
        .ok_or_else(|| "Formato inesperado nas sugestões do YouTube.".to_owned())?;

    let mut out = Vec::new();
    let mut seen_lower = std::collections::HashSet::new();
    for entry in suggestions {
        let Some(raw) = entry.as_str() else {
            continue;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if seen_lower.insert(lower) {
            out.push(trimmed.to_owned());
            if out.len() >= 10 {
                break;
            }
        }
    }
    Ok(out)
}

pub fn merge_search_suggestions(
    query: &str,
    recent_searches: &[String],
    remote_suggestions: &[String],
    max_items: usize,
) -> Vec<SearchSuggestionItem> {
    if max_items == 0 {
        return Vec::new();
    }
    let trimmed = query.trim();
    let needle = trimmed.to_lowercase();
    let mut merged = Vec::new();
    let mut seen_lower = std::collections::HashSet::new();

    let mut matching_recent: Vec<(&str, bool)> = recent_searches
        .iter()
        .filter_map(|raw| {
            let clean = raw.trim();
            if clean.is_empty() {
                return None;
            }
            if needle.is_empty() {
                return Some((clean, true));
            }
            let lower = clean.to_lowercase();
            if lower.starts_with(&needle) {
                Some((clean, true))
            } else if lower.contains(&needle) {
                Some((clean, false))
            } else {
                None
            }
        })
        .collect();
    if !needle.is_empty() {
        matching_recent.sort_by_key(|(_, is_prefix)| !*is_prefix);
    }

    for (recent, _) in matching_recent {
        if merged.len() >= max_items {
            break;
        }
        let lower = recent.to_lowercase();
        if seen_lower.insert(lower) {
            merged.push(SearchSuggestionItem {
                text: recent.to_owned(),
                is_recent: true,
            });
        }
    }

    if !needle.is_empty() {
        for suggestion in remote_suggestions {
            if merged.len() >= max_items {
                break;
            }
            let clean = suggestion.trim();
            if clean.is_empty() {
                continue;
            }
            let lower = clean.to_lowercase();
            if seen_lower.insert(lower) {
                merged.push(SearchSuggestionItem {
                    text: clean.to_owned(),
                    is_recent: false,
                });
            }
        }
    }

    merged
}

pub fn fetch_search_suggestions_at(
    endpoint: &str,
    query: &str,
    cancellation: &Cancellation,
) -> Result<Vec<String>, String> {
    if cancellation.cancelled() {
        return Err("Consulta de sugestões cancelada.".to_owned());
    }
    let trimmed = query.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with(':')
    {
        return Ok(Vec::new());
    }
    let response = crate::http_client::shared()
        .get(endpoint)
        .timeout(SEARCH_SUGGEST_TIMEOUT)
        .query(&[
            ("client", "firefox"),
            ("ds", "yt"),
            ("hl", "pt"),
            ("q", trimmed),
        ])
        .header(reqwest::header::USER_AGENT, SUBTITLE_USER_AGENT)
        .header(
            reqwest::header::ACCEPT_LANGUAGE,
            "pt-BR,pt;q=0.9,en-US;q=0.8,en;q=0.7",
        )
        .send()
        .map_err(|err| format!("Falha ao buscar sugestões: {err}"))?;
    if cancellation.cancelled() {
        return Err("Consulta de sugestões cancelada.".to_owned());
    }
    if !response.status().is_success() {
        return Err(format!(
            "Servidor de sugestões retornou HTTP {}",
            response.status().as_u16()
        ));
    }
    let mut limited = response.take((MAX_SEARCH_SUGGEST_BYTES + 1) as u64);
    let mut bytes = Vec::new();
    limited
        .read_to_end(&mut bytes)
        .map_err(|err| format!("Falha ao ler sugestões: {err}"))?;
    if cancellation.cancelled() {
        return Err("Consulta de sugestões cancelada.".to_owned());
    }
    if bytes.len() > MAX_SEARCH_SUGGEST_BYTES {
        return Err("Resposta de sugestões excedeu o limite permitido.".to_owned());
    }
    let body = String::from_utf8_lossy(&bytes);
    parse_youtube_suggest_json(&body)
}

pub fn fetch_search_suggestions(
    query: &str,
    cancellation: &Cancellation,
) -> Result<Vec<String>, String> {
    fetch_search_suggestions_at(YOUTUBE_SUGGEST_ENDPOINT, query, cancellation)
}
