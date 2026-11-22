use super::{
    bucket_revision_cache_tag, collections_revision_cache_tag, empty_state_card_ui,
    library_uniform_visible_row_range, str_fingerprint,
};
use crate::ui_icons::*;
use crate::*;
use eframe::egui::{self, Color32, RichText};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq)]
struct LocalCollectionDetailCacheKey {
    collection_idx: usize,
    collections_revision: u64,
    library_revision: u64,
    video_count: usize,
    name_len: usize,
    metadata_revision: u64,
}

#[derive(Clone)]
struct CachedLocalCollectionDetail {
    key: LocalCollectionDetailCacheKey,
    name: String,
    videos: Vec<Video>,
    is_short_flags: Vec<bool>,
}

#[derive(Clone)]
struct PlaylistGridEntry {
    card: playlist_sync::SyncedPlaylistCard,
    local_idx: Option<usize>,
    sort_order: usize,
}

#[derive(Clone, PartialEq, Eq)]
struct PlaylistsGridCacheKey {
    remote_revision: u64,
    remote_items_len: usize,
    oauth_items_len: usize,
    has_innertube_items: bool,
    server_sorted_for: Option<playlist_sync::PlaylistSort>,
    collections_revision: u64,
    collections_tag: u64,
    wl_display_total: usize,
    wl_cover_thumb: String,
    liked_display_total: usize,
    liked_cover_thumb: String,
    active_filter: playlist_sync::PlaylistCategoryFilter,
    active_sort: playlist_sync::PlaylistSort,
    pt: bool,
}

fn color32_from_argb_u32(argb: Option<u32>) -> Option<Color32> {
    let c = argb?;
    let a = ((c >> 24) & 0xFF) as u8;
    if a == 0 {
        return None;
    }
    let r = ((c >> 16) & 0xFF) as u8;
    let g = ((c >> 8) & 0xFF) as u8;
    let b = (c & 0xFF) as u8;
    Some(Color32::from_rgba_unmultiplied(r, g, b, a))
}

#[derive(Clone)]
struct CachedPlaylistsGrid {
    key: PlaylistsGridCacheKey,
    entries: Vec<PlaylistGridEntry>,
}

pub(crate) fn derive_playlist_gradient_stops(
    seed_text: &str,
    dark_theme: bool,
) -> (Color32, Color32, Color32) {
    let fp = str_fingerprint(seed_text);
    let r_seed = ((fp & 0xff) as u8).clamp(36, 210);
    let g_seed = (((fp >> 11) & 0xff) as u8).clamp(36, 195);
    let b_seed = (((fp >> 23) & 0xff) as u8).clamp(48, 220);
    if dark_theme {
        let top_left = Color32::from_rgb(
            (u16::from(r_seed) * 58 / 100) as u8,
            (u16::from(g_seed) * 48 / 100) as u8,
            (u16::from(b_seed) * 64 / 100) as u8,
        );
        let top_right = Color32::from_rgb(
            (u16::from(b_seed) * 44 / 100) as u8,
            (u16::from(r_seed) * 40 / 100) as u8,
            (u16::from(g_seed) * 56 / 100) as u8,
        );
        let bottom = Color32::from_rgb(20, 22, 28);
        (top_left, top_right, bottom)
    } else {
        let top_left =
            Color32::from_rgb(188 + (r_seed / 5), 186 + (g_seed / 5), 196 + (b_seed / 6));
        let top_right =
            Color32::from_rgb(194 + (b_seed / 6), 188 + (r_seed / 5), 198 + (g_seed / 5));
        let bottom = Color32::from_rgb(242, 243, 247);
        (top_left, top_right, bottom)
    }
}

