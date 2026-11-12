//! OAuth independent local library and timestamp bookmarks.
use crate::search::Video;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
#[cfg(not(test))]
use std::path::PathBuf;

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ITEMS: usize = 500;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Library {
    pub favorites: Vec<VideoRecord>,
    pub watch_later: Vec<VideoRecord>,
    pub collections: Vec<Collection>,
    pub bookmarks: Vec<Bookmark>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MergeSummary {
    pub added: usize,
    pub conflicts: usize,
    pub skipped_for_limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VideoRecord {
    pub video: Video,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Collection {
    pub name: String,
    pub videos: Vec<VideoRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Bookmark {
    pub video_id: String,
    pub title: String,
    pub time_ms: u64,
    pub note: String,
}

#[cfg(not(test))]
pub fn path() -> io::Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))?;
    let root = PathBuf::from(local);
    if !root.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LOCALAPPDATA must be absolute",
        ));
    }
    Ok(root.join("FastTube").join("library.json"))
}

pub fn load(path: &Path) -> io::Result<Library> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Library::default()),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    let root: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if root.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err(invalid());
    }
    let library: Library =
        serde_json::from_value(root.get("library").cloned().ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
    library.validate().map_err(|_| invalid())?;
    Ok(library)
}

impl Library {
    pub fn item_count(&self) -> usize {
        self.favorites.len()
            + self.watch_later.len()
            + self.bookmarks.len()
            + self
                .collections
                .iter()
                .map(|collection| collection.videos.len())
                .sum::<usize>()
    }

    /// Adds imported records that are absent locally. Local records win conflicts.
    pub fn merged(&self, imported: &Library) -> (Library, MergeSummary) {
        let mut merged = self.clone();
        let mut summary = MergeSummary::default();
        let mut favorite_keys: HashSet<_> = merged
            .favorites
            .iter()
            .filter_map(|record| crate::history::youtube_video_id(&record.video.url))
            .collect();
        let mut watch_later_keys: HashSet<_> = merged
            .watch_later
            .iter()
            .filter_map(|record| crate::history::youtube_video_id(&record.video.url))
            .collect();
        for record in &imported.favorites {
            if let Some(id) = crate::history::youtube_video_id(&record.video.url) {
                if favorite_keys.contains(&id) {
                    summary.conflicts += 1;
                } else if merged.item_count() < MAX_ITEMS {
                    favorite_keys.insert(id);
                    merged.favorites.push(record.clone());
                    summary.added += 1;
                } else {
                    summary.skipped_for_limit += 1;
                }
            }
        }
        for record in &imported.watch_later {
            if let Some(id) = crate::history::youtube_video_id(&record.video.url) {
                if watch_later_keys.contains(&id) {
                    summary.conflicts += 1;
                } else if merged.item_count() < MAX_ITEMS {
                    watch_later_keys.insert(id);
                    merged.watch_later.push(record.clone());
                    summary.added += 1;
                } else {
                    summary.skipped_for_limit += 1;
                }
            }
        }
        for imported_collection in &imported.collections {
            let target_index = merged
                .collections
                .iter()
                .position(|collection| collection.name == imported_collection.name);
            if let Some(index) = target_index {
                for record in &imported_collection.videos {
                    if let Some(id) = crate::history::youtube_video_id(&record.video.url) {
                        let already_present =
                            merged.collections[index].videos.iter().any(|existing| {
                                crate::history::youtube_video_id(&existing.video.url).as_deref()
                                    == Some(id.as_str())
                            });
                        if already_present {
                            summary.conflicts += 1;
                        } else if merged.item_count() < MAX_ITEMS {
                            merged.collections[index].videos.push(record.clone());
                            summary.added += 1;
                        } else {
                            summary.skipped_for_limit += 1;
                        }
                    }
                }
            } else if merged.collections.len() < 100
                && merged
                    .item_count()
                    .saturating_add(imported_collection.videos.len())
                    <= MAX_ITEMS
            {
                summary.added += imported_collection.videos.len();
                merged.collections.push(imported_collection.clone());
            } else {
                summary.skipped_for_limit += imported_collection.videos.len();
            }
        }
        let mut bookmark_keys: HashSet<_> = merged
            .bookmarks
            .iter()
            .map(|bookmark| {
                (
                    bookmark.video_id.clone(),
                    bookmark.time_ms,
                    bookmark.note.clone(),
                )
            })
            .collect();
        for bookmark in &imported.bookmarks {
            let key = (
                bookmark.video_id.clone(),
                bookmark.time_ms,
                bookmark.note.clone(),
            );
            if bookmark_keys.contains(&key) {
                summary.conflicts += 1;
            } else if merged.item_count() < MAX_ITEMS {
                bookmark_keys.insert(key);
                merged.bookmarks.push(bookmark.clone());
                summary.added += 1;
            } else {
                summary.skipped_for_limit += 1;
            }
        }
        (merged, summary)
    }

