use super::{
    HistoryDisplayCacheKey, HistoryDisplaySection, LocalInsightsCacheKey,
    library_uniform_visible_row_range, library_variable_visible_range,
    remove_liked_video_from_state, resolve_history_display_cache, resolve_local_insights_cache,
};
use crate::FastTube;
use crate::history::{self, Entry as HistoryEntry, HistoryFilter};
use crate::insights;
use crate::library::Bookmark;
use crate::playlist_sync::RemotePlaylistDetailItem;
use crate::search::{SearchResult, Video, VideoMetadata};
use std::collections::HashSet;
use std::sync::Arc;

fn sample_result(video_id: &str, url: &str) -> SearchResult {
    SearchResult {
        video: Video {
            title: format!("Video {video_id}"),
            channel: "Channel".into(),
            url: url.into(),
            thumbnail: String::new(),
            duration: "5:00".into(),
        },
        metadata: VideoMetadata::default(),
    }
}

#[test]
fn remove_liked_video_from_state_matches_by_video_id_and_updates_total() {
    let mut items = vec![
        sample_result("dQw4w9WgXcQ", "https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
        sample_result("9bZkp7q19f0", "https://www.youtube.com/watch?v=9bZkp7q19f0"),
    ];
    let mut seen_keys = HashSet::from(["dQw4w9WgXcQ".to_owned(), "9bZkp7q19f0".to_owned()]);
    let mut total = Some(2);

    let removed = remove_liked_video_from_state(
        &mut items,
        &mut seen_keys,
        &mut total,
        "https://youtu.be/dQw4w9WgXcQ",
    );

    assert_eq!(removed, 1);
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].video.url,
        "https://www.youtube.com/watch?v=9bZkp7q19f0"
    );
    assert!(!seen_keys.contains("dQw4w9WgXcQ"));
    assert!(seen_keys.contains("9bZkp7q19f0"));
    assert_eq!(total, Some(1));
}

#[test]
fn history_display_cache_reuses_and_invalidates_on_filter_query_or_data_change() {
    let mut builds = 0usize;
    let base_key = HistoryDisplayCacheKey {
        filter: HistoryFilter::All,
        search_query: String::new(),
        pt: true,
        local_len: 10,
        local_first_sig: Some(("dQw4w9WgXcQ".into(), 12_000, Some(60_000), Some(1_000))),
        local_last_sig: Some(("9bZkp7q19f0".into(), 5_000)),
        local_minute_bucket: 100,
        remote_loaded: false,
        remote_page_count: 0,
        remote_sections_len: 0,
        remote_items_len: 0,
        remote_seen_len: 0,
        remote_first_id: None,
        remote_last_id: None,
    };
    let first = resolve_history_display_cache(None, base_key.clone(), || {
        builds += 1;
        (
            vec![HistoryDisplaySection::from_entries(
                "Hoje".into(),
                Vec::new(),
            )],
            false,
        )
    });
    assert_eq!(builds, 1);

    // Same key reuses cached Arc without invoking build closure.
    let reused = resolve_history_display_cache(Some(&first), base_key.clone(), || {
        builds += 1;
        (Vec::new(), false)
    });
    assert_eq!(builds, 1);
    assert!(Arc::ptr_eq(&first, &reused));

    // Changing query invalidates cache.
    let mut query_key = base_key.clone();
    query_key.search_query = "rust".into();
    let rebuilt = resolve_history_display_cache(Some(&first), query_key, || {
        builds += 1;
        (Vec::new(), false)
    });
    assert_eq!(builds, 2);
    assert!(!Arc::ptr_eq(&first, &rebuilt));

    // Changing language invalidates cache.
    let mut lang_key = base_key;
    lang_key.pt = false;
    let rebuilt_lang = resolve_history_display_cache(Some(&first), lang_key, || {
        builds += 1;
        (Vec::new(), false)
    });
    assert_eq!(builds, 3);
    assert!(!Arc::ptr_eq(&first, &rebuilt_lang));
}

