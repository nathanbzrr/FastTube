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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(from = "LibraryWire")]
pub struct Library {
    pub favorites: Vec<VideoRecord>,
    pub watch_later: Vec<VideoRecord>,
    pub collections: Vec<Collection>,
    pub bookmarks: Vec<Bookmark>,
    #[serde(skip)]
    watch_later_ids: HashSet<String>,
    #[serde(skip)]
    favorite_ids: HashSet<String>,
    #[serde(skip)]
    watch_later_revision: u64,
    #[serde(skip)]
    favorites_revision: u64,
    #[serde(skip)]
    collections_revision: u64,
    #[serde(skip)]
    bookmarks_revision: u64,
}

impl PartialEq for Library {
    fn eq(&self, other: &Self) -> bool {
        self.favorites == other.favorites
            && self.watch_later == other.watch_later
            && self.collections == other.collections
            && self.bookmarks == other.bookmarks
    }
}

impl Eq for Library {}

#[derive(Deserialize)]
struct LibraryWire {
    favorites: Vec<VideoRecord>,
    watch_later: Vec<VideoRecord>,
    collections: Vec<Collection>,
    bookmarks: Vec<Bookmark>,
}

impl From<LibraryWire> for Library {
    fn from(wire: LibraryWire) -> Self {
        let mut library = Self {
            favorites: wire.favorites,
            watch_later: wire.watch_later,
            collections: wire.collections,
            bookmarks: wire.bookmarks,
            ..Self::default()
        };
        library.rebuild_indexes();
        library
    }
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

#[allow(dead_code)]
pub fn load_with_fingerprint(
    path: &Path,
) -> io::Result<(Library, Option<crate::history::FileFingerprint>)> {
    let library = load(path)?;
    Ok((library, crate::history::file_fingerprint(path)))
}

impl Library {
    pub fn rebuild_indexes(&mut self) {
        self.watch_later_ids.clear();
        self.watch_later_ids.reserve(self.watch_later.len());
        for record in &self.watch_later {
            if let Some(id) = crate::history::youtube_video_id(&record.video.url) {
                self.watch_later_ids.insert(id);
            }
        }
        self.favorite_ids.clear();
        self.favorite_ids.reserve(self.favorites.len());
        for record in &self.favorites {
            if let Some(id) = crate::history::youtube_video_id(&record.video.url) {
                self.favorite_ids.insert(id);
            }
        }
    }

    #[allow(dead_code)]
    pub fn is_video_id_in_watch_later(&self, video_id: &str) -> bool {
        self.watch_later_ids.contains(video_id)
    }

    #[allow(dead_code)]
    pub fn is_in_watch_later(&self, video_id_or_url: &str) -> bool {
        if crate::history::is_valid_video_id(video_id_or_url) {
            return self.watch_later_ids.contains(video_id_or_url);
        }
        crate::history::youtube_video_id(video_id_or_url)
            .is_some_and(|id| self.watch_later_ids.contains(&id))
    }

    #[allow(dead_code)]
    pub fn is_video_id_in_favorites(&self, video_id: &str) -> bool {
        self.favorite_ids.contains(video_id)
    }

    #[allow(dead_code)]
    pub fn is_in_favorites(&self, video_id_or_url: &str) -> bool {
        if crate::history::is_valid_video_id(video_id_or_url) {
            return self.favorite_ids.contains(video_id_or_url);
        }
        crate::history::youtube_video_id(video_id_or_url)
            .is_some_and(|id| self.favorite_ids.contains(&id))
    }

    #[allow(dead_code)]
    pub fn watch_later_revision(&self) -> u64 {
        self.watch_later_revision
    }

    #[allow(dead_code)]
    pub fn favorites_revision(&self) -> u64 {
        self.favorites_revision
    }

    #[allow(dead_code)]
    pub fn collections_revision(&self) -> u64 {
        self.collections_revision
    }

