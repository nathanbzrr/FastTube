use super::channel::{
    CHANNEL_PAGE_CACHE_CAPACITY, CHANNEL_PAGE_CACHE_TTL, fetch_channel_page_cached_with,
    fetch_with_cache, parse_channel_about, parse_channel_page, parse_channel_playlists,
    parse_channel_videos_batch,
};
use super::playlist::{
    RESOLUTION_CACHE_LIMIT, RESOLUTION_CACHE_MAX_HEIGHTS, RESOLUTION_CACHE_MAX_URL_BYTES,
    RESOLUTION_CACHE_TTL, ResolutionCache, cached_resolutions, find_matching_subtitle_track,
    get_cached_subtitle_tracks, merge_vtt_segments, parse_public_playlist_videos,
    parse_resolutions_from_json, parse_subtitles, resolve_yt_dlp_sub_request,
    store_cached_subtitle_tracks, subtitle_hls_fallbacks, validate_vtt_payload,
};
use super::query::{
    is_playlist_url, merge_home_feeds, parse_home_feed, parse_results, parse_yt_lockup_video,
};
use super::*;
use crate::process::Cancellation;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[test]
fn bundled_yt_dlp_is_resolved_from_executable_directory() {
    let root = std::env::temp_dir().join(format!(
        "fasttube-ytdlp-discovery-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let app_dir = root.join("portable");
    std::fs::create_dir_all(&app_dir).unwrap();
    let app = app_dir.join("fasttube.exe");
    std::fs::write(&app, b"fixture executable").unwrap();

    #[cfg(windows)]
    let helper_name = "yt-dlp.exe";
    #[cfg(not(windows))]
    let helper_name = "yt-dlp";
    assert_eq!(bundled_yt_dlp(&app), None);
    std::fs::write(app_dir.join(helper_name), b"fixture helper").unwrap();
    assert_eq!(bundled_yt_dlp(&app), Some(app_dir.join(helper_name)));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn libmpv_script_option_uses_the_bundled_helper_path() {
    assert_eq!(
        ytdl_hook_path_option(std::path::Path::new(r"D:\FastTube, portable\yt-dlp.exe")),
        r"ytdl_hook-ytdl_path=D:\\FastTube\, portable\\yt-dlp.exe"
    );
}

#[test]
fn home_feed_distinguishes_videos_playlists_and_untrusted_entries() {
    let output = br#"{"id":"dQw4w9WgXcQ","title":"Video","ie_key":"Youtube","url":"https://www.youtube.com/watch?v=dQw4w9WgXcQ"}
{"id":"PLabc_123","title":"Playlist","ie_key":"YoutubeTab","url":"https://www.youtube.com/playlist?list=PLabc_123"}
{"id":"bad&x=1","title":"Bad","ie_key":"YoutubeTab","url":"https://example.org/"}
{"id":"dQw4w9WgXcQ","ie_key":"Youtube","url":"https://example.org/video"}
{"id":"something","ie_key":"Other","url":"https://example.org/"}
"#;
    let feed = parse_home_feed(output).unwrap();
    assert_eq!(feed.items.len(), 2);
    assert_eq!(
        feed.playlists,
        ["https://www.youtube.com/playlist?list=PLabc_123"]
    );
    assert_eq!(feed.items[1].video.url, feed.playlists[0]);
    assert!(parse_home_feed(b"not json").is_err());
    assert!(!is_playlist_url(
        "https://www.youtube.com/playlist?list=a&x=b"
    ));
}

#[test]
fn home_batch_playlist_url_encoding_and_deduplication_work() {
    assert_eq!(
        build_playlist_search_url("The Strokes mix"),
        "https://www.youtube.com/results?search_query=The+Strokes+mix&sp=EgIQAw%3D%3D"
    );
    let fallback_line = br#"{"id":"abcDEF12345","title":"Video","ie_key":"Youtube","url":"https://www.youtube.com/watch?v=abcDEF12345","playlist_channel":"Canal X","playlist_channel_id":"UC1234567890123456789012"}"#;
    let parsed = parse_home_feed(fallback_line).unwrap();
    assert_eq!(parsed.items.len(), 1);
    assert_eq!(parsed.items[0].video.channel, "Canal X");
    assert_eq!(
        parsed.items[0].metadata.channel_id.as_deref(),
        Some("UC1234567890123456789012")
    );

    let f1 = parse_home_feed(
        br#"{"id":"dQw4w9WgXcQ","title":"V1","ie_key":"Youtube","url":"https://www.youtube.com/watch?v=dQw4w9WgXcQ"}
{"id":"PLabc_123","title":"Mix One","ie_key":"YoutubeTab","url":"https://www.youtube.com/playlist?list=PLabc_123"}"#,
    )
    .unwrap();
    let f2 = parse_home_feed(
        br#"{"id":"dQw4w9WgXcQ","title":"V1 dup","ie_key":"Youtube","url":"https://youtu.be/dQw4w9WgXcQ"}
{"id":"xyz12345678","title":"V2","ie_key":"Youtube","url":"https://www.youtube.com/watch?v=xyz12345678"}
{"id":"PLabc_123","title":"Mix One dup","ie_key":"YoutubeTab","url":"https://www.youtube.com/playlist?list=PLabc_123"}"#,
    )
    .unwrap();
    let merged = merge_home_feeds(vec![f1, f2]);
    assert_eq!(merged.items.len(), 3);
    assert_eq!(merged.playlists.len(), 1);
}

use std::sync::mpsc;

#[test]
fn resolution_cache_reuses_success_and_expires() {
    let cache = Mutex::new(ResolutionCache::default());
    assert_eq!(
        cached_resolutions(&cache, "video", || Ok(vec![1080, 720])),
        Ok(vec![1080, 720])
    );
    assert_eq!(
        cached_resolutions(&cache, "video", || panic!("cache miss")),
        Ok(vec![1080, 720])
    );
    cache.lock().unwrap().entries[0].inserted = Instant::now() - RESOLUTION_CACHE_TTL;
    assert_eq!(
        cached_resolutions(&cache, "video", || Ok(vec![480])),
        Ok(vec![480])
    );
}

#[test]
fn resolution_cache_does_not_store_errors_or_oversized_values() {
    let cache = Mutex::new(ResolutionCache::default());
    assert!(cached_resolutions(&cache, "video", || Err("failed".into())).is_err());
    assert_eq!(
        cached_resolutions(&cache, "video", || Ok(Vec::new())),
        Ok(Vec::new())
    );
    let heights = vec![1; RESOLUTION_CACHE_MAX_HEIGHTS + 1];
    assert_eq!(
        cached_resolutions(&cache, "video", || Ok(heights.clone())),
        Ok(heights)
    );
    let url = "x".repeat(RESOLUTION_CACHE_MAX_URL_BYTES + 1);
    assert_eq!(
        cached_resolutions(&cache, &url, || Ok(vec![720])),
        Ok(vec![720])
    );
    assert!(cache.lock().unwrap().entries.is_empty());
    assert_eq!(
        cached_resolutions(&cache, "video", || Ok(vec![720])),
        Ok(vec![720])
    );
}

#[test]
fn resolution_cache_evicts_oldest_entry() {
    let cache = Mutex::new(ResolutionCache::default());
    for index in 0..=RESOLUTION_CACHE_LIMIT {
        cached_resolutions(&cache, &index.to_string(), || Ok(vec![720])).unwrap();
    }
    let guard = cache.lock().unwrap();
    let entries = &guard.entries;
    assert_eq!(entries.len(), RESOLUTION_CACHE_LIMIT);
    assert!(!entries.iter().any(|entry| entry.url == "0"));
    assert!(
        entries
            .iter()
            .any(|entry| entry.url == RESOLUTION_CACHE_LIMIT.to_string())
    );
}

fn wait_for_followers(cache: &Mutex<ResolutionCache>, url: &str, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let followers = cache
            .lock()
            .unwrap()
            .in_flight
            .get(url)
            .map_or(0, |flight| Arc::strong_count(flight).saturating_sub(2));
        if followers >= count {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "followers did not join in-flight query"
        );
        std::thread::yield_now();
    }
}

fn concurrent_result(result: Result<Vec<u32>, String>, url: String) {
    let cache = Arc::new(Mutex::new(ResolutionCache::default()));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let leader_cache = Arc::clone(&cache);
    let leader_url = url.clone();
    let expected = result.clone();
    let leader = std::thread::spawn(move || {
        cached_resolutions(&leader_cache, &leader_url, || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            result
        })
    });
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let followers: Vec<_> = (0..4)
        .map(|_| {
            let cache = Arc::clone(&cache);
            let url = url.clone();
            std::thread::spawn(move || {
                cached_resolutions(&cache, &url, || panic!("duplicate fetch"))
            })
        })
        .collect();
    wait_for_followers(&cache, &url, followers.len());
    release_tx.send(()).unwrap();
    assert_eq!(leader.join().unwrap(), expected);
    for follower in followers {
        assert_eq!(follower.join().unwrap(), expected);
    }
    assert!(cache.lock().unwrap().in_flight.is_empty());
    if expected.is_err()
        || expected.as_ref().unwrap().is_empty()
        || url.len() > RESOLUTION_CACHE_MAX_URL_BYTES
    {
        assert_eq!(
            cached_resolutions(&cache, &url, || Ok(vec![480])),
            Ok(vec![480])
        );
    }
}

#[test]
fn concurrent_same_url_shares_results_and_errors() {
    concurrent_result(Ok(vec![1080]), "video".to_owned());
    concurrent_result(Err("failed".to_owned()), "video".to_owned());
    concurrent_result(Ok(Vec::new()), "video".to_owned());
    concurrent_result(
        Ok(vec![720]),
        "x".repeat(RESOLUTION_CACHE_MAX_URL_BYTES + 1),
    );
}

#[test]
fn distinct_urls_fetch_in_parallel() {
    let cache = Arc::new(Mutex::new(ResolutionCache::default()));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first_cache = Arc::clone(&cache);
    let first = std::thread::spawn(move || {
        cached_resolutions(&first_cache, "first", || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(vec![1080])
        })
    });
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let second_cache = Arc::clone(&cache);
    let second = std::thread::spawn(move || {
        done_tx
            .send(cached_resolutions(&second_cache, "second", || {
                Ok(vec![720])
            }))
            .unwrap();
    });
    let result = done_rx.recv_timeout(Duration::from_secs(10));
    release_tx.send(()).unwrap();
    assert_eq!(result.unwrap(), Ok(vec![720]));
    assert_eq!(first.join().unwrap(), Ok(vec![1080]));
    second.join().unwrap();
}

#[test]
fn panic_wakes_followers_and_allows_retry() {
    let cache = Arc::new(Mutex::new(ResolutionCache::default()));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let leader_cache = Arc::clone(&cache);
    let leader = std::thread::spawn(move || {
        cached_resolutions(&leader_cache, "video", || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            panic!("fetch panic")
        })
    });
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let follower_cache = Arc::clone(&cache);
    let follower = std::thread::spawn(move || {
        cached_resolutions(&follower_cache, "video", || panic!("duplicate fetch"))
    });
    wait_for_followers(&cache, "video", 1);
    release_tx.send(()).unwrap();
    assert!(leader.join().is_err());
    assert!(follower.join().unwrap().is_err());
    assert!(cache.lock().unwrap().in_flight.is_empty());
    assert_eq!(
        cached_resolutions(&cache, "video", || Ok(vec![480])),
        Ok(vec![480])
    );
}