#[test]
fn local_insights_cache_reuses_and_invalidates_on_state_change() {
    let mut builds = 0usize;
    let base_key = LocalInsightsCacheKey {
        history_len: 5,
        history_first_sig: Some(("dQw4w9WgXcQ".into(), 30_000, Some(60_000), Some(1_000))),
        history_last_sig: Some(("9bZkp7q19f0".into(), 10_000)),
        library_revision: 3,
        library_bucket_revisions: (1, 2, 1, 0),
        library_counts: (1, 2, 1, 0),
        followed_len: 4,
        followed_first_id: Some("UC1234567890123456789012".into()),
        profile_token_count: 12,
        feed_signal_revision: 2,
        feed_generation: 1,
    };
    let first = resolve_local_insights_cache(None, base_key.clone(), || {
        builds += 1;
        insights::LocalInsights {
            total_videos: 5,
            ..Default::default()
        }
    });
    assert_eq!(builds, 1);

    let reused = resolve_local_insights_cache(Some(&first), base_key.clone(), || {
        builds += 1;
        insights::LocalInsights::default()
    });
    assert_eq!(builds, 1);
    assert!(Arc::ptr_eq(&first, &reused));

    let mut changed_key = base_key;
    changed_key.followed_len = 5;
    let rebuilt = resolve_local_insights_cache(Some(&first), changed_key, || {
        builds += 1;
        insights::LocalInsights {
            followed_channels_count: 5,
            ..Default::default()
        }
    });
    assert_eq!(builds, 2);
    assert_eq!(rebuilt.insights.followed_channels_count, 5);
}

#[test]
fn library_uniform_visible_row_range_culls_offscreen_rows_accurately() {
    let total_rows = 50;
    let row_h = 148.0;
    let gap = 10.0;
    let expected_total = total_rows as f32 * row_h + (total_rows - 1) as f32 * gap;
    let (range, top_h, bottom_h) =
        library_uniform_visible_row_range(total_rows, row_h, gap, 0.0, 2000.0, 2700.0, 2);
    assert!(range.start > 5);
    assert!(range.end < total_rows);
    let rendered = range.end - range.start;
    let rendered_h = rendered as f32 * row_h + rendered.saturating_sub(1) as f32 * gap;
    assert!((top_h + rendered_h + bottom_h - expected_total).abs() < 0.01);
}

