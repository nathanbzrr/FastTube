//! Optional opt-in community metadata integrations: Return YouTube Dislike and DeArrow.

use crate::process::Cancellation;
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::time::Duration;

const API_TIMEOUT: Duration = Duration::from_secs(6);
const MAX_RESPONSE_BYTES: usize = 128 * 1024;
const CACHE_CAPACITY: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct RydStats {
    pub video_id: String,
    pub likes: u64,
    pub dislikes: u64,
    pub rating: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeArrowBranding {
    pub video_id: String,
    pub clean_title: Option<String>,
    pub thumbnail_time: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct CommunityExtrasOutcome {
    pub video_id: String,
    pub ryd: Option<RydStats>,
    pub dearrow: Option<DeArrowBranding>,
}

#[derive(Default)]
pub struct CommunityExtrasCache {
    entries: HashMap<String, CommunityExtrasOutcome>,
    order: VecDeque<String>,
}

impl CommunityExtrasCache {
    pub fn get(&self, video_id: &str) -> Option<&CommunityExtrasOutcome> {
        self.entries.get(video_id)
    }

    pub fn insert(&mut self, video_id: String, outcome: CommunityExtrasOutcome) {
        if !self.entries.contains_key(&video_id) {
            if self.order.len() >= CACHE_CAPACITY
                && let Some(oldest) = self.order.pop_front()
            {
                self.entries.remove(&oldest);
            }
            self.order.push_back(video_id.clone());
        }
        self.entries.insert(video_id, outcome);
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
}

pub fn valid_video_id(video_id: &str) -> bool {
    video_id.len() == 11
        && video_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[derive(Deserialize)]
struct RawRydResponse {
    likes: Option<u64>,
    dislikes: Option<u64>,
    rating: Option<f64>,
}

pub fn parse_ryd_response(video_id: &str, bytes: &[u8]) -> Option<RydStats> {
    let raw: RawRydResponse = serde_json::from_slice(bytes).ok()?;
    Some(RydStats {
        video_id: video_id.to_owned(),
        likes: raw.likes?,
        dislikes: raw.dislikes?,
        rating: raw.rating.unwrap_or(0.0),
    })
}

#[derive(Deserialize)]
struct RawDeArrowTitle {
    title: String,
    #[serde(default)]
    votes: i64,
    #[serde(default)]
    locked: bool,
}

#[derive(Deserialize)]
struct RawDeArrowThumb {
    timestamp: Option<f64>,
    #[serde(default)]
    votes: i64,
    #[serde(default)]
    locked: bool,
    #[serde(default)]
    original: bool,
}

#[derive(Deserialize)]
struct RawDeArrowResponse {
    #[serde(default)]
    titles: Vec<RawDeArrowTitle>,
    #[serde(default)]
    thumbnails: Vec<RawDeArrowThumb>,
}

pub fn parse_dearrow_response(video_id: &str, bytes: &[u8]) -> Option<DeArrowBranding> {
    let raw: RawDeArrowResponse = serde_json::from_slice(bytes).ok()?;
    let clean_title = raw
        .titles
        .into_iter()
        .filter(|t| !t.title.trim().is_empty() && (t.locked || t.votes >= 0))
        .max_by_key(|t| (t.locked, t.votes))
        .map(|t| t.title.replace('>', "").trim().to_owned())
        .filter(|t| !t.is_empty());
    let thumbnail_time = raw
        .thumbnails
        .into_iter()
        .filter(|t| !t.original && (t.locked || t.votes >= 0))
        .max_by_key(|t| (t.locked, t.votes))
        .and_then(|t| t.timestamp.filter(|ts| ts.is_finite() && *ts >= 0.0));
    if clean_title.is_none() && thumbnail_time.is_none() {
        return None;
    }
    Some(DeArrowBranding {
        video_id: video_id.to_owned(),
        clean_title,
        thumbnail_time,
    })
}

pub fn fetch_community_extras(
    video_id: &str,
    ryd_enabled: bool,
    dearrow_enabled: bool,
    cancel: &Cancellation,
) -> CommunityExtrasOutcome {
    let mut outcome = CommunityExtrasOutcome {
        video_id: video_id.to_owned(),
        ryd: None,
        dearrow: None,
    };
    if (!ryd_enabled && !dearrow_enabled) || !valid_video_id(video_id) || cancel.cancelled() {
        return outcome;
    }
    let Ok(client) = reqwest::blocking::Client::builder()
        .timeout(API_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return outcome;
    };

    if ryd_enabled && !cancel.cancelled() {
        let url = format!("https://returnyoutubedislikeapi.com/votes?videoId={video_id}");
        if let Ok(resp) = client.get(&url).send()
            && resp.status().is_success()
        {
            let mut body = Vec::new();
            if resp
                .take((MAX_RESPONSE_BYTES + 1) as u64)
                .read_to_end(&mut body)
                .is_ok()
                && body.len() <= MAX_RESPONSE_BYTES
            {
                outcome.ryd = parse_ryd_response(video_id, &body);
            }
        }
    }

    if dearrow_enabled && !cancel.cancelled() {
        let url = format!("https://sponsor.ajay.app/api/branding?videoID={video_id}");
        if let Ok(resp) = client.get(&url).send()
            && resp.status().is_success()
        {
            let mut body = Vec::new();
            if resp
                .take((MAX_RESPONSE_BYTES + 1) as u64)
                .read_to_end(&mut body)
                .is_ok()
                && body.len() <= MAX_RESPONSE_BYTES
            {
                outcome.dearrow = parse_dearrow_response(video_id, &body);
            }
        }
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ryd_and_dearrow_payloads() {
        let ryd = parse_ryd_response(
            "dQw4w9WgXcQ",
            br#"{"likes": 12000, "dislikes": 430, "rating": 4.86}"#,
        )
        .unwrap();
        assert_eq!(ryd.dislikes, 430);

        let dearrow = parse_dearrow_response(
            "dQw4w9WgXcQ",
            br#"{"titles":[{"title":">Clean Video Title","votes":5,"locked":false}],"thumbnails":[{"timestamp":42.5,"votes":3,"locked":false,"original":false}]}"#,
        )
        .unwrap();
        assert_eq!(dearrow.clean_title.as_deref(), Some("Clean Video Title"));
        assert_eq!(dearrow.thumbnail_time, Some(42.5));
    }

    #[test]
    fn disabled_flags_perform_no_work() {
        let cancel = Cancellation::default();
        let out = fetch_community_extras("dQw4w9WgXcQ", false, false, &cancel);
        assert!(out.ryd.is_none() && out.dearrow.is_none());
        assert!(!valid_video_id("invalid"));
    }
}