fn paint_playlist_gradient_banner_backdrop(
    ui: &egui::Ui,
    rect: egui::Rect,
    top_left: Color32,
    top_right: Color32,
    bottom: Color32,
) {
    let inset = rect.shrink(1.5);
    if inset.width() <= 2.0 || inset.height() <= 2.0 {
        return;
    }
    let mut mesh = egui::epaint::Mesh::default();
    let v0 = mesh.vertices.len() as u32;
    mesh.vertices.push(egui::epaint::Vertex {
        pos: inset.left_top(),
        uv: egui::epaint::WHITE_UV,
        color: top_left,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: inset.right_top(),
        uv: egui::epaint::WHITE_UV,
        color: top_right,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: inset.right_bottom(),
        uv: egui::epaint::WHITE_UV,
        color: bottom,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: inset.left_bottom(),
        uv: egui::epaint::WHITE_UV,
        color: bottom,
    });
    mesh.add_triangle(v0, v0 + 1, v0 + 2);
    mesh.add_triangle(v0, v0 + 2, v0 + 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

impl FastTube {
    pub(crate) fn ensure_liked_videos_merged_cache(&mut self) {
        let local_bucket_tag = bucket_revision_cache_tag(
            self.library.data.favorites_revision(),
            self.library
                .data
                .favorites
                .first()
                .map(|r| r.video.url.as_str()),
            self.library
                .data
                .favorites
                .last()
                .map(|r| r.video.url.as_str()),
        );
        let key = Some((
            local_bucket_tag,
            self.library.data.favorites.len(),
            self.library.liked_videos.revision,
            self.library.liked_videos.items.len(),
        ));
        if self.library.liked_videos.merged_cache_key == key {
            return;
        }

        // Incremental fast path for progressive remote streaming: append only newly
        // received remote items when the existing merged prefix matches `items[..prev_remote]`.
        let prev_remote_count = self
            .library
            .liked_videos
            .merged_videos
            .len()
            .saturating_sub(self.library.liked_videos.extra_local_count);
        let remote_len = self.library.liked_videos.items.len();
        let can_append_incrementally = prev_remote_count > 0
            && prev_remote_count <= remote_len
            && self.library.liked_videos.seen_keys.len() == remote_len
            && self.library.liked_videos.merged_videos.len()
                == self.library.liked_videos.merged_is_short.len()
            && self
                .library
                .liked_videos
                .merged_videos
                .first()
                .map(|v| v.url.as_str())
                == self
                    .library
                    .liked_videos
                    .items
                    .first()
                    .map(|it| it.video.url.as_str())
            && self
                .library
                .liked_videos
                .merged_videos
                .get(prev_remote_count - 1)
                .map(|v| v.url.as_str())
                == self
                    .library
                    .liked_videos
                    .items
                    .get(prev_remote_count - 1)
                    .map(|it| it.video.url.as_str());

        if can_append_incrementally {
            self.library
                .liked_videos
                .merged_videos
                .truncate(prev_remote_count);
            self.library
                .liked_videos
                .merged_is_short
                .truncate(prev_remote_count);
            for item in &self.library.liked_videos.items[prev_remote_count..] {
                let is_short = item.metadata.kind == search::SearchResultKind::Short
                    || item.video.url.contains("/shorts/");
                self.library
                    .liked_videos
                    .merged_videos
                    .push(item.video.clone());
                self.library.liked_videos.merged_is_short.push(is_short);
            }
            self.library.liked_videos.extra_local_count = 0;
            if !self.library.data.favorites.is_empty() {
                let mut local_seen = HashSet::with_capacity(self.library.data.favorites.len());
                for rec in &self.library.data.favorites {
                    let k = history::youtube_video_id(&rec.video.url)
                        .unwrap_or_else(|| rec.video.url.clone());
                    if !self.library.liked_videos.seen_keys.contains(&k) && local_seen.insert(k) {
                        let is_short = self
                            .video_metadata
                            .get(&rec.video.url)
                            .is_some_and(|m| m.kind == search::SearchResultKind::Short)
                            || video_is_short(&rec.video);
                        self.library
                            .liked_videos
                            .merged_videos
                            .push(rec.video.clone());
                        self.library.liked_videos.merged_is_short.push(is_short);
                        self.library.liked_videos.extra_local_count += 1;
                    }
                }
            }
            self.library.liked_videos.merged_cache_key = key;
            return;
        }

        self.library.liked_videos.merged_videos.clear();
        self.library.liked_videos.merged_is_short.clear();
        self.library.liked_videos.extra_local_count = 0;
        let mut seen = HashSet::with_capacity(remote_len + self.library.data.favorites.len());
        for item in &self.library.liked_videos.items {
            let k = history::youtube_video_id(&item.video.url)
                .unwrap_or_else(|| item.video.url.clone());
            if seen.insert(k) {
                let is_short = item.metadata.kind == search::SearchResultKind::Short
                    || item.video.url.contains("/shorts/");
                self.library
                    .liked_videos
                    .merged_videos
                    .push(item.video.clone());
                self.library.liked_videos.merged_is_short.push(is_short);
            }
        }
        for rec in &self.library.data.favorites {
            let k =
                history::youtube_video_id(&rec.video.url).unwrap_or_else(|| rec.video.url.clone());
            if seen.insert(k) {
                let is_short = self
                    .video_metadata
                    .get(&rec.video.url)
                    .is_some_and(|m| m.kind == search::SearchResultKind::Short)
                    || video_is_short(&rec.video);
                self.library
                    .liked_videos
                    .merged_videos
                    .push(rec.video.clone());
                self.library.liked_videos.merged_is_short.push(is_short);
                self.library.liked_videos.extra_local_count += 1;
            }
        }
        self.library.liked_videos.merged_cache_key = key;
    }

    pub(crate) fn liked_videos_display_total(&self) -> usize {
        let remote_loaded = self.library.liked_videos.items.len();
        let remote_total = self
            .library
            .liked_videos
            .total_results
            .map(|t| (t as usize).max(remote_loaded))
            .unwrap_or(remote_loaded);
        if self.library.liked_videos.rx.is_some() {
            remote_total.max(self.library.liked_videos.merged_videos.len())
        } else {
            remote_total.saturating_add(self.library.liked_videos.extra_local_count)
        }
    }

    pub(crate) fn ensure_watch_later_merged_cache(&mut self) {
        let local_bucket_tag = bucket_revision_cache_tag(
            self.library.data.watch_later_revision(),
            self.library
                .data
                .watch_later
                .first()
                .map(|r| r.video.url.as_str()),
            self.library
                .data
                .watch_later
                .last()
                .map(|r| r.video.url.as_str()),
        );
        let key = Some((
            local_bucket_tag,
            self.library.data.watch_later.len(),
            self.library.watch_later_remote.revision,
            self.library.watch_later_remote.items.len(),
        ));
        if self.library.watch_later_remote.merged_cache_key == key {
            return;
        }

        let prev_remote_count = self
            .library
            .watch_later_remote
            .merged_videos
            .len()
            .saturating_sub(self.library.watch_later_remote.extra_local_count);
        let remote_len = self.library.watch_later_remote.items.len();
        let can_append_incrementally = prev_remote_count > 0
            && prev_remote_count <= remote_len
            && self.library.watch_later_remote.seen_keys.len() == remote_len
            && self.library.watch_later_remote.merged_videos.len()
                == self.library.watch_later_remote.merged_is_short.len()
            && self
                .library
                .watch_later_remote
                .merged_videos
                .first()
                .map(|v| v.url.as_str())
                == self
                    .library
                    .watch_later_remote
                    .items
                    .first()
                    .map(|it| it.result.video.url.as_str())
            && self
                .library
                .watch_later_remote
                .merged_videos
                .get(prev_remote_count - 1)
                .map(|v| v.url.as_str())
                == self
                    .library
                    .watch_later_remote
                    .items
                    .get(prev_remote_count - 1)
                    .map(|it| it.result.video.url.as_str());

        if can_append_incrementally {
            self.library
                .watch_later_remote
                .merged_videos
                .truncate(prev_remote_count);
            self.library
                .watch_later_remote
                .merged_is_short
                .truncate(prev_remote_count);
            for item in &self.library.watch_later_remote.items[prev_remote_count..] {
                let is_short = item.result.metadata.kind == search::SearchResultKind::Short
                    || item.result.video.url.contains("/shorts/");
                self.library
                    .watch_later_remote
                    .merged_videos
                    .push(item.result.video.clone());
                self.library
                    .watch_later_remote
                    .merged_is_short
                    .push(is_short);
            }
            self.library.watch_later_remote.extra_local_count = 0;
            if !self.library.data.watch_later.is_empty() {
                let mut local_seen = HashSet::with_capacity(self.library.data.watch_later.len());
                for rec in &self.library.data.watch_later {
                    let k = history::youtube_video_id(&rec.video.url)
                        .unwrap_or_else(|| rec.video.url.clone());
                    if !self.library.watch_later_remote.seen_keys.contains(&k)
                        && local_seen.insert(k)
                    {
                        let is_short = self
                            .video_metadata
                            .get(&rec.video.url)
                            .is_some_and(|m| m.kind == search::SearchResultKind::Short)
                            || video_is_short(&rec.video);
                        self.library
                            .watch_later_remote
                            .merged_videos
                            .push(rec.video.clone());
                        self.library
                            .watch_later_remote
                            .merged_is_short
                            .push(is_short);
                        self.library.watch_later_remote.extra_local_count += 1;
                    }
                }
            }
            self.library.watch_later_remote.merged_cache_key = key;
            return;
        }

        self.library.watch_later_remote.merged_videos.clear();
        self.library.watch_later_remote.merged_is_short.clear();
        self.library.watch_later_remote.extra_local_count = 0;
        let mut seen = HashSet::with_capacity(remote_len + self.library.data.watch_later.len());
        for item in &self.library.watch_later_remote.items {
            let k = history::youtube_video_id(&item.result.video.url)
                .unwrap_or_else(|| item.result.video.url.clone());
            if seen.insert(k) {
                let is_short = item.result.metadata.kind == search::SearchResultKind::Short
                    || item.result.video.url.contains("/shorts/");
                self.library
                    .watch_later_remote
                    .merged_videos
                    .push(item.result.video.clone());
                self.library
                    .watch_later_remote
                    .merged_is_short
                    .push(is_short);
            }
        }
        for rec in &self.library.data.watch_later {
            let k =
                history::youtube_video_id(&rec.video.url).unwrap_or_else(|| rec.video.url.clone());
            if seen.insert(k) {
                let is_short = self
                    .video_metadata
                    .get(&rec.video.url)
                    .is_some_and(|m| m.kind == search::SearchResultKind::Short)
                    || video_is_short(&rec.video);
                self.library
                    .watch_later_remote
                    .merged_videos
                    .push(rec.video.clone());
                self.library
                    .watch_later_remote
                    .merged_is_short
                    .push(is_short);
                self.library.watch_later_remote.extra_local_count += 1;
            }
        }
        self.library.watch_later_remote.merged_cache_key = key;
    }

    pub(crate) fn ensure_active_playlist_detail_merged_cache(&mut self) {
        let pl_fp = self
            .library
            .active_playlist_detail
            .playlist_id
            .as_deref()
            .map(str_fingerprint)
            .unwrap_or(0);
        let remote_len = self.library.active_playlist_detail.items.len();
        let key = Some((
            pl_fp,
            0usize,
            self.library.active_playlist_detail.revision,
            remote_len,
        ));
        if self.library.active_playlist_detail.merged_cache_key == key {
            return;
        }
        let prev_len = self.library.active_playlist_detail.merged_videos.len();
        let can_append = prev_len > 0
            && prev_len <= remote_len
            && prev_len == self.library.active_playlist_detail.merged_is_short.len()
            && self
                .library
                .active_playlist_detail
                .merged_videos
                .first()
                .map(|v| v.url.as_str())
                == self
                    .library
                    .active_playlist_detail
                    .items
                    .first()
                    .map(|it| it.result.video.url.as_str())
            && self
                .library
                .active_playlist_detail
                .merged_videos
                .get(prev_len - 1)
                .map(|v| v.url.as_str())
                == self
                    .library
                    .active_playlist_detail
                    .items
                    .get(prev_len - 1)
                    .map(|it| it.result.video.url.as_str());
        if can_append {
            for item in &self.library.active_playlist_detail.items[prev_len..] {
                let is_short = item.result.metadata.kind == search::SearchResultKind::Short
                    || item.result.video.url.contains("/shorts/");
                self.library
                    .active_playlist_detail
                    .merged_videos
                    .push(item.result.video.clone());
                self.library
                    .active_playlist_detail
                    .merged_is_short
                    .push(is_short);
            }
            self.library.active_playlist_detail.merged_cache_key = key;
            return;
        }
        self.library.active_playlist_detail.merged_videos.clear();
        self.library.active_playlist_detail.merged_is_short.clear();
        self.library.active_playlist_detail.extra_local_count = 0;
        self.library
            .active_playlist_detail
            .merged_videos
            .reserve(remote_len);
        self.library
            .active_playlist_detail
            .merged_is_short
            .reserve(remote_len);
        for item in &self.library.active_playlist_detail.items {
            let is_short = item.result.metadata.kind == search::SearchResultKind::Short
                || item.result.video.url.contains("/shorts/");
            self.library
                .active_playlist_detail
                .merged_videos
                .push(item.result.video.clone());
            self.library
                .active_playlist_detail
                .merged_is_short
                .push(is_short);
        }
        self.library.active_playlist_detail.merged_cache_key = key;
    }

    pub(crate) fn watch_later_display_total(&self) -> usize {
        let remote_loaded = self.library.watch_later_remote.items.len();
        let remote_total = self
            .library
            .watch_later_remote
            .total_results
            .map(|t| (t as usize).max(remote_loaded))
            .unwrap_or(remote_loaded);
        if self.library.watch_later_remote.rx.is_some() {
            remote_total.max(self.library.watch_later_remote.merged_videos.len())
        } else {
            remote_total.saturating_add(self.library.watch_later_remote.extra_local_count)
        }
    }

    pub(crate) fn playlists_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> Option<Video> {
        let pt = self.ui_pt();
        if let Some(idx) = self.library.selected_collection_idx {
            if let Some(col) = self.library.data.collections.get(idx) {
                let col_cache_id = egui::Id::new("fasttube_local_collection_detail_cache");
                let col_key = LocalCollectionDetailCacheKey {
                    collection_idx: idx,
                    collections_revision: self.library.data.collections_revision(),
                    library_revision: self.library.revision,
                    video_count: col.videos.len(),
                    name_len: col.name.len(),
                    metadata_revision: self.feed.metadata_revision,
                };
                let cached_col = ctx
                    .data(|d| d.get_temp::<Arc<CachedLocalCollectionDetail>>(col_cache_id))
                    .filter(|c| c.key == col_key)
                    .unwrap_or_else(|| {
                        let videos: Vec<Video> =
                            col.videos.iter().map(|r| r.video.clone()).collect();
                        let is_short_flags: Vec<bool> = videos
                            .iter()
                            .map(|v| {
                                self.metadata_for(&v.url)
                                    .is_some_and(|m| m.kind == search::SearchResultKind::Short)
                                    || video_is_short(v)
                            })
                            .collect();
                        let built = Arc::new(CachedLocalCollectionDetail {
                            key: col_key,
                            name: col.name.clone(),
                            videos,
                            is_short_flags,
                        });
                        ctx.data_mut(|d| d.insert_temp(col_cache_id, Arc::clone(&built)));
                        built
                    });
                let videos = &cached_col.videos;
                let is_short_flags = &cached_col.is_short_flags;
                let count = videos.len();
                let count_fmt = format_grouped_count(count, pt);
                let owner = self
                    .auth
                    .my_channel_title
                    .clone()
                    .unwrap_or_else(|| if pt { "Privada" } else { "Private" }.to_owned());
                let subtitle = if pt {
                    format!(
                        "{owner} · {count_fmt} {} · Playlist local",
                        if count == 1 { "vídeo" } else { "vídeos" }
                    )
                } else {
                    format!(
                        "{owner} · {count_fmt} {} · Local playlist",
                        if count == 1 { "video" } else { "videos" }
                    )
                };
                let pl_id = format!("local:{idx}");
                let (chosen, remove_url, _, _) = self.youtube_playlist_detail_page_ui(
                    ui,
                    ctx,
                    videos,
                    is_short_flags,
                    PlaylistDetailSpec {
                        playlist_id: &pl_id,
                        owner: &owner,
                        privacy: playlist_sync::PlaylistPrivacy::Private,
                        total_count: Some(count),
                        local_collection_idx: Some(idx),
                        is_editable: true,
                        title: &cached_col.name,
                        subtitle: &subtitle,
                        empty_message: if pt {
                            "Nenhum vídeo nesta playlist ainda."
                        } else {
                            "No videos in this playlist yet."
                        },
                        scroll_id: "playlist_detail_local_scroll",
                        can_back_to_playlists: true,
                        remove_label: Some(if pt {
                            "Remover da playlist"
                        } else {
                            "Remove from playlist"
                        }),
                        loading: false,
                        can_refresh: false,
                        error: None,
                    },
                    palette,
                );
                if let Some(url) = remove_url
                    && self.library.data.remove_from_collection(idx, &url)
                {
                    self.save_library();
                }
                return chosen;
            }
            self.library.selected_collection_idx = None;
        }

        if let Some(active_pl_id) = self.library.active_playlist_detail.playlist_id.clone() {
            self.ensure_active_playlist_detail_merged_cache();
            let videos = std::mem::take(&mut self.library.active_playlist_detail.merged_videos);
            let is_short_flags =
                std::mem::take(&mut self.library.active_playlist_detail.merged_is_short);
            let meta = self.library.active_playlist_detail.meta.clone();
            let title = meta
                .as_ref()
                .map(|m| m.title.clone())
                .filter(|s| !s.trim().is_empty())
                .or_else(|| self.library.active_remote_playlist_title.clone())
                .unwrap_or_else(|| {
                    if pt {
                        "Playlist do YouTube"
                    } else {
                        "YouTube Playlist"
                    }
                    .to_owned()
                });
            let owner = meta
                .as_ref()
                .map(|m| m.owner.clone())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "YouTube".to_owned());
            let privacy = meta
                .as_ref()
                .map(|m| m.privacy)
                .unwrap_or(playlist_sync::PlaylistPrivacy::Unknown);
            let is_editable = meta.as_ref().is_some_and(|m| m.is_editable);
            let loaded_count = videos.len();
            let total_count = self
                .library
                .active_playlist_detail
                .total_results
                .map(|t| (t as usize).max(loaded_count))
                .unwrap_or(loaded_count);
            let total_fmt = format_grouped_count(total_count, pt);
            let loaded_fmt = format_grouped_count(loaded_count, pt);
            let loading = self.library.active_playlist_detail.rx.is_some();
            let error = self.library.active_playlist_detail.error.clone();
            let privacy_lbl = privacy.label(pt);
            let count_suffix = if loading {
                if total_count > 0 {
                    if pt {
                        format!("{total_fmt} vídeos (carregando {loaded_fmt}/{total_fmt}...)")
                    } else {
                        format!("{total_fmt} videos (loading {loaded_fmt}/{total_fmt}...)")
                    }
                } else if pt {
                    "Carregando vídeos da playlist...".to_owned()
                } else {
                    "Loading playlist videos...".to_owned()
                }
            } else if pt {
                format!(
                    "{total_fmt} {}",
                    if total_count == 1 {
                        "vídeo"
                    } else {
                        "vídeos"
                    }
                )
            } else {
                format!(
                    "{total_fmt} {}",
                    if total_count == 1 { "video" } else { "videos" }
                )
            };
            let subtitle = if privacy_lbl.is_empty() {
                format!("{owner} · {count_suffix}")
            } else {
                format!("{owner} · {count_suffix} · {privacy_lbl}")
            };
            let remove_label = is_editable.then_some(if pt {
                "Remover da playlist"
            } else {
                "Remove from playlist"
            });
            let (chosen, remove_url, refresh_clicked, cancel_clicked) = self
                .youtube_playlist_detail_page_ui(
                    ui,
                    ctx,
                    &videos,
                    &is_short_flags,
                    PlaylistDetailSpec {
                        playlist_id: &active_pl_id,
                        owner: &owner,
                        privacy,
                        total_count: Some(total_count),
                        local_collection_idx: None,
                        is_editable,
                        title: &title,
                        subtitle: &subtitle,
                        empty_message: if pt {
                            "Nenhum vídeo encontrado nesta playlist."
                        } else {
                            "No videos found in this playlist."
                        },
                        scroll_id: "playlist_detail_synced_scroll",
                        can_back_to_playlists: true,
                        remove_label,
                        loading,
                        can_refresh: true,
                        error: error.as_deref(),
                    },
                    palette,
                );
            self.library.active_playlist_detail.merged_videos = videos;
            self.library.active_playlist_detail.merged_is_short = is_short_flags;
            if cancel_clicked {
                self.library.active_playlist_detail.cancel_active();
            } else if refresh_clicked {
                self.open_remote_playlist_detail(active_pl_id.clone(), Some(title), None, ctx);
            }
            if let Some(url) = remove_url
                && let Some(item) = self
                    .library
                    .active_playlist_detail
                    .items
                    .iter()
                    .find(|it| it.result.video.url == url)
                    .cloned()
                && let Some(vid_id) = history::youtube_video_id(&url)
            {
                self.dispatch_playlist_remove_video(active_pl_id, vid_id, item.set_video_id, ctx);
            }
            return chosen;
        }

        if self.feed.home.title.as_deref() == Some("Vídeos da playlist") {
            let videos = std::mem::take(&mut self.feed.videos);
            let is_short_flags: Vec<bool> = videos
                .iter()
                .map(|v| {
                    self.video_metadata
                        .get(&v.url)
                        .is_some_and(|m| m.kind == search::SearchResultKind::Short)
                        || v.url.contains("/shorts/")
                })
                .collect();
            let loaded_count = videos.len();
            let total_count = self
                .feed
                .home
                .playlist_total_results
                .map(|t| (t as usize).max(loaded_count))
                .unwrap_or(loaded_count);
            let total_fmt = format_grouped_count(total_count, pt);
            let loaded_fmt = format_grouped_count(loaded_count, pt);
            let fallback_title = if pt {
                "Playlist do YouTube"
            } else {
                "YouTube Playlist"
            };
            let custom_title = self.library.active_remote_playlist_title.clone();
            let title = custom_title.as_deref().unwrap_or(fallback_title);
            let loading = self.feed.home.stream_task.is_running();
            let subtitle = if loading {
                if total_count > 0 {
                    if pt {
                        format!(
                            "{total_fmt} vídeos (carregando {loaded_fmt}/{total_fmt}...) · Conta Google"
                        )
                    } else {
                        format!(
                            "{total_fmt} videos (loading {loaded_fmt}/{total_fmt}...) · Google Account"
                        )
                    }
                } else if pt {
                    "Carregando vídeos da playlist...".to_owned()
                } else {
                    "Loading playlist videos...".to_owned()
                }
            } else if pt {
                format!(
                    "{total_fmt} {} · Conta Google",
                    if total_count == 1 {
                        "vídeo"
                    } else {
                        "vídeos"
                    }
                )
            } else {
                format!(
                    "{total_fmt} {} · Google Account",
                    if total_count == 1 { "video" } else { "videos" }
                )
            };
            let (chosen, _, _, _) = self.youtube_playlist_detail_page_ui(
                ui,
                ctx,
                &videos,
                &is_short_flags,
                PlaylistDetailSpec {
                    playlist_id: "remote",
                    owner: "",
                    privacy: playlist_sync::PlaylistPrivacy::Unknown,
                    total_count: Some(total_count),
                    local_collection_idx: None,
                    is_editable: false,
                    title,
                    subtitle: &subtitle,
                    empty_message: if pt {
                        "Nenhum vídeo encontrado nesta playlist."
                    } else {
                        "No videos found in this playlist."
                    },
                    scroll_id: "playlist_detail_remote_scroll",
                    can_back_to_playlists: true,
                    remove_label: None,
                    loading,
                    can_refresh: false,
                    error: None,
                },
                palette,
            );
            self.feed.videos = videos;
            return chosen;
        }

        self.ensure_watch_later_merged_cache();
        self.ensure_liked_videos_merged_cache();
        let wl_display_total = self
            .watch_later_display_total()
            .max(self.library.data.watch_later.len());
        let wl_cover_thumb = self
            .library
            .watch_later_remote
            .merged_videos
            .first()
            .or_else(|| self.library.data.watch_later.first().map(|r| &r.video))
            .map(video_thumb_url)
            .unwrap_or_default();
        let liked_display_total = self.liked_videos_display_total();
        let liked_cover_thumb = self
            .library
            .liked_videos
            .merged_videos
            .first()
            .map(video_thumb_url)
            .unwrap_or_default();

        let dark_theme = self.is_dark_theme();
        let active_filter = self.library.remote_playlists.active_filter;
        let active_sort = self.library.remote_playlists.active_sort;
        let server_sorted_for = self.library.remote_playlists.server_sorted_for;
        let has_innertube_items = !self.library.remote_playlists.all_items.is_empty();
        let remote_loading = self.library.remote_playlists.rx.is_some()
            || self.auth.remote_playlists_task.is_running();
        let needs_connect =
            self.library.remote_playlists.needs_connect && self.auth.youtube_web_session.is_none();
        let remote_error = self.library.remote_playlists.error.clone();
        let library_notice = self.library.notice.clone();

        let grid_cache_id = egui::Id::new("fasttube_playlists_grid_cache");
        let collections_rev = self.library.data.collections_revision();
        let grid_cache_key = PlaylistsGridCacheKey {
            remote_revision: self.library.remote_playlists.revision,
            remote_items_len: self.library.remote_playlists.items.len(),
            oauth_items_len: self.library.remote_playlists.oauth_items.len(),
            has_innertube_items,
            server_sorted_for,
            collections_revision: collections_rev,
            collections_tag: collections_revision_cache_tag(
                collections_rev,
                &self.library.data.collections,
            ),
            wl_display_total,
            wl_cover_thumb: wl_cover_thumb.clone(),
            liked_display_total,
            liked_cover_thumb: liked_cover_thumb.clone(),
            active_filter,
            active_sort,
            pt,
        };
        let cached_grid = ctx
            .data(|d| d.get_temp::<Arc<CachedPlaylistsGrid>>(grid_cache_id))
            .filter(|c| c.key == grid_cache_key)
            .unwrap_or_else(|| {
                let mut synced_cards = self.library.remote_playlists.items.clone();
                if !has_innertube_items
                    && synced_cards.is_empty()
                    && !self.library.remote_playlists.oauth_items.is_empty()
                {
                    synced_cards = self
                        .library
                        .remote_playlists
                        .oauth_items
                        .iter()
                        .filter(|c| c.matches_filter(active_filter))
                        .cloned()
                        .collect();
                }
                let mut entries: Vec<PlaylistGridEntry> = Vec::with_capacity(
                    synced_cards.len() + self.library.data.collections.len() + 2,
                );
                let mut has_synced_wl = false;
                let mut has_synced_ll = false;
                let mut order_counter = 0_usize;
                for mut card in synced_cards {
                    if card.id == "WL" {
                        has_synced_wl = true;
                        let best_count = card
                            .video_count
                            .map(|c| (c as usize).max(wl_display_total))
                            .unwrap_or(wl_display_total);
                        card.video_count = Some(best_count as u64);
                        card.badge_text = format!(
                            "{} {}",
                            format_grouped_count(best_count, pt),
                            if pt {
                                if best_count == 1 { "vídeo" } else { "vídeos" }
                            } else if best_count == 1 {
                                "video"
                            } else {
                                "videos"
                            }
                        );
                        if card.thumbnail_url.trim().is_empty() && !wl_cover_thumb.is_empty() {
                            card.thumbnail_url = wl_cover_thumb.clone();
                        }
                    } else if card.id == "LL" {
                        has_synced_ll = true;
                        let best_count = card
                            .video_count
                            .map(|c| (c as usize).max(liked_display_total))
                            .unwrap_or(liked_display_total);
                        card.video_count = Some(best_count as u64);
                        card.badge_text = format!(
                            "{} {}",
                            format_grouped_count(best_count, pt),
                            if pt {
                                if best_count == 1 { "vídeo" } else { "vídeos" }
                            } else if best_count == 1 {
                                "video"
                            } else {
                                "videos"
                            }
                        );
                        if card.thumbnail_url.trim().is_empty() && !liked_cover_thumb.is_empty() {
                            card.thumbnail_url = liked_cover_thumb.clone();
                        }
                    }
                    entries.push(PlaylistGridEntry {
                        card,
                        local_idx: None,
                        sort_order: order_counter,
                    });
                    order_counter += 1;
                }

                let include_owned_fallbacks = matches!(
                    active_filter,
                    playlist_sync::PlaylistCategoryFilter::All
                        | playlist_sync::PlaylistCategoryFilter::Playlists
                        | playlist_sync::PlaylistCategoryFilter::Owned
                );
                if !has_innertube_items && include_owned_fallbacks && !has_synced_wl {
                    entries.push(PlaylistGridEntry {
                        card: playlist_sync::SyncedPlaylistCard {
                            id: "WL".into(),
                            title: if pt {
                                "Assistir mais tarde"
                            } else {
                                "Watch later"
                            }
                            .into(),
                            thumbnail_url: wl_cover_thumb,
                            video_count: Some(wl_display_total as u64),
                            badge_text: format!(
                                "{} {}",
                                format_grouped_count(wl_display_total, pt),
                                if pt {
                                    if wl_display_total == 1 {
                                        "vídeo"
                                    } else {
                                        "vídeos"
                                    }
                                } else if wl_display_total == 1 {
                                    "video"
                                } else {
                                    "videos"
                                }
                            ),
                            owner_label: if pt { "Particular" } else { "Private" }.into(),
                            subtitle_line: if pt {
                                "Particular · Playlist"
                            } else {
                                "Private · Playlist"
                            }
                            .into(),
                            updated_text: None,
                            privacy: playlist_sync::PlaylistPrivacy::Private,
                            kind: playlist_sync::PlaylistKind::WatchLater,
                            is_mix: false,
                            is_music: false,
                            is_owned: true,
                            is_saved: false,
                            seed_video_id: None,
                            browse_id: Some("VLWL".into()),
                            has_full_playlist_link: true,
                            stack_color_dark: None,
                            stack_color_light: None,
                        },
                        local_idx: None,
                        sort_order: order_counter,
                    });
                    order_counter += 1;
                }
                if !has_innertube_items && include_owned_fallbacks && !has_synced_ll {
                    entries.push(PlaylistGridEntry {
                        card: playlist_sync::SyncedPlaylistCard {
                            id: "LL".into(),
                            title: if pt {
                                "Vídeos com \"Gostei\""
                            } else {
                                "Liked videos"
                            }
                            .into(),
                            thumbnail_url: liked_cover_thumb,
                            video_count: Some(liked_display_total as u64),
                            badge_text: format!(
                                "{} {}",
                                format_grouped_count(liked_display_total, pt),
                                if pt {
                                    if liked_display_total == 1 {
                                        "vídeo"
                                    } else {
                                        "vídeos"
                                    }
                                } else if liked_display_total == 1 {
                                    "video"
                                } else {
                                    "videos"
                                }
                            ),
                            owner_label: if pt { "Particular" } else { "Private" }.into(),
                            subtitle_line: if pt {
                                "Particular · Playlist"
                            } else {
                                "Private · Playlist"
                            }
                            .into(),
                            updated_text: None,
                            privacy: playlist_sync::PlaylistPrivacy::Private,
                            kind: playlist_sync::PlaylistKind::LikedVideos,
                            is_mix: false,
                            is_music: false,
                            is_owned: true,
                            is_saved: false,
                            seed_video_id: None,
                            browse_id: Some("VLLL".into()),
                            has_full_playlist_link: true,
                            stack_color_dark: None,
                            stack_color_light: None,
                        },
                        local_idx: None,
                        sort_order: order_counter,
                    });
                    order_counter += 1;
                }
                if include_owned_fallbacks {
                    for (idx, col) in self.library.data.collections.iter().enumerate() {
                        let vid_count = col.videos.len();
                        let thumb_url = col
                            .videos
                            .first()
                            .map(|r| video_thumb_url(&r.video))
                            .unwrap_or_default();
                        entries.push(PlaylistGridEntry {
                            card: playlist_sync::SyncedPlaylistCard {
                                id: format!("local:{idx}"),
                                title: col.name.clone(),
                                thumbnail_url: thumb_url,
                                video_count: Some(vid_count as u64),
                                badge_text: format!(
                                    "{} {}",
                                    format_grouped_count(vid_count, pt),
                                    if pt {
                                        if vid_count == 1 { "vídeo" } else { "vídeos" }
                                    } else if vid_count == 1 {
                                        "video"
                                    } else {
                                        "videos"
                                    }
                                ),
                                owner_label: "Local".into(),
                                subtitle_line: "Local · Playlist".into(),
                                updated_text: None,
                                privacy: playlist_sync::PlaylistPrivacy::Private,
                                kind: playlist_sync::PlaylistKind::Local,
                                is_mix: false,
                                is_music: false,
                                is_owned: true,
                                is_saved: false,
                                seed_video_id: None,
                                browse_id: None,
                                has_full_playlist_link: true,
                                stack_color_dark: None,
                                stack_color_light: None,
                            },
                            local_idx: Some(idx),
                            sort_order: order_counter,
                        });
                        order_counter += 1;
                    }
                }
                if active_sort == playlist_sync::PlaylistSort::AlphabeticalAZ
                    && server_sorted_for != Some(playlist_sync::PlaylistSort::AlphabeticalAZ)
                {
                    let mut keyed: Vec<(String, PlaylistGridEntry)> = entries
                        .into_iter()
                        .map(|e| (e.card.title.to_lowercase(), e))
                        .collect();
                    keyed.sort_by(|a, b| {
                        a.0.cmp(&b.0)
                            .then_with(|| a.1.sort_order.cmp(&b.1.sort_order))
                    });
                    entries = keyed.into_iter().map(|(_, e)| e).collect();
                }
                let built = Arc::new(CachedPlaylistsGrid {
                    key: grid_cache_key,
                    entries,
                });
                ctx.data_mut(|d| d.insert_temp(grid_cache_id, Arc::clone(&built)));
                built
            });
        let grid_entries = &cached_grid.entries;

        let mut next_filter_sort: Option<(
            playlist_sync::PlaylistCategoryFilter,
            playlist_sync::PlaylistSort,
        )> = None;
        let mut connect_youtube = false;
        let mut refresh_playlists = false;
        let mut open_section: Option<BrowseSection> = None;
        let mut open_local_idx: Option<usize> = None;
        let mut delete_local_idx: Option<usize> = None;
        let mut open_detail_card: Option<playlist_sync::SyncedPlaylistCard> = None;
        let mut open_mix_card: Option<playlist_sync::SyncedPlaylistCard> = None;
        let mut play_card: Option<(playlist_sync::SyncedPlaylistCard, Option<usize>)> = None;
        let mut share_card: Option<playlist_sync::SyncedPlaylistCard> = None;

        egui::ScrollArea::vertical()
            .id_salt("browse_playlists_grid_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Playlists")
                            .size(34.0)
                            .strong()
                            .color(palette.channel_fg),
                    );
                    if remote_loading {
                        ui.add_space(6.0);
                        crate::skeleton::visible_spinner(ui);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if styled_pill_button(
                            ui,
                            if pt {
                                "+ Nova playlist"
                            } else {
                                "+ New playlist"
                            },
                            self.library.new_playlist_composer_open,
                            &palette,
                        )
                        .clicked()
                        {
                            self.library.new_playlist_composer_open =
                                !self.library.new_playlist_composer_open;
                        }
                        if styled_pill_button(ui, "↻", false, &palette)
                            .on_hover_text(if pt {
                                "Atualizar playlists"
                            } else {
                                "Refresh playlists"
                            })
                            .clicked()
                        {
                            refresh_playlists = true;
                        }
                        if needs_connect
                            && styled_pill_button(
                                ui,
                                if pt {
                                    "Conectar YouTube"
                                } else {
                                    "Connect YouTube"
                                },
                                false,
                                &palette,
                            )
                            .clicked()
                        {
                            connect_youtube = true;
                        }
                    });
                });

                if self.library.new_playlist_composer_open {
                    ui.add_space(10.0);
                    egui::Frame::new()
                        .fill(palette.card_alt_bg)
                        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                        .corner_radius(10.0)
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let name_resp = ui.add_sized(
                                    [240.0, 30.0],
                                    egui::TextEdit::singleline(&mut self.library.collection_name)
                                        .hint_text(if pt {
                                            "Nome da nova playlist..."
                                        } else {
                                            "New playlist name..."
                                        }),
                                );
                                let create_local = styled_pill_button(
                                    ui,
                                    if pt { "Criar local" } else { "Create local" },
                                    false,
                                    &palette,
                                )
                                .clicked()
                                    || (name_resp.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                                if create_local {
                                    let candidate =
                                        std::mem::take(&mut self.library.collection_name);
                                    if !candidate.trim().is_empty() {
                                        match self.library.data.create_collection(&candidate) {
                                            Ok(_) => {
                                                self.save_library();
                                                self.library.new_playlist_composer_open = false;
                                            }
                                            Err(err) => {
                                                self.library.notice = Some(err.to_owned());
                                            }
                                        }
                                    }
                                }
                                if self.auth.authenticated
                                    && styled_pill_button(
                                        ui,
                                        if pt {
                                            "Criar no YouTube"
                                        } else {
                                            "Create on YouTube"
                                        },
                                        false,
                                        &palette,
                                    )
                                    .clicked()
                                {
                                    let candidate =
                                        std::mem::take(&mut self.library.collection_name);
                                    if !candidate.trim().is_empty() {
                                        self.start_create_remote_playlist(candidate, None, ctx);
                                        self.library.new_playlist_composer_open = false;
                                    }
                                }
                            });
                        });
                }

                if let Some(msg) = remote_error.as_deref().or(library_notice.as_deref()) {
                    ui.add_space(8.0);
                    egui::Frame::new()
                        .fill(if remote_error.is_some() {
                            palette.error_bg
                        } else {
                            palette.notice_bg
                        })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if remote_error.is_some() {
                                palette.error_stroke
                            } else {
                                palette.notice_stroke
                            },
                        ))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.label(RichText::new(msg).size(12.5).color(
                                if remote_error.is_some() {
                                    palette.error_banner_fg
                                } else {
                                    palette.notice_fg
                                },
                            ));
                        });
                }

                ui.add_space(14.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.spacing_mut().item_spacing.y = 8.0;
                    let sort_resp = youtube_chip_button(
                        ui,
                        active_sort.label(pt),
                        false,
                        true,
                        &palette,
                        dark_theme,
                    );
                    let sort_popup_id = ui.make_persistent_id("playlists_sort_popup");
                    if sort_resp.clicked() {
                        egui::Popup::toggle_id(ui.ctx(), sort_popup_id);
                    }
                    egui::Popup::from_toggle_button_response(&sort_resp)
                        .id(sort_popup_id)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                        .show(|ui| {
                            ui.set_min_width(190.0);
                            for sort_opt in [
                                playlist_sync::PlaylistSort::RecentlyAdded,
                                playlist_sync::PlaylistSort::AlphabeticalAZ,
                            ] {
                                let selected = active_sort == sort_opt;
                                let row = egui::Button::new(
                                    RichText::new(sort_opt.label(pt)).size(13.0).strong().color(
                                        if selected {
                                            palette.active_button_fg
                                        } else {
                                            palette.channel_fg
                                        },
                                    ),
                                )
                                .fill(if selected {
                                    palette.active_button_bg
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .corner_radius(egui::CornerRadius::same(6))
                                .min_size(egui::vec2(ui.available_width(), 28.0));
                                if ui
                                    .add(row)
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    next_filter_sort = Some((active_filter, sort_opt));
                                }
                            }
                        });

                    for chip in playlist_sync::PlaylistCategoryFilter::CHIPS {
                        let is_active = active_filter == chip;
                        if youtube_chip_button(
                            ui,
                            chip.label(pt),
                            is_active,
                            false,
                            &palette,
                            dark_theme,
                        )
                        .clicked()
                        {
                            let target_filter = if is_active {
                                playlist_sync::PlaylistCategoryFilter::All
                            } else {
                                chip
                            };
                            next_filter_sort = Some((target_filter, active_sort));
                        }
                    }
                });
                ui.add_space(20.0);

                if grid_entries.is_empty() {
                    empty_state_card_ui(
                        ui,
                        "Playlists",
                        if pt {
                            "Nenhuma playlist encontrada nesta categoria."
                        } else {
                            "No playlists found in this filter."
                        },
                        &palette,
                    );
                    return;
                }

                let spacing = 16.0;
                let row_gap = 24.0;
                let avail_w = ui.available_width().max(240.0);
                let cols = ((avail_w + spacing) / 260.0).floor().clamp(1.0, 4.0) as usize;
                let card_w = ((avail_w - spacing * ((cols.saturating_sub(1)) as f32))
                    / (cols as f32))
                    .max(190.0);
                let thumb_h = (card_w * 9.0 / 16.0).round();
                let est_row_h = 6.0 + thumb_h + 8.0 + 92.0;
                let total_rows = grid_entries.len().div_ceil(cols);

                self.thumbnails.begin_pass();
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let grid_top = ui.cursor().top();
                    let clip = ui.clip_rect();
                    let (visible_rows, top_spacer_h, bottom_spacer_h) =
                        library_uniform_visible_row_range(
                            total_rows,
                            est_row_h,
                            row_gap,
                            grid_top,
                            clip.top(),
                            clip.bottom(),
                            2,
                        );
                    if top_spacer_h > 0.0 {
                        ui.allocate_exact_size(
                            egui::vec2(avail_w, top_spacer_h),
                            egui::Sense::hover(),
                        );
                    }
                    let rendered_end = visible_rows.end;
                    for row_idx in visible_rows {
                        let start = row_idx * cols;
                        let end = (start + cols).min(grid_entries.len());
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.spacing_mut().item_spacing.x = spacing;
                            for (col_idx, entry) in grid_entries[start..end].iter().enumerate() {
                                let card = &entry.card;
                                let mut open_clicked = false;
                                let mut view_full_clicked = false;
                                let mut play_menu_clicked = false;
                                let mut menu_share = false;
                                let mut menu_delete = false;
                                ui.push_id(("pl_grid_card", row_idx, col_idx, &card.id), |ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width(card_w);
                                        ui.set_max_width(card_w);
                                        ui.spacing_mut().item_spacing.y = 0.0;

                                        let (stack_rect, _) = ui.allocate_exact_size(
                                            egui::vec2(card_w, 6.0),
                                            egui::Sense::hover(),
                                        );
                                        let stack_color = color32_from_argb_u32(if dark_theme {
                                            card.stack_color_dark
                                        } else {
                                            card.stack_color_light
                                        });
                                        paint_playlist_stack_tab(
                                            ui,
                                            stack_rect,
                                            stack_color,
                                            dark_theme,
                                        );

                                        let thumb_resp = if !card.thumbnail_url.trim().is_empty() {
                                            self.thumbnails.show_cover_rounded_borderless(
                                                ui,
                                                ctx,
                                                &card.thumbnail_url,
                                                egui::vec2(card_w, thumb_h),
                                                12.0,
                                            )
                                        } else {
                                            let (rect, r) = ui.allocate_exact_size(
                                                egui::vec2(card_w, thumb_h),
                                                egui::Sense::click(),
                                            );
                                            ui.painter().rect_filled(
                                                rect,
                                                12.0,
                                                palette.card_alt_bg,
                                            );
                                            r.on_hover_cursor(egui::CursorIcon::PointingHand)
                                        };
                                        if card.is_mix {
                                            paint_mix_badge_overlay(
                                                ui,
                                                thumb_resp.rect,
                                                &card.badge_text,
                                            );
                                        } else {
                                            paint_playlist_badge_overlay(
                                                ui,
                                                thumb_resp.rect,
                                                &card.badge_text,
                                            );
                                        }
                                        let hover_t = ctx.animate_bool_with_time(
                                            ui.make_persistent_id((
                                                "pl_card_hover",
                                                row_idx,
                                                col_idx,
                                                &card.id,
                                            )),
                                            thumb_resp.hovered(),
                                            0.14,
                                        );
                                        paint_card_hover_highlight(
                                            ui,
                                            thumb_resp.rect,
                                            12.0,
                                            hover_t,
                                            &palette,
                                        );
                                        if thumb_resp.clicked() {
                                            open_clicked = true;
                                        }
                                        ui.add_space(8.0);
                                        ui.horizontal_top(|ui| {
                                            ui.spacing_mut().item_spacing.x = 4.0;
                                            let title_w = (card_w - 30.0).max(80.0);
                                            let title_galley = layout_two_line_clamped_galley(
                                                ui,
                                                &card.title,
                                                title_w,
                                                egui::FontId::proportional(14.5),
                                                palette.channel_fg,
                                            );
                                            let (title_rect, title_resp) = ui.allocate_exact_size(
                                                egui::vec2(
                                                    title_w,
                                                    title_galley.size().y.max(18.0),
                                                ),
                                                egui::Sense::click(),
                                            );
                                            ui.painter().galley(
                                                title_rect.min,
                                                title_galley,
                                                palette.channel_fg,
                                            );
                                            paint_focus_ring_if_focused(ui, &title_resp, 6.0);
                                            if title_resp
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .on_hover_text(&card.title)
                                                .clicked()
                                            {
                                                open_clicked = true;
                                            }
                                            let menu_id = ui.make_persistent_id((
                                                "pl_card_menu",
                                                row_idx,
                                                col_idx,
                                                &card.id,
                                            ));
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::TOP),
                                                |ui| {
                                                    let dots = paint_more_dots_button(ui, &palette);
                                                    if dots.clicked() {
                                                        egui::Popup::toggle_id(ui.ctx(), menu_id);
                                                    }
                                                    egui::Popup::from_toggle_button_response(&dots)
                                                        .id(menu_id)
                                                        .close_behavior(
                                                            egui::PopupCloseBehavior::CloseOnClick,
                                                        )
                                                        .show(|ui| {
                                                            ui.set_min_width(195.0);
                                                            if search_menu_item_ui(
                                                                ui,
                                                                SearchMenuIcon::Queue,
                                                                if pt {
                                                                    "Reproduzir"
                                                                } else {
                                                                    "Play"
                                                                },
                                                                &palette,
                                                            )
                                                            .clicked()
                                                            {
                                                                play_menu_clicked = true;
                                                            }
                                                            if card.has_full_playlist_link
                                                                && !card.is_mix
                                                                && search_menu_item_ui(
                                                                    ui,
                                                                    SearchMenuIcon::Queue,
                                                                    if pt {
                                                                        "Ver playlist completa"
                                                                    } else {
                                                                        "View full playlist"
                                                                    },
                                                                    &palette,
                                                                )
                                                                .clicked()
                                                            {
                                                                view_full_clicked = true;
                                                            }
                                                            if entry.local_idx.is_none()
                                                                && card.id != "WL"
                                                                && card.id != "LL"
                                                                && search_menu_item_ui(
                                                                    ui,
                                                                    SearchMenuIcon::Share,
                                                                    if pt {
                                                                        "Compartilhar"
                                                                    } else {
                                                                        "Share"
                                                                    },
                                                                    &palette,
                                                                )
                                                                .clicked()
                                                            {
                                                                menu_share = true;
                                                            }
                                                            if entry.local_idx.is_some() {
                                                                ui.separator();
                                                                if search_menu_item_ui(
                                                                    ui,
                                                                    SearchMenuIcon::Trash,
                                                                    if pt {
                                                                        "Excluir playlist"
                                                                    } else {
                                                                        "Delete playlist"
                                                                    },
                                                                    &palette,
                                                                )
                                                                .clicked()
                                                                {
                                                                    menu_delete = true;
                                                                }
                                                            }
                                                        });
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        let subtitle = if !card.subtitle_line.trim().is_empty() {
                                            card.subtitle_line.as_str()
                                        } else if !card.owner_label.trim().is_empty() {
                                            card.owner_label.as_str()
                                        } else {
                                            "Playlist"
                                        };
                                        let sub_galley = layout_two_line_clamped_galley(
                                            ui,
                                            subtitle,
                                            card_w,
                                            egui::FontId::proportional(12.5),
                                            palette.neutral_status_fg,
                                        );
                                        let (sub_rect, _) = ui.allocate_exact_size(
                                            egui::vec2(card_w, sub_galley.size().y.max(15.0)),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().galley(
                                            sub_rect.min,
                                            sub_galley,
                                            palette.neutral_status_fg,
                                        );
                                        if let Some(upd) = card.updated_text.as_deref()
                                            && !upd.trim().is_empty()
                                        {
                                            ui.add_space(2.0);
                                            ui.label(
                                                RichText::new(upd)
                                                    .size(12.0)
                                                    .color(palette.neutral_status_fg),
                                            );
                                        }
                                        if card.has_full_playlist_link && !card.is_mix {
                                            ui.add_space(4.0);
                                            let full_lbl = if pt {
                                                "Ver playlist completa"
                                            } else {
                                                "View full playlist"
                                            };
                                            let measure = ui.painter().layout_no_wrap(
                                                full_lbl.to_owned(),
                                                egui::FontId::proportional(12.5),
                                                palette.neutral_status_fg,
                                            );
                                            let (link_rect, link_resp) = ui.allocate_exact_size(
                                                egui::vec2(
                                                    measure.size().x.min(card_w),
                                                    measure.size().y.max(16.0),
                                                ),
                                                egui::Sense::click(),
                                            );
                                            let link_color = if link_resp.hovered() {
                                                palette.channel_fg
                                            } else {
                                                palette.neutral_status_fg
                                            };
                                            ui.painter().text(
                                                link_rect.left_center(),
                                                egui::Align2::LEFT_CENTER,
                                                full_lbl,
                                                egui::FontId::proportional(12.5),
                                                link_color,
                                            );
                                            paint_focus_ring_if_focused(ui, &link_resp, 4.0);
                                            if link_resp
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .clicked()
                                            {
                                                view_full_clicked = true;
                                            }
                                        }
                                    });
                                });
                                if menu_delete && let Some(idx) = entry.local_idx {
                                    delete_local_idx = Some(idx);
                                } else if menu_share {
                                    share_card = Some(card.clone());
                                } else if play_menu_clicked {
                                    play_card = Some((card.clone(), entry.local_idx));
                                } else if view_full_clicked || open_clicked {
                                    if card.id == "WL" {
                                        open_section = Some(BrowseSection::WatchLater);
                                    } else if card.id == "LL" {
                                        open_section = Some(BrowseSection::LikedVideos);
                                    } else if let Some(idx) = entry.local_idx {
                                        open_local_idx = Some(idx);
                                    } else if card.is_mix && !view_full_clicked {
                                        open_mix_card = Some(card.clone());
                                    } else {
                                        open_detail_card = Some(card.clone());
                                    }
                                }
                            }
                        });
                        if row_idx + 1 < rendered_end {
                            ui.add_space(row_gap);
                        }
                    }
                    if bottom_spacer_h > 0.0 {
                        ui.allocate_exact_size(
                            egui::vec2(avail_w, bottom_spacer_h),
                            egui::Sense::hover(),
                        );
                    }
                });
                ui.add_space(row_gap);
                self.thumbnails.end_pass(ctx);
            });

        if connect_youtube {
            self.start_playlists_sync(true, true, ctx);
        } else if refresh_playlists {
            self.start_playlists_sync(true, false, ctx);
        } else if let Some((f, s)) = next_filter_sort {
            self.start_playlists_filtered_sync(f, s, ctx);
        }
        if let Some(idx) = delete_local_idx {
            if self.library.data.delete_collection(idx).is_some() {
                self.save_library();
            }
        } else if let Some(card) = share_card {
            let share_url = if let Some(vid) = card.seed_video_id.as_deref() {
                format!("https://www.youtube.com/watch?v={vid}&list={}", card.id)
            } else {
                format!("https://www.youtube.com/playlist?list={}", card.id)
            };
            self.open_share_modal(&Video {
                title: card.title,
                channel: card.owner_label,
                url: share_url,
                thumbnail: card.thumbnail_url,
                duration: String::new(),
            });
        } else if let Some((card, local_idx)) = play_card {
            if let Some(idx) = local_idx {
                if let Some(col) = self.library.data.collections.get(idx)
                    && !col.videos.is_empty()
                {
                    let videos: Vec<Video> = col.videos.iter().map(|r| r.video.clone()).collect();
                    let pl = app_state::ActivePlaylistPlayback::new(
                        format!("local:{idx}"),
                        col.name.clone(),
                        self.auth
                            .my_channel_title
                            .clone()
                            .unwrap_or_else(|| "Local".to_owned()),
                        playlist_sync::PlaylistPrivacy::Private,
                        false,
                        videos,
                        Some(col.videos.len()),
                        0,
                        false,
                        1,
                        false,
                        Some(idx),
                    );
                    self.start_playlist_playback(pl);
                } else {
                    self.library.detail_chip_filter = 0;
                    self.library.selected_collection_idx = Some(idx);
                }
            } else if card.id == "WL" && !self.library.watch_later_remote.merged_videos.is_empty() {
                let videos = self.library.watch_later_remote.merged_videos.clone();
                let total = self.watch_later_display_total().max(videos.len());
                let pl = app_state::ActivePlaylistPlayback::new(
                    "WL".to_owned(),
                    card.title,
                    card.owner_label,
                    playlist_sync::PlaylistPrivacy::Private,
                    false,
                    videos,
                    Some(total),
                    0,
                    false,
                    1,
                    self.library.watch_later_remote.rx.is_some(),
                    None,
                );
                self.start_playlist_playback(pl);
            } else if card.id == "LL" && !self.library.liked_videos.merged_videos.is_empty() {
                let videos = self.library.liked_videos.merged_videos.clone();
                let total = self.liked_videos_display_total().max(videos.len());
                let pl = app_state::ActivePlaylistPlayback::new(
                    "LL".to_owned(),
                    card.title,
                    card.owner_label,
                    playlist_sync::PlaylistPrivacy::Private,
                    false,
                    videos,
                    Some(total),
                    0,
                    false,
                    1,
                    self.library.liked_videos.rx.is_some(),
                    None,
                );
                self.start_playlist_playback(pl);
            } else {
                self.start_watch_mix_or_playlist(&card, ctx);
            }
        } else if let Some(card) = open_mix_card {
            self.start_watch_mix_or_playlist(&card, ctx);
        } else if let Some(card) = open_detail_card {
            self.open_remote_playlist_detail(card.id, Some(card.title), card.seed_video_id, ctx);
        } else if let Some(sec) = open_section {
            self.library.detail_chip_filter = 0;
            self.ui.browse_section = sec;
            if sec == BrowseSection::WatchLater {
                self.on_enter_watch_later_section(ctx);
            } else if sec == BrowseSection::LikedVideos {
                self.start_liked_videos_fetch(false, ctx);
            }
        } else if let Some(idx) = open_local_idx {
            self.library.detail_chip_filter = 0;
            self.library.selected_collection_idx = Some(idx);
        }
        None
    }

    pub(crate) fn watch_later_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> Option<Video> {
        let pt = self.ui_pt();
        self.ensure_watch_later_merged_cache();
        let display_total = self.watch_later_display_total();
        let remote_loaded = self.library.watch_later_remote.items.len();
        let loading = self.library.watch_later_remote.rx.is_some();
        let error = self.library.watch_later_remote.error.clone();
        let needs_connect = self.library.watch_later_remote.needs_connect;
        let videos = std::mem::take(&mut self.library.watch_later_remote.merged_videos);
        let is_short_flags = std::mem::take(&mut self.library.watch_later_remote.merged_is_short);

        let owner = self
            .library
            .watch_later_remote
            .meta
            .as_ref()
            .map(|m| m.owner.clone())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| self.auth.my_channel_title.clone())
            .unwrap_or_else(|| if pt { "Você" } else { "You" }.to_owned());
        let total_fmt = format_grouped_count(display_total, pt);
        let loaded_fmt = format_grouped_count(remote_loaded, pt);
        let subtitle = if loading {
            if display_total > 0 {
                if pt {
                    format!(
                        "{owner} · {total_fmt} vídeos (carregando {loaded_fmt}/{total_fmt}...) · Privada"
                    )
                } else {
                    format!(
                        "{owner} · {total_fmt} videos (loading {loaded_fmt}/{total_fmt}...) · Private"
                    )
                }
            } else if pt {
                format!("{owner} · Carregando Assistir mais tarde... · Privada")
            } else {
                format!("{owner} · Loading Watch later... · Private")
            }
        } else if pt {
            format!(
                "{owner} · {total_fmt} {} · Privada",
                if display_total == 1 {
                    "vídeo"
                } else {
                    "vídeos"
                }
            )
        } else {
            format!(
                "{owner} · {total_fmt} {} · Private",
                if display_total == 1 {
                    "video"
                } else {
                    "videos"
                }
            )
        };
        let (chosen, remove_url, refresh_clicked, cancel_clicked) = self
            .youtube_playlist_detail_page_ui(
                ui,
                ctx,
                &videos,
                &is_short_flags,
                PlaylistDetailSpec {
                    playlist_id: "WL",
                    owner: &owner,
                    privacy: playlist_sync::PlaylistPrivacy::Private,
                    total_count: Some(display_total),
                    local_collection_idx: None,
                    is_editable: true,
                    title: if pt {
                        "Assistir mais tarde"
                    } else {
                        "Watch later"
                    },
                    subtitle: &subtitle,
                    empty_message: if pt {
                        "Nenhum vídeo em Assistir mais tarde. Salve vídeos pelo botão '+' nos cards."
                    } else {
                        "No videos in Watch later yet. Save videos using the menu on any video."
                    },
                    scroll_id: "watch_later_detail_scroll",
                    can_back_to_playlists: false,
                    remove_label: Some(if pt {
                        "Remover de Assistir mais tarde"
                    } else {
                        "Remove from Watch later"
                    }),
                    loading,
                    can_refresh: true,
                    error: error.as_deref(),
                },
                palette,
            );
        self.library.watch_later_remote.merged_videos = videos;
        self.library.watch_later_remote.merged_is_short = is_short_flags;

        if cancel_clicked {
            self.library.watch_later_remote.cancel_active();
        } else if refresh_clicked {
            self.start_watch_later_sync(true, needs_connect, ctx);
        }
        if let Some(url) = remove_url {
            if self.library.data.remove_from_watch_later(&url) {
                self.save_library();
            }
            if let Some(remote_item) = self
                .library
                .watch_later_remote
                .items
                .iter()
                .find(|it| it.result.video.url == url)
                .cloned()
                && let Some(vid_id) = history::youtube_video_id(&url)
            {
                self.dispatch_playlist_remove_video(
                    "WL".into(),
                    vid_id,
                    remote_item.set_video_id,
                    ctx,
                );
            }
        }
        chosen
    }

    pub(crate) fn liked_videos_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> Option<Video> {
        let pt = self.ui_pt();
        self.ensure_liked_videos_merged_cache();
        let display_total = self.liked_videos_display_total();
        let remote_loaded = self.library.liked_videos.items.len();
        let loading = self.library.liked_videos.rx.is_some();
        let error = self.library.liked_videos.error.clone();
        let can_refresh = self.auth.authenticated
            || self.auth.youtube_web_session.is_some()
            || self.auth.cached_has_saved_profile;
        let videos = std::mem::take(&mut self.library.liked_videos.merged_videos);
        let is_short_flags = std::mem::take(&mut self.library.liked_videos.merged_is_short);

        let owner = self
            .auth
            .my_channel_title
            .clone()
            .unwrap_or_else(|| if pt { "Você" } else { "You" }.to_owned());
        let total_fmt = format_grouped_count(display_total, pt);
        let loaded_fmt = format_grouped_count(remote_loaded, pt);
        let subtitle = if loading {
            if display_total > 0 {
                if pt {
                    format!(
                        "{owner} · {total_fmt} vídeos (carregando {loaded_fmt}/{total_fmt}...) · Privada"
                    )
                } else {
                    format!(
                        "{owner} · {total_fmt} videos (loading {loaded_fmt}/{total_fmt}...) · Private"
                    )
                }
            } else if pt {
                format!("{owner} · Carregando vídeos com \"Gostei\"... · Privada")
            } else {
                format!("{owner} · Loading liked videos... · Private")
            }
        } else if pt {
            format!(
                "{owner} · {total_fmt} {} · Privada",
                if display_total == 1 {
                    "vídeo"
                } else {
                    "vídeos"
                }
            )
        } else {
            format!(
                "{owner} · {total_fmt} {} · Private",
                if display_total == 1 {
                    "video"
                } else {
                    "videos"
                }
            )
        };
        let (chosen, remove_url, refresh_clicked, cancel_clicked) = self
            .youtube_playlist_detail_page_ui(
                ui,
                ctx,
                &videos,
                &is_short_flags,
                PlaylistDetailSpec {
                    playlist_id: "LL",
                    owner: &owner,
                    privacy: playlist_sync::PlaylistPrivacy::Private,
                    total_count: Some(display_total),
                    local_collection_idx: None,
                    is_editable: false,
                    title: if pt {
                        "Vídeos com \"Gostei\""
                    } else {
                        "Liked videos"
                    },
                    subtitle: &subtitle,
                    empty_message: if pt {
                        "Nenhum vídeo com \"Gostei\" ainda. Curta vídeos ou entre com sua conta."
                    } else {
                        "No liked videos yet. Favorite videos or sign in with your account."
                    },
                    scroll_id: "liked_videos_detail_scroll",
                    can_back_to_playlists: false,
                    remove_label: Some(if pt {
                        "Remover dos vídeos com \"Gostei\""
                    } else {
                        "Remove from Liked videos"
                    }),
                    loading,
                    can_refresh,
                    error: error.as_deref(),
                },
                palette,
            );
        self.library.liked_videos.merged_videos = videos;
        self.library.liked_videos.merged_is_short = is_short_flags;

        if cancel_clicked {
            self.cancel_liked_videos_fetch();
        } else if refresh_clicked {
            self.start_liked_videos_fetch(true, ctx);
        }
        if let Some(url) = remove_url {
            let vid_id = history::youtube_video_id(&url);
            let in_remote_liked = self.library.liked_videos.items.iter().any(|item| {
                vid_id.as_deref().is_some_and(|id| {
                    history::youtube_video_id(&item.video.url).as_deref() == Some(id)
                }) || item.video.url == url
            });
            let blocked_by_inflight_rating = in_remote_liked
                && self.auth.session.is_some()
                && self.auth.rating_task.is_running();
            if blocked_by_inflight_rating {
                self.toasts.info(if pt {
                    "Aguarde a avaliação anterior terminar."
                } else {
                    "Wait for the previous rating action to finish."
                });
            } else {
                if in_remote_liked {
                    if let Some(vid) = vid_id.clone() {
                        if self.auth.session.is_some() {
                            self.start_video_rating(vid, None, ctx);
                        } else if self.auth.youtube_web_session.is_some() {
                            self.dispatch_playlist_remove_video("LL".into(), vid, None, ctx);
                        }
                    }
                    let removed = remove_liked_video_from_state(
                        &mut self.library.liked_videos.items,
                        &mut self.library.liked_videos.seen_keys,
                        &mut self.library.liked_videos.total_results,
                        &url,
                    );
                    if removed > 0 {
                        self.library.liked_videos.merged_cache_key = None;
                        self.library.liked_videos.revision =
                            self.library.liked_videos.revision.wrapping_add(1);
                    }
                    if let Some(vid) = &vid_id {
                        self.auth.ratings.remove(vid);
                    }
                }
                let mut removed_local = self.library.data.remove_from_favorites(&url);
                if !removed_local
                    && let Some(id) = vid_id.as_deref()
                    && let Some(local_url) = self
                        .library
                        .data
                        .favorites
                        .iter()
                        .find(|rec| {
                            history::youtube_video_id(&rec.video.url).as_deref() == Some(id)
                        })
                        .map(|rec| rec.video.url.clone())
                {
                    removed_local = self.library.data.remove_from_favorites(&local_url);
                }
                if removed_local {
                    self.save_library();
                }
            }
        }
        chosen
    }

    fn youtube_playlist_detail_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        videos: &[Video],
        is_short_flags: &[bool],
        spec: PlaylistDetailSpec<'_>,
        palette: ThemePalette,
    ) -> (Option<Video>, Option<String>, bool, bool) {
        let avail_w = ui.available_width().max(320.0);
        let side_by_side = avail_w >= 860.0;
        let filtered_indices: Vec<usize> = (0..videos.len())
            .filter(|&idx| {
                let is_short = is_short_flags.get(idx).copied().unwrap_or(false);
                match self.library.detail_chip_filter {
                    1 => !is_short,
                    2 => is_short,
                    _ => true,
                }
            })
            .collect();
        let cover_url = videos.first().map(video_thumb_url).unwrap_or_default();

        self.thumbnails.begin_pass();
        let (banner_act, list_act) = if side_by_side {
            let left_w = 348.0;
            let right_w = (avail_w - left_w - 24.0).max(320.0);
            let col_h = ui.available_height().max(480.0);
            let mut b_act = PlaylistBannerAction::default();
            let mut l_act = PlaylistListAction::default();
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(left_w, col_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_max_width(left_w);
                        b_act = self.playlist_detail_banner_ui(
                            ui,
                            ctx,
                            filtered_indices.is_empty(),
                            &cover_url,
                            &spec,
                            &palette,
                        );
                    },
                );
                ui.add_space(24.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(right_w, col_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_max_width(right_w);
                        l_act = self.playlist_detail_list_ui(
                            ui,
                            ctx,
                            videos,
                            &filtered_indices,
                            &spec,
                            &palette,
                        );
                    },
                );
            });
            (b_act, l_act)
        } else {
            let b_act = self.playlist_detail_banner_ui(
                ui,
                ctx,
                filtered_indices.is_empty(),
                &cover_url,
                &spec,
                &palette,
            );
            ui.add_space(16.0);
            let l_act =
                self.playlist_detail_list_ui(ui, ctx, videos, &filtered_indices, &spec, &palette);
            (b_act, l_act)
        };
        self.thumbnails.end_pass(ctx);

        let mut chosen = list_act.chosen;
        if banner_act.go_back {
            self.library.selected_collection_idx = None;
            self.library.active_remote_playlist_title = None;
            self.library.active_playlist_detail.cancel_active();
            self.library.active_playlist_detail.playlist_id = None;
            if self.feed.home.title.as_deref() == Some("Vídeos da playlist") {
                self.feed.home.title = None;
                self.feed.home.playlist_total_results = None;
            }
        }
        if banner_act.play_all && !filtered_indices.is_empty() {
            let filtered_videos: Vec<Video> = filtered_indices
                .iter()
                .map(|&idx| videos[idx].clone())
                .collect();
            let pl = app_state::ActivePlaylistPlayback::new(
                spec.playlist_id.to_owned(),
                spec.title.to_owned(),
                spec.owner.to_owned(),
                spec.privacy,
                spec.playlist_id.starts_with("RD"),
                filtered_videos,
                spec.total_count,
                0,
                false,
                1,
                spec.loading,
                spec.local_collection_idx,
            );
            self.start_playlist_playback(pl);
            chosen = None;
        } else if banner_act.shuffle && !filtered_indices.is_empty() {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1)
                .max(1);
            let filtered_videos: Vec<Video> = filtered_indices
                .iter()
                .map(|&idx| videos[idx].clone())
                .collect();
            let start_idx = (seed as usize) % filtered_videos.len();
            let pl = app_state::ActivePlaylistPlayback::new(
                spec.playlist_id.to_owned(),
                spec.title.to_owned(),
                spec.owner.to_owned(),
                spec.privacy,
                spec.playlist_id.starts_with("RD"),
                filtered_videos,
                spec.total_count,
                start_idx,
                true,
                seed,
                spec.loading,
                spec.local_collection_idx,
            );
            self.start_playlist_playback(pl);
            chosen = None;
        } else if banner_act.enqueue_all && !filtered_indices.is_empty() {
            let filtered_videos: Vec<Video> = filtered_indices
                .iter()
                .map(|&idx| videos[idx].clone())
                .collect();
            let _ = self.enqueue_all_videos(&filtered_videos);
        } else if banner_act.download_playlist && !filtered_indices.is_empty() {
            let filtered_videos: Vec<Video> = filtered_indices
                .iter()
                .map(|&idx| videos[idx].clone())
                .collect();
            self.start_batch_download_playlist(
                &filtered_videos,
                self.downloads.selected_format,
                ctx,
            );
        } else if banner_act.remove_watched && spec.playlist_id == "WL" {
            self.remove_watched_from_watch_later(ctx);
        } else if (banner_act.load_more_mix || list_act.load_more_mix)
            && spec.playlist_id.starts_with("RD")
        {
            self.load_more_mix_playlist_detail(spec.playlist_id.to_owned(), ctx);
        } else if let Some(pos) = list_act.chosen_pos
            && !filtered_indices.is_empty()
        {
            let filtered_videos: Vec<Video> = filtered_indices
                .iter()
                .map(|&idx| videos[idx].clone())
                .collect();
            let start_idx = pos.min(filtered_videos.len().saturating_sub(1));
            let pl = app_state::ActivePlaylistPlayback::new(
                spec.playlist_id.to_owned(),
                spec.title.to_owned(),
                spec.owner.to_owned(),
                spec.privacy,
                spec.playlist_id.starts_with("RD"),
                filtered_videos,
                spec.total_count,
                start_idx,
                false,
                1,
                spec.loading,
                spec.local_collection_idx,
            );
            self.start_playlist_playback(pl);
            chosen = None;
        }
        if let Some(new_title) = banner_act.rename_to {
            if let Some(col_idx) = spec.local_collection_idx {
                let pt = self.ui_pt();
                match self.library.data.rename_collection(col_idx, &new_title) {
                    Ok(()) => {
                        self.library.error = None;
                        self.library.notice = Some(
                            if pt {
                                "Playlist renomeada."
                            } else {
                                "Playlist renamed."
                            }
                            .to_owned(),
                        );
                        self.save_library();
                    }
                    Err(err) => {
                        self.library.error = Some(err.to_owned());
                    }
                }
            } else {
                self.dispatch_playlist_rename(spec.playlist_id.to_owned(), new_title, ctx);
            }
        }
        if let Some(privacy) = banner_act.set_privacy
            && spec.local_collection_idx.is_none()
        {
            self.dispatch_playlist_set_privacy(spec.playlist_id.to_owned(), privacy, ctx);
        }
        if banner_act.delete_playlist {
            if let Some(col_idx) = spec.local_collection_idx {
                let pt = self.ui_pt();
                if self.library.data.delete_collection(col_idx).is_some() {
                    self.library.selected_collection_idx = None;
                    self.library.notice = Some(
                        if pt {
                            "Playlist excluída."
                        } else {
                            "Playlist deleted."
                        }
                        .to_owned(),
                    );
                    self.save_library();
                }
            } else {
                self.dispatch_playlist_delete(spec.playlist_id.to_owned(), ctx);
            }
        }
        if let Some((from_idx, to_idx)) = list_act.reorder_from_to {
            self.reorder_playlist_detail_item(
                spec.playlist_id,
                spec.local_collection_idx,
                from_idx,
                to_idx,
                ctx,
            );
        }
        if let Some(video) = list_act.enqueue_target {
            let _ = self.enqueue_video(video);
        }
        if let Some(video) = list_act.download_target {
            self.start_download_video(video, downloads::DownloadFormat::VideoMp4Best, ctx);
        }
        if let Some(video) = list_act.share_target {
            self.open_share_modal(&video);
        }
        if let Some(target) = list_act.open_channel_target {
            self.open_channel(target, ctx);
        }

        (
            chosen,
            list_act.remove_url,
            banner_act.refresh,
            banner_act.cancel_load,
        )
    }

    fn playlist_detail_banner_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        filtered_empty: bool,
        cover_url: &str,
        spec: &PlaylistDetailSpec<'_>,
        palette: &ThemePalette,
    ) -> PlaylistBannerAction {
        let pt = self.ui_pt();
        let dark_theme = self.is_dark_theme();
        let mut act = PlaylistBannerAction::default();
        if spec.can_back_to_playlists
            && styled_pill_button(ui, "‹  Playlists", false, palette).clicked()
        {
            act.go_back = true;
        }
        if spec.can_back_to_playlists {
            ui.add_space(10.0);
        }
        let gradient_seed = if cover_url.trim().is_empty() {
            spec.title
        } else {
            cover_url
        };
        let (grad_top_left, grad_top_right, grad_bottom) =
            derive_playlist_gradient_stops(gradient_seed, dark_theme);
        egui::Frame::new()
            .fill(palette.card_alt_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(16.0)
            .inner_margin(egui::Margin::same(22))
            .show(ui, |ui| {
                let banner_w = ui.available_width().max(220.0);
                let cover_h = (banner_w * 9.0 / 16.0).round();
                let header_top = ui.cursor().min;
                let grad_rect = egui::Rect::from_min_size(
                    header_top - egui::vec2(10.0, 10.0),
                    egui::vec2(banner_w + 20.0, cover_h + 74.0),
                );
                paint_playlist_gradient_banner_backdrop(
                    ui,
                    grad_rect,
                    grad_top_left,
                    grad_top_right,
                    grad_bottom,
                );
                if !cover_url.trim().is_empty() {
                    let _ = self.thumbnails.show_rounded(
                        ui,
                        ctx,
                        cover_url,
                        egui::vec2(banner_w, cover_h),
                        12.0,
                    );
                } else {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(banner_w, cover_h), egui::Sense::hover());
                    paint_playlist_gradient_banner_backdrop(
                        ui,
                        rect,
                        grad_top_left,
                        grad_top_right,
                        grad_bottom,
                    );
                }
                ui.add_space(16.0);
                ui.label(
                    RichText::new(spec.title)
                        .size(24.0)
                        .strong()
                        .color(palette.channel_fg),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(spec.subtitle)
                        .size(13.0)
                        .color(palette.neutral_status_fg),
                );
                ui.add_space(14.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                    let play_lbl = if pt {
                        "▶  Reproduzir tudo"
                    } else {
                        "▶  Play all"
                    };
                    let play_btn = egui::Button::new(
                        RichText::new(play_lbl)
                            .size(13.5)
                            .strong()
                            .color(palette.active_button_fg),
                    )
                    .fill(palette.active_button_bg)
                    .corner_radius(egui::CornerRadius::same(18))
                    .min_size(egui::vec2(0.0, 34.0));
                    if ui
                        .add_enabled(!filtered_empty, play_btn)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        act.play_all = true;
                    }
                    if styled_pill_button(
                        ui,
                        if pt {
                            "🔀 Aleatório"
                        } else {
                            "🔀 Shuffle"
                        },
                        false,
                        palette,
                    )
                    .clicked()
                        && !filtered_empty
                    {
                        act.shuffle = true;
                    }
                    if styled_pill_button(ui, if pt { "+ Fila" } else { "+ Queue" }, false, palette)
                        .clicked()
                        && !filtered_empty
                    {
                        act.enqueue_all = true;
                    }
                    if styled_pill_button(
                        ui,
                        if pt {
                            "⬇ Baixar playlist"
                        } else {
                            "⬇ Download playlist"
                        },
                        false,
                        palette,
                    )
                    .clicked()
                        && !filtered_empty
                    {
                        act.download_playlist = true;
                    }
                    if spec.playlist_id == "WL"
                        && styled_pill_button(
                            ui,
                            if pt {
                                "Remover assistidos"
                            } else {
                                "Remove watched"
                            },
                            false,
                            palette,
                        )
                        .clicked()
                        && !filtered_empty
                    {
                        act.remove_watched = true;
                    }
                    if spec.playlist_id.starts_with("RD")
                        && !spec.loading
                        && styled_pill_button(
                            ui,
                            if pt { "Carregar mais" } else { "Load more" },
                            false,
                            palette,
                        )
                        .clicked()
                    {
                        act.load_more_mix = true;
                    }
                    if spec.loading {
                        if styled_pill_button(
                            ui,
                            if pt { "✕ Cancelar" } else { "✕ Cancel" },
                            false,
                            palette,
                        )
                        .clicked()
                        {
                            act.cancel_load = true;
                        }
                    } else if spec.can_refresh
                        && styled_pill_button(
                            ui,
                            if pt { "↻ Atualizar" } else { "↻ Refresh" },
                            false,
                            palette,
                        )
                        .clicked()
                    {
                        act.refresh = true;
                    }
                });
                let can_crud_playlist = spec.local_collection_idx.is_some()
                    || (spec.is_editable
                        && spec.playlist_id != "WL"
                        && spec.playlist_id != "LL"
                        && !spec.playlist_id.starts_with("RD"));
                if can_crud_playlist {
                    ui.add_space(10.0);
                    let rename_open_id =
                        egui::Id::new(("fasttube_pl_detail_rename_open", spec.playlist_id));
                    let rename_draft_id =
                        egui::Id::new(("fasttube_pl_detail_rename_draft", spec.playlist_id));
                    let del_confirm_id =
                        egui::Id::new(("fasttube_pl_detail_del_confirm", spec.playlist_id));
                    let mut rename_open =
                        ctx.data(|d| d.get_temp::<bool>(rename_open_id).unwrap_or(false));
                    let mut del_confirm =
                        ctx.data(|d| d.get_temp::<bool>(del_confirm_id).unwrap_or(false));
                    if rename_open {
                        let mut draft = ctx.data(|d| {
                            d.get_temp::<String>(rename_draft_id)
                                .unwrap_or_else(|| spec.title.to_owned())
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.add_sized(
                                [180.0, 28.0],
                                egui::TextEdit::singleline(&mut draft).hint_text(if pt {
                                    "Título da playlist"
                                } else {
                                    "Playlist title"
                                }),
                            );
                            if styled_compact_button(
                                ui,
                                if pt { "Salvar" } else { "Save" },
                                true,
                                palette,
                            )
                            .clicked()
                            {
                                let trimmed = draft.trim().to_owned();
                                if !trimmed.is_empty() {
                                    act.rename_to = Some(trimmed);
                                }
                                rename_open = false;
                            }
                            if styled_ghost_button(
                                ui,
                                if pt { "Cancelar" } else { "Cancel" },
                                palette,
                            )
                            .clicked()
                            {
                                rename_open = false;
                            }
                        });
                        ctx.data_mut(|d| {
                            d.insert_temp(rename_open_id, rename_open);
                            d.insert_temp(rename_draft_id, draft);
                        });
                    } else if del_confirm {
                        ui.horizontal_wrapped(|ui| {
                            if styled_danger_button(
                                ui,
                                if pt {
                                    "Confirmar exclusão"
                                } else {
                                    "Confirm delete"
                                },
                                palette,
                            )
                            .clicked()
                            {
                                act.delete_playlist = true;
                                del_confirm = false;
                            }
                            if styled_ghost_button(
                                ui,
                                if pt { "Cancelar" } else { "Cancel" },
                                palette,
                            )
                            .clicked()
                            {
                                del_confirm = false;
                            }
                        });
                        ctx.data_mut(|d| d.insert_temp(del_confirm_id, del_confirm));
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                            if styled_compact_button(
                                ui,
                                if pt { "Renomear" } else { "Rename" },
                                false,
                                palette,
                            )
                            .clicked()
                            {
                                ctx.data_mut(|d| {
                                    d.insert_temp(rename_open_id, true);
                                    d.insert_temp(rename_draft_id, spec.title.to_owned());
                                    d.insert_temp(del_confirm_id, false);
                                });
                            }
                            if spec.local_collection_idx.is_none() {
                                for priv_opt in [
                                    playlist_sync::PlaylistPrivacy::Public,
                                    playlist_sync::PlaylistPrivacy::Unlisted,
                                    playlist_sync::PlaylistPrivacy::Private,
                                ] {
                                    let active = spec.privacy == priv_opt;
                                    if styled_compact_button(
                                        ui,
                                        priv_opt.label(pt),
                                        active,
                                        palette,
                                    )
                                    .clicked()
                                        && !active
                                    {
                                        act.set_privacy = Some(priv_opt);
                                    }
                                }
                            }
                            if styled_danger_button(
                                ui,
                                if pt { "Excluir" } else { "Delete" },
                                palette,
                            )
                            .clicked()
                            {
                                ctx.data_mut(|d| {
                                    d.insert_temp(del_confirm_id, true);
                                    d.insert_temp(rename_open_id, false);
                                });
                            }
                        });
                    }
                }
                if let Some(err) = spec.error {
                    ui.add_space(10.0);
                    egui::Frame::new()
                        .fill(palette.error_bg)
                        .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.label(RichText::new(err).size(12.0).color(palette.error_banner_fg));
                        });
                }
            });
        act
    }

    #[allow(clippy::too_many_arguments)]
    fn playlist_item_menu_ui(
        &mut self,
        ui: &mut egui::Ui,
        video: &Video,
        menu_id: egui::Id,
        remove_label: Option<&str>,
        reorder_ctx: Option<(usize, usize)>,
        palette: &ThemePalette,
        act: &mut PlaylistListAction,
    ) {
        let pt = self.ui_pt();
        let dots = paint_more_dots_button(ui, palette);
        if dots.clicked() {
            egui::Popup::toggle_id(ui.ctx(), menu_id);
        }
        egui::Popup::from_toggle_button_response(&dots)
            .id(menu_id)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .show(|ui| {
                ui.set_min_width(210.0);
                if search_menu_item_ui(
                    ui,
                    SearchMenuIcon::Queue,
                    if pt {
                        "Adicionar à fila"
                    } else {
                        "Add to queue"
                    },
                    palette,
                )
                .clicked()
                {
                    act.enqueue_target = Some(video.clone());
                }
                if search_menu_item_ui(
                    ui,
                    SearchMenuIcon::Share,
                    if pt { "Compartilhar" } else { "Share" },
                    palette,
                )
                .clicked()
                {
                    act.share_target = Some(video.clone());
                }
                if self.channel_target_for_video(video).is_some()
                    && search_menu_item_ui(
                        ui,
                        SearchMenuIcon::Queue,
                        if pt { "Abrir canal" } else { "Open channel" },
                        palette,
                    )
                    .clicked()
                {
                    act.open_channel_target = self.channel_target_for_video(video);
                }
                if search_menu_item_ui(
                    ui,
                    SearchMenuIcon::Download,
                    if pt { "Baixar" } else { "Download" },
                    palette,
                )
                .clicked()
                {
                    act.download_target = Some(video.clone());
                }
                if let Some((vid_idx, total_len)) = reorder_ctx
                    && total_len > 1
                {
                    ui.separator();
                    if vid_idx > 0
                        && search_menu_item_ui(
                            ui,
                            SearchMenuIcon::Queue,
                            if pt {
                                "Mover para o topo"
                            } else {
                                "Move to top"
                            },
                            palette,
                        )
                        .clicked()
                    {
                        act.reorder_from_to = Some((vid_idx, 0));
                    }
                    if vid_idx + 1 < total_len
                        && search_menu_item_ui(
                            ui,
                            SearchMenuIcon::Queue,
                            if pt {
                                "Mover para o fim"
                            } else {
                                "Move to bottom"
                            },
                            palette,
                        )
                        .clicked()
                    {
                        act.reorder_from_to = Some((vid_idx, total_len - 1));
                    }
                }
                if let Some(rm_lbl) = remove_label {
                    ui.separator();
                    if search_menu_item_ui(ui, SearchMenuIcon::Trash, rm_lbl, palette).clicked() {
                        act.remove_url = Some(video.url.clone());
                    }
                }
            });
    }

    fn playlist_detail_shorts_grid_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        videos: &[Video],
        filtered_indices: &[usize],
        spec: &PlaylistDetailSpec<'_>,
        palette: &ThemePalette,
    ) -> PlaylistListAction {
        let pt = self.ui_pt();
        let mut act = PlaylistListAction::default();
        let avail_w = ui.available_width().max(260.0);
        let gap = 12.0;
        let cols = ((avail_w + gap) / 215.0).floor().clamp(2.0, 5.0) as usize;
        let card_w = ((avail_w - gap * (cols.saturating_sub(1) as f32)) / cols as f32).max(120.0);
        let thumb_h = (card_w * 16.0 / 9.0).round();
        let footer_h = 74.0;
        let row_h = thumb_h + footer_h;
        let total_rows = filtered_indices.len().div_ceil(cols);

        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 14.0;
            egui::ScrollArea::vertical()
                .id_salt((spec.scroll_id, "shorts_grid"))
                .auto_shrink([false, false])
                .show_rows(ui, row_h, total_rows, |ui, row_range| {
                    let row_w = ui.available_width().max(card_w);
                    for row_idx in row_range {
                        let start = row_idx * cols;
                        let end = (start + cols).min(filtered_indices.len());
                        ui.allocate_ui_with_layout(
                            egui::vec2(row_w, row_h),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_min_height(row_h);
                                ui.set_max_height(row_h);
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = gap;
                                    for (offset, &vid_idx) in
                                        filtered_indices[start..end].iter().enumerate()
                                    {
                                        let pos = start + offset;
                                        let video = &videos[vid_idx];
                                        let thumb_url = video_thumb_url(video);
                                        let views = self
                                            .metadata_for(&video.url)
                                            .and_then(|m| m.view_count);
                                        ui.push_id(("pl_short_card", spec.scroll_id, vid_idx), |ui| {
                                            ui.allocate_ui_with_layout(
                                                egui::vec2(card_w, row_h),
                                                egui::Layout::top_down(egui::Align::Min),
                                                |ui| {
                                                    ui.set_width(card_w);
                                                    ui.set_max_width(card_w);
                                                    let thumb_resp = if !thumb_url.trim().is_empty() {
                                                        self.thumbnails.show_cover_rounded(
                                                            ui,
                                                            ctx,
                                                            &thumb_url,
                                                            egui::vec2(card_w, thumb_h),
                                                            12.0,
                                                        )
                                                    } else {
                                                        let (r, resp) = ui.allocate_exact_size(
                                                            egui::vec2(card_w, thumb_h),
                                                            egui::Sense::click(),
                                                        );
                                                        ui.painter().rect_filled(
                                                            r,
                                                            12.0,
                                                            palette.card_alt_bg,
                                                        );
                                                        resp
                                                    };
                                                    let hover_t = ctx.animate_bool_with_time(
                                                        ui.make_persistent_id((
                                                            "pl_short_hover",
                                                            spec.scroll_id,
                                                            vid_idx,
                                                        )),
                                                        thumb_resp.hovered(),
                                                        0.14,
                                                    );
                                                    paint_card_hover_highlight(
                                                        ui,
                                                        thumb_resp.rect,
                                                        12.0,
                                                        hover_t,
                                                        palette,
                                                    );
                                                    if thumb_resp.clicked() {
                                                        act.chosen = Some(video.clone());
                                                        act.chosen_pos = Some(pos);
                                                    }
                                                    ui.add_space(8.0);
                                                    ui.horizontal_top(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 4.0;
                                                        let title_w = (card_w - 28.0).max(56.0);
                                                        ui.vertical(|ui| {
                                                            ui.set_width(title_w);
                                                            ui.set_max_width(title_w);
                                                            let short_title =
                                                                truncate_text_chars(&video.title, 48);
                                                            let title_resp = ui
                                                                .add(
                                                                    egui::Button::new(
                                                                        RichText::new(short_title)
                                                                            .size(14.0)
                                                                            .strong()
                                                                            .color(palette.channel_fg),
                                                                    )
                                                                    .frame(false)
                                                                    .wrap(),
                                                                )
                                                                .on_hover_cursor(
                                                                    egui::CursorIcon::PointingHand,
                                                                )
                                                                .on_hover_text(&video.title);
                                                            if title_resp.clicked() {
                                                                act.chosen = Some(video.clone());
                                                                act.chosen_pos = Some(pos);
                                                            }
                                                        });
                                                        let menu_id = ui.make_persistent_id((
                                                            "pl_short_menu",
                                                            spec.scroll_id,
                                                            vid_idx,
                                                        ));
                                                        ui.with_layout(
                                                            egui::Layout::right_to_left(
                                                                egui::Align::TOP,
                                                            ),
                                                            |ui| {
                                                                self.playlist_item_menu_ui(
                                                                    ui,
                                                                    video,
                                                                    menu_id,
                                                                    spec.remove_label,
                                                                    None,
                                                                    palette,
                                                                    &mut act,
                                                                );
                                                            },
                                                        );
                                                    });
                                                    if let Some(count) = views {
                                                        ui.label(
                                                            RichText::new(
                                                                format_playlist_short_views(
                                                                    count, pt,
                                                                ),
                                                            )
                                                            .size(12.5)
                                                            .color(palette.neutral_status_fg),
                                                        );
                                                    } else if !video.channel.trim().is_empty() {
                                                        ui.label(
                                                            RichText::new(&video.channel)
                                                                .size(12.5)
                                                                .color(palette.neutral_status_fg),
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                    }
                                });
                            },
                        );
                    }
                });
        });
        act
    }

    fn playlist_detail_list_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        videos: &[Video],
        filtered_indices: &[usize],
        spec: &PlaylistDetailSpec<'_>,
        palette: &ThemePalette,
    ) -> PlaylistListAction {
        let pt = self.ui_pt();
        let mut act = PlaylistListAction::default();
        let chips = if pt {
            [(0_u8, "Tudo"), (1, "Vídeos"), (2, "Shorts")]
        } else {
            [(0_u8, "All"), (1, "Videos"), (2, "Shorts")]
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (chip, lbl) in chips {
                if styled_pill_button(ui, lbl, self.library.detail_chip_filter == chip, palette)
                    .clicked()
                {
                    self.library.detail_chip_filter = chip;
                }
            }
        });
        ui.add_space(12.0);
        if filtered_indices.is_empty() {
            if spec.loading {
                let loading_msg = if self.library.detail_chip_filter == 2 {
                    if pt {
                        "Carregando Shorts..."
                    } else {
                        "Loading Shorts..."
                    }
                } else if pt {
                    "Carregando vídeos..."
                } else {
                    "Loading videos..."
                };
                empty_state_card_ui(ui, spec.title, loading_msg, palette);
            } else {
                empty_state_card_ui(ui, spec.title, spec.empty_message, palette);
            }
            return act;
        }
        if self.library.detail_chip_filter == 2 {
            return self.playlist_detail_shorts_grid_ui(
                ui,
                ctx,
                videos,
                filtered_indices,
                spec,
                palette,
            );
        }

        let can_reorder = (spec.local_collection_idx.is_some() || spec.is_editable)
            && self.library.detail_chip_filter == 0
            && filtered_indices.len() > 1
            && !spec.playlist_id.starts_with("RD");
        let drag_id = egui::Id::new(("fasttube_pl_drag_src", spec.playlist_id));
        let mut active_drag_src = ctx.data(|d| d.get_temp::<usize>(drag_id));
        let mut hovered_drop_target: Option<(usize, bool, egui::Rect)> = None;

        let row_h = 94.0;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            egui::ScrollArea::vertical()
                .id_salt(spec.scroll_id)
                .auto_shrink([false, false])
                .show_rows(ui, row_h, filtered_indices.len(), |ui, row_range| {
                    let row_w = ui.available_width();
                    for pos in row_range {
                        let vid_idx = filtered_indices[pos];
                        let video = &videos[vid_idx];
                        let thumb_url = video_thumb_url(video);
                        let is_drag_source = active_drag_src == Some(vid_idx);
                        ui.push_id(("pl_row", spec.scroll_id, vid_idx), |ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(row_w, row_h),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_min_height(row_h);
                                    ui.set_max_height(row_h);
                                    let row_resp = ui.horizontal_top(|ui| {
                                        ui.set_min_width(ui.available_width());
                                        if can_reorder {
                                            let handle_resp = ui
                                                .add_sized(
                                                    [22.0, 90.0],
                                                    egui::Label::new(
                                                        RichText::new("⋮⋮")
                                                            .size(14.0)
                                                            .strong()
                                                            .color(if is_drag_source {
                                                                palette.info_accent_fg
                                                            } else {
                                                                palette.neutral_status_fg
                                                            }),
                                                    )
                                                    .sense(egui::Sense::click_and_drag()),
                                                )
                                                .on_hover_cursor(egui::CursorIcon::Grab)
                                                .on_hover_text(if pt {
                                                    "Arraste para reordenar"
                                                } else {
                                                    "Drag to reorder"
                                                });
                                            if handle_resp.drag_started()
                                                || (handle_resp.dragged()
                                                    && active_drag_src.is_none())
                                            {
                                                active_drag_src = Some(vid_idx);
                                                ctx.data_mut(|d| d.insert_temp(drag_id, vid_idx));
                                            }
                                        }
                                        ui.add_sized(
                                            [if can_reorder { 28.0 } else { 38.0 }, 90.0],
                                            egui::Label::new(
                                                RichText::new(format!("{}", pos + 1))
                                                    .size(13.0)
                                                    .color(palette.neutral_status_fg),
                                            ),
                                        );
                                        let thumb_size = egui::vec2(160.0, 90.0);
                                        let thumb_resp = if !thumb_url.trim().is_empty() {
                                            self.thumbnails
                                                .show_rounded(ui, ctx, &thumb_url, thumb_size, 8.0)
                                        } else {
                                            let (r, resp) = ui.allocate_exact_size(
                                                thumb_size,
                                                egui::Sense::click(),
                                            );
                                            ui.painter().rect_filled(r, 8.0, palette.card_alt_bg);
                                            resp
                                        };
                                        paint_thumbnail_overlays(
                                            ui,
                                            thumb_resp.rect,
                                            &video.duration,
                                            None,
                                            false,
                                        );
                                        ui.add_space(12.0);
                                        ui.vertical(|ui| {
                                            ui.set_max_width(
                                                (ui.available_width() - 40.0).max(120.0),
                                            );
                                            let title_resp = ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new(&video.title)
                                                            .size(14.5)
                                                            .strong()
                                                            .color(palette.channel_fg),
                                                    )
                                                    .frame(false),
                                                )
                                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                                            ui.add_space(3.0);
                                            let mut meta_parts = Vec::new();
                                            if !video.channel.trim().is_empty() {
                                                meta_parts.push(video.channel.trim().to_owned());
                                            }
                                            if let Some(sub) = format_youtube_card_subline(
                                                "",
                                                self.metadata_for(&video.url),
                                            ) {
                                                meta_parts.push(sub);
                                            } else if !video.duration.trim().is_empty() {
                                                meta_parts.push(video.duration.trim().to_owned());
                                            }
                                            let meta = meta_parts.join(" · ");
                                            ui.label(
                                                RichText::new(meta)
                                                    .size(12.5)
                                                    .color(palette.neutral_status_fg),
                                            );
                                            if thumb_resp.clicked() || title_resp.clicked() {
                                                act.chosen = Some(video.clone());
                                                act.chosen_pos = Some(pos);
                                            }
                                        });
                                        let menu_id = ui.make_persistent_id((
                                            "pl_row_menu",
                                            spec.scroll_id,
                                            vid_idx,
                                        ));
                                        let reorder_ctx =
                                            can_reorder.then_some((vid_idx, videos.len()));
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::TOP),
                                            |ui| {
                                                self.playlist_item_menu_ui(
                                                    ui,
                                                    video,
                                                    menu_id,
                                                    spec.remove_label,
                                                    reorder_ctx,
                                                    palette,
                                                    &mut act,
                                                );
                                            },
                                        );
                                    });
                                    let row_rect = row_resp.response.rect;
                                    if active_drag_src.is_some()
                                        && let Some(pointer_pos) =
                                            ctx.input(|i| i.pointer.interact_pos())
                                        && row_rect
                                            .expand2(egui::vec2(4.0, 4.0))
                                            .contains(pointer_pos)
                                    {
                                        let insert_after = pointer_pos.y > row_rect.center().y;
                                        hovered_drop_target =
                                            Some((vid_idx, insert_after, row_rect));
                                    }
                                    paint_card_hover_highlight(
                                        ui,
                                        row_rect.expand2(egui::vec2(6.0, 4.0)),
                                        10.0,
                                        ctx.animate_bool_with_time(
                                            ui.make_persistent_id((
                                                "pl_row_hover",
                                                spec.scroll_id,
                                                vid_idx,
                                            )),
                                            row_resp.response.hovered(),
                                            0.12,
                                        ),
                                        palette,
                                    );
                                },
                            );
                        });
                    }
                    if let (Some(src), Some((target_idx, insert_after, rect))) =
                        (active_drag_src, hovered_drop_target)
                        && src != target_idx
                    {
                        let line_y = if insert_after {
                            rect.bottom() + 2.0
                        } else {
                            rect.top() - 2.0
                        };
                        ui.painter().line_segment(
                            [
                                egui::pos2(rect.left() + 4.0, line_y),
                                egui::pos2(rect.right() - 4.0, line_y),
                            ],
                            egui::Stroke::new(2.5, palette.info_accent_fg),
                        );
                    }
                    if spec.playlist_id.starts_with("RD") && !spec.loading {
                        ui.add_space(10.0);
                        if styled_pill_button(
                            ui,
                            if pt {
                                "Carregar mais do Mix"
                            } else {
                                "Load more Mix"
                            },
                            false,
                            palette,
                        )
                        .clicked()
                        {
                            act.load_more_mix = true;
                        }
                    }
                });
        });
        if let Some(src_idx) = active_drag_src {
            let pointer_released = ctx.input(|i| !i.pointer.primary_down());
            if pointer_released {
                ctx.data_mut(|d| d.remove::<usize>(drag_id));
                if let Some((target_idx, insert_after, _)) = hovered_drop_target {
                    let mut dst_idx = if insert_after {
                        target_idx + 1
                    } else {
                        target_idx
                    };
                    if src_idx < dst_idx {
                        dst_idx = dst_idx.saturating_sub(1);
                    }
                    dst_idx = dst_idx.min(videos.len().saturating_sub(1));
                    if src_idx != dst_idx {
                        act.reorder_from_to = Some((src_idx, dst_idx));
                    }
                }
            } else {
                ctx.request_repaint();
            }
        }
        act
    }

    pub(crate) fn remove_watched_from_watch_later(&mut self, ctx: &egui::Context) {
        let pt = self.ui_pt();
        let watched_ids: HashSet<String> = self
            .history
            .data
            .entries()
            .iter()
            .filter(|e| {
                e.is_near_end()
                    || e.duration_ms
                        .is_some_and(|d| d > 0 && (e.position_ms as f64 / d as f64) >= 0.90)
            })
            .map(|e| e.video_id.clone())
            .collect();
        if watched_ids.is_empty() {
            let msg = if pt {
                "Nenhum vídeo assistido encontrado em Assistir mais tarde."
            } else {
                "No watched videos found in Watch later."
            };
            self.toasts.info(msg);
            return;
        }
        let local_urls: Vec<String> = self
            .library
            .data
            .watch_later
            .iter()
            .filter(|r| {
                history::youtube_video_id(&r.video.url).is_some_and(|id| watched_ids.contains(&id))
            })
            .map(|r| r.video.url.clone())
            .collect();
        let mut removed_count = 0usize;
        for url in local_urls {
            if self.library.data.remove_from_watch_later(&url) {
                removed_count += 1;
            }
        }
        if removed_count > 0 {
            self.save_library();
        }
        let remote_matches: Vec<(String, Option<String>)> = self
            .library
            .watch_later_remote
            .items
            .iter()
            .filter_map(|it| {
                let vid = history::youtube_video_id(&it.result.video.url)?;
                watched_ids
                    .contains(&vid)
                    .then(|| (vid, it.set_video_id.clone()))
            })
            .collect();
        for (vid, set_vid) in remote_matches {
            self.dispatch_playlist_remove_video("WL".into(), vid, set_vid, ctx);
            removed_count += 1;
        }
        let msg = if pt {
            format!("{removed_count} vídeo(s) assistido(s) removido(s).")
        } else {
            format!("{removed_count} watched video(s) removed.")
        };
        self.toasts.success(msg);
    }

    pub(crate) fn load_more_mix_playlist_detail(
        &mut self,
        playlist_id: String,
        ctx: &egui::Context,
    ) {
        let seed_vid = self
            .library
            .active_playlist_detail
            .items
            .last()
            .or_else(|| self.library.active_playlist_detail.items.first())
            .and_then(|it| history::youtube_video_id(&it.result.video.url));
        let title = self
            .library
            .active_playlist_detail
            .meta
            .as_ref()
            .map(|m| m.title.clone())
            .or_else(|| self.library.active_remote_playlist_title.clone());
        self.open_remote_playlist_detail(playlist_id, title, seed_vid, ctx);
    }

    pub(crate) fn dispatch_playlist_rename(
        &mut self,
        playlist_id: String,
        new_title: String,
        ctx: &egui::Context,
    ) {
        let pt = self.ui_pt();
        if let Some(meta) = self.library.active_playlist_detail.meta.as_mut() {
            meta.title = new_title.clone();
        }
        self.library.active_remote_playlist_title = Some(new_title.clone());
        for card in &mut self.library.remote_playlists.items {
            if card.id == playlist_id {
                card.title = new_title.clone();
            }
        }
        self.library.remote_playlists.revision =
            self.library.remote_playlists.revision.wrapping_add(1);
        let Some(session) = self.resolve_active_web_session() else {
            return;
        };
        if !cfg!(test) {
            let ctx_clone = ctx.clone();
            std::thread::spawn(move || {
                let cancel = process::Cancellation::new();
                let _ = playlist_sync::rename_playlist_innertube(
                    &playlist_id,
                    &new_title,
                    &session,
                    &cancel,
                );
                ctx_clone.request_repaint();
            });
        }
        self.toasts.success(if pt {
            "Playlist renomeada."
        } else {
            "Playlist renamed."
        });
    }

    pub(crate) fn dispatch_playlist_set_privacy(
        &mut self,
        playlist_id: String,
        privacy: playlist_sync::PlaylistPrivacy,
        ctx: &egui::Context,
    ) {
        let pt = self.ui_pt();
        if let Some(meta) = self.library.active_playlist_detail.meta.as_mut() {
            meta.privacy = privacy;
        }
        for card in &mut self.library.remote_playlists.items {
            if card.id == playlist_id {
                card.privacy = privacy;
            }
        }
        self.library.remote_playlists.revision =
            self.library.remote_playlists.revision.wrapping_add(1);
        let Some(session) = self.resolve_active_web_session() else {
            return;
        };
        if !cfg!(test) {
            let ctx_clone = ctx.clone();
            std::thread::spawn(move || {
                let cancel = process::Cancellation::new();
                let _ = playlist_sync::set_playlist_privacy_innertube(
                    &playlist_id,
                    privacy,
                    &session,
                    &cancel,
                );
                ctx_clone.request_repaint();
            });
        }
        self.toasts.success(if pt {
            "Privacidade da playlist atualizada."
        } else {
            "Playlist privacy updated."
        });
    }

    pub(crate) fn dispatch_playlist_delete(&mut self, playlist_id: String, ctx: &egui::Context) {
        let pt = self.ui_pt();
        self.library
            .remote_playlists
            .items
            .retain(|c| c.id != playlist_id);
        self.library.remote_playlists.revision =
            self.library.remote_playlists.revision.wrapping_add(1);
        if self.library.active_playlist_detail.playlist_id.as_deref() == Some(playlist_id.as_str())
        {
            self.library.active_playlist_detail.cancel_active();
            self.library.active_playlist_detail.playlist_id = None;
            self.library.active_remote_playlist_title = None;
        }
        let Some(session) = self.resolve_active_web_session() else {
            return;
        };
        if !cfg!(test) {
            let ctx_clone = ctx.clone();
            std::thread::spawn(move || {
                let cancel = process::Cancellation::new();
                let _ = playlist_sync::delete_playlist_innertube(&playlist_id, &session, &cancel);
                ctx_clone.request_repaint();
            });
        }
        self.toasts.success(if pt {
            "Playlist excluída."
        } else {
            "Playlist deleted."
        });
    }

    pub(crate) fn dispatch_playlist_move_video(
        &mut self,
        playlist_id: String,
        set_video_id: String,
        predecessor_set_video_id: Option<String>,
        ctx: &egui::Context,
    ) {
        let Some(session) = self.resolve_active_web_session() else {
            return;
        };
        if !cfg!(test) {
            let ctx_clone = ctx.clone();
            std::thread::spawn(move || {
                let cancel = process::Cancellation::new();
                let _ = playlist_sync::move_playlist_video_innertube(
                    &playlist_id,
                    &set_video_id,
                    predecessor_set_video_id.as_deref(),
                    &session,
                    &cancel,
                );
                ctx_clone.request_repaint();
            });
        }
    }

    pub(crate) fn reorder_playlist_detail_item(
        &mut self,
        playlist_id: &str,
        local_collection_idx: Option<usize>,
        from_idx: usize,
        to_idx: usize,
        ctx: &egui::Context,
    ) {
        if from_idx == to_idx {
            return;
        }
        if let Some(col_idx) = local_collection_idx {
            if let Some(col) = self.library.data.collections.get_mut(col_idx)
                && from_idx < col.videos.len()
                && to_idx < col.videos.len()
            {
                let item = col.videos.remove(from_idx);
                col.videos.insert(to_idx, item);
                self.library.revision = self.library.revision.wrapping_add(1);
                self.save_library();
            }
            return;
        }
        if playlist_id == "WL" {
            let remote_len = self.library.watch_later_remote.items.len();
            if from_idx < remote_len && to_idx < remote_len {
                let moved = self.library.watch_later_remote.items.remove(from_idx);
                let moved_set_id = moved.set_video_id.clone();
                self.library.watch_later_remote.items.insert(to_idx, moved);
                let pred_set_id = if to_idx > 0 {
                    self.library
                        .watch_later_remote
                        .items
                        .get(to_idx - 1)
                        .and_then(|it| it.set_video_id.clone())
                } else {
                    None
                };
                self.library.watch_later_remote.merged_cache_key = None;
                self.library.watch_later_remote.revision =
                    self.library.watch_later_remote.revision.wrapping_add(1);
                if let Some(set_id) = moved_set_id {
                    self.dispatch_playlist_move_video("WL".into(), set_id, pred_set_id, ctx);
                }
            } else if from_idx < self.library.data.watch_later.len()
                && to_idx < self.library.data.watch_later.len()
            {
                let item = self.library.data.watch_later.remove(from_idx);
                self.library.data.watch_later.insert(to_idx, item);
                self.library.watch_later_remote.merged_cache_key = None;
                self.library.revision = self.library.revision.wrapping_add(1);
                self.save_library();
            }
            return;
        }
        let remote_len = self.library.active_playlist_detail.items.len();
        if from_idx < remote_len && to_idx < remote_len {
            let moved = self.library.active_playlist_detail.items.remove(from_idx);
            let moved_set_id = moved.set_video_id.clone();
            self.library
                .active_playlist_detail
                .items
                .insert(to_idx, moved);
            let pred_set_id = if to_idx > 0 {
                self.library
                    .active_playlist_detail
                    .items
                    .get(to_idx - 1)
                    .and_then(|it| it.set_video_id.clone())
            } else {
                None
            };
            self.library.active_playlist_detail.merged_cache_key = None;
            self.library.active_playlist_detail.revision =
                self.library.active_playlist_detail.revision.wrapping_add(1);
            if let Some(set_id) = moved_set_id {
                self.dispatch_playlist_move_video(playlist_id.to_owned(), set_id, pred_set_id, ctx);
            }
        }
    }
}