#[test]
fn merged_cache_bucket_key_ignores_unrelated_library_mutations_and_appends_incrementally() {
    let mut app = FastTube::default();
    let r1 = sample_result("dQw4w9WgXcQ", "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    let r2 = sample_result("9bZkp7q19f0", "https://www.youtube.com/watch?v=9bZkp7q19f0");
    app.library
        .watch_later_remote
        .items
        .push(RemotePlaylistDetailItem {
            result: r1.clone(),
            set_video_id: None,
            index_text: None,
        });
    app.library
        .watch_later_remote
        .seen_keys
        .insert("dQw4w9WgXcQ".into());
    app.library.watch_later_remote.revision = 1;
    app.ensure_watch_later_merged_cache();
    let initial_key = app.library.watch_later_remote.merged_cache_key;
    assert_eq!(app.library.watch_later_remote.merged_videos.len(), 1);

    // Unrelated bookmark mutation bumps `library.revision` without changing Watch Later's bucket key.
    let _ = app.library.data.add_bookmark(Bookmark {
        video_id: "dQw4w9WgXcQ".into(),
        title: "Bookmarked".into(),
        time_ms: 1_000,
        note: "Note".into(),
    });
    app.library.revision = app.library.revision.wrapping_add(1);
    app.ensure_watch_later_merged_cache();
    assert_eq!(app.library.watch_later_remote.merged_cache_key, initial_key);

    // Appending a new remote page increments merged_videos cleanly even when merged_cache_key was cleared.
    app.library
        .watch_later_remote
        .items
        .push(RemotePlaylistDetailItem {
            result: r2,
            set_video_id: None,
            index_text: None,
        });
    app.library
        .watch_later_remote
        .seen_keys
        .insert("9bZkp7q19f0".into());
    app.library.watch_later_remote.merged_cache_key = None;
    app.library.watch_later_remote.revision = 2;
    app.ensure_watch_later_merged_cache();
    assert_eq!(app.library.watch_later_remote.merged_videos.len(), 2);
    assert_eq!(
        history::youtube_video_id(&app.library.watch_later_remote.merged_videos[1].url).as_deref(),
        Some("9bZkp7q19f0")
    );
}

#[test]
fn merged_cache_liked_videos_uses_favorites_revision_and_ignores_watch_later_mutations() {
    let mut app = FastTube::default();
    let r1 = sample_result("dQw4w9WgXcQ", "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    let wl_video =
        sample_result("9bZkp7q19f0", "https://www.youtube.com/watch?v=9bZkp7q19f0").video;
    let fav_video =
        sample_result("3JZ_D3ELwOQ", "https://www.youtube.com/watch?v=3JZ_D3ELwOQ").video;

    app.library.liked_videos.items.push(r1);
    app.library
        .liked_videos
        .seen_keys
        .insert("dQw4w9WgXcQ".into());
    app.library.liked_videos.revision = 1;
    app.ensure_liked_videos_merged_cache();
    let initial_key = app.library.liked_videos.merged_cache_key;
    assert_eq!(app.library.liked_videos.merged_videos.len(), 1);

    // Mutating Watch Later must not invalidate the Liked Videos merged cache key.
    assert!(app.library.data.add_to_watch_later(wl_video));
    app.library.revision = app.library.revision.wrapping_add(1);
    app.ensure_liked_videos_merged_cache();
    assert_eq!(app.library.liked_videos.merged_cache_key, initial_key);

    // Mutating Favorites bumps `favorites_revision()` and updates the merged cache.
    assert!(app.library.data.add_to_favorites(fav_video));
    app.ensure_liked_videos_merged_cache();
    assert_ne!(app.library.liked_videos.merged_cache_key, initial_key);
    assert_eq!(app.library.liked_videos.merged_videos.len(), 2);
}

#[test]
fn history_display_section_metrics_match_mixed_shorts_and_videos() {
    let make_entry = |id: &str, short: bool| HistoryEntry {
        video_id: id.into(),
        title: format!("Title {id}"),
        channel: "Chan".into(),
        position_ms: 10_000,
        duration_ms: Some(if short { 30_000 } else { 300_000 }),
        watched_at_ms: Some(1_000),
        is_short: Some(short),
        ..HistoryEntry::default()
    };
    let section = HistoryDisplaySection::from_entries(
        "Today".into(),
        vec![
            make_entry("aaaaaaaaaaa", true),
            make_entry("bbbbbbbbbbb", true),
            make_entry("ccccccccccc", false),
            make_entry("ddddddddddd", true),
        ],
    );
    let thumb_h = 138.0;
    let shelf_h = 410.0;
    let spacing = 6.0;
    let expected_all = shelf_h + (18.0 + spacing) + thumb_h + (16.0 + spacing) + shelf_h;
    let got_all = section.estimated_body_height(HistoryFilter::All, thumb_h, shelf_h, spacing);
    assert!((got_all - expected_all).abs() < 0.01);

    let expected_videos = 4.0 * thumb_h + 3.0 * (16.0 + spacing);
    let got_videos =
        section.estimated_body_height(HistoryFilter::Videos, thumb_h, shelf_h, spacing);
    assert!((got_videos - expected_videos).abs() < 0.01);

    let got_shorts =
        section.estimated_body_height(HistoryFilter::Shorts, thumb_h, shelf_h, spacing);
    assert!((got_shorts - shelf_h).abs() < 0.01);
}

#[test]
fn library_variable_visible_range_culls_downloads_with_exact_spacers() {
    let heights: Vec<f32> = (0..80)
        .map(|i| if i % 3 == 0 { 132.0 } else { 110.0 })
        .collect();
    let gap = 12.0;
    let expected_total: f32 =
        heights.iter().sum::<f32>() + (heights.len().saturating_sub(1) as f32) * gap;
    let (range, top_h, bottom_h) =
        library_variable_visible_range(&heights, gap, 0.0, 2600.0, 3400.0, 2);
    assert!(range.start > 15);
    assert!(range.end < 40);
    let rendered_h: f32 = heights[range.clone()].iter().sum::<f32>()
        + ((range.end - range.start).saturating_sub(1) as f32) * gap;
    assert!((top_h + rendered_h + bottom_h - expected_total).abs() < 0.05);
}

#[test]
fn clamp_shelf_offset_and_playlist_gradient_stops_are_deterministic() {
    assert_eq!(super::clamp_shelf_offset(0, 10, 4), (0, 4));
    assert_eq!(super::clamp_shelf_offset(5, 10, 4), (5, 9));
    assert_eq!(super::clamp_shelf_offset(9, 10, 4), (6, 10));
    assert_eq!(super::clamp_shelf_offset(4, 3, 4), (0, 3));

    let (top_a, right_a, bot_a) =
        super::derive_playlist_gradient_stops("https://i.ytimg.com/vi/dQw4w9WgXcQ/hq.jpg", true);
    let (top_b, right_b, bot_b) =
        super::derive_playlist_gradient_stops("https://i.ytimg.com/vi/dQw4w9WgXcQ/hq.jpg", true);
    assert_eq!((top_a, right_a, bot_a), (top_b, right_b, bot_b));
    let (top_light, _, _) =
        super::derive_playlist_gradient_stops("https://i.ytimg.com/vi/dQw4w9WgXcQ/hq.jpg", false);
    assert_ne!(top_a, top_light);
}

#[test]
fn history_entry_is_live_detects_live_streams_and_vods() {
    let mut app = FastTube::default();
    let vod = HistoryEntry {
        video_id: "dQw4w9WgXcQ".into(),
        title: "Regular Video".into(),
        channel: "Chan".into(),
        position_ms: 10_000,
        duration_ms: Some(200_000),
        ..HistoryEntry::default()
    };
    assert!(!app.history_entry_is_live(&vod, None));
    assert!(app.history_entry_is_live(&vod, Some(true)));

    let live_titled = HistoryEntry {
        video_id: "9bZkp7q19f0".into(),
        title: "Coding [AO VIVO]".into(),
        channel: "Chan".into(),
        ..HistoryEntry::default()
    };
    assert!(app.history_entry_is_live(&live_titled, None));

    app.video_metadata.insert(
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        VideoMetadata {
            is_live: true,
            ..VideoMetadata::default()
        },
    );
    assert!(app.history_entry_is_live(&vod, None));
}

#[test]
fn reorder_playlist_detail_item_reorders_local_collection_and_watch_later() {
    let mut app = FastTube::default();
    let ctx = eframe::egui::Context::default();
    let col_idx = app
        .library
        .data
        .create_collection("Reorder Test")
        .expect("create collection");
    let v1 = sample_result("dQw4w9WgXcQ", "https://www.youtube.com/watch?v=dQw4w9WgXcQ").video;
    let v2 = sample_result("9bZkp7q19f0", "https://www.youtube.com/watch?v=9bZkp7q19f0").video;
    let v3 = sample_result("3JZ_D3ELwOQ", "https://www.youtube.com/watch?v=3JZ_D3ELwOQ").video;
    assert!(
        app.library
            .data
            .toggle_video_in_collection("Reorder Test", v1)
    );
    assert!(
        app.library
            .data
            .toggle_video_in_collection("Reorder Test", v2)
    );
    assert!(
        app.library
            .data
            .toggle_video_in_collection("Reorder Test", v3)
    );

    // `toggle_video_in_collection` prepends newest entries, so initial order is [v3, v2, v1].
    app.reorder_playlist_detail_item("local:0", Some(col_idx), 0, 2, &ctx);
    let urls: Vec<&str> = app.library.data.collections[col_idx]
        .videos
        .iter()
        .map(|r| r.video.url.as_str())
        .collect();
    assert_eq!(
        urls,
        vec![
            "https://www.youtube.com/watch?v=9bZkp7q19f0",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=3JZ_D3ELwOQ",
        ]
    );
}