    pub(crate) fn validate(&self) -> Result<(), ()> {
        let count = self.favorites.len()
            + self.watch_later.len()
            + self.bookmarks.len()
            + self
                .collections
                .iter()
                .map(|c| c.videos.len())
                .sum::<usize>();
        if count > MAX_ITEMS || self.collections.len() > 100 {
            return Err(());
        }
        let valid_records = |records: &[VideoRecord]| {
            records.iter().all(|r| {
                let v = &r.video;
                !v.title.trim().is_empty()
                    && v.title.len() <= 4096
                    && v.channel.len() <= 4096
                    && v.url.len() <= 4096
                    && v.thumbnail.len() <= 4096
                    && v.duration.len() <= 256
                    && crate::history::youtube_video_id(&v.url).is_some()
            })
        };
        if !valid_records(&self.favorites)
            || !valid_records(&self.watch_later)
            || self.collections.iter().any(|c| {
                c.name.trim().is_empty() || c.name.len() > 128 || !valid_records(&c.videos)
            })
            || self.bookmarks.iter().any(|b| {
                !crate::history::is_valid_video_id(&b.video_id)
                    || b.title.len() > 4096
                    || b.note.len() > 4096
            })
        {
            return Err(());
        }
        Ok(())
    }
    pub fn toggle_favorite(&mut self, video: Video) {
        toggle(&mut self.favorites, video);
    }
    pub fn toggle_watch_later(&mut self, video: Video) {
        toggle(&mut self.watch_later, video);
    }
    pub fn add_bookmark(&mut self, bookmark: Bookmark) -> io::Result<()> {
        if self.bookmarks.len() >= MAX_ITEMS
            || !crate::history::is_valid_video_id(&bookmark.video_id)
            || bookmark.note.len() > 4096
            || bookmark.title.len() > 4096
        {
            return Err(invalid());
        }
        self.bookmarks.push(bookmark);
        Ok(())
    }
}
fn toggle(records: &mut Vec<VideoRecord>, video: Video) {
    if let Some(id) = crate::history::youtube_video_id(&video.url) {
        if let Some(index) = records.iter().position(|r| {
            crate::history::youtube_video_id(&r.video.url).as_deref() == Some(id.as_str())
        }) {
            records.remove(index);
        } else if records.len() < MAX_ITEMS {
            records.insert(0, VideoRecord { video });
        }
    }
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid local library")
}
pub fn save(path: &Path, library: &Library) -> io::Result<()> {
    library.validate().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "library exceeds limits or contains invalid entries",
        )
    })?;
    let bytes = serde_json::to_vec(&serde_json::json!({"version":1,"library":library}))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "library exceeds file size limit",
        ));
    }
    crate::history::write_atomic(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistence_is_atomic_and_rejects_corruption() {
        let path =
            std::env::temp_dir().join(format!("fasttube-library-{}.json", std::process::id()));
        let mut lib = Library::default();
        lib.toggle_favorite(Video {
            title: "T".into(),
            channel: "C".into(),
            url: "https://youtu.be/abcdefghijk".into(),
            thumbnail: String::new(),
            duration: "1:00".into(),
        });
        save(&path, &lib).unwrap();
        assert_eq!(load(&path).unwrap(), lib);
        fs::write(&path, b"bad").unwrap();
        assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn merge_keeps_local_records_and_adds_missing_imported_records() {
        let local_video = Video {
            title: "Local title".into(),
            channel: "local".into(),
            url: "https://youtu.be/abcdefghijk".into(),
            thumbnail: String::new(),
            duration: "1:00".into(),
        };
        let imported_video = Video {
            title: "Imported".into(),
            channel: "remote".into(),
            url: "https://youtu.be/lmnopqrstuv".into(),
            thumbnail: String::new(),
            duration: "2:00".into(),
        };
        let mut local = Library::default();
        local.toggle_favorite(local_video.clone());
        let mut imported = Library::default();
        imported.toggle_favorite(local_video.clone());
        imported.toggle_watch_later(imported_video.clone());
        let (merged, summary) = local.merged(&imported);
        assert_eq!(merged.favorites, local.favorites);
        assert_eq!(
            merged.watch_later,
            vec![VideoRecord {
                video: imported_video
            }]
        );
        assert_eq!(summary.added, 1);
        assert_eq!(summary.conflicts, 1);
    }
}
