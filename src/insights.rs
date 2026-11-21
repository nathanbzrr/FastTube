//! Local watch insights computed from history, library, and recommendation profile.

use crate::followed_channels::FollowedChannels;
use crate::history::History;
use crate::library::Library;
use crate::recommendations::UserProfile;
use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChannelStat {
    pub channel: String,
    pub videos: usize,
    pub watch_ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocalInsights {
    pub total_videos: usize,
    pub completed_videos: usize,
    pub in_progress_videos: usize,
    pub total_watch_ms: u64,
    pub top_channels: Vec<ChannelStat>,
    pub top_topics: Vec<(String, f64)>,
    pub favorites_count: usize,
    pub watch_later_count: usize,
    pub collections_count: usize,
    pub bookmarks_count: usize,
    pub followed_channels_count: usize,
}

impl LocalInsights {
    #[allow(dead_code)]
    pub fn completion_rate_percent(&self) -> f32 {
        if self.total_videos == 0 {
            0.0
        } else {
            (self.completed_videos as f32 / self.total_videos as f32) * 100.0
        }
    }

    #[allow(dead_code)]
    pub fn average_watch_ms(&self) -> u64 {
        if self.total_videos == 0 {
            0
        } else {
            self.total_watch_ms / self.total_videos as u64
        }
    }
}

pub fn compute_local_insights(
    history: &History,
    library: &Library,
    profile: &UserProfile,
    followed: &FollowedChannels,
) -> LocalInsights {
    let entries = history.entries();
    let mut total_watch_ms: u64 = 0;
    let mut completed_videos = 0usize;
    let mut in_progress_videos = 0usize;
    let mut by_channel: HashMap<String, (String, usize, u64)> = HashMap::new();

    for entry in entries {
        let completed = entry.is_near_end() && entry.position_ms > 0;
        let watched = match (completed, entry.duration_ms) {
            (true, Some(dur)) if dur > entry.position_ms => dur,
            _ => entry.position_ms,
        };
        total_watch_ms = total_watch_ms.saturating_add(watched);
        if completed {
            completed_videos += 1;
        } else if entry.position_ms > 0 {
            in_progress_videos += 1;
        }
        let ch = entry.channel.trim();
        if !ch.is_empty() {
            // History entries are newest-first, so keeping the first casing preserves the latest.
            let slot = by_channel
                .entry(ch.to_lowercase())
                .or_insert_with(|| (ch.to_owned(), 0, 0));
            slot.1 += 1;
            slot.2 = slot.2.saturating_add(watched);
        }
    }

    let mut top_channels: Vec<ChannelStat> = by_channel
        .into_values()
        .map(|(channel, videos, watch_ms)| ChannelStat {
            channel,
            videos,
            watch_ms,
        })
        .collect();
    top_channels.sort_by(|a, b| {
        b.watch_ms
            .cmp(&a.watch_ms)
            .then_with(|| b.videos.cmp(&a.videos))
            .then_with(|| a.channel.cmp(&b.channel))
    });
    top_channels.truncate(8);

    let mut top_topics: Vec<(String, f64)> = profile
        .token_weights
        .iter()
        .filter(|(_, w)| **w > 0.05)
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    top_topics.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    top_topics.truncate(12);

    LocalInsights {
        total_videos: entries.len(),
        completed_videos,
        in_progress_videos,
        total_watch_ms,
        top_channels,
        top_topics,
        favorites_count: library.favorites.len(),
        watch_later_count: library.watch_later.len(),
        collections_count: library.collections.len(),
        bookmarks_count: library.bookmarks.len(),
        followed_channels_count: followed.len(),
    }
}

pub fn format_watch_duration_human(total_ms: u64, pt: bool) -> String {
    let total_secs = total_ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    if hours > 0 {
        if pt {
            format!("{hours}h {mins}min")
        } else {
            format!("{hours}h {mins}m")
        }
    } else if mins > 0 {
        if pt {
            format!("{mins} min")
        } else {
            format!("{mins}m")
        }
    } else if pt {
        format!("{total_secs} s")
    } else {
        format!("{total_secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::Entry;

    #[test]
    fn aggregates_watch_time_and_top_channels() {
        let mut history = History::default();
        let _ = history.record(Entry {
            video_id: "aaaaaaaaaaa".into(),
            title: "Rust 1".into(),
            channel: "Alpha".into(),
            position_ms: 585_000,
            duration_ms: Some(600_000),
            watched_at_ms: Some(1),
        });
        let _ = history.record(Entry {
            video_id: "bbbbbbbbbbb".into(),
            title: "Rust 2".into(),
            channel: "Beta".into(),
            position_ms: 60_000,
            duration_ms: Some(300_000),
            watched_at_ms: Some(2),
        });
        let mut profile = UserProfile::default();
        profile.token_weights.insert("rust".into(), 2.5);
        let insights = compute_local_insights(
            &history,
            &Library::default(),
            &profile,
            &FollowedChannels::default(),
        );
        assert_eq!(insights.total_videos, 2);
        assert_eq!(insights.completed_videos, 1);
        assert_eq!(insights.in_progress_videos, 1);
        assert_eq!(insights.total_watch_ms, 660_000);
        assert_eq!(insights.top_channels[0].channel, "Alpha");
        assert_eq!(insights.top_topics[0].0, "rust");
    }

    #[test]
    fn merges_channel_casing_and_computes_completion_metrics() {
        let empty = LocalInsights::default();
        assert_eq!(empty.completion_rate_percent(), 0.0);
        assert_eq!(empty.average_watch_ms(), 0);

        let mut history = History::default();
        let _ = history.record(Entry {
            video_id: "aaaaaaaaaaa".into(),
            title: "Older".into(),
            channel: "rustlang".into(),
            position_ms: 120_000,
            duration_ms: Some(300_000),
            watched_at_ms: Some(1),
        });
        let _ = history.record(Entry {
            video_id: "bbbbbbbbbbb".into(),
            title: "Newer".into(),
            channel: "RustLang".into(),
            position_ms: 295_000,
            duration_ms: Some(300_000),
            watched_at_ms: Some(2),
        });
        let insights = compute_local_insights(
            &history,
            &Library::default(),
            &UserProfile::default(),
            &FollowedChannels::default(),
        );
        assert_eq!(insights.total_videos, 2);
        assert_eq!(insights.completed_videos, 1);
        assert_eq!(insights.top_channels.len(), 1);
        assert_eq!(insights.top_channels[0].channel, "RustLang");
        assert_eq!(insights.top_channels[0].videos, 2);
        assert_eq!(insights.top_channels[0].watch_ms, 420_000);
        assert!((insights.completion_rate_percent() - 50.0).abs() < 0.001);
        assert_eq!(insights.average_watch_ms(), 210_000);
    }
}