    #[allow(dead_code)]
    pub fn bookmarks_revision(&self) -> u64 {
        self.bookmarks_revision
    }

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
        merged.rebuild_indexes();
        let mut summary = MergeSummary::default();
        let mut total_items = merged.item_count();
        let mut favorites_changed = false;
        let mut watch_later_changed = false;
        let mut collections_changed = false;
        let mut bookmarks_changed = false;

        for record in &imported.favorites {
            if let Some(id) = crate::history::youtube_video_id(&record.video.url) {
                if merged.favorite_ids.contains(&id) {
                    summary.conflicts += 1;
                } else if total_items < MAX_ITEMS {
                    merged.favorite_ids.insert(id);
                    merged.favorites.push(record.clone());
                    total_items += 1;
                    summary.added += 1;
                    favorites_changed = true;
                } else {
                    summary.skipped_for_limit += 1;
                }
            }
        }
        for record in &imported.watch_later {
            if let Some(id) = crate::history::youtube_video_id(&record.video.url) {
                if merged.watch_later_ids.contains(&id) {
                    summary.conflicts += 1;
                } else if total_items < MAX_ITEMS {
                    merged.watch_later_ids.insert(id);
                    merged.watch_later.push(record.clone());
                    total_items += 1;
                    summary.added += 1;
                    watch_later_changed = true;
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
                let mut existing_ids: HashSet<String> = merged.collections[index]
                    .videos
                    .iter()
                    .filter_map(|existing| crate::history::youtube_video_id(&existing.video.url))
                    .collect();
                for record in &imported_collection.videos {
                    if let Some(id) = crate::history::youtube_video_id(&record.video.url) {
                        if existing_ids.contains(&id) {
                            summary.conflicts += 1;
                        } else if total_items < MAX_ITEMS {
                            existing_ids.insert(id);
                            merged.collections[index].videos.push(record.clone());
                            total_items += 1;
                            summary.added += 1;
                            collections_changed = true;
                        } else {
                            summary.skipped_for_limit += 1;
                        }
                    }
                }
            } else if merged.collections.len() < 100
                && total_items.saturating_add(imported_collection.videos.len()) <= MAX_ITEMS
            {
                total_items += imported_collection.videos.len();
                summary.added += imported_collection.videos.len();
                merged.collections.push(imported_collection.clone());
                collections_changed = true;
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
            } else if total_items < MAX_ITEMS {
                bookmark_keys.insert(key);
                merged.bookmarks.push(bookmark.clone());
                total_items += 1;
                summary.added += 1;
                bookmarks_changed = true;
            } else {
                summary.skipped_for_limit += 1;
            }
        }
        if favorites_changed {
            merged.favorites_revision = merged.favorites_revision.wrapping_add(1);
        }
        if watch_later_changed {
            merged.watch_later_revision = merged.watch_later_revision.wrapping_add(1);
        }
        if collections_changed {
            merged.collections_revision = merged.collections_revision.wrapping_add(1);
        }
        if bookmarks_changed {
            merged.bookmarks_revision = merged.bookmarks_revision.wrapping_add(1);
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
    #[allow(dead_code)]
    pub fn toggle_favorite(&mut self, video: Video) {
        if toggle_indexed(&mut self.favorites, &mut self.favorite_ids, video) {
            self.favorites_revision = self.favorites_revision.wrapping_add(1);
        }
    }
    pub fn toggle_watch_later(&mut self, video: Video) {
        if toggle_indexed(&mut self.watch_later, &mut self.watch_later_ids, video) {
            self.watch_later_revision = self.watch_later_revision.wrapping_add(1);
        }
    }
    #[allow(dead_code)]
    pub fn add_to_watch_later(&mut self, video: Video) -> bool {
        let Some(id) = crate::history::youtube_video_id(&video.url) else {
            return false;
        };
        if self.watch_later_ids.contains(&id) || self.item_count() >= MAX_ITEMS {
            return false;
        }
        self.watch_later_ids.insert(id);
        self.watch_later.insert(0, VideoRecord { video });
        self.watch_later_revision = self.watch_later_revision.wrapping_add(1);
        true
    }
    #[allow(dead_code)]
    pub fn add_to_favorites(&mut self, video: Video) -> bool {
        let Some(id) = crate::history::youtube_video_id(&video.url) else {
            return false;
        };
        if self.favorite_ids.contains(&id) || self.item_count() >= MAX_ITEMS {
            return false;
        }
        self.favorite_ids.insert(id);
        self.favorites.insert(0, VideoRecord { video });
        self.favorites_revision = self.favorites_revision.wrapping_add(1);
        true
    }
    #[allow(dead_code)]
    pub fn clear_watch_later(&mut self) -> usize {
        let removed = self.watch_later.len();
        if removed > 0 {
            self.watch_later.clear();
            self.watch_later_ids.clear();
            self.watch_later_revision = self.watch_later_revision.wrapping_add(1);
        }
        removed
    }
    #[allow(dead_code)]
    pub fn clear_favorites(&mut self) -> usize {
        let removed = self.favorites.len();
        if removed > 0 {
            self.favorites.clear();
            self.favorite_ids.clear();
            self.favorites_revision = self.favorites_revision.wrapping_add(1);
        }
        removed
    }
    #[allow(dead_code)]
    pub fn clear_collections(&mut self) -> usize {
        let removed = self.collections.len();
        if removed > 0 {
            self.collections.clear();
            self.collections_revision = self.collections_revision.wrapping_add(1);
        }
        removed
    }
    #[allow(dead_code)]
    pub fn clear_bookmarks(&mut self) -> usize {
        let removed = self.bookmarks.len();
        if removed > 0 {
            self.bookmarks.clear();
            self.bookmarks_revision = self.bookmarks_revision.wrapping_add(1);
        }
        removed
    }
    #[allow(dead_code)]
    pub fn toggle_video_in_collection(&mut self, collection_name: &str, video: Video) -> bool {
        let trimmed = collection_name.trim();
        if trimmed.is_empty() || trimmed.len() > 128 {
            return false;
        }
        let Some(vid) = crate::history::youtube_video_id(&video.url) else {
            return false;
        };
        let total_items = self.item_count();
        let mut changed = false;
        if let Some(collection) = self
            .collections
            .iter_mut()
            .find(|c| c.name.eq_ignore_ascii_case(trimmed))
        {
            if let Some(pos) = collection.videos.iter().position(|v| {
                crate::history::youtube_video_id(&v.video.url).as_deref() == Some(&vid)
            }) {
                collection.videos.remove(pos);
                changed = true;
            } else if total_items < MAX_ITEMS {
                collection.videos.insert(0, VideoRecord { video });
                changed = true;
            }
        } else if self.collections.len() < 100 && total_items < MAX_ITEMS {
            self.collections.push(Collection {
                name: trimmed.to_owned(),
                videos: vec![VideoRecord { video }],
            });
            changed = true;
        }
        if changed {
            self.collections_revision = self.collections_revision.wrapping_add(1);
        }
        changed
    }
    pub fn add_bookmark(&mut self, bookmark: Bookmark) -> io::Result<()> {
        if self.item_count() >= MAX_ITEMS
            || !crate::history::is_valid_video_id(&bookmark.video_id)
            || bookmark.note.len() > 4096
            || bookmark.title.len() > 4096
        {
            return Err(invalid());
        }
        self.bookmarks.push(bookmark);
        self.bookmarks_revision = self.bookmarks_revision.wrapping_add(1);
        Ok(())
    }

    pub fn update_bookmark_note(&mut self, index: usize, note: &str) -> Result<(), &'static str> {
        let Some(mark) = self.bookmarks.get_mut(index) else {
            return Err("Marcador não encontrado.");
        };
        let trimmed = note.trim();
        if trimmed.len() > 4096 {
            return Err("A nota do marcador excede o limite de 4096 bytes.");
        }
        if mark.note != trimmed {
            mark.note = trimmed.to_owned();
            self.bookmarks_revision = self.bookmarks_revision.wrapping_add(1);
        }
        Ok(())
    }

    pub fn remove_bookmark(&mut self, index: usize) -> Option<Bookmark> {
        let removed = (index < self.bookmarks.len()).then(|| self.bookmarks.remove(index));
        if removed.is_some() {
            self.bookmarks_revision = self.bookmarks_revision.wrapping_add(1);
        }
        removed
    }

    pub fn create_collection(&mut self, name: &str) -> Result<usize, &'static str> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("Informe um nome para a coleção.");
        }
        if trimmed.len() > 128 {
            return Err("O nome da coleção excede o limite de 128 bytes.");
        }
        if self.collections.len() >= 100 {
            return Err("Limite de 100 coleções atingido.");
        }
        if self
            .collections
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(trimmed))
        {
            return Err("Já existe uma coleção com esse nome.");
        }
        self.collections.push(Collection {
            name: trimmed.to_owned(),
            videos: Vec::new(),
        });
        self.collections_revision = self.collections_revision.wrapping_add(1);
        Ok(self.collections.len() - 1)
    }

    pub fn next_available_collection_name(&self, base: &str) -> String {
        let base = base.trim();
        let base = if base.is_empty() { "Fila salva" } else { base };
        let truncated = truncate_bytes(base, 118);
        if !self
            .collections
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(truncated))
        {
            return truncated.to_owned();
        }
        for suffix in 2..=200usize {
            let candidate = format!("{truncated} ({suffix})");
            if candidate.len() <= 128
                && !self
                    .collections
                    .iter()
                    .any(|c| c.name.eq_ignore_ascii_case(&candidate))
            {
                return candidate;
            }
        }
        truncated.to_owned()
    }

    /// Snapshots videos into a new collection, deduplicating by YouTube ID and enforcing library limits.
    pub fn create_collection_with_videos(
        &mut self,
        name: &str,
        videos: &[Video],
    ) -> Result<(usize, usize), &'static str> {
        let valid_videos: Vec<Video> = videos
            .iter()
            .filter(|v| {
                !v.title.trim().is_empty()
                    && v.title.len() <= 4096
                    && v.channel.len() <= 4096
                    && v.url.len() <= 4096
                    && v.thumbnail.len() <= 4096
                    && v.duration.len() <= 256
                    && crate::history::youtube_video_id(&v.url).is_some()
            })
            .cloned()
            .collect();
        if valid_videos.is_empty() {
            return Err("Nenhum vídeo compatível do YouTube para salvar na coleção.");
        }
        let col_idx = self.create_collection(name)?;
        let mut seen_ids = HashSet::new();
        let mut total_items = self.item_count();
        let mut added = 0usize;
        for video in valid_videos {
            if total_items >= MAX_ITEMS {
                break;
            }
            if let Some(id) = crate::history::youtube_video_id(&video.url)
                && seen_ids.insert(id)
            {
                self.collections[col_idx].videos.push(VideoRecord { video });
                total_items += 1;
                added += 1;
            }
        }
        Ok((col_idx, added))
    }

    pub fn rename_collection(&mut self, index: usize, new_name: &str) -> Result<(), &'static str> {
        if index >= self.collections.len() {
            return Err("Coleção não encontrada.");
        }
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err("Informe um nome válido para a coleção.");
        }
        if trimmed.len() > 128 {
            return Err("O nome da coleção excede o limite de 128 bytes.");
        }
        if self
            .collections
            .iter()
            .enumerate()
            .any(|(i, c)| i != index && c.name.eq_ignore_ascii_case(trimmed))
        {
            return Err("Já existe outra coleção com esse nome.");
        }
        if self.collections[index].name != trimmed {
            self.collections[index].name = trimmed.to_owned();
            self.collections_revision = self.collections_revision.wrapping_add(1);
        }
        Ok(())
    }

    pub fn delete_collection(&mut self, index: usize) -> Option<Collection> {
        let removed = (index < self.collections.len()).then(|| self.collections.remove(index));
        if removed.is_some() {
            self.collections_revision = self.collections_revision.wrapping_add(1);
        }
        removed
    }

    pub fn remove_from_watch_later(&mut self, video_url: &str) -> bool {
        let removed =
            remove_video_indexed(&mut self.watch_later, &mut self.watch_later_ids, video_url);
        if removed {
            self.watch_later_revision = self.watch_later_revision.wrapping_add(1);
        }
        removed
    }

    pub fn remove_from_favorites(&mut self, video_url: &str) -> bool {
        let removed = remove_video_indexed(&mut self.favorites, &mut self.favorite_ids, video_url);
        if removed {
            self.favorites_revision = self.favorites_revision.wrapping_add(1);
        }
        removed
    }

    pub fn remove_from_collection(&mut self, collection_index: usize, video_url: &str) -> bool {
        let removed = self
            .collections
            .get_mut(collection_index)
            .is_some_and(|collection| remove_video(&mut collection.videos, video_url));
        if removed {
            self.collections_revision = self.collections_revision.wrapping_add(1);
        }
        removed
    }
}