#[test]
fn parses_flat_youtube_results_and_metadata() {
    let input = br#"{"id":"abc_123","title":"Example","channel":null,"uploader":"Creator","channel_id":"UC8butISFwT-Wl7EV0hUK0BQ","channel_url":"https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ","duration":3661.8,"view_count":0,"upload_date":"20261106","description":"Description","thumbnails":[{"width":320},{"width":300,"url":"https://example.com/image.jpg"}]}"#;
    let results = parse_results(input, true).unwrap();
    let result = &results[0];
    assert_eq!(result.video.url, "https://www.youtube.com/watch?v=abc_123");
    assert_eq!(result.video.channel, "Creator");
    assert_eq!(result.video.duration, "1:01:01");
    assert_eq!(result.video.thumbnail, "https://example.com/image.jpg");
    assert_eq!(result.metadata.view_count, Some(0));
    assert_eq!(result.metadata.upload_date.as_deref(), Some("20261106"));
    assert_eq!(result.metadata.description.as_deref(), Some("Description"));
    assert_eq!(
        result.metadata.channel_id.as_deref(),
        Some("UC8butISFwT-Wl7EV0hUK0BQ")
    );
    assert_eq!(
        result.metadata.channel_url.as_deref(),
        Some("https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ")
    );
    assert_eq!(
        channel_target_for_metadata(&result.metadata),
        Some(ChannelTarget::ChannelId(
            "UC8butISFwT-Wl7EV0hUK0BQ".to_owned()
        ))
    );
}

#[test]
fn preserves_urls_and_absent_metadata() {
    let input = br#"{"id":"123","webpage_url":"https://example.com/watch/123","thumbnail":"https://example.com/image.jpg","view_count":null,"description":""}

{"id":"other"}"#;
    let results = parse_results(input, false).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].video.url, "https://example.com/watch/123");
    assert_eq!(results[0].video.thumbnail, "https://example.com/image.jpg");
    assert!(results[0].metadata.view_count.is_none());
    assert!(results[0].metadata.upload_date.is_none());
    assert!(results[0].metadata.description.is_none());
    assert!(results[0].metadata.channel_id.is_none());
    assert!(results[0].metadata.channel_url.is_none());
    assert!(channel_target_for_metadata(&results[0].metadata).is_none());
}

#[test]
fn handles_flat_urls_youtube_ids_and_invalid_json() {
    let input = br#"{"id":"x","url":"https://example.com/x"}
{"id":"abc-123","ie_key":"Youtube"}
{"id":"bad?id","extractor_key":"Youtube"}"#;
    let results = parse_results(input, false).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].video.url, "https://example.com/x");
    assert_eq!(
        results[1].video.url,
        "https://www.youtube.com/watch?v=abc-123"
    );
    assert!(parse_results(b"{invalid}", true).is_err());
}

#[test]
fn validates_channel_ids_handles_and_urls_strictly() {
    let valid_id = "UC8butISFwT-Wl7EV0hUK0BQ";
    assert!(is_valid_channel_id(valid_id));
    assert!(!is_valid_channel_id("UC8butISFwT-Wl7EV0hUK0B")); // 23 chars
    assert!(!is_valid_channel_id("UC8butISFwT-Wl7EV0hUK0BQQ")); // 25 chars
    assert!(!is_valid_channel_id("AB8butISFwT-Wl7EV0hUK0BQ")); // wrong prefix
    assert!(!is_valid_channel_id("UC8butISFwT-Wl7EV0hUK0B?")); // invalid char

    assert!(is_valid_channel_handle("@freecodecamp"));
    assert!(is_valid_channel_handle("@rust_lang.org-1"));
    assert!(!is_valid_channel_handle("freecodecamp")); // missing @
    assert!(!is_valid_channel_handle("@ab")); // too short (<3 after @)
    assert!(!is_valid_channel_handle(&format!("@{}", "a".repeat(31)))); // too long (>30 after @)
    assert!(!is_valid_channel_handle("@bad/handle"));
    assert!(!is_valid_channel_handle("@bad handle"));

    assert_eq!(
        parse_channel_target(valid_id),
        Some(ChannelTarget::ChannelId(valid_id.to_owned()))
    );
    assert_eq!(
        parse_channel_target("  @freecodecamp  "),
        Some(ChannelTarget::Handle("@freecodecamp".to_owned()))
    );
    assert_eq!(
        parse_channel_target(
            "https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ/videos?view=0#top"
        ),
        Some(ChannelTarget::ChannelId(valid_id.to_owned()))
    );
    assert_eq!(
        parse_channel_target("https://m.youtube.com/@freecodecamp/streams/"),
        Some(ChannelTarget::Handle("@freecodecamp".to_owned()))
    );
    assert_eq!(
        youtube_channel_url("http://youtube.com:80/@freecodecamp"),
        Some("https://www.youtube.com/@freecodecamp".to_owned())
    );
    assert_eq!(
        youtube_channel_videos_url(valid_id),
        Some("https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ/videos".to_owned())
    );

    // Reject video URLs, external domains, credentials, arbitrary ports, and unknown paths
    assert!(parse_channel_target("https://www.youtube.com/watch?v=dQw4w9WgXcQ").is_none());
    assert!(parse_channel_target("https://youtu.be/dQw4w9WgXcQ").is_none());
    assert!(parse_channel_target("https://evil.com/channel/UC8butISFwT-Wl7EV0hUK0BQ").is_none());
    assert!(
        parse_channel_target("https://user@www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ")
            .is_none()
    );
    assert!(
        parse_channel_target("https://www.youtube.com:8080/channel/UC8butISFwT-Wl7EV0hUK0BQ")
            .is_none()
    );
    assert!(parse_channel_target("https://www.youtube.com/@freecodecamp/unknown").is_none());
    assert!(parse_channel_target("-option-injection").is_none());
    assert!(fetch_channel_page("invalid-channel").is_err());
}

#[test]
fn extracts_channel_association_only_when_reliable() {
    // 1. Valid channel_id + handle URL -> prefers canonical /channel/<UC...> URL
    let with_id = br#"{"id":"v1","ie_key":"Youtube","title":"V1","channel":"Ch","channel_id":"UC8butISFwT-Wl7EV0hUK0BQ","uploader_url":"https://www.youtube.com/@freecodecamp"}"#;
    let res = parse_results(with_id, true).unwrap();
    assert_eq!(
        res[0].metadata.channel_id.as_deref(),
        Some("UC8butISFwT-Wl7EV0hUK0BQ")
    );
    assert_eq!(
        res[0].metadata.channel_url.as_deref(),
        Some("https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ")
    );

    // 2. Only handle available (no UC id) -> preserves handle URL without inventing UC id
    let handle_only = br#"{"id":"v2","ie_key":"Youtube","title":"V2","channel":"Ch","uploader_id":"@freecodecamp"}"#;
    let res_handle = parse_results(handle_only, true).unwrap();
    assert!(res_handle[0].metadata.channel_id.is_none());
    assert_eq!(
        res_handle[0].metadata.channel_url.as_deref(),
        Some("https://www.youtube.com/@freecodecamp")
    );
    assert_eq!(
        channel_target_for_metadata(&res_handle[0].metadata),
        Some(ChannelTarget::Handle("@freecodecamp".to_owned()))
    );

    // 3. Only display name or invalid channel_id/channel_url -> does not invent association
    let unreliable = br#"{"id":"v3","ie_key":"Youtube","title":"V3","channel":"Just A Name","channel_id":"invalid_id","channel_url":"https://example.com/c/foo"}"#;
    let res_unreliable = parse_results(unreliable, true).unwrap();
    assert_eq!(res_unreliable[0].video.channel, "Just A Name");
    assert!(res_unreliable[0].metadata.channel_id.is_none());
    assert!(res_unreliable[0].metadata.channel_url.is_none());
    assert!(channel_target_for_metadata(&res_unreliable[0].metadata).is_none());

    // 4. Playlist-level fields (playlist_channel_id / playlist_uploader_id / playlist_channel)
    // on a search or playlist entry must NOT be treated as the video's own channel association.
    let playlist_only = br#"{"id":"v4","ie_key":"Youtube","title":"V4","playlist_channel":"Playlist Owner","playlist_uploader":"Playlist Owner","playlist_channel_id":"UC8butISFwT-Wl7EV0hUK0BQ","playlist_uploader_id":"@freecodecamp"}"#;
    let res_playlist = parse_results(playlist_only, true).unwrap();
    assert_eq!(res_playlist[0].video.channel, "");
    assert!(res_playlist[0].metadata.channel_id.is_none());
    assert!(res_playlist[0].metadata.channel_url.is_none());
    assert!(channel_target_for_metadata(&res_playlist[0].metadata).is_none());
}

#[test]
fn parses_channel_playlist_json_with_limit_and_fallbacks() {
    let payload = br#"{
            "id": "UC8butISFwT-Wl7EV0hUK0BQ",
            "channel": "freeCodeCamp.org",
            "channel_id": "UC8butISFwT-Wl7EV0hUK0BQ",
            "title": "freeCodeCamp.org - Videos",
            "channel_follower_count": 12000000,
            "description": "Learn to code for free.",
            "uploader_id": "@freecodecamp",
            "uploader_url": "https://www.youtube.com/@freecodecamp",
            "channel_is_verified": true,
            "_type": "playlist",
            "thumbnails": [
                {"url": "https://yt3.googleusercontent.com/banner=w2560", "width": 2560, "height": 424, "id": "5"},
                {"url": "https://yt3.googleusercontent.com/avatar=s900", "width": 900, "height": 900, "id": "avatar_uncropped"}
            ],
            "entries": [
                {
                    "id": "eHHxRJulX74",
                    "ie_key": "Youtube",
                    "title": "First Video",
                    "duration": 3661,
                    "view_count": null,
                    "thumbnails": [{"url": "https://i.ytimg.com/vi/eHHxRJulX74/hq720.jpg", "width": 360, "height": 202}]
                },
                {
                    "id": "lR931Iz8Z6U",
                    "ie_key": "Youtube",
                    "title": "Second Video",
                    "duration": 125,
                    "view_count": 102000,
                    "thumbnails": [{"url": "https://i.ytimg.com/vi/lR931Iz8Z6U/hq720.jpg", "width": 360, "height": 202}]
                },
                {
                    "id": "third_video1",
                    "ie_key": "Youtube",
                    "title": "Third Video",
                    "duration": 60
                }
            ]
        }"#;

    let target = ChannelTarget::Handle("@freecodecamp".to_owned());
    let page = parse_channel_page(payload, &target, 2).unwrap();

    assert_eq!(
        page.target,
        ChannelTarget::ChannelId("UC8butISFwT-Wl7EV0hUK0BQ".to_owned())
    );
    assert_eq!(page.channel_id.as_deref(), Some("UC8butISFwT-Wl7EV0hUK0BQ"));
    assert_eq!(page.title.as_deref(), Some("freeCodeCamp.org"));
    assert_eq!(page.handle.as_deref(), Some("@freecodecamp"));
    assert_eq!(
        page.canonical_url,
        "https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ"
    );
    assert_eq!(page.description.as_deref(), Some("Learn to code for free."));
    assert_eq!(page.subscriber_count, Some(12_000_000));
    assert_eq!(page.verified, Some(true));
    assert_eq!(
        page.avatar_url.as_deref(),
        Some("https://yt3.googleusercontent.com/avatar=s900")
    );
    assert_eq!(
        page.banner_url.as_deref(),
        Some("https://yt3.googleusercontent.com/banner=w2560")
    );
    assert_eq!(page.videos.len(), 2);
    assert_eq!(page.videos[0].video.title, "First Video");
    assert_eq!(page.videos[0].video.channel, "freeCodeCamp.org");
    assert_eq!(page.videos[0].video.duration, "1:01:01");
    assert_eq!(
        page.videos[0].video.url,
        "https://www.youtube.com/watch?v=eHHxRJulX74"
    );
    assert!(page.videos[0].metadata.view_count.is_none());
    assert_eq!(
        page.videos[0].metadata.channel_id.as_deref(),
        Some("UC8butISFwT-Wl7EV0hUK0BQ")
    );
    assert_eq!(
        page.videos[0].metadata.channel_url.as_deref(),
        Some("https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ")
    );
    assert_eq!(page.videos[1].video.channel, "freeCodeCamp.org");
    assert_eq!(
        page.videos[1].metadata.channel_id.as_deref(),
        Some("UC8butISFwT-Wl7EV0hUK0BQ")
    );
    assert_eq!(
        page.videos[1].metadata.channel_url.as_deref(),
        Some("https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ")
    );
    assert_eq!(page.videos[1].metadata.view_count, Some(102_000));
}