struct PlaylistDetailSpec<'a> {
    playlist_id: &'a str,
    owner: &'a str,
    privacy: playlist_sync::PlaylistPrivacy,
    total_count: Option<usize>,
    local_collection_idx: Option<usize>,
    is_editable: bool,
    title: &'a str,
    subtitle: &'a str,
    empty_message: &'a str,
    scroll_id: &'a str,
    can_back_to_playlists: bool,
    remove_label: Option<&'a str>,
    loading: bool,
    can_refresh: bool,
    error: Option<&'a str>,
}

#[derive(Default)]
struct PlaylistBannerAction {
    go_back: bool,
    play_all: bool,
    shuffle: bool,
    enqueue_all: bool,
    download_playlist: bool,
    remove_watched: bool,
    load_more_mix: bool,
    rename_to: Option<String>,
    set_privacy: Option<playlist_sync::PlaylistPrivacy>,
    delete_playlist: bool,
    refresh: bool,
    cancel_load: bool,
}

#[derive(Default)]
struct PlaylistListAction {
    chosen: Option<Video>,
    chosen_pos: Option<usize>,
    enqueue_target: Option<Video>,
    download_target: Option<Video>,
    share_target: Option<Video>,
    open_channel_target: Option<ChannelTarget>,
    remove_url: Option<String>,
    reorder_from_to: Option<(usize, usize)>,
    load_more_mix: bool,
}