pub fn format_bookmarks_markdown(bookmarks: &[Bookmark]) -> String {
    let mut lines = Vec::with_capacity(bookmarks.len());
    for mark in bookmarks {
        let Some(url) = crate::history::youtube_watch_url(&mark.video_id) else {
            continue;
        };
        let total_secs = mark.time_ms / 1000;
        let ts = if total_secs >= 3600 {
            format!(
                "{}:{:02}:{:02}",
                total_secs / 3600,
                (total_secs / 60) % 60,
                total_secs % 60
            )
        } else {
            format!("{:02}:{:02}", total_secs / 60, total_secs % 60)
        };
        let note = mark.note.trim();
        if note.is_empty() {
            lines.push(format!("- [{ts}]({url}&t={total_secs}s)"));
        } else {
            lines.push(format!("- [{ts}]({url}&t={total_secs}s) {note}"));
        }
    }
    lines.join("\n")
}

fn truncate_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn remove_video(records: &mut Vec<VideoRecord>, video_url: &str) -> bool {
    let target_id = crate::history::youtube_video_id(video_url);
    let previous = records.len();
    records.retain(|record| {
        if let Some(target_id) = target_id.as_deref() {
            crate::history::youtube_video_id(&record.video.url).as_deref() != Some(target_id)
        } else {
            record.video.url != video_url
        }
    });
    previous != records.len()
}