#[test]
fn parses_channel_page_with_missing_optional_fields_and_jsonl_fallback() {
    let minimal_playlist = r#"{
            "_type": "playlist",
            "title": "Canal Sem Extras - Vídeos",
            "entries": []
        }"#
    .as_bytes();
    let requested = ChannelTarget::Handle("@canalsemextras".to_owned());
    let page = parse_channel_page(minimal_playlist, &requested, 20).unwrap();
    assert_eq!(page.target, requested);
    assert!(page.channel_id.is_none());
    assert_eq!(page.title.as_deref(), Some("Canal Sem Extras"));
    assert_eq!(page.handle.as_deref(), Some("@canalsemextras"));
    assert_eq!(
        page.canonical_url,
        "https://www.youtube.com/@canalsemextras"
    );
    assert!(page.description.is_none());
    assert!(page.subscriber_count.is_none());
    assert!(page.avatar_url.is_none());
    assert!(page.banner_url.is_none());
    assert!(page.verified.is_none());
    assert!(page.videos.is_empty());

    let jsonl = br#"{"id":"dQw4w9WgXcQ","ie_key":"Youtube","title":"Video 1","playlist_channel":"Canal JSONL","playlist_channel_id":"UC8butISFwT-Wl7EV0hUK0BQ","playlist_uploader_id":"@canaljsonl"}
{"id":"abc_123-XYZ","ie_key":"Youtube","title":"Video 2"}"#;
    let jsonl_page = parse_channel_page(jsonl, &requested, 2).unwrap();
    assert_eq!(
        jsonl_page.channel_id.as_deref(),
        Some("UC8butISFwT-Wl7EV0hUK0BQ")
    );
    assert_eq!(jsonl_page.title.as_deref(), Some("Canal JSONL"));
    assert_eq!(jsonl_page.handle.as_deref(), Some("@canaljsonl"));
    assert_eq!(jsonl_page.videos.len(), 2);
    for video in &jsonl_page.videos {
        assert_eq!(video.video.channel, "Canal JSONL");
        assert_eq!(
            video.metadata.channel_id.as_deref(),
            Some("UC8butISFwT-Wl7EV0hUK0BQ")
        );
        assert_eq!(
            video.metadata.channel_url.as_deref(),
            Some("https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ")
        );
    }
    let jsonl_limited = parse_channel_page(jsonl, &requested, 1).unwrap();
    assert_eq!(jsonl_limited.videos.len(), 1);

    assert!(parse_channel_page(b"   \n  ", &requested, 10).is_err());
    assert!(parse_channel_page(b"{not-json}", &requested, 10).is_err());
}

fn sample_channel_page(handle: &str, title: &str, video_count: usize) -> ChannelPage {
    let target = ChannelTarget::Handle(handle.to_owned());
    let canonical_url = target.canonical_url();
    let videos = (0..video_count)
        .map(|idx| SearchResult {
            video: Video {
                title: format!("{title} #{idx}"),
                channel: title.to_owned(),
                url: format!("https://www.youtube.com/watch?v=video_{idx:05}"),
                thumbnail: String::new(),
                duration: "1:00".to_owned(),
            },
            metadata: VideoMetadata {
                view_count: Some(idx as u64),
                upload_date: None,
                description: None,
                channel_id: None,
                channel_url: Some(canonical_url.clone()),
                ..Default::default()
            },
        })
        .collect();
    ChannelPage {
        target,
        channel_id: None,
        title: Some(title.to_owned()),
        handle: Some(handle.to_owned()),
        canonical_url,
        description: Some(format!("Description for {title}")),
        subscriber_count: Some(1_000),
        view_count: None,
        avatar_url: None,
        banner_url: None,
        verified: Some(true),
        tags: Vec::new(),
        videos,
        ..Default::default()
    }
}

#[test]
fn decode_youtube_redirect_url_extracts_target_query_param() {
    let raw = "https://www.youtube.com/redirect?event=channel_header&redir_token=QUFFLUhqa18&q=http%3A%2F%2Fwww.twitch.tv%2Fthebausffs";
    assert_eq!(
        decode_youtube_redirect_url(raw),
        "http://www.twitch.tv/thebausffs"
    );
    assert_eq!(
        decode_youtube_redirect_url("discord.gg/thebausffs"),
        "https://discord.gg/thebausffs"
    );
}

#[test]
fn parse_yt_lockup_video_parses_channel_grid_video_item() {
    let sample = serde_json::json!({
        "contentType": "LOCKUP_CONTENT_TYPE_VIDEO",
        "contentId": "o582gceJ3uY",
        "contentImage": {
            "thumbnailViewModel": {
                "image": {
                    "sources": [
                        {"url": "https://i.ytimg.com/vi/o582gceJ3uY/hqdefault.jpg", "width": 336, "height": 188}
                    ]
                },
                "overlays": [{
                    "thumbnailOverlayBadgeViewModel": {
                        "thumbnailBadges": [{
                            "thumbnailBadgeViewModel": {
                                "text": "14:39",
                                "badgeStyle": "THUMBNAIL_OVERLAY_BADGE_STYLE_DEFAULT"
                            }
                        }]
                    }
                }]
            }
        },
        "metadata": {
            "lockupMetadataViewModel": {
                "title": {"content": "THE BEST GRAGAS YOU WILL EVER SEE!"},
                "metadata": {
                    "contentMetadataViewModel": {
                        "metadataRows": [{
                            "metadataParts": [
                                {"text": {"content": "314 mil visualizações"}},
                                {"text": {"content": "há 1 ano"}}
                            ]
                        }]
                    }
                }
            }
        }
    });
    let parsed = parse_yt_lockup_video(&sample).expect("should parse lockup video");
    assert_eq!(parsed.video.title, "THE BEST GRAGAS YOU WILL EVER SEE!");
    assert_eq!(
        parsed.video.url,
        "https://www.youtube.com/watch?v=o582gceJ3uY"
    );
    assert_eq!(parsed.video.duration, "14:39");
    assert_eq!(parsed.metadata.view_count, Some(314_000));
    assert_eq!(parsed.metadata.view_count_text.as_deref(), Some("314 mil"));
    assert_eq!(
        parsed.metadata.published_time_text.as_deref(),
        Some("há 1 ano")
    );
}

#[test]
fn channel_cache_hit_returns_clone_without_calling_fetch() {
    use std::cell::Cell;

    let cache = Mutex::new(crate::innertube::ChannelIdCache::default());
    let target = ChannelTarget::Handle("@freecodecamp".to_owned());
    let calls = Cell::new(0usize);

    let first = fetch_with_cache(&cache, "@freecodecamp", &target, false, || {
        calls.set(calls.get() + 1);
        Ok(sample_channel_page("@freecodecamp", "freeCodeCamp", 2))
    })
    .unwrap();
    assert!(!first.from_cache);
    assert_eq!(calls.get(), 1);
    assert_eq!(first.page.title.as_deref(), Some("freeCodeCamp"));

    let second = fetch_with_cache(&cache, "@freecodecamp", &target, false, || {
        calls.set(calls.get() + 1);
        panic!("fetch closure should not be called on cache hit");
    })
    .unwrap();
    assert!(second.from_cache);
    assert_eq!(calls.get(), 1);
    assert_eq!(second.page, first.page);
}

#[test]
fn channel_cache_expired_entry_is_miss_and_triggers_new_fetch() {
    use std::cell::Cell;

    let now = Instant::now();
    let mut direct_cache =
        crate::innertube::ChannelIdCache::new(CHANNEL_PAGE_CACHE_CAPACITY, CHANNEL_PAGE_CACHE_TTL);
    let cid = "UC8butISFwT-Wl7EV0hUK0BQ";
    direct_cache.insert(
        cid,
        &["@freecodecamp", "https://www.youtube.com/@freecodecamp"],
        sample_channel_page("@freecodecamp", "Initial", 1),
        now,
    );
    assert!(
        direct_cache
            .get(
                "@freecodecamp",
                now + CHANNEL_PAGE_CACHE_TTL - Duration::from_secs(1)
            )
            .is_some()
    );
    assert!(
        direct_cache
            .get("@freecodecamp", now + CHANNEL_PAGE_CACHE_TTL)
            .is_none()
    );
    assert_eq!(direct_cache.len(), 0);

    let ttl = Duration::from_millis(20);
    let cache = Mutex::new(crate::innertube::ChannelIdCache::new(4, ttl));
    let target = ChannelTarget::Handle("@freecodecamp".to_owned());
    let calls = Cell::new(0usize);
    let first = fetch_with_cache(&cache, "@freecodecamp", &target, false, || {
        calls.set(calls.get() + 1);
        Ok(sample_channel_page("@freecodecamp", "V1", 1))
    })
    .unwrap();
    assert!(!first.from_cache);
    assert_eq!(calls.get(), 1);

    std::thread::sleep(Duration::from_millis(30));

    let refreshed = fetch_with_cache(&cache, "@freecodecamp", &target, false, || {
        calls.set(calls.get() + 1);
        Ok(sample_channel_page("@freecodecamp", "V2", 2))
    })
    .unwrap();
    assert!(!refreshed.from_cache);
    assert_eq!(calls.get(), 2);
    assert_eq!(refreshed.page.title.as_deref(), Some("V2"));
}

#[test]
fn channel_cache_capacity_lru_evicts_least_recently_used_and_get_refreshes_recency() {
    let base = Instant::now();
    let mut cache = crate::innertube::ChannelIdCache::new(6, CHANNEL_PAGE_CACHE_TTL);

    for idx in 1..=6 {
        let handle = format!("@channel_{idx}");
        let cid = format!("UC123456789012345678901{idx}");
        cache.insert(
            &cid,
            &[&handle],
            sample_channel_page(&handle, &format!("Channel {idx}"), 1),
            base + Duration::from_secs(idx as u64),
        );
    }
    assert_eq!(cache.len(), 6);

    // Touch @channel_1 and @channel_2 so @channel_3 becomes the least recently used.
    assert!(
        cache
            .get("@channel_1", base + Duration::from_secs(10))
            .is_some()
    );
    assert!(
        cache
            .get("@channel_2", base + Duration::from_secs(11))
            .is_some()
    );

    // Insert 7th and 8th entries.
    cache.insert(
        "UC1234567890123456789017",
        &["@channel_7"],
        sample_channel_page("@channel_7", "Channel 7", 1),
        base + Duration::from_secs(12),
    );
    assert_eq!(cache.len(), 6);

    cache.insert(
        "UC1234567890123456789018",
        &["@channel_8"],
        sample_channel_page("@channel_8", "Channel 8", 1),
        base + Duration::from_secs(13),
    );
    assert_eq!(cache.len(), 6);

    let check_time = base + Duration::from_secs(14);
    // @channel_3 and @channel_4 had the oldest recency and must be evicted.
    assert!(cache.get("@channel_3", check_time).is_none());
    assert!(cache.get("@channel_4", check_time).is_none());

    // Refreshed (@channel_1, @channel_2) and newer (@channel_5..8) entries survive.
    for surviving in [1, 2, 5, 6, 7, 8] {
        let handle = format!("@channel_{surviving}");
        assert!(
            cache.get(&handle, check_time).is_some(),
            "expected {handle} to remain in cache"
        );
    }
}