fn format_grouped_count(n: usize, pt: bool) -> String {
    let sep = if pt { '.' } else { ',' };
    let digits = n.to_string();
    let len = digits.len();
    if len <= 3 {
        return digits;
    }
    let mut out = String::with_capacity(len + (len - 1) / 3);
    for (idx, ch) in digits.chars().enumerate() {
        if idx > 0 && (len - idx).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(ch);
    }
    out
}

fn format_playlist_short_views(views: u64, pt: bool) -> String {
    if pt {
        if views >= 1_000_000_000 {
            let billions = views as f64 / 1_000_000_000.0;
            if billions >= 10.0 || (views % 1_000_000_000) < 100_000_000 {
                format!("{} bi de visualizações", views / 1_000_000_000)
            } else {
                format!(
                    "{} bi de visualizações",
                    format!("{billions:.1}").replace('.', ",")
                )
            }
        } else if views >= 1_000_000 {
            let millions = views as f64 / 1_000_000.0;
            if millions >= 10.0 || (views % 1_000_000) < 100_000 {
                format!("{} mi de visualizações", views / 1_000_000)
            } else {
                format!(
                    "{} mi de visualizações",
                    format!("{millions:.1}").replace('.', ",")
                )
            }
        } else if views >= 1_000 {
            format!("{} mil visualizações", views / 1_000)
        } else if views == 1 {
            "1 visualização".to_owned()
        } else {
            format!("{views} visualizações")
        }
    } else if views >= 1_000_000_000 {
        let billions = views as f64 / 1_000_000_000.0;
        if billions >= 10.0 || (views % 1_000_000_000) < 100_000_000 {
            format!("{}B views", views / 1_000_000_000)
        } else {
            format!("{billions:.1}B views")
        }
    } else if views >= 1_000_000 {
        let millions = views as f64 / 1_000_000.0;
        if millions >= 10.0 || (views % 1_000_000) < 100_000 {
            format!("{}M views", views / 1_000_000)
        } else {
            format!("{millions:.1}M views")
        }
    } else if views >= 1_000 {
        format!("{}K views", views / 1_000)
    } else if views == 1 {
        "1 view".to_owned()
    } else {
        format!("{views} views")
    }
}