fn remove_video_indexed(
    records: &mut Vec<VideoRecord>,
    index: &mut HashSet<String>,
    video_url: &str,
) -> bool {
    let target_id = crate::history::youtube_video_id(video_url);
    let previous = records.len();
    records.retain(|record| {
        if let Some(target_id) = target_id.as_deref() {
            crate::history::youtube_video_id(&record.video.url).as_deref() != Some(target_id)
        } else {
            record.video.url != video_url
        }
    });
    let removed = previous != records.len();
    if removed {
        if let Some(id) = target_id {
            index.remove(&id);
        } else {
            index.clear();
            for record in records.iter() {
                if let Some(id) = crate::history::youtube_video_id(&record.video.url) {
                    index.insert(id);
                }
            }
        }
    }
    removed
}

fn toggle_indexed(
    records: &mut Vec<VideoRecord>,
    index: &mut HashSet<String>,
    video: Video,
) -> bool {
    let Some(id) = crate::history::youtube_video_id(&video.url) else {
        return false;
    };
    if index.contains(&id) {
        if let Some(pos) = records.iter().position(|r| {
            crate::history::youtube_video_id(&r.video.url).as_deref() == Some(id.as_str())
        }) {
            records.remove(pos);
        }
        index.remove(&id);
        return true;
    }
    if records.len() < MAX_ITEMS {
        index.insert(id);
        records.insert(0, VideoRecord { video });
        return true;
    }
    false
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid local library")
}