#[test]
fn channel_cache_deduplicates_handle_and_uc_channel_id_aliases() {
    use std::cell::Cell;

    let cache = Mutex::new(crate::innertube::ChannelIdCache::default());
    let calls = Cell::new(0usize);
    let uc_id = "UC8butISFwT-Wl7EV0hUK0BQ";

    let first = fetch_channel_page_cached_with(
        &cache,
        "@freecodecamp",
        20,
        false,
        |target, effective_limit| {
            calls.set(calls.get() + 1);
            assert_eq!(target, &ChannelTarget::Handle("@freecodecamp".to_owned()));
            assert_eq!(effective_limit, 20);
            let mut page = sample_channel_page("@freecodecamp", "freeCodeCamp.org", 20);
            page.channel_id = Some(uc_id.to_owned());
            page.target = ChannelTarget::ChannelId(uc_id.to_owned());
            page.canonical_url = format!("https://www.youtube.com/channel/{uc_id}");
            Ok(page)
        },
    )
    .unwrap();
    assert!(!first.from_cache);
    assert_eq!(calls.get(), 1);
    assert_eq!(cache.lock().unwrap().len(), 1);

    // Lookup by bare UC ID, /channel/UC... URL, and /@handle/videos URL must all hit the single entry.
    for alias in [
        uc_id,
        "https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ",
        "https://www.youtube.com/@freecodecamp/videos",
        "@FreeCodeCamp",
    ] {
        let hit = fetch_channel_page_cached_with(&cache, alias, 20, false, |_, _| {
            panic!("expected cache hit for alias {alias}");
        })
        .unwrap();
        assert!(hit.from_cache);
        assert_eq!(hit.page.channel_id.as_deref(), Some(uc_id));
        assert_eq!(hit.page.title.as_deref(), Some("freeCodeCamp.org"));
    }
    assert_eq!(cache.lock().unwrap().len(), 1);
}

#[test]
fn channel_cache_force_refresh_bypasses_fresh_entry_and_stores_new_page() {
    use std::cell::Cell;

    let cache = Mutex::new(crate::innertube::ChannelIdCache::default());
    let target = ChannelTarget::Handle("@freecodecamp".to_owned());
    let calls = Cell::new(0usize);

    let initial = fetch_with_cache(&cache, "@freecodecamp", &target, false, || {
        calls.set(calls.get() + 1);
        Ok(sample_channel_page("@freecodecamp", "Old Title", 1))
    })
    .unwrap();
    assert!(!initial.from_cache);
    assert_eq!(initial.page.title.as_deref(), Some("Old Title"));

    let forced = fetch_with_cache(&cache, "@freecodecamp", &target, true, || {
        calls.set(calls.get() + 1);
        Ok(sample_channel_page("@freecodecamp", "Refreshed Title", 3))
    })
    .unwrap();
    assert!(!forced.from_cache);
    assert_eq!(calls.get(), 2);
    assert_eq!(forced.page.title.as_deref(), Some("Refreshed Title"));
    assert_eq!(forced.page.videos.len(), 3);
    assert_eq!(cache.lock().unwrap().len(), 1);

    let subsequent_hit = fetch_with_cache(&cache, "@freecodecamp", &target, false, || {
        panic!("should hit updated cache entry")
    })
    .unwrap();
    assert!(subsequent_hit.from_cache);
    assert_eq!(
        subsequent_hit.page.title.as_deref(),
        Some("Refreshed Title")
    );
    assert_eq!(subsequent_hit.page.videos.len(), 3);
}

#[test]
fn channel_cache_does_not_store_errors() {
    use std::cell::Cell;

    let cache = Mutex::new(crate::innertube::ChannelIdCache::default());
    let target = ChannelTarget::Handle("@freecodecamp".to_owned());
    let calls = Cell::new(0usize);

    let err = fetch_with_cache(&cache, "@freecodecamp", &target, false, || {
        calls.set(calls.get() + 1);
        Err("yt-dlp failed".to_owned())
    });
    assert_eq!(err.unwrap_err(), "yt-dlp failed");
    assert_eq!(calls.get(), 1);
    assert_eq!(cache.lock().unwrap().len(), 0);

    let recovered = fetch_with_cache(&cache, "@freecodecamp", &target, false, || {
        calls.set(calls.get() + 1);
        Ok(sample_channel_page("@freecodecamp", "Recovered", 2))
    })
    .unwrap();
    assert!(!recovered.from_cache);
    assert_eq!(calls.get(), 2);
    assert_eq!(recovered.page.title.as_deref(), Some("Recovered"));
    assert_eq!(cache.lock().unwrap().len(), 1);
}

#[test]
fn channel_cache_rejects_invalid_input_before_cache_or_fetch() {
    use std::cell::Cell;

    let cache = Mutex::new(crate::innertube::ChannelIdCache::default());
    let calls = Cell::new(0usize);

    let err = fetch_channel_page_cached_with(
        &cache,
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        20,
        false,
        |_, _| {
            calls.set(calls.get() + 1);
            Ok(sample_channel_page("@freecodecamp", "Never", 1))
        },
    )
    .unwrap_err();
    assert_eq!(
        err,
        "Identificador ou URL de canal do YouTube inválido ou não suportado."
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(cache.lock().unwrap().len(), 0);

    let public_err = fetch_channel_page_with_cache("invalid-channel", 20, false).unwrap_err();
    assert_eq!(
        public_err,
        "Identificador ou URL de canal do YouTube inválido ou não suportado."
    );
}

#[test]
fn parses_channel_videos_batch_and_enforces_pagination_limits() {
    let payload = br#"{
            "_type": "playlist",
            "id": "UC8butISFwT-Wl7EV0hUK0BQ",
            "channel": "freeCodeCamp.org",
            "channel_id": "UC8butISFwT-Wl7EV0hUK0BQ",
            "entries": [
                {"id": "dQw4w9WgXcQ", "ie_key": "Youtube", "title": "Batch Video 1", "duration": 60},
                {"id": "0123456789A", "ie_key": "Youtube", "title": "Batch Video 2", "duration": 120}
            ]
        }"#;
    let target = ChannelTarget::ChannelId("UC8butISFwT-Wl7EV0hUK0BQ".to_owned());

    let batch = parse_channel_videos_batch(payload, &target, 21, 22, 2).unwrap();
    assert_eq!(batch.videos.len(), 2);
    assert_eq!(batch.start, 21);
    assert_eq!(batch.end, 22);
    assert!(batch.has_more);
    assert!(!batch.reached_max_limit);

    let partial = parse_channel_videos_batch(payload, &target, 21, 40, 20).unwrap();
    assert_eq!(partial.videos.len(), 2);
    assert!(!partial.has_more);
    assert!(!partial.reached_max_limit);

    let capped = parse_channel_videos_batch(payload, &target, 99, 100, 2).unwrap();
    assert_eq!(capped.videos.len(), 2);
    assert!(!capped.has_more);
    assert!(capped.reached_max_limit);

    let cancel = Cancellation::default();
    assert!(fetch_channel_videos_batch_with_cancel("@freecodecamp", 0, 20, &cancel).is_err());
    assert!(
        fetch_channel_videos_batch_with_cancel(
            "@freecodecamp",
            MAX_CHANNEL_VIDEOS_LIMIT + 1,
            20,
            &cancel
        )
        .is_err()
    );
    assert!(fetch_channel_videos_batch_with_cancel("invalid-channel", 21, 20, &cancel).is_err());
}

#[test]
fn parses_public_channel_playlists_and_playlist_videos_safely() {
    let playlists_payload = br#"{
            "_type": "playlist",
            "title": "freeCodeCamp.org - Playlists",
            "entries": [
                {
                    "_type": "url",
                    "ie_key": "YoutubeTab",
                    "id": "PLWKjhJtqVAbnupFp3_281",
                    "title": "Python Tutorials",
                    "url": "https://www.youtube.com/playlist?list=PLWKjhJtqVAbnupFp3_281",
                    "playlist_count": 15,
                    "description": "Learn Python",
                    "thumbnails": [{"url": "https://i.ytimg.com/vi/abc/hqdefault.jpg", "width": 320, "height": 180}]
                },
                {
                    "_type": "url",
                    "ie_key": "YoutubeTab",
                    "id": "PLWKjhJtqVAbnupFp3_281",
                    "title": "Duplicate Playlist"
                },
                {
                    "_type": "url",
                    "id": "bad&id=1",
                    "url": "https://evil.example/playlist?list=bad"
                }
            ]
        }"#;
    let playlists = parse_channel_playlists(playlists_payload, 30).unwrap();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].id, "PLWKjhJtqVAbnupFp3_281");
    assert_eq!(playlists[0].title, "Python Tutorials");
    assert_eq!(
        playlists[0].url,
        "https://www.youtube.com/playlist?list=PLWKjhJtqVAbnupFp3_281"
    );
    assert_eq!(playlists[0].item_count, Some(15));
    assert_eq!(playlists[0].description.as_deref(), Some("Learn Python"));

    let playlist_videos_payload = br#"{
            "_type": "playlist",
            "id": "PLWKjhJtqVAbnupFp3_281",
            "title": "Python Tutorials",
            "channel": "freeCodeCamp.org",
            "channel_id": "UC8butISFwT-Wl7EV0hUK0BQ",
            "entries": [
                {"id": "dQw4w9WgXcQ", "ie_key": "Youtube", "title": "Lesson 1", "duration": 300},
                {"id": "bad_id", "ie_key": "Youtube", "title": "Invalid ID"},
                {"id": "0123456789A", "ie_key": "Youtube", "title": "Lesson 2", "duration": 450}
            ]
        }"#;
    let videos = parse_public_playlist_videos(playlist_videos_payload, 50).unwrap();
    assert_eq!(videos.len(), 2);
    assert_eq!(
        videos[0].video.url,
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
    );
    assert_eq!(videos[0].video.channel, "freeCodeCamp.org");
    assert_eq!(
        videos[0].metadata.channel_id.as_deref(),
        Some("UC8butISFwT-Wl7EV0hUK0BQ")
    );
    assert_eq!(
        videos[1].video.url,
        "https://www.youtube.com/watch?v=0123456789A"
    );

    assert!(youtube_playlist_url("PLWKjhJtqVAbnupFp3_281").is_some());
    assert!(youtube_playlist_url("bad&injection").is_none());
}

#[test]
fn parses_channel_about_info_and_truncates_oversized_fields() {
    let long_desc = "A".repeat(MAX_CHANNEL_DESCRIPTION_CHARS + 500);
    let payload = serde_json::json!({
        "id": "UC8butISFwT-Wl7EV0hUK0BQ",
        "channel": "freeCodeCamp.org",
        "channel_id": "UC8butISFwT-Wl7EV0hUK0BQ",
        "title": "freeCodeCamp.org - About",
        "uploader_id": "@freecodecamp",
        "description": long_desc,
        "channel_follower_count": 10500000u64,
        "view_count": 850000000u64,
        "channel_is_verified": true,
        "tags": ["programming", "rust", "programming", "web"],
        "thumbnails": [
            {"url": "https://yt3.googleusercontent.com/avatar=s900", "width": 900, "height": 900, "id": "avatar"}
        ]
    });
    let target = ChannelTarget::Handle("@freecodecamp".to_owned());
    let about = parse_channel_about(&serde_json::to_vec(&payload).unwrap(), &target).unwrap();
    assert_eq!(
        about.channel_id.as_deref(),
        Some("UC8butISFwT-Wl7EV0hUK0BQ")
    );
    assert_eq!(about.title.as_deref(), Some("freeCodeCamp.org"));
    assert_eq!(about.handle.as_deref(), Some("@freecodecamp"));
    assert_eq!(
        about.canonical_url.as_deref(),
        Some("https://www.youtube.com/channel/UC8butISFwT-Wl7EV0hUK0BQ")
    );
    assert_eq!(
        about.description.as_ref().map(|d| d.chars().count()),
        Some(MAX_CHANNEL_DESCRIPTION_CHARS)
    );
    assert_eq!(about.subscriber_count, Some(10_500_000));
    assert_eq!(about.view_count, Some(850_000_000));
    assert_eq!(about.verified, Some(true));
    assert_eq!(about.tags, vec!["programming", "rust", "web"]);
    assert_eq!(
        about.avatar_url.as_deref(),
        Some("https://yt3.googleusercontent.com/avatar=s900")
    );
}