fn video_is_short(video: &Video) -> bool {
    if video.url.contains("/shorts/") {
        return true;
    }
    let parts: Vec<&str> = video.duration.trim().split(':').collect();
    match parts.as_slice() {
        [m, s] => {
            if let (Ok(mins), Ok(secs)) = (m.parse::<u64>(), s.parse::<u64>()) {
                mins == 0 || (mins == 1 && secs <= 2)
            } else {
                false
            }
        }
        _ => false,
    }
}

fn video_thumb_url(video: &Video) -> String {
    if !video.thumbnail.trim().is_empty() {
        video.thumbnail.clone()
    } else if let Some(vid) = history::youtube_video_id(&video.url) {
        format!("https://i.ytimg.com/vi/{vid}/hqdefault.jpg")
    } else {
        String::new()
    }
}

pub(super) fn remove_liked_video_from_state(
    items: &mut Vec<SearchResult>,
    seen_keys: &mut std::collections::HashSet<String>,
    total_results: &mut Option<u64>,
    url: &str,
) -> usize {
    let target_id = history::youtube_video_id(url);
    let before = items.len();
    items.retain(|item| {
        let matches_id = target_id
            .as_deref()
            .is_some_and(|id| history::youtube_video_id(&item.video.url).as_deref() == Some(id));
        !(matches_id || item.video.url == url)
    });
    let removed = before - items.len();
    if let Some(id) = target_id.as_deref() {
        seen_keys.remove(id);
    }
    seen_keys.remove(url);
    if removed > 0
        && let Some(total) = total_results
    {
        *total = total.saturating_sub(removed as u64);
    }
    removed
}
