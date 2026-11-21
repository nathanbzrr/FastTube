//! Local-only followed channels persistence and OAuth-free RSS feed polling.

use crate::oauth::parse_youtube_atom_feed;
use crate::process::Cancellation;
use crate::search::{self, ChannelTarget, SearchResult};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_FOLLOWED_CHANNELS: usize = 500;
const RSS_MAX_BYTES: usize = 512 * 1024;
const RSS_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FollowedChannel {
    pub channel_id: String,
    pub title: String,
    pub handle: Option<String>,
    pub url: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub followed_at_ms: u64,
    #[serde(default)]
    pub last_seen_video_id: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FollowedChannels {
    pub channels: Vec<FollowedChannel>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FollowedMergeSummary {
    pub added: usize,
    pub conflicts: usize,
    pub skipped_for_limit: usize,
}

impl FollowedChannels {
    pub fn len(&self) -> usize {
        self.channels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }

    pub fn is_following(&self, channel_id_or_handle: &str) -> bool {
        let needle = channel_id_or_handle.trim();
        if needle.is_empty() {
            return false;
        }
        self.channels.iter().any(|ch| {
            ch.channel_id.eq_ignore_ascii_case(needle)
                || ch
                    .handle
                    .as_deref()
                    .is_some_and(|h| h.eq_ignore_ascii_case(needle))
        })
    }

    pub fn is_following_target(
        &self,
        target: Option<&ChannelTarget>,
        id_hint: Option<&str>,
    ) -> bool {
        if let Some(id) = id_hint.map(str::trim).filter(|s| !s.is_empty())
            && self.is_following(id)
        {
            return true;
        }
        match target {
            Some(ChannelTarget::ChannelId(id)) => self.is_following(id),
            Some(ChannelTarget::Handle(handle)) => self.is_following(handle),
            None => false,
        }
    }

    pub fn follow(&mut self, channel: FollowedChannel) -> Result<bool, &'static str> {
        let id = channel.channel_id.trim();
        if id.is_empty() || id.len() > 128 {
            return Err("Identificador de canal inválido.");
        }
        if let Some(existing) = self.channels.iter_mut().find(|ch| {
            ch.channel_id.eq_ignore_ascii_case(id)
                || (channel.handle.is_some() && ch.handle == channel.handle)
        }) {
            if existing.title.trim().is_empty() && !channel.title.trim().is_empty() {
                existing.title = channel.title;
            }
            if existing.avatar_url.is_none() {
                existing.avatar_url = channel.avatar_url;
            }
            if existing.handle.is_none() {
                existing.handle = channel.handle;
            }
            return Ok(false);
        }
        if self.channels.len() >= MAX_FOLLOWED_CHANNELS {
            return Err("Limite de canais seguidos locais atingido (500).");
        }
        self.channels.push(channel);
        Ok(true)
    }

    pub fn unfollow(&mut self, target: Option<&ChannelTarget>, id_hint: Option<&str>) -> bool {
        let before = self.channels.len();
        self.channels.retain(|ch| {
            let matches_id = id_hint
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_some_and(|id| ch.channel_id.eq_ignore_ascii_case(id));
            let matches_target = match target {
                Some(ChannelTarget::ChannelId(id)) => ch.channel_id.eq_ignore_ascii_case(id),
                Some(ChannelTarget::Handle(handle)) => ch
                    .handle
                    .as_deref()
                    .is_some_and(|h| h.eq_ignore_ascii_case(handle)),
                None => false,
            };
            !(matches_id || matches_target)
        });
        self.channels.len() != before
    }

    pub fn mark_seen(&mut self, channel_id: &str, video_id: &str) -> bool {
        let Some(entry) = self
            .channels
            .iter_mut()
            .find(|ch| ch.channel_id.eq_ignore_ascii_case(channel_id))
        else {
            return false;
        };
        if entry.last_seen_video_id.as_deref() == Some(video_id) {
            return false;
        }
        entry.last_seen_video_id = Some(video_id.to_owned());
        true
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.channels.len() > MAX_FOLLOWED_CHANNELS {
            return Err("Too many followed channels");
        }
        for ch in &self.channels {
            if ch.channel_id.trim().is_empty()
                || ch.channel_id.len() > 128
                || ch.title.len() > 512
                || ch.url.len() > 2048
            {
                return Err("Invalid followed channel entry");
            }
        }
        Ok(())
    }

    /// Adds imported channels that are absent locally while preserving local metadata on conflicts.
    pub fn merged(&self, imported: &Self) -> (Self, FollowedMergeSummary) {
        let mut merged = self.clone();
        let mut summary = FollowedMergeSummary::default();
        for ch in &imported.channels {
            let id = ch.channel_id.trim();
            if id.is_empty() || id.len() > 128 {
                continue;
            }
            let existing_pos = merged.channels.iter().position(|cur| {
                cur.channel_id.eq_ignore_ascii_case(id)
                    || ch.handle.as_deref().is_some_and(|h| {
                        cur.handle
                            .as_deref()
                            .is_some_and(|existing_h| existing_h.eq_ignore_ascii_case(h))
                    })
            });
            if let Some(pos) = existing_pos {
                summary.conflicts += 1;
                let existing = &mut merged.channels[pos];
                if existing.title.trim().is_empty() && !ch.title.trim().is_empty() {
                    existing.title = ch.title.clone();
                }
                if existing.avatar_url.is_none() {
                    existing.avatar_url = ch.avatar_url.clone();
                }
                if existing.handle.is_none() {
                    existing.handle = ch.handle.clone();
                }
            } else if merged.channels.len() < MAX_FOLLOWED_CHANNELS {
                merged.channels.push(ch.clone());
                summary.added += 1;
            } else {
                summary.skipped_for_limit += 1;
            }
        }
        (merged, summary)
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

pub fn path() -> io::Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))?;
    let root = PathBuf::from(local);
    if !root.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LOCALAPPDATA must be absolute",
        ));
    }
    Ok(root.join("FastTube").join("followed-channels.json"))
}