#[test]
fn channel_target_exposes_shorts_and_streams_tab_urls() {
    let target = ChannelTarget::Handle("@rustlang".to_owned());
    assert_eq!(
        target.shorts_tab_url(),
        "https://www.youtube.com/@rustlang/shorts"
    );
    assert_eq!(
        target.streams_tab_url(),
        "https://www.youtube.com/@rustlang/streams"
    );
}

#[test]
fn search_filters_filter_by_type_duration_and_sort_by_views() {
    let mk = |title: &str, url: &str, dur: &str, views: Option<u64>| SearchResult {
        video: Video {
            title: title.into(),
            channel: "Canal".into(),
            url: url.into(),
            thumbnail: String::new(),
            duration: dur.into(),
        },
        metadata: VideoMetadata {
            view_count: views,
            ..Default::default()
        },
    };
    let items = vec![
        mk(
            "Short",
            "https://www.youtube.com/shorts/aaa",
            "0:45",
            Some(100),
        ),
        mk(
            "Medium A",
            "https://www.youtube.com/watch?v=bbb",
            "8:10",
            Some(500),
        ),
        mk(
            "Medium B",
            "https://www.youtube.com/watch?v=ccc",
            "12:00",
            Some(1200),
        ),
        mk(
            "Long",
            "https://www.youtube.com/watch?v=ddd",
            "1:05:00",
            Some(3000),
        ),
        mk("Live", "https://www.youtube.com/watch?v=eee", "", Some(900)),
    ];

    assert_eq!(parse_duration_label_secs("8:10"), Some(490));
    assert_eq!(parse_duration_label_secs("1:05:00"), Some(3900));
    assert_eq!(parse_duration_label_secs(""), None);

    let shorts = apply_search_filters(
        items.clone(),
        SearchFilters {
            kind: SearchTypeFilter::Shorts,
            ..Default::default()
        },
    );
    assert_eq!(shorts.len(), 1);
    assert_eq!(shorts[0].video.title, "Short");

    let live = apply_search_filters(
        items.clone(),
        SearchFilters {
            kind: SearchTypeFilter::Live,
            ..Default::default()
        },
    );
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].video.title, "Live");

    let medium_by_views = apply_search_filters(
        items,
        SearchFilters {
            sort: SearchSort::Views,
            duration: crate::innertube::SearchDuration::Between4And20Min,
            result_type: crate::innertube::SearchResultType::Video,
            ..Default::default()
        },
    );
    assert_eq!(medium_by_views.len(), 2);
    assert_eq!(medium_by_views[0].video.title, "Medium B");
    assert_eq!(medium_by_views[1].video.title, "Medium A");
}

#[test]
fn encode_search_sp_matches_official_youtube_tokens() {
    assert_eq!(encode_search_sp(SearchFilters::default()), None);
    assert_eq!(
        encode_search_sp(SearchFilters {
            sort: SearchSort::Views,
            ..Default::default()
        })
        .as_deref(),
        Some("CAM=")
    );
    assert_eq!(
        encode_search_sp(SearchFilters {
            sort: SearchSort::Date,
            ..Default::default()
        })
        .as_deref(),
        Some("CAI=")
    );
    assert_eq!(
        encode_search_sp(SearchFilters {
            kind: SearchTypeFilter::Shorts,
            ..Default::default()
        })
        .as_deref(),
        Some("EgIQCQ==")
    );
    assert_eq!(
        encode_search_sp(SearchFilters {
            result_type: crate::innertube::SearchResultType::Channel,
            ..Default::default()
        })
        .as_deref(),
        Some("EgIQAg==")
    );
    assert_eq!(
        encode_search_sp(SearchFilters {
            result_type: crate::innertube::SearchResultType::Playlist,
            ..Default::default()
        })
        .as_deref(),
        Some("EgIQAw==")
    );
    assert_eq!(
        encode_search_sp(SearchFilters {
            duration: crate::innertube::SearchDuration::Under4Min,
            ..Default::default()
        })
        .as_deref(),
        Some("EgIYAQ==")
    );
    assert_eq!(
        encode_search_sp(SearchFilters {
            upload_date: crate::innertube::SearchUploadDate::Today,
            ..Default::default()
        })
        .as_deref(),
        Some("EgIIAg==")
    );
    assert_eq!(
        encode_search_sp(SearchFilters {
            features: vec![crate::innertube::SearchFeature::Live],
            ..Default::default()
        })
        .as_deref(),
        Some("EgJAAQ==")
    );
    assert_eq!(
        encode_search_sp(SearchFilters {
            sort: SearchSort::Views,
            result_type: crate::innertube::SearchResultType::Video,
            ..Default::default()
        })
        .as_deref(),
        Some("CAMSAhAB")
    );
}

#[test]
fn search_sort_date_and_multiselect_filters_convert_to_innertube_filters() {
    use crate::i18n::UiLanguage;
    use crate::innertube::{
        SearchDuration, SearchFeature, SearchResultType, SearchSortBy, SearchUploadDate,
    };

    assert_eq!(SearchSort::Date.label(UiLanguage::PtBr), "Data de envio");
    assert_eq!(SearchSort::Date.label(UiLanguage::En), "Upload date");
    assert_eq!(SearchSort::Rating.label(UiLanguage::PtBr), "Avaliação");
    assert_eq!(SearchSort::Rating.label(UiLanguage::En), "Rating");

    let mut filters = SearchFilters {
        sort: SearchSort::Date,
        upload_date: SearchUploadDate::ThisWeek,
        result_type: SearchResultType::Video,
        duration: SearchDuration::Between4And20Min,
        features: vec![SearchFeature::FourK, SearchFeature::Subtitles],
        kind: SearchTypeFilter::All,
    };
    assert!(!filters.is_default());
    assert!(filters.has_feature(SearchFeature::FourK));
    assert!(filters.has_feature(SearchFeature::Subtitles));
    filters.toggle_feature(SearchFeature::Hdr);
    assert!(filters.has_feature(SearchFeature::Hdr));

    let innertube_filters = filters.to_innertube();
    assert_eq!(innertube_filters.sort_by, SearchSortBy::UploadDate);
    assert_eq!(innertube_filters.upload_date, SearchUploadDate::ThisWeek);
    assert_eq!(innertube_filters.result_type, SearchResultType::Video);
    assert_eq!(innertube_filters.duration, SearchDuration::Between4And20Min);
    assert_eq!(
        innertube_filters.features,
        vec![
            SearchFeature::FourK,
            SearchFeature::Subtitles,
            SearchFeature::Hdr,
        ]
    );

    let round_tripped = SearchFilters::from_innertube(&innertube_filters);
    assert_eq!(round_tripped.sort, SearchSort::Date);
    assert_eq!(round_tripped.upload_date, SearchUploadDate::ThisWeek);
    assert_eq!(round_tripped.result_type, SearchResultType::Video);
    assert_eq!(round_tripped.duration, SearchDuration::Between4And20Min);
    assert_eq!(round_tripped.features, innertube_filters.features);
}

#[test]
fn channel_page_from_innertube_preserves_dynamic_tab_endpoints_sort_chips_and_continuation() {
    use crate::innertube::{
        ChannelSortChip, ChannelTabEndpoint, ChannelTabKind, InnerTubeChannelPage,
    };

    let items: Vec<Video> = (0..120)
        .map(|idx| Video {
            title: format!("Video #{idx}"),
            channel: "Test Channel".into(),
            url: format!("https://www.youtube.com/watch?v=vid_{idx:07}"),
            thumbnail: String::new(),
            duration: "5:00".into(),
        })
        .collect();
    let metadata: Vec<VideoMetadata> = (0..120).map(|_| VideoMetadata::default()).collect();
    let tabs = vec![
        ChannelTabEndpoint {
            title: "Videos".into(),
            browse_id: "UC8butISFwT-Wl7EV0hUK0BQ".into(),
            params: Some("EgZ2aWRlb3PyBgQKAjoA".into()),
            selected: true,
            kind: ChannelTabKind::Videos,
        },
        ChannelTabEndpoint {
            title: "Shorts".into(),
            browse_id: "UC8butISFwT-Wl7EV0hUK0BQ".into(),
            params: Some("EgZzaG9ydHPyBgUKA5oBAA%3D%3D".into()),
            selected: false,
            kind: ChannelTabKind::Shorts,
        },
        ChannelTabEndpoint {
            title: "Releases".into(),
            browse_id: "UC8butISFwT-Wl7EV0hUK0BQ".into(),
            params: Some("Eg releases".into()),
            selected: false,
            kind: ChannelTabKind::Releases,
        },
    ];
    let sort_chips = vec![
        ChannelSortChip {
            title: "Latest".into(),
            continuation: "cont_latest".into(),
            selected: true,
        },
        ChannelSortChip {
            title: "Popular".into(),
            continuation: "cont_popular".into(),
            selected: false,
        },
        ChannelSortChip {
            title: "Oldest".into(),
            continuation: "cont_oldest".into(),
            selected: false,
        },
    ];
    let it_page = InnerTubeChannelPage {
        channel_id: Some("UC8butISFwT-Wl7EV0hUK0BQ".into()),
        title: Some("freeCodeCamp.org".into()),
        handle: Some("@freecodecamp".into()),
        subscriber_count_text: Some("10M subscribers".into()),
        video_count_text: Some("1.8K videos".into()),
        avatar_url: Some("https://yt3.ggpht.com/avatar=s176".into()),
        banner_url: Some("https://yt3.ggpht.com/banner=w1707".into()),
        verified: true,
        official_artist: false,
        tabs: tabs.clone(),
        sort_chips: sort_chips.clone(),
        items,
        metadata,
        continuation: Some("next_channel_page_token_120".into()),
        community_posts: Vec::new(),
    };

    let target = ChannelTarget::Handle("@freecodecamp".into());
    let page = channel_page_from_innertube(it_page, &target);

    assert_eq!(page.channel_id.as_deref(), Some("UC8butISFwT-Wl7EV0hUK0BQ"));
    assert_eq!(page.handle.as_deref(), Some("@freecodecamp"));
    // Verify no 100-video cap is applied on InnerTube channel browse responses.
    assert_eq!(page.videos.len(), 120);
    assert_eq!(page.tab_endpoints, tabs);
    assert_eq!(page.sort_chips, sort_chips);
    assert_eq!(
        page.next_continuation.as_deref(),
        Some("next_channel_page_token_120")
    );
    assert!(page.has_shorts_tab);
    assert_eq!(
        page.videos_popular_continuation.as_deref(),
        Some("cont_popular")
    );
    assert_eq!(
        page.videos_oldest_continuation.as_deref(),
        Some("cont_oldest")
    );
}

