//! Local watch insights computed from history, library, and recommendation profile.

use crate::followed_channels::FollowedChannels;
use crate::history::History;
use crate::library::Library;
use crate::recommendations::UserProfile;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum InsightsTimeframe {
    Days7,
    #[default]
    Days30,
    AllTime,
}

impl InsightsTimeframe {
    pub const ALL: [Self; 3] = [Self::Days7, Self::Days30, Self::AllTime];

    pub fn label_pt(self) -> &'static str {
        match self {
            Self::Days7 => "7d",
            Self::Days30 => "30d",
            Self::AllTime => "Tudo",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::Days7 => "7d",
            Self::Days30 => "30d",
            Self::AllTime => "All",
        }
    }

    pub fn label(self, pt: bool) -> &'static str {
        if pt { self.label_pt() } else { self.label_en() }
    }

    pub fn bucket_count(self) -> usize {
        match self {
            Self::Days7 => 7,
            Self::Days30 | Self::AllTime => 30,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DailyWatchBucket {
    pub date_label: String,
    pub day_epoch: i64,
    pub watch_ms: u64,
    pub videos_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TopVideoStat {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub url: String,
    pub watch_ms: u64,
}

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
    pub top_channel_ids: Vec<String>,
    pub top_video_urls: Vec<String>,
    pub top_videos: Vec<TopVideoStat>,
    pub daily_buckets: Vec<DailyWatchBucket>,
    pub top_topics: Vec<(String, f64)>,
    pub favorites_count: usize,
    pub watch_later_count: usize,
    pub collections_count: usize,
    pub bookmarks_count: usize,
    pub followed_channels_count: usize,
}

impl LocalInsights {
    pub fn completion_rate_percent(&self) -> f32 {
        if self.total_videos == 0 {
            0.0
        } else {
            (self.completed_videos as f32 / self.total_videos as f32) * 100.0
        }
    }

    pub fn average_watch_ms(&self) -> u64 {
        if self.total_videos == 0 {
            0
        } else {
            self.total_watch_ms / self.total_videos as u64
        }
    }
}

fn civil_month_day_from_days(days_since_unix_epoch: i64) -> (u32, u32) {
    // Howard Hinnant's civil_from_days algorithm.
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (m, d)
}

fn format_day_bucket_label(day_index: i64) -> String {
    let (month, day) = civil_month_day_from_days(day_index);
    format!("{day:02}/{month:02}")
}

#[allow(dead_code)]
pub fn compute_local_insights(
    history: &History,
    library: &Library,
    profile: &UserProfile,
    followed: &FollowedChannels,
) -> LocalInsights {
    let now_unix = (crate::history::current_unix_epoch_ms() / 1000) as i64;
    compute_with_timeframe(
        history,
        library,
        profile,
        followed,
        InsightsTimeframe::AllTime,
        now_unix,
    )
}

pub fn compute_with_timeframe(
    history: &History,
    library: &Library,
    profile: &UserProfile,
    followed: &FollowedChannels,
    timeframe: InsightsTimeframe,
    now_unix: i64,
) -> LocalInsights {
    const SECS_PER_DAY: i64 = 86_400;
    let now_day = now_unix.max(0).div_euclid(SECS_PER_DAY);
    let bucket_count = timeframe.bucket_count();
    let start_day = now_day - (bucket_count as i64 - 1);

    let mut daily_buckets: Vec<DailyWatchBucket> = (0..bucket_count)
        .map(|offset| {
            let day_idx = start_day + offset as i64;
            DailyWatchBucket {
                date_label: format_day_bucket_label(day_idx),
                day_epoch: day_idx * SECS_PER_DAY,
                watch_ms: 0,
                videos_count: 0,
            }
        })
        .collect();

    let entries = history.entries();
    let mut total_videos = 0usize;
    let mut total_watch_ms: u64 = 0;
    let mut completed_videos = 0usize;
    let mut in_progress_videos = 0usize;
    let mut by_channel: HashMap<String, (String, usize, u64)> = HashMap::new();
    let mut video_stats: Vec<TopVideoStat> = Vec::new();

    for entry in entries {
        let entry_day = entry
            .watched_at_ms
            .filter(|&ms| ms > 0)
            .map(|ms| ((ms / 1000) as i64).div_euclid(SECS_PER_DAY));

        let in_timeframe = match timeframe {
            InsightsTimeframe::AllTime => true,
            InsightsTimeframe::Days7 | InsightsTimeframe::Days30 => {
                entry_day.is_some_and(|d| d >= start_day && d <= now_day)
            }
        };
        if !in_timeframe {
            continue;
        }

        total_videos += 1;
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

        if let Some(d) = entry_day
            && d >= start_day
            && d <= now_day
        {
            let slot_idx = (d - start_day) as usize;
            if let Some(bucket) = daily_buckets.get_mut(slot_idx) {
                bucket.watch_ms = bucket.watch_ms.saturating_add(watched);
                bucket.videos_count += 1;
            }
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

        let url = crate::history::youtube_watch_url(&entry.video_id)
            .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={}", entry.video_id));
        video_stats.push(TopVideoStat {
            video_id: entry.video_id.clone(),
            title: entry.title.clone(),
            channel: entry.channel.clone(),
            url,
            watch_ms: watched,
        });
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

    let mut followed_by_name: HashMap<String, String> =
        HashMap::with_capacity(followed.channels.len() * 2);
    for fc in &followed.channels {
        let key = fc.title.trim().to_lowercase();
        if !key.is_empty() {
            followed_by_name
                .entry(key)
                .or_insert_with(|| fc.channel_id.clone());
        }
        if let Some(handle) = fc.handle.as_deref() {
            let hkey = handle.trim().trim_start_matches('@').to_lowercase();
            if !hkey.is_empty() {
                followed_by_name
                    .entry(hkey)
                    .or_insert_with(|| fc.channel_id.clone());
            }
        }
    }

    let top_channel_ids: Vec<String> = top_channels
        .iter()
        .map(|stat| {
            let lower = stat.channel.trim().trim_start_matches('@').to_lowercase();
            followed_by_name
                .get(&lower)
                .cloned()
                .unwrap_or_else(|| stat.channel.clone())
        })
        .collect();

    video_stats.sort_by(|a, b| {
        b.watch_ms
            .cmp(&a.watch_ms)
            .then_with(|| a.video_id.cmp(&b.video_id))
    });
    video_stats.truncate(8);
    let top_video_urls: Vec<String> = video_stats.iter().map(|v| v.url.clone()).collect();

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
        total_videos,
        completed_videos,
        in_progress_videos,
        total_watch_ms,
        top_channels,
        top_channel_ids,
        top_video_urls,
        top_videos: video_stats,
        daily_buckets,
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
            ..Default::default()
        });
        let _ = history.record(Entry {
            video_id: "bbbbbbbbbbb".into(),
            title: "Rust 2".into(),
            channel: "Beta".into(),
            position_ms: 60_000,
            duration_ms: Some(300_000),
            watched_at_ms: Some(2),
            ..Default::default()
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
            ..Default::default()
        });
        let _ = history.record(Entry {
            video_id: "bbbbbbbbbbb".into(),
            title: "Newer".into(),
            channel: "RustLang".into(),
            position_ms: 295_000,
            duration_ms: Some(300_000),
            watched_at_ms: Some(2),
            ..Default::default()
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

    #[test]
    fn compute_with_timeframe_populates_daily_buckets_and_drill_down_links() {
        use crate::followed_channels::FollowedChannel;

        const DAY_SECS: i64 = 86_400;
        let now_unix = 20_000 * DAY_SECS + 3_600; // Day 20000
        let today_ms = (now_unix as u64) * 1000;
        let three_days_ago_ms = ((now_unix - 3 * DAY_SECS) as u64) * 1000;
        let fifteen_days_ago_ms = ((now_unix - 15 * DAY_SECS) as u64) * 1000;
        let forty_days_ago_ms = ((now_unix - 40 * DAY_SECS) as u64) * 1000;

        let mut history = History::default();
        let _ = history.record(Entry {
            video_id: "aaaaaaaaaaa".into(),
            title: "Old Video".into(),
            channel: "OldChan".into(),
            position_ms: 100_000,
            duration_ms: Some(200_000),
            watched_at_ms: Some(forty_days_ago_ms),
            ..Default::default()
        });
        let _ = history.record(Entry {
            video_id: "bbbbbbbbbbb".into(),
            title: "Month Video".into(),
            channel: "MonthChan".into(),
            position_ms: 295_000,
            duration_ms: Some(300_000),
            watched_at_ms: Some(fifteen_days_ago_ms),
            ..Default::default()
        });
        let _ = history.record(Entry {
            video_id: "ccccccccccc".into(),
            title: "Week Video".into(),
            channel: "AlphaChan".into(),
            position_ms: 180_000,
            duration_ms: Some(400_000),
            watched_at_ms: Some(three_days_ago_ms),
            ..Default::default()
        });
        let _ = history.record(Entry {
            video_id: "ddddddddddd".into(),
            title: "Today Video".into(),
            channel: "AlphaChan".into(),
            position_ms: 590_000,
            duration_ms: Some(600_000),
            watched_at_ms: Some(today_ms),
            ..Default::default()
        });

        let followed = FollowedChannels {
            channels: vec![FollowedChannel {
                channel_id: "UC1111111111111111111111".into(),
                title: "AlphaChan".into(),
                handle: Some("@alphachan".into()),
                url: "https://www.youtube.com/channel/UC1111111111111111111111".into(),
                avatar_url: None,
                followed_at_ms: 1,
                last_seen_video_id: None,
            }],
        };

        // 7-day timeframe includes only the 2 recent entries and produces 7 daily bars.
        let insights_7d = compute_with_timeframe(
            &history,
            &Library::default(),
            &UserProfile::default(),
            &followed,
            InsightsTimeframe::Days7,
            now_unix,
        );
        assert_eq!(insights_7d.daily_buckets.len(), 7);
        assert_eq!(insights_7d.total_videos, 2);
        assert_eq!(insights_7d.completed_videos, 1);
        assert_eq!(insights_7d.in_progress_videos, 1);
        assert_eq!(insights_7d.total_watch_ms, 780_000);
        assert_eq!(
            insights_7d.daily_buckets.last().unwrap().watch_ms,
            600_000,
            "today's bucket should hold completed 600s video"
        );
        assert_eq!(insights_7d.daily_buckets.last().unwrap().videos_count, 1);
        assert_eq!(insights_7d.daily_buckets[3].watch_ms, 180_000);
        assert_eq!(
            insights_7d.top_channel_ids,
            vec!["UC1111111111111111111111".to_owned()]
        );
        assert_eq!(
            insights_7d.top_video_urls[0],
            "https://www.youtube.com/watch?v=ddddddddddd"
        );

        // 30-day timeframe includes the 15-day-old entry and produces 30 daily bars.
        let insights_30d = compute_with_timeframe(
            &history,
            &Library::default(),
            &UserProfile::default(),
            &followed,
            InsightsTimeframe::Days30,
            now_unix,
        );
        assert_eq!(insights_30d.daily_buckets.len(), 30);
        assert_eq!(insights_30d.total_videos, 3);
        assert_eq!(insights_30d.total_watch_ms, 1_080_000);

        // AllTime includes the 40-day-old entry as well.
        let insights_all = compute_with_timeframe(
            &history,
            &Library::default(),
            &UserProfile::default(),
            &followed,
            InsightsTimeframe::AllTime,
            now_unix,
        );
        assert_eq!(insights_all.daily_buckets.len(), 30);
        assert_eq!(insights_all.total_videos, 4);
        assert_eq!(insights_all.total_watch_ms, 1_180_000);
    }
}