pub fn load(path: &Path) -> io::Result<FollowedChannels> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(FollowedChannels::default());
        }
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "canais seguidos inválidos");
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    let root: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if root.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err(invalid());
    }
    let data: FollowedChannels =
        serde_json::from_value(root.get("followed").cloned().ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
    data.validate().map_err(|_| invalid())?;
    Ok(data)
}

pub fn save(path: &Path, data: &FollowedChannels) -> io::Result<()> {
    data.validate()
        .map_err(|msg| io::Error::new(io::ErrorKind::InvalidInput, msg))?;
    let payload = serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "followed": data,
    }))?;
    if payload.len() as u64 > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "followed channels exceed storage limit",
        ));
    }
    crate::history::write_atomic(path, &payload)
}

pub struct LocalFeedOutput {
    pub items: Vec<SearchResult>,
    pub new_video_urls: HashSet<String>,
    #[allow(dead_code)]
    pub latest_ids_by_channel: HashMap<String, String>,
    pub errors: usize,
}

fn is_valid_uc_channel_id(id: &str) -> bool {
    id.starts_with("UC")
        && id.len() >= 16
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn fetch_local_followed_feed(
    channels: &[FollowedChannel],
    cancel: &Cancellation,
) -> Result<LocalFeedOutput, String> {
    if cancel.cancelled() {
        return Err("Atualização cancelada.".into());
    }
    if channels.is_empty() {
        return Ok(LocalFeedOutput {
            items: Vec::new(),
            new_video_urls: HashSet::new(),
            latest_ids_by_channel: HashMap::new(),
            errors: 0,
        });
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(RSS_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Não foi possível iniciar o cliente HTTP.".to_owned())?;

    let mut dated: Vec<(String, SearchResult)> = Vec::new();
    let mut new_video_urls = HashSet::new();
    let mut latest_ids_by_channel = HashMap::new();
    let mut errors = 0usize;

    for ch in channels {
        if cancel.cancelled() {
            return Err("Atualização cancelada.".into());
        }
        let mut fetched: Option<Vec<(String, SearchResult)>> = None;
        if is_valid_uc_channel_id(&ch.channel_id) {
            let rss_url = format!(
                "https://www.youtube.com/feeds/videos.xml?channel_id={}",
                ch.channel_id
            );
            if let Ok(resp) = client.get(&rss_url).send()
                && resp.status().is_success()
            {
                let mut body = Vec::new();
                if resp
                    .take((RSS_MAX_BYTES + 1) as u64)
                    .read_to_end(&mut body)
                    .is_ok()
                    && body.len() <= RSS_MAX_BYTES
                    && let Ok(xml) = std::str::from_utf8(&body)
                {
                    fetched = parse_youtube_atom_feed(xml, &ch.title, &ch.channel_id);
                }
            }
        }
        if fetched.is_none() {
            let target = if is_valid_uc_channel_id(&ch.channel_id) {
                Some(ChannelTarget::ChannelId(ch.channel_id.clone()))
            } else {
                ch.handle.as_ref().map(|h| ChannelTarget::Handle(h.clone()))
            };
            if let Some(target) = target
                && let Ok(outcome) = search::fetch_channel_page_with_cancel(
                    &target.canonical_url(),
                    10,
                    false,
                    cancel,
                )
            {
                let fallback_items = outcome
                    .page
                    .videos
                    .into_iter()
                    .map(|r| {
                        let ts = r.metadata.upload_date.clone().unwrap_or_default();
                        (ts, r)
                    })
                    .collect();
                fetched = Some(fallback_items);
            }
        }

        let Some(entries) = fetched else {
            errors += 1;
            continue;
        };

        if let Some((_, first)) = entries.first()
            && let Some(first_id) = crate::history::youtube_video_id(&first.video.url)
        {
            latest_ids_by_channel.insert(ch.channel_id.clone(), first_id.clone());
            if let Some(seen) = ch.last_seen_video_id.as_deref() {
                for (_, item) in &entries {
                    if let Some(vid) = crate::history::youtube_video_id(&item.video.url) {
                        if vid == seen {
                            break;
                        }
                        new_video_urls.insert(item.video.url.clone());
                    }
                }
            } else {
                new_video_urls.insert(first.video.url.clone());
            }
        }
        dated.extend(entries);
    }

    dated.sort_by(|a, b| b.0.cmp(&a.0));
    let mut seen_urls = HashSet::new();
    let mut items = Vec::new();
    for (_, item) in dated {
        if seen_urls.insert(item.video.url.clone()) {
            items.push(item);
        }
    }

    Ok(LocalFeedOutput {
        items,
        new_video_urls,
        latest_ids_by_channel,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_channel(id: &str, title: &str) -> FollowedChannel {
        FollowedChannel {
            channel_id: id.to_owned(),
            title: title.to_owned(),
            handle: Some(format!("@{title}")),
            url: format!("https://www.youtube.com/channel/{id}"),
            avatar_url: None,
            followed_at_ms: 1_700_000_000_000,
            last_seen_video_id: None,
        }
    }

    #[test]
    fn follow_unfollow_and_atomic_round_trip() {
        let dir = std::env::temp_dir().join(format!("fasttube-followed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("followed-channels.json");

        let mut followed = FollowedChannels::default();
        assert!(
            followed
                .follow(sample_channel("UC0000000000000001", "RustLang"))
                .unwrap()
        );
        assert!(
            !followed
                .follow(sample_channel("UC0000000000000001", "RustLang"))
                .unwrap()
        );
        assert!(followed.is_following("UC0000000000000001"));
        assert!(followed.is_following("@RustLang"));
        assert!(followed.mark_seen("UC0000000000000001", "dQw4w9WgXcQ"));

        save(&file, &followed).unwrap();
        let loaded = load(&file).unwrap();
        assert_eq!(loaded, followed);

        let mut updated = loaded;
        assert!(updated.unfollow(
            Some(&ChannelTarget::ChannelId("UC0000000000000001".to_owned())),
            None
        ));
        assert!(updated.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_rejected_without_modification() {
        let dir =
            std::env::temp_dir().join(format!("fasttube-followed-corrupt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("followed-channels.json");
        fs::write(&file, b"{corrupt").unwrap();
        assert!(load(&file).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"{corrupt");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_keeps_local_channels_and_adds_missing_within_limit() {
        let mut local = FollowedChannels::default();
        let mut ch1 = sample_channel("UC0000000000000001", "Alpha");
        ch1.avatar_url = None;
        local.follow(ch1).unwrap();

        let mut imported = FollowedChannels::default();
        let mut imp1 = sample_channel("UC0000000000000001", "Alpha Remote");
        imp1.avatar_url = Some("https://example.test/avatar.png".into());
        let imp2 = sample_channel("UC0000000000000002", "Beta");
        imported.follow(imp1).unwrap();
        imported.follow(imp2).unwrap();

        let (merged, summary) = local.merged(&imported);
        assert_eq!(summary.added, 1);
        assert_eq!(summary.conflicts, 1);
        assert_eq!(summary.skipped_for_limit, 0);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged.channels[0].title, "Alpha");
        assert_eq!(
            merged.channels[0].avatar_url.as_deref(),
            Some("https://example.test/avatar.png")
        );
        assert_eq!(merged.channels[1].channel_id, "UC0000000000000002");
    }
}