#[test]
fn parse_yt_initial_data_extracts_live_video_channel_playlist_and_short() {
    let payload = serde_json::json!({
        "contents": {
            "twoColumnSearchResultsRenderer": {
                "primaryContents": {
                    "sectionListRenderer": {
                        "contents": [{
                            "itemSectionRenderer": {
                                "contents": [
                                    {
                                        "videoRenderer": {
                                            "videoId": "live1234567",
                                            "title": { "runs": [{ "text": "GEN 1x1 BLG | Worlds 2026" }] },
                                            "viewCountText": { "runs": [{ "text": "64.100" }, { "text": " assistindo" }] },
                                            "shortViewCountText": { "runs": [{ "text": "64\u{00a0}mil" }, { "text": " assistindo" }] },
                                            "ownerText": {
                                                "runs": [{
                                                    "text": "CBLOL",
                                                    "navigationEndpoint": {
                                                        "browseEndpoint": {
                                                            "browseId": "UC48rkW4j4z7N8h3r1bB9wXA",
                                                            "canonicalBaseUrl": "/@CBLOL"
                                                        }
                                                    }
                                                }]
                                            },
                                            "ownerBadges": [{
                                                "metadataBadgeRenderer": {
                                                    "style": "BADGE_STYLE_TYPE_VERIFIED",
                                                    "tooltip": "Verificado"
                                                }
                                            }],
                                            "badges": [
                                                { "metadataBadgeRenderer": { "style": "BADGE_STYLE_TYPE_LIVE_NOW", "label": "AO VIVO" } },
                                                { "metadataBadgeRenderer": { "style": "BADGE_STYLE_TYPE_SIMPLE", "label": "Novo" } }
                                            ],
                                            "detailedMetadataSnippets": [{
                                                "snippetText": { "runs": [{ "text": "Final do Worlds 2026 ao vivo." }] }
                                            }],
                                            "thumbnail": { "thumbnails": [{ "url": "https://i.ytimg.com/vi/live1234567/hq720.jpg", "width": 720 }] },
                                            "channelThumbnailSupportedRenderers": {
                                                "channelThumbnailWithLinkRenderer": {
                                                    "thumbnail": { "thumbnails": [{ "url": "//yt3.ggpht.com/cblol=s68", "width": 68 }] },
                                                    "navigationEndpoint": {
                                                        "browseEndpoint": {
                                                            "browseId": "UC48rkW4j4z7N8h3r1bB9wXA",
                                                            "canonicalBaseUrl": "/@CBLOL"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    {
                                        "channelRenderer": {
                                            "channelId": "UCA-UimI-hyK_YVWNgKJsJ_w",
                                            "title": { "simpleText": "IOM Worlds 2026" },
                                            "thumbnail": { "thumbnails": [{ "url": "//yt3.ggpht.com/iom=s176", "width": 176 }] },
                                            "subscriberCountText": { "simpleText": "@IOMWorlds2026" },
                                            "videoCountText": { "simpleText": "989 inscritos" }
                                        }
                                    },
                                    {
                                        "lockupViewModel": {
                                            "contentId": "PLuiPiC1Hk8eLgTHu-INIEjm12_czwvMq4",
                                            "contentType": "LOCKUP_CONTENT_TYPE_PLAYLIST",
                                            "metadata": {
                                                "lockupMetadataViewModel": {
                                                    "title": { "content": "Worlds 2026 Jogos" },
                                                    "metadata": {
                                                        "contentMetadataViewModel": {
                                                            "metadataRows": [{
                                                                "metadataParts": [{
                                                                    "text": { "content": "Ilha das Lendas" }
                                                                }]
                                                            }]
                                                        }
                                                    }
                                                }
                                            },
                                            "contentImage": {
                                                "collectionThumbnailViewModel": {
                                                    "primaryThumbnail": {
                                                        "thumbnailViewModel": {
                                                            "image": { "sources": [{ "url": "https://i.ytimg.com/vi/abc/hq720.jpg", "width": 720 }] },
                                                            "overlays": [{
                                                                "thumbnailOverlayBadgeViewModel": {
                                                                    "thumbnailBadges": [{
                                                                        "thumbnailBadgeViewModel": { "text": "91 v\u{00ed}deos" }
                                                                    }]
                                                                }
                                                            }]
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    {
                                        "shortsLockupViewModel": {
                                            "entityId": "shorts-shelf-item-GtxjVvdMG90",
                                            "overlayMetadata": {
                                                "primaryText": { "content": "Worlds 2026 Short" },
                                                "secondaryText": { "content": "4\u{00a0}mil visualiza\u{00e7}\u{00f5}es" }
                                            }
                                        }
                                    }
                                ]
                            }
                        }]
                    }
                }
            }
        }
    });
    let items = parse_yt_initial_data_search_results(&payload);
    assert_eq!(items.len(), 4);

    assert_eq!(items[0].metadata.kind, SearchResultKind::Video);
    assert!(items[0].metadata.is_live);
    assert!(items[0].metadata.channel_verified);
    assert_eq!(
        items[0].metadata.view_count_text.as_deref(),
        Some("64 mil assistindo")
    );
    assert_eq!(items[0].metadata.badges, vec!["Novo"]);
    assert_eq!(
        items[0].metadata.channel_avatar_url.as_deref(),
        Some("https://yt3.ggpht.com/cblol=s68")
    );

    assert_eq!(items[1].metadata.kind, SearchResultKind::Channel);
    assert_eq!(
        items[1].metadata.subscriber_count_text.as_deref(),
        Some("@IOMWorlds2026 \u{2022} 989 inscritos")
    );

    assert_eq!(items[2].metadata.kind, SearchResultKind::Playlist);
    assert_eq!(
        items[2].metadata.playlist_count.as_deref(),
        Some("91 v\u{00ed}deos")
    );

    assert_eq!(items[3].metadata.kind, SearchResultKind::Short);
    assert_eq!(
        items[3].video.url,
        "https://www.youtube.com/shorts/GtxjVvdMG90"
    );
}

#[test]
fn subtitle_track_cache_and_resolutions_json_parsing_work_together() {
    let json = serde_json::json!({
        "formats": [
            { "vcodec": "none", "height": 0 },
            { "vcodec": "avc1", "height": 720 },
            { "vcodec": "vp9", "height": 1080 },
            { "vcodec": "av01", "height": 1080 },
            { "vcodec": "avc1", "height": 480 }
        ],
        "subtitles": {
            "pt": [{ "ext": "vtt", "url": "https://www.youtube.com/api/timedtext?v=subcache123&lang=pt&fmt=vtt" }]
        },
        "automatic_captions": {
            "en": [{ "ext": "vtt", "url": "https://www.youtube.com/api/timedtext?v=subcache123&lang=en&fmt=vtt" }]
        }
    });

    let heights = parse_resolutions_from_json(&json).unwrap();
    assert_eq!(heights, vec![1080, 720, 480]);

    let tracks = parse_subtitles(&json);
    assert_eq!(tracks.len(), 2);

    let watch_url = "https://www.youtube.com/watch?v=subcache123";
    let short_url = "https://youtu.be/subcache123?t=10";
    store_cached_subtitle_tracks(watch_url, &tracks);
    assert_eq!(get_cached_subtitle_tracks(short_url), Some(tracks));
    assert_eq!(
        get_cached_subtitle_tracks("https://example.com/not-yt"),
        None
    );
}

#[test]
fn parse_youtube_suggest_json_deduplicates_and_validates_shape() {
    let raw = r#"["rust",["rust tutorial","  Rust Tutorial ","rust egui","","cargo clippy"]]"#;
    let parsed = parse_youtube_suggest_json(raw).unwrap();
    assert_eq!(parsed, vec!["rust tutorial", "rust egui", "cargo clippy"]);

    assert!(parse_youtube_suggest_json("{}").is_err());
    assert!(parse_youtube_suggest_json("[\"rust\"]").is_err());
}

#[test]
fn merge_search_suggestions_prioritizes_recent_prefix_and_deduplicates_remote() {
    let recent = vec![
        "aprender rust".to_owned(),
        "rust egui".to_owned(),
        "mpv player".to_owned(),
    ];
    let remote = vec![
        "Rust Egui".to_owned(),
        "rust async tokio".to_owned(),
        "rust lang brasil".to_owned(),
    ];
    let merged = merge_search_suggestions("rust", &recent, &remote, 5);
    assert_eq!(
        merged,
        vec![
            SearchSuggestionItem {
                text: "rust egui".into(),
                is_recent: true,
            },
            SearchSuggestionItem {
                text: "aprender rust".into(),
                is_recent: true,
            },
            SearchSuggestionItem {
                text: "rust async tokio".into(),
                is_recent: false,
            },
            SearchSuggestionItem {
                text: "rust lang brasil".into(),
                is_recent: false,
            },
        ]
    );
}

#[test]
fn fetch_search_suggestions_respects_cancellation_and_parses_local_http_response() {
    let cancelled = Cancellation::default();
    cancelled.cancel();
    assert!(fetch_search_suggestions_at("http://127.0.0.1:9", "rust", &cancelled).is_err());

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        use std::io::{Read as _, Write as _};
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let body = r#"["rust egui",["rust egui tutorial","rust egui table"]]"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let endpoint = format!("http://{addr}/complete/search");
    let cancel = Cancellation::default();
    let results = fetch_search_suggestions_at(&endpoint, "rust egui", &cancel).unwrap();
    assert_eq!(results, vec!["rust egui tutorial", "rust egui table"]);
    let _ = handle.join();
}

#[test]
fn ytdl_raw_options_formats_no_playlist_and_safe_cache_dir() {
    assert_eq!(format_ytdl_hook_raw_options(None), "no-playlist=");
    assert_eq!(
        format_ytdl_hook_raw_options(Some(std::path::Path::new(r"C:\FastTube\cache\yt-dlp"))),
        r"no-playlist=,cache-dir=C:\FastTube\cache\yt-dlp"
    );
    assert_eq!(
        format_ytdl_hook_raw_options(Some(std::path::Path::new(r"C:\Bad,Dir=1\yt-dlp"))),
        "no-playlist="
    );
}

#[test]
fn prime_and_read_cached_playback_metadata_round_trip() {
    let url = "https://www.youtube.com/watch?v=CachePrim11";
    let short_url = "https://youtu.be/CachePrim11";
    let sub = SubtitleTrack {
        language: "pt-BR".into(),
        automatic: false,
        url: "https://www.youtube.com/api/timedtext?v=CachePrim11&lang=pt-BR&fmt=vtt".into(),
    };
    let sb = StoryboardSpec {
        rows: 5,
        columns: 5,
        thumb_width: 160,
        thumb_height: 90,
        fragments: vec![StoryboardFragment {
            url: "https://i.ytimg.com/sb/CachePrim11/storyboard3_L2/M0.jpg?sigh=abc".into(),
            duration_secs: 50.0,
        }],
    };
    prime_playback_metadata_cache(
        url,
        &[1080, 720, 480],
        std::slice::from_ref(&sub),
        true,
        PlaybackExtraInfo {
            is_live: false,
            title: Some("Primed Title".into()),
            channel: Some("Primed Channel".into()),
            channel_id: Some("UC1234567890123456789012".into()),
            channel_url: Some("https://www.youtube.com/channel/UC1234567890123456789012".into()),
            description: Some("Desc".into()),
            concurrent_view_count: None,
            storyboard: Some(sb.clone()),
        },
    );
    let cached = cached_playback_metadata(short_url).expect("primed metadata should hit");
    assert_eq!(cached.heights, vec![1080, 720, 480]);
    assert_eq!(cached.subtitles, vec![sub]);
    assert_eq!(cached.title.as_deref(), Some("Primed Title"));
    assert_eq!(cached.storyboard, Some(sb));
}

#[test]
fn subtitle_parser_prefers_native_over_tlang_within_same_language_and_deduplicates_orig() {
    let en_native = "https://www.youtube.com/api/timedtext?v=TedrETAJeNM&kind=asr&lang=en&fmt=vtt";
    let pt_native = "https://www.youtube.com/api/timedtext?v=TedrETAJeNM&kind=asr&lang=pt&fmt=vtt";
    let value = serde_json::json!({
        "subtitles": {},
        "automatic_captions": {
            "en-orig": [
                { "ext": "vtt", "url": en_native }
            ],
            "en": [
                { "ext": "vtt", "url": en_native },
                { "ext": "vtt", "url": "https://www.youtube.com/api/timedtext?v=TedrETAJeNM&kind=asr&lang=pt&tlang=en&fmt=vtt" }
            ],
            "pt": [
                { "ext": "vtt", "url": "https://www.youtube.com/api/timedtext?v=TedrETAJeNM&kind=asr&lang=en&tlang=pt&fmt=vtt" },
                { "ext": "vtt", "url": pt_native }
            ],
            "pt-orig": [
                { "ext": "vtt", "url": pt_native }
            ],
            "es": [
                { "ext": "vtt", "url": "https://www.youtube.com/api/timedtext?v=TedrETAJeNM&kind=asr&lang=en&tlang=es&fmt=vtt" }
            ]
        }
    });

    let tracks = parse_subtitles(&value);
    assert_eq!(
        tracks,
        vec![
            SubtitleTrack {
                language: "en".into(),
                automatic: true,
                url: en_native.into(),
            },
            SubtitleTrack {
                language: "pt".into(),
                automatic: true,
                url: pt_native.into(),
            },
        ]
    );
}

#[test]
fn subtitle_parser_deduplicates_single_language_orig_alias_and_manual_hls_mirror() {
    let single_pt_url =
        "https://www.youtube.com/api/timedtext?v=Single00001&kind=asr&lang=pt&fmt=vtt";
    let single_lang_json = serde_json::json!({
        "subtitles": {},
        "automatic_captions": {
            "pt": [{ "ext": "vtt", "url": single_pt_url }],
            "pt-orig": [{ "ext": "vtt", "url": single_pt_url }]
        }
    });
    let single_tracks = parse_subtitles(&single_lang_json);
    assert_eq!(
        single_tracks,
        vec![SubtitleTrack {
            language: "pt".into(),
            automatic: true,
            url: single_pt_url.into(),
        }]
    );

    let manual_pt_br = "https://www.youtube.com/api/timedtext?v=Mirror00001&lang=pt-BR&fmt=vtt";
    let mirror_hls = "https://manifest.googlevideo.com/api/manifest/hls_timedtext_playlist/tts_params/caps%3Dasr%26lang%3Dpt-BR/playlist/index.m3u8";
    let mirror_json = serde_json::json!({
        "subtitles": {
            "pt-BR": [{ "ext": "vtt", "url": manual_pt_br }]
        },
        "automatic_captions": {
            "pt-BR": [
                { "ext": "vtt", "protocol": "m3u8_native", "url": mirror_hls }
            ]
        }
    });
    let mirror_tracks = parse_subtitles(&mirror_json);
    assert_eq!(
        mirror_tracks,
        vec![SubtitleTrack {
            language: "pt-BR".into(),
            automatic: false,
            url: manual_pt_br.into(),
        }]
    );
    let registered_fallback = subtitle_hls_fallbacks()
        .lock()
        .unwrap()
        .get(manual_pt_br)
        .cloned();
    assert_eq!(registered_fallback.as_deref(), Some(mirror_hls));
}

#[test]
fn subtitle_parser_prefers_native_hls_over_translated_direct() {
    let native_hls = "https://manifest.googlevideo.com/api/manifest/hls_timedtext_playlist/tts_params/caps%3Dasr%26kind%3Dasr%26lang%3Dpt/playlist/index.m3u8";
    let value = serde_json::json!({
        "subtitles": {},
        "automatic_captions": {
            "pt": [
                { "ext": "vtt", "protocol": "m3u8_native", "url": native_hls },
                { "ext": "vtt", "url": "https://www.youtube.com/api/timedtext?v=HlsNative01&lang=en&tlang=pt&fmt=vtt" }
            ]
        }
    });
    let tracks = parse_subtitles(&value);
    assert_eq!(
        tracks,
        vec![SubtitleTrack {
            language: "pt".into(),
            automatic: true,
            url: native_hls.into(),
        }]
    );
}

#[test]
fn validate_vtt_payload_rejects_empty_and_html_responses() {
    assert!(validate_vtt_payload(String::new()).is_err());
    assert!(validate_vtt_payload("   \r\n\t ".into()).is_err());
    assert!(validate_vtt_payload("WEBVTT\n\n".into()).is_err());
    assert!(
        validate_vtt_payload("<html><body>429 Too Many Requests</body></html>".into()).is_err()
    );

    let valid = "WEBVTT\n\n00:00:01.000 --> 00:00:02.500\nOlá mundo\n".to_owned();
    assert_eq!(validate_vtt_payload(valid.clone()).unwrap(), valid);
}

#[test]
fn resolve_yt_dlp_sub_request_handles_hls_and_splits_manual_vs_auto() {
    let hls_url =
        "https://manifest.googlevideo.com/api/manifest/hls_timedtext_playlist/playlist/index.m3u8";
    let (watch_url, lang, flags) = resolve_yt_dlp_sub_request(
        Some("https://youtu.be/dQw4w9WgXcQ"),
        hls_url,
        Some("pt-BR"),
        Some(false),
    )
    .expect("HLS URL with video_url should resolve");
    assert_eq!(watch_url, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    assert_eq!(lang, "pt-BR");
    assert_eq!(flags, &["--write-subs"]);

    let (watch_url, lang, flags) = resolve_yt_dlp_sub_request(
        Some("https://www.youtube.com/watch?v=TedrETAJeNM"),
        hls_url,
        Some("pt-orig"),
        Some(true),
    )
    .expect("automatic -orig track should strip suffix and use --write-auto-subs");
    assert_eq!(watch_url, "https://www.youtube.com/watch?v=TedrETAJeNM");
    assert_eq!(lang, "pt");
    assert_eq!(flags, &["--write-auto-subs"]);

    let tlang_url = "https://www.youtube.com/api/timedtext?v=TedrETAJeNM&lang=en&tlang=es&fmt=vtt";
    let (_, lang, flags) = resolve_yt_dlp_sub_request(None, tlang_url, None, None)
        .expect("tlang URL without video_url should resolve");
    assert_eq!(lang, "es");
    assert_eq!(flags, &["--write-subs", "--write-auto-subs"]);
}

#[test]
fn find_matching_subtitle_track_matches_canonical_lang_and_kind() {
    let tracks = vec![
        SubtitleTrack {
            language: "pt".into(),
            automatic: false,
            url: "https://www.youtube.com/api/timedtext?v=1&lang=pt&fmt=vtt".into(),
        },
        SubtitleTrack {
            language: "pt".into(),
            automatic: true,
            url: "https://www.youtube.com/api/timedtext?v=1&kind=asr&lang=pt&fmt=vtt".into(),
        },
        SubtitleTrack {
            language: "en".into(),
            automatic: true,
            url: "https://www.youtube.com/api/timedtext?v=1&kind=asr&lang=en&fmt=vtt".into(),
        },
    ];

    let en_auto = find_matching_subtitle_track(&tracks, Some("EN-ORIG"), Some(true))
        .expect("EN-ORIG should match automatic en track");
    assert_eq!(en_auto.url, tracks[2].url);

    let pt_manual = find_matching_subtitle_track(&tracks, Some("pt"), Some(false))
        .expect("manual pt should match manual track");
    assert_eq!(pt_manual.url, tracks[0].url);

    let pt_auto = find_matching_subtitle_track(&tracks, Some("pt-orig"), Some(true))
        .expect("automatic pt-orig should match automatic pt track");
    assert_eq!(pt_auto.url, tracks[1].url);

    let pt_any = find_matching_subtitle_track(&tracks, Some("pt"), None)
        .expect("unspecified automatic flag should prefer manual track");
    assert_eq!(pt_any.url, tracks[0].url);

    assert!(find_matching_subtitle_track(&tracks, Some("   "), None).is_none());
    assert!(find_matching_subtitle_track(&tracks, Some("es"), None).is_none());
    assert!(find_matching_subtitle_track(&tracks, Some("en"), Some(false)).is_none());
}

#[test]
fn subtitle_parser_selects_vtt_and_rejects_untrusted_hosts() {
    let value = serde_json::json!({
        "subtitles": {"pt-BR": [{"ext":"vtt", "url":"https://www.youtube.com/api/timedtext?v=123"}], "bad": [{"ext":"vtt", "url":"https://youtube.com.evil.example/captions"}]},
        "automatic_captions": {"en": [{"ext":"vtt", "url":"https://www.youtube.com/api/timedtext?lang=en"}], "fr": [{"ext":"srt", "url":"https://www.youtube.com/subs"}]}
    });
    let tracks = parse_subtitles(&value);
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].language, "pt-BR");
    assert!(!tracks[0].automatic);
    assert!(tracks[1].automatic);
}

#[test]
fn subtitle_parser_prefers_direct_vtt_and_filters_tlang_when_native_exists() {
    let value = serde_json::json!({
        "subtitles": {},
        "automatic_captions": {
            "en": [
                {"ext": "vtt", "protocol": "m3u8_native", "url": "https://manifest.googlevideo.com/api/manifest/hls_timedtext_playlist/index.m3u8"},
                {"ext": "vtt", "url": "https://www.youtube.com/api/timedtext?v=123&lang=en&fmt=vtt"}
            ],
            "pt": [
                {"ext": "vtt", "url": "https://www.youtube.com/api/timedtext?v=123&lang=en&tlang=pt&fmt=vtt"}
            ],
            "it": [
                {"ext": "vtt", "url": "https://www.youtube.com/api/timedtext?v=123&lang=en&tlang=it&fmt=vtt"}
            ]
        }
    });
    let tracks = parse_subtitles(&value);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].language, "en");
    assert_eq!(
        tracks[0].url,
        "https://www.youtube.com/api/timedtext?v=123&lang=en&fmt=vtt"
    );
}

#[test]
fn hls_vtt_segment_merger_strips_duplicate_headers() {
    let seg1 = "WEBVTT\nKind: captions\nLanguage: pt-BR\n\n00:00:01.000 --> 00:00:02.000\nOi\n";
    let seg2 =
        "\u{feff}WEBVTT\nKind: captions\nLanguage: pt-BR\n\n00:00:02.000 --> 00:00:03.000\nMundo\n";
    let merged = merge_vtt_segments([seg1, seg2]);
    assert_eq!(merged.matches("WEBVTT").count(), 1);
    assert!(merged.contains("Oi"));
    assert!(merged.contains("Mundo"));
}

#[test]
fn parse_results_extracts_iso_date_from_yt_dlp_timestamp() {
    let raw = br#"{"id":"AbxnXTCqdBc","title":"TS Video","channel":"XM360","url":"https://www.youtube.com/watch?v=AbxnXTCqdBc","duration":205,"view_count":11000,"upload_date":"20161122","timestamp":1479772800}"#;
    let results = parse_results(raw, true).expect("parse results");
    assert_eq!(
        results[0].metadata.upload_date.as_deref(),
        Some("2016-11-22T00:00:00Z")
    );
    assert_eq!(
        crate::recommendations::parse_upload_date_epoch(
            results[0].metadata.upload_date.as_deref().unwrap()
        ),
        Some(1_479_772_800)
    );

    let release_raw = br#"{"id":"rel00000001","title":"Premiered","channel":"Chan","url":"https://www.youtube.com/watch?v=rel00000001","release_timestamp":1794000000}"#;
    let release_res = parse_results(release_raw, true).expect("release results");
    assert_eq!(
        release_res[0].metadata.upload_date.as_deref(),
        Some("2026-11-06T21:20:00Z")
    );
}

#[test]
fn parse_yt_lockup_and_video_info_handle_nbsp_play_arrow_and_bullets() {
    let lockup = serde_json::json!({
        "contentType": "LOCKUP_CONTENT_TYPE_VIDEO",
        "contentId": "TMZ00000001",
        "metadata": {
            "lockupMetadataViewModel": {
                "title": { "content": "Harvey Levin Learns GTA" },
                "metadata": {
                    "contentMetadataViewModel": {
                        "metadataRows": [
                            { "metadataParts": [{ "text": { "content": "TMZ Clips" } }] },
                            {
                                "metadataParts": [
                                    {
                                        "text": { "content": "105\u{00a0}mil" },
                                        "accessibilityLabel": "105 mil visualizações",
                                        "leadingIcon": { "name": "PLAY_ARROW_OUTLINED" }
                                    },
                                    {
                                        "text": { "content": "há\u{00a0}10\u{00a0}h" },
                                        "accessibilityLabel": "há 10 horas"
                                    }
                                ]
                            }
                        ]
                    }
                }
            }
        }
    });
    let parsed = parse_yt_lockup_video(&lockup).expect("lockup parsed");
    assert_eq!(parsed.video.channel, "TMZ Clips");
    assert_eq!(parsed.metadata.view_count, Some(105_000));
    assert_eq!(parsed.metadata.view_count_text.as_deref(), Some("105 mil"));
    assert_eq!(
        parsed.metadata.published_time_text.as_deref(),
        Some("há 10 h")
    );

    let combined = serde_json::json!({
        "contentType": "LOCKUP_CONTENT_TYPE_VIDEO",
        "contentId": "COMB0000001",
        "metadata": {
            "lockupMetadataViewModel": {
                "title": { "content": "Combined Bullet" },
                "metadata": {
                    "contentMetadataViewModel": {
                        "metadataRows": [
                            { "metadataParts": [{ "text": { "content": "Channel A" } }] },
                            { "metadataParts": [{ "text": { "content": "854K views • Premiered 2 days ago" } }] }
                        ]
                    }
                }
            }
        }
    });
    let parsed_combined = parse_yt_lockup_video(&combined).expect("combined parsed");
    assert_eq!(parsed_combined.metadata.view_count, Some(854_000));
    assert_eq!(
        parsed_combined.metadata.published_time_text.as_deref(),
        Some("Premiered 2 days ago")
    );

    let (vc, vtext, pdate) =
        parse_video_info_views_and_date("12\u{00a0}mil visualizações • há\u{00a0}7\u{00a0}meses");
    assert_eq!(vc, Some(12_000));
    assert_eq!(vtext.as_deref(), Some("12 mil"));
    assert_eq!(pdate.as_deref(), Some("há 7 meses"));

    let (vc2, vtext2, pdate2) = parse_video_info_views_and_date("6K views · 1d ago");
    assert_eq!(vc2, Some(6_000));
    assert_eq!(vtext2.as_deref(), Some("6K"));
    assert_eq!(pdate2.as_deref(), Some("1d ago"));
}

#[test]
fn channel_page_from_innertube_separates_playlists_from_videos_and_hides_missing_tabs() {
    use crate::innertube::{ChannelTabEndpoint, ChannelTabKind, InnerTubeChannelPage};
    use crate::playlist_sync::PlaylistPrivacy;

    let cid = "UCnathanbzrr123456789012";
    let it_page = InnerTubeChannelPage {
        channel_id: Some(cid.into()),
        title: Some("nathan".into()),
        handle: Some("@nathanbzrr".into()),
        subscriber_count_text: Some("3 inscritos".into()),
        video_count_text: None,
        tabs: vec![
            ChannelTabEndpoint {
                title: "Playlists".into(),
                browse_id: cid.into(),
                params: Some("EglwbGF5bGlzdHM".into()),
                selected: true,
                kind: ChannelTabKind::Playlists,
            },
            ChannelTabEndpoint {
                title: "Posts".into(),
                browse_id: cid.into(),
                params: Some("EgVwb3N0cw".into()),
                selected: false,
                kind: ChannelTabKind::Posts,
            },
        ],
        items: vec![
            Video {
                title: "musica".into(),
                channel: "nathan".into(),
                url: "https://www.youtube.com/playlist?list=PLmusica1234567890".into(),
                thumbnail: "https://i.ytimg.com/vi/abc12345678/hqdefault.jpg".into(),
                duration: String::new(),
            },
            Video {
                title: "PC".into(),
                channel: "nathan".into(),
                url: "https://www.youtube.com/playlist?list=PLpcbuild123456789".into(),
                thumbnail: "https://i.ytimg.com/vi/xyz12345678/hqdefault.jpg".into(),
                duration: String::new(),
            },
        ],
        metadata: vec![
            VideoMetadata {
                kind: SearchResultKind::Playlist,
                playlist_count: Some("434 vídeos".into()),
                badges: vec!["Pública".into()],
                ..Default::default()
            },
            VideoMetadata {
                kind: SearchResultKind::Playlist,
                playlist_count: Some("4 vídeos".into()),
                badges: vec!["Particular".into()],
                ..Default::default()
            },
        ],
        continuation: Some("PLAYLISTS_CONT_TOKEN".into()),
        ..Default::default()
    };

    let page = channel_page_from_innertube(it_page, &ChannelTarget::ChannelId(cid.into()));
    assert!(page.videos.is_empty());
    assert!(!page.has_videos_tab);
    assert!(!page.has_home_tab);
    assert!(page.has_playlists_tab);
    assert!(page.sort_chips.is_empty());
    assert!(page.next_continuation.is_none());
    assert_eq!(page.initial_playlists.len(), 2);
    assert_eq!(page.initial_playlists[0].title, "musica");
    assert_eq!(page.initial_playlists[0].item_count, Some(434));
    assert_eq!(page.initial_playlists[0].privacy, PlaylistPrivacy::Public);
    assert_eq!(page.initial_playlists[1].title, "PC");
    assert_eq!(page.initial_playlists[1].item_count, Some(4));
    assert_eq!(page.initial_playlists[1].privacy, PlaylistPrivacy::Private);
}

#[test]
fn parse_channel_initial_data_and_yt_dlp_filter_playlists_out_of_videos() {
    use crate::playlist_sync::PlaylistPrivacy;

    let cid = "UCnathanbzrr123456789012";
    let playlist_lockup = serde_json::json!({
        "lockupViewModel": {
            "contentType": "LOCKUP_CONTENT_TYPE_PLAYLIST",
            "contentId": "PLmusica1234567890",
            "metadata": {
                "lockupMetadataViewModel": {
                    "title": { "content": "musica" },
                    "metadata": {
                        "contentMetadataViewModel": {
                            "metadataRows": [
                                { "metadataParts": [{ "text": { "content": "Playlist · Pública" } }] }
                            ]
                        }
                    }
                }
            },
            "contentImage": {
                "collectionThumbnailViewModel": {
                    "primaryThumbnail": {
                        "thumbnailViewModel": {
                            "image": { "sources": [{ "url": "https://i.ytimg.com/vi/abc12345678/hqdefault.jpg", "width": 480 }] },
                            "overlays": [{ "thumbnailOverlayBadgeViewModel": { "thumbnailBadges": [{ "thumbnailBadgeViewModel": { "text": "434 vídeos" } }] } }]
                        }
                    }
                }
            }
        }
    });
    let featured = serde_json::json!({
        "metadata": {
            "channelMetadataRenderer": {
                "externalId": cid,
                "title": "nathan",
                "vanityChannelUrl": "https://www.youtube.com/@nathanbzrr"
            }
        },
        "contents": {
            "twoColumnBrowseResultsRenderer": {
                "tabs": [
                    {
                        "tabRenderer": {
                            "title": "Playlists",
                            "selected": true,
                            "endpoint": {
                                "commandMetadata": { "webCommandMetadata": { "url": "/@nathanbzrr/playlists" } },
                                "browseEndpoint": { "browseId": cid, "params": "EglwbGF5bGlzdHM" }
                            },
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "itemSectionRenderer": {
                                                "contents": [
                                                    {
                                                        "shelfRenderer": {
                                                            "title": { "runs": [{ "text": "Playlists criadas" }] },
                                                            "content": {
                                                                "horizontalListRenderer": {
                                                                    "items": [playlist_lockup]
                                                                }
                                                            }
                                                        }
                                                    },
                                                    {
                                                        "continuationItemRenderer": {
                                                            "continuationEndpoint": {
                                                                "continuationCommand": { "token": "pl-cont-token-should-not-leak" }
                                                            }
                                                        }
                                                    }
                                                ]
                                            }
                                        }
                                    ]
                                }
                            }
                        }
                    }
                ]
            }
        }
    });

    let page = parse_channel_initial_data(
        &featured,
        None,
        &ChannelTarget::Handle("@nathanbzrr".into()),
        20,
    )
    .expect("channel initial data should parse");
    assert!(page.videos.is_empty());
    assert!(page.home_shelves.is_empty());
    assert!(!page.has_videos_tab);
    assert!(!page.has_home_tab);
    assert!(page.has_playlists_tab);
    assert!(page.sort_chips.is_empty());
    assert!(page.next_continuation.is_none());
    assert_eq!(page.initial_playlists.len(), 1);
    assert_eq!(page.initial_playlists[0].title, "musica");
    assert_eq!(page.initial_playlists[0].privacy, PlaylistPrivacy::Public);
    assert_eq!(page.initial_playlists[0].item_count, Some(434));

    let yt_dlp_payload = serde_json::json!({
        "_type": "playlist",
        "channel_id": cid,
        "channel": "nathan",
        "entries": [
            { "id": "PLmusica1234567890", "title": "musica", "url": "https://www.youtube.com/playlist?list=PLmusica1234567890" },
            { "id": "dQw4w9WgXcQ", "title": "Uploaded clip", "duration": 42.0 }
        ]
    });
    let yt_page = parse_channel_page(
        yt_dlp_payload.to_string().as_bytes(),
        &ChannelTarget::ChannelId(cid.into()),
        20,
    )
    .expect("yt-dlp JSON should parse");
    assert_eq!(yt_page.videos.len(), 1);
    assert_eq!(yt_page.videos[0].video.title, "Uploaded clip");
    assert!(yt_page.has_videos_tab);
}

#[test]
fn merge_channel_playlists_deduplicates_enriches_and_preserves_private() {
    use crate::playlist_sync::PlaylistPrivacy;

    let primary = vec![ChannelPlaylist {
        id: "PLmusica1234567890".into(),
        title: "musica".into(),
        url: "https://www.youtube.com/playlist?list=PLmusica1234567890".into(),
        thumbnail: String::new(),
        item_count: Some(434),
        description: None,
        privacy: PlaylistPrivacy::Unknown,
    }];
    let extra = vec![
        ChannelPlaylist {
            id: "PLmusica1234567890".into(),
            title: "musica".into(),
            url: "https://www.youtube.com/playlist?list=PLmusica1234567890".into(),
            thumbnail: "https://i.ytimg.com/vi/abc12345678/hqdefault.jpg".into(),
            item_count: Some(434),
            description: Some("Music favorites".into()),
            privacy: PlaylistPrivacy::Public,
        },
        ChannelPlaylist {
            id: "PLpcbuild123456789".into(),
            title: "PC".into(),
            url: "https://www.youtube.com/playlist?list=PLpcbuild123456789".into(),
            thumbnail: "https://i.ytimg.com/vi/xyz12345678/hqdefault.jpg".into(),
            item_count: Some(4),
            description: None,
            privacy: PlaylistPrivacy::Private,
        },
    ];

    let merged = merge_channel_playlists(primary, extra, 30);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].id, "PLmusica1234567890");
    assert_eq!(
        merged[0].thumbnail,
        "https://i.ytimg.com/vi/abc12345678/hqdefault.jpg"
    );
    assert_eq!(merged[0].privacy, PlaylistPrivacy::Public);
    assert_eq!(merged[0].description.as_deref(), Some("Music favorites"));
    assert_eq!(merged[1].id, "PLpcbuild123456789");
    assert_eq!(merged[1].title, "PC");
    assert_eq!(merged[1].privacy, PlaylistPrivacy::Private);
    assert_eq!(merged[1].item_count, Some(4));
}