pub fn serialize_bytes(library: &Library) -> io::Result<Vec<u8>> {
    library.validate().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "library exceeds limits or contains invalid entries",
        )
    })?;
    #[derive(Serialize)]
    struct LibraryFile<'a> {
        version: u8,
        library: &'a Library,
    }
    let bytes = serde_json::to_vec(&LibraryFile {
        version: 1,
        library,
    })?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "library exceeds file size limit",
        ));
    }
    Ok(bytes)
}

pub fn save(path: &Path, library: &Library) -> io::Result<()> {
    let bytes = serialize_bytes(library)?;
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

    #[test]
    fn collection_crud_rename_delete_and_item_removal_respect_invariants() {
        let v1 = Video {
            title: "Video 1".into(),
            channel: "Canal A".into(),
            url: "https://www.youtube.com/watch?v=abcdefghijk".into(),
            thumbnail: String::new(),
            duration: "3:15".into(),
        };
        let v2 = Video {
            title: "Video 2".into(),
            channel: "Canal B".into(),
            url: "https://youtu.be/lmnopqrstuv".into(),
            thumbnail: String::new(),
            duration: "4:20".into(),
        };
        let mut lib = Library::default();
        assert!(lib.create_collection("   ").is_err());
        let idx = lib.create_collection("Estudos").unwrap();
        assert_eq!(idx, 0);
        assert!(lib.create_collection("estudos").is_err());

        lib.toggle_watch_later(v1.clone());
        lib.toggle_favorite(v2.clone());
        let (_, added) = lib
            .create_collection_with_videos("Fila salva", &[v1.clone(), v1.clone(), v2.clone()])
            .unwrap();
        assert_eq!(added, 2);
        assert_eq!(lib.collections[1].videos.len(), 2);
        assert_eq!(
            lib.next_available_collection_name("Fila salva"),
            "Fila salva (2)"
        );

        assert!(lib.rename_collection(0, "  Estudos Rust  ").is_ok());
        assert_eq!(lib.collections[0].name, "Estudos Rust");
        assert!(lib.rename_collection(0, "fila salva").is_err());

        assert!(lib.remove_from_collection(1, &v1.url));
        assert_eq!(lib.collections[1].videos.len(), 1);
        assert!(!lib.remove_from_collection(1, &v1.url));

        assert!(lib.remove_from_watch_later(&v1.url));
        assert!(lib.watch_later.is_empty());
        assert!(lib.remove_from_favorites(&v2.url));
        assert!(lib.favorites.is_empty());

        let deleted = lib.delete_collection(0).unwrap();
        assert_eq!(deleted.name, "Estudos Rust");
        assert_eq!(lib.collections.len(), 1);
    }

    #[test]
    fn bookmark_edit_remove_and_markdown_export_work() {
        let mut lib = Library::default();
        lib.add_bookmark(Bookmark {
            video_id: "abcdefghijk".into(),
            title: "Aula 1".into(),
            time_ms: 85_000,
            note: "Ponto chave".into(),
        })
        .unwrap();
        lib.add_bookmark(Bookmark {
            video_id: "lmnopqrstuv".into(),
            title: "Aula 2".into(),
            time_ms: 3_665_000,
            note: "  ".into(),
        })
        .unwrap();
        assert!(lib.update_bookmark_note(0, "  Nota editada  ").is_ok());
        assert_eq!(lib.bookmarks[0].note, "Nota editada");

        let md = format_bookmarks_markdown(&lib.bookmarks);
        assert_eq!(
            md,
            "- [01:25](https://www.youtube.com/watch?v=abcdefghijk&t=85s) Nota editada\n- [1:01:05](https://www.youtube.com/watch?v=lmnopqrstuv&t=3665s)"
        );

        assert!(lib.remove_bookmark(1).is_some());
        assert_eq!(lib.bookmarks.len(), 1);
    }

    #[test]
    fn indexed_watch_later_favorites_and_per_bucket_revisions_stay_synchronized() {
        let v1 = Video {
            title: "Video 1".into(),
            channel: "Channel A".into(),
            url: "https://www.youtube.com/watch?v=abcdefghijk".into(),
            thumbnail: String::new(),
            duration: "3:15".into(),
        };
        let v2 = Video {
            title: "Video 2".into(),
            channel: "Channel B".into(),
            url: "https://youtu.be/lmnopqrstuv".into(),
            thumbnail: String::new(),
            duration: "4:20".into(),
        };

        let mut lib = Library::default();
        assert_eq!(lib.watch_later_revision(), 0);
        assert_eq!(lib.favorites_revision(), 0);
        assert_eq!(lib.collections_revision(), 0);
        assert_eq!(lib.bookmarks_revision(), 0);

        // Watch Later mutation only bumps watch_later_revision and updates O(1) index.
        lib.toggle_watch_later(v1.clone());
        assert!(lib.is_in_watch_later("abcdefghijk"));
        assert!(lib.is_in_watch_later("https://youtu.be/abcdefghijk"));
        assert!(!lib.is_in_favorites("abcdefghijk"));
        assert_eq!(lib.watch_later_revision(), 1);
        assert_eq!(lib.favorites_revision(), 0);
        assert_eq!(lib.collections_revision(), 0);
        assert_eq!(lib.bookmarks_revision(), 0);

        // Favorites mutation only bumps favorites_revision.
        assert!(lib.add_to_favorites(v2.clone()));
        assert!(!lib.add_to_favorites(v2.clone())); // duplicate no-op
        assert!(lib.is_in_favorites("lmnopqrstuv"));
        assert_eq!(lib.watch_later_revision(), 1);
        assert_eq!(lib.favorites_revision(), 1);

        // Bookmark mutation only bumps bookmarks_revision.
        lib.add_bookmark(Bookmark {
            video_id: "abcdefghijk".into(),
            title: "Video 1".into(),
            time_ms: 15_000,
            note: "Note".into(),
        })
        .unwrap();
        assert_eq!(lib.bookmarks_revision(), 1);
        assert_eq!(lib.watch_later_revision(), 1);
        assert_eq!(lib.collections_revision(), 0);

        // Collection mutation only bumps collections_revision.
        assert!(lib.toggle_video_in_collection("Study", v1.clone()));
        assert_eq!(lib.collections_revision(), 1);
        assert_eq!(lib.watch_later_revision(), 1);

        // Save & load rebuilds HashSet indexes automatically.
        let path = std::env::temp_dir().join(format!(
            "fasttube-library-index-test-{}.json",
            std::process::id()
        ));
        save(&path, &lib).unwrap();
        let (loaded, fp) = load_with_fingerprint(&path).unwrap();
        assert!(fp.is_some());
        assert!(loaded.is_in_watch_later("abcdefghijk"));
        assert!(loaded.is_in_favorites("lmnopqrstuv"));
        let _ = fs::remove_file(&path);

        // Merge updates indexes and bumps only mutated bucket revisions.
        let mut imported = Library::default();
        imported.toggle_watch_later(v2.clone());
        let (mut merged, summary) = lib.merged(&imported);
        assert_eq!(summary.added, 1);
        assert!(merged.is_in_watch_later("abcdefghijk"));
        assert!(merged.is_in_watch_later("lmnopqrstuv"));
        assert_eq!(
            merged.watch_later_revision(),
            lib.watch_later_revision() + 1
        );
        assert_eq!(merged.favorites_revision(), lib.favorites_revision());

        // Clear empties indexes and bumps revision.
        assert_eq!(merged.clear_watch_later(), 2);
        assert!(!merged.is_in_watch_later("abcdefghijk"));
        assert!(!merged.is_in_watch_later("lmnopqrstuv"));
    }
}
