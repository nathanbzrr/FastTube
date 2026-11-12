use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

pub const THUMBNAIL_WIDTH: f32 = 160.0;
pub const THUMBNAIL_HEIGHT: f32 = 90.0;
pub const THUMBNAIL_SIZE: Vec2 = egui::vec2(THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT);

/// Limite de entradas de miniaturas acompanhadas pelo cache da aplicação.
pub const MAX_RETAINED_THUMBNAILS: usize = 48;

/// Limite de vagas lógicas para carregamentos de miniaturas.
pub const MAX_CONCURRENT_LOADS: usize = 6;

/// Máximo de novos carregamentos iniciados por passagem de interface.
pub const MAX_NEW_LOADS_PER_PASS: usize = 3;

/// Intervalo mínimo entre o início de novos carregamentos para suavizar picos.
pub const LOAD_START_INTERVAL: Duration = Duration::from_millis(20);

/// Margem vertical além da janela de recorte visível considerada próxima à rolagem.
pub const VIEWPORT_OVERSCAN_PX: f32 = 110.0;

/// Tempo limite para considerar um carregamento pendente como expirado e liberar a vaga.
pub const IN_FLIGHT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryState {
    Loading { started_at: Instant },
    Ready { size: [u32; 2] },
    Failed,
}

#[derive(Clone, Debug)]
struct CacheEntry {
    state: EntryState,
    last_visible_pass: u64,
    last_access_tick: u64,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub uri: String,
    pub is_visible: bool,
    pub sort_key: i32,
}

#[derive(Clone, Debug)]
struct RetiredInFlight {
    uri: String,
    forget_until: Instant,
}

pub struct ThumbnailCache {
    entries: HashMap<String, CacheEntry>,
    candidates: Vec<Candidate>,
    retired_in_flight: VecDeque<RetiredInFlight>,
    pass_index: u64,
    access_counter: u64,
    last_load_started_at: Option<Instant>,
    max_retained: usize,
    max_concurrent_loads: usize,
    max_new_loads_per_pass: usize,
    load_start_interval: Duration,
    allow_burst_loads: bool,
}

impl Default for ThumbnailCache {
    fn default() -> Self {
        let mut cache = Self::with_limits(
            MAX_RETAINED_THUMBNAILS,
            MAX_CONCURRENT_LOADS,
            MAX_NEW_LOADS_PER_PASS,
            LOAD_START_INTERVAL,
        );
        cache.allow_burst_loads = true;
        cache
    }
}

impl ThumbnailCache {
    pub fn with_limits(
        max_retained: usize,
        max_concurrent_loads: usize,
        max_new_loads_per_pass: usize,
        load_start_interval: Duration,
    ) -> Self {
        let max_retained = max_retained.max(1);
        let max_concurrent_loads = max_concurrent_loads.clamp(1, max_retained);
        let max_new_loads_per_pass = max_new_loads_per_pass.clamp(1, max_concurrent_loads);
        Self {
            entries: HashMap::with_capacity(max_retained),
            candidates: Vec::new(),
            retired_in_flight: VecDeque::new(),
            pass_index: 0,
            access_counter: 0,
            last_load_started_at: None,
            max_retained,
            max_concurrent_loads,
            max_new_loads_per_pass,
            load_start_interval,
            allow_burst_loads: false,
        }
    }

    #[cfg(test)]
    pub fn retained_count(&self) -> usize {
        self.entries.len()
    }

    pub fn in_flight_count(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| matches!(entry.state, EntryState::Loading { .. }))
            .count()
    }

    fn bump_tick(&mut self) -> u64 {
        self.access_counter = self.access_counter.wrapping_add(1);
        self.access_counter
    }

    /// Limpa todas as miniaturas retidas (ex.: ao trocar os resultados de uma pesquisa)
    /// e acompanha temporariamente as requisições que ainda estavam em voo.
    pub fn reset(&mut self, ctx: &egui::Context, previous_videos: &[crate::search::Video]) {
        let now = Instant::now();
        let forgotten = self.drain_all_for_reset(now);
        for uri in forgotten {
            ctx.forget_image(&uri);
        }
        for video in previous_videos {
            if !video.thumbnail.is_empty() {
                ctx.forget_image(&video.thumbnail);
            }
        }
    }

    pub fn retain_for_videos(
        &mut self,
        ctx: &egui::Context,
        previous_videos: &[crate::search::Video],
        new_videos: &[crate::search::Video],
    ) {
        let retained: std::collections::HashSet<&str> = new_videos
            .iter()
            .filter(|video| !video.thumbnail.is_empty())
            .map(|video| video.thumbnail.as_str())
            .collect();
        let now = Instant::now();
        self.candidates
            .retain(|candidate| retained.contains(candidate.uri.as_str()));
        let removed: Vec<String> = self
            .entries
            .keys()
            .filter(|uri| !retained.contains(uri.as_str()))
            .cloned()
            .collect();
        for uri in removed {
            if let Some(entry) = self.entries.remove(&uri)
                && matches!(entry.state, EntryState::Loading { .. })
            {
                self.push_retired_in_flight(&uri, now);
            }
            ctx.forget_image(&uri);
        }
        for video in previous_videos {
            if !video.thumbnail.is_empty() && !retained.contains(video.thumbnail.as_str()) {
                ctx.forget_image(&video.thumbnail);
            }
        }
        self.retired_in_flight
            .retain(|item| !retained.contains(item.uri.as_str()));
    }

    fn drain_all_for_reset(&mut self, now: Instant) -> Vec<String> {
        self.candidates.clear();
        let entries = std::mem::take(&mut self.entries);
        let mut forgotten = Vec::with_capacity(entries.len());
        for (uri, entry) in entries {
            if matches!(entry.state, EntryState::Loading { .. }) {
                self.push_retired_in_flight(&uri, now);
            }
            forgotten.push(uri);
        }
        forgotten
    }

    fn push_retired_in_flight(&mut self, uri: &str, now: Instant) {
        if let Some(existing) = self
            .retired_in_flight
            .iter_mut()
            .find(|item| item.uri == uri)
        {
            existing.forget_until = now + IN_FLIGHT_TIMEOUT;
            return;
        }
        self.retired_in_flight.push_back(RetiredInFlight {
            uri: uri.to_owned(),
            forget_until: now + IN_FLIGHT_TIMEOUT,
        });
    }

    /// Executa manutenção periódica mesmo quando a lista não está visível (ex.: em tela cheia).
    pub fn maintain(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        for uri in self.sweep_expired(now) {
            ctx.forget_image(&uri);
        }
    }

    fn sweep_expired(&mut self, now: Instant) -> Vec<String> {
        let mut to_forget = Vec::new();

        let timed_out: Vec<String> = self
            .entries
            .iter()
            .filter_map(|(uri, entry)| match entry.state {
                EntryState::Loading { started_at }
                    if now.saturating_duration_since(started_at) >= IN_FLIGHT_TIMEOUT =>
                {
                    Some(uri.clone())
                }
                _ => None,
            })
            .collect();

        for uri in timed_out {
            if let Some(entry) = self.entries.get_mut(&uri) {
                entry.state = EntryState::Failed;
            }
            self.push_retired_in_flight(&uri, now);
            to_forget.push(uri);
        }

        let mut remaining = VecDeque::with_capacity(self.retired_in_flight.len());
        while let Some(item) = self.retired_in_flight.pop_front() {
            if !matches!(
                self.entries.get(&item.uri).map(|entry| entry.state),
                Some(EntryState::Loading { .. } | EntryState::Ready { .. })
            ) {
                to_forget.push(item.uri.clone());
            }
            if now < item.forget_until {
                remaining.push_back(item);
            }
        }
        self.retired_in_flight = remaining;

        to_forget
    }

    pub fn begin_pass(&mut self) {
        self.pass_index = self.pass_index.wrapping_add(1);
        self.candidates.clear();
    }

    /// Renderiza o espaço fixo `160x90` da miniatura, garantindo estabilidade geométrica
    /// de rolagem antes, durante e após o carregamento ou descarte.
    pub fn show(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, uri: &str) -> egui::Response {
        self.show_sized(ui, ctx, uri, THUMBNAIL_SIZE)
    }

    pub fn show_sized(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        uri: &str,
        size: Vec2,
    ) -> egui::Response {
        self.show_rounded(ui, ctx, uri, size, 8.0)
    }

    pub fn show_rounded(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        uri: &str,
        size: Vec2,
        corner_radius: f32,
    ) -> egui::Response {
        let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
        response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

        let viewport = ui.clip_rect();
        let is_visible = viewport_intersects(viewport, rect, 0.0);
        let is_near_viewport = viewport_intersects(viewport, rect, VIEWPORT_OVERSCAN_PX);

        let bg_color = if response.hovered() {
            Color32::from_rgb(32, 32, 36)
        } else {
            Color32::from_rgb(20, 20, 23)
        };
        ui.painter().rect_filled(rect, corner_radius, bg_color);

        if uri.is_empty() {
            paint_placeholder(ui, rect, PlaceholderKind::Empty);
            paint_hover_outline_rounded(ui, rect, response.hovered(), corner_radius);
            return response;
        }

        if is_near_viewport {
            let tick = self.bump_tick();
            let pass = self.pass_index;
            if let Some(entry) = self.entries.get_mut(uri) {
                entry.last_visible_pass = pass;
                entry.last_access_tick = tick;
            }
        }

        let state = self.entries.get(uri).map(|entry| entry.state);
        match state {
            Some(EntryState::Ready { .. } | EntryState::Loading { .. }) => {
                if is_near_viewport || matches!(state, Some(EntryState::Loading { .. })) {
                    match Self::poll_texture(ctx, uri) {
                        PollOutcome::Ready { texture_id, size } => {
                            if let Some(entry) = self.entries.get_mut(uri) {
                                entry.state = EntryState::Ready { size };
                            }
                            if is_visible {
                                paint_fitted_texture_rounded(
                                    ui,
                                    rect,
                                    texture_id,
                                    size,
                                    corner_radius,
                                );
                            }
                        }
                        PollOutcome::Pending => {
                            if is_visible {
                                paint_placeholder(ui, rect, PlaceholderKind::Loading);
                            }
                        }
                        PollOutcome::Failed => {
                            if let Some(entry) = self.entries.get_mut(uri) {
                                entry.state = EntryState::Failed;
                            }
                            ctx.forget_image(uri);
                            if is_visible {
                                paint_placeholder(ui, rect, PlaceholderKind::Failed);
                            }
                        }
                    }
                } else if is_visible {
                    paint_placeholder(ui, rect, PlaceholderKind::Pending);
                }
            }
            Some(EntryState::Failed) => {
                if is_visible {
                    paint_placeholder(ui, rect, PlaceholderKind::Failed);
                }
            }
            None => {
                if is_near_viewport {
                    self.record_candidate(uri, is_visible, rect.top() - viewport.top());
                }
                if is_visible {
                    paint_placeholder(ui, rect, PlaceholderKind::Pending);
                }
            }
        }

        paint_hover_outline_rounded(ui, rect, response.hovered(), corner_radius);
        response
    }

    fn record_candidate(&mut self, uri: &str, is_visible: bool, relative_top: f32) {
        if self.entries.contains_key(uri) {
            return;
        }
        let sort_key = relative_top.round() as i32;
        if let Some(existing) = self.candidates.iter_mut().find(|item| item.uri == uri) {
            existing.is_visible |= is_visible;
            existing.sort_key = existing.sort_key.min(sort_key);
            return;
        }
        self.candidates.push(Candidate {
            uri: uri.to_owned(),
            is_visible,
            sort_key,
        });
    }

    pub fn end_pass(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let (admitted, evicted) = self.schedule_pass(now);

        for uri in evicted {
            ctx.forget_image(&uri);
        }

        for uri in admitted {
            match Self::poll_texture(ctx, &uri) {
                PollOutcome::Ready { size, .. } => {
                    if let Some(entry) = self.entries.get_mut(&uri) {
                        entry.state = EntryState::Ready { size };
                    }
                }
                PollOutcome::Pending => {}
                PollOutcome::Failed => {
                    if let Some(entry) = self.entries.get_mut(&uri) {
                        entry.state = EntryState::Failed;
                    }
                    ctx.forget_image(&uri);
                }
            }
        }

        if self.in_flight_count() > 0 || !self.retired_in_flight.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(80));
        } else if !self.candidates.is_empty()
            && (self.entries.len() < self.max_retained
                || self.entries.values().any(|entry| {
                    entry.last_visible_pass < self.pass_index
                        && !matches!(entry.state, EntryState::Loading { .. })
                }))
        {
            let wait = self
                .last_load_started_at
                .map(|started| {
                    self.load_start_interval
                        .saturating_sub(now.saturating_duration_since(started))
                })
                .unwrap_or(Duration::ZERO)
                .max(Duration::from_millis(16));
            ctx.request_repaint_after(wait);
        }
    }

    fn schedule_pass(&mut self, now: Instant) -> (Vec<String>, Vec<String>) {
        let mut evicted = Vec::new();
        let mut admitted = Vec::new();

        while self.entries.len() > self.max_retained {
            if let Some(uri) = self.evict_one() {
                evicted.push(uri);
            } else {
                break;
            }
        }

        self.candidates.sort_by(|first, second| {
            second
                .is_visible
                .cmp(&first.is_visible)
                .then_with(|| first.sort_key.cmp(&second.sort_key))
        });

        let cooldown_ready = self
            .last_load_started_at
            .map(|started| now.saturating_duration_since(started) >= self.load_start_interval)
            .unwrap_or(true);

        if !cooldown_ready {
            return (admitted, evicted);
        }

        let mut started_this_pass = 0usize;
        let candidates = std::mem::take(&mut self.candidates);
        let mut remaining_candidates = Vec::new();

        for candidate in candidates {
            if self.entries.contains_key(&candidate.uri) {
                continue;
            }
            if self
                .retired_in_flight
                .iter()
                .any(|item| item.uri == candidate.uri)
            {
                remaining_candidates.push(candidate);
                continue;
            }
            let can_start = started_this_pass < self.max_new_loads_per_pass
                && self.in_flight_count() + self.retired_in_flight.len()
                    < self.max_concurrent_loads
                && (started_this_pass == 0
                    || self.allow_burst_loads
                    || self.load_start_interval.is_zero());
            if !can_start {
                remaining_candidates.push(candidate);
                continue;
            }

            if self.entries.len() >= self.max_retained {
                if let Some(victim) = self.evict_one() {
                    evicted.push(victim);
                } else {
                    remaining_candidates.push(candidate);
                    continue;
                }
            }

            let tick = self.bump_tick();
            self.entries.insert(
                candidate.uri.clone(),
                CacheEntry {
                    state: EntryState::Loading { started_at: now },
                    last_visible_pass: self.pass_index,
                    last_access_tick: tick,
                },
            );
            self.last_load_started_at = Some(now);
            started_this_pass += 1;
            admitted.push(candidate.uri);
        }

        self.candidates = remaining_candidates;
        (admitted, evicted)
    }

    fn evict_one(&mut self) -> Option<String> {
        let current_pass = self.pass_index;

        // Prioridade 1: entradas prontas ou com falha que estão fora da região visível atual (LRU).
        let offscreen_finished = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                entry.last_visible_pass < current_pass
                    && !matches!(entry.state, EntryState::Loading { .. })
            })
            .min_by_key(|(_, entry)| (entry.last_visible_pass, entry.last_access_tick))
            .map(|(uri, _)| uri.clone());

        if let Some(uri) = offscreen_finished {
            self.entries.remove(&uri);
            return Some(uri);
        }

        None
    }

    fn poll_texture(ctx: &egui::Context, uri: &str) -> PollOutcome {
        let size_hint = egui::SizeHint::Size {
            width: 720,
            height: 405,
            maintain_aspect_ratio: true,
        };
        match ctx.try_load_texture(uri, egui::TextureOptions::LINEAR, size_hint) {
            Ok(egui::load::TexturePoll::Ready { texture }) => {
                let width = texture.size.x.max(1.0).round() as u32;
                let height = texture.size.y.max(1.0).round() as u32;
                PollOutcome::Ready {
                    texture_id: texture.id,
                    size: [width, height],
                }
            }
            Ok(egui::load::TexturePoll::Pending { .. }) => PollOutcome::Pending,
            Err(_) => PollOutcome::Failed,
        }
    }
}

enum PollOutcome {
    Ready {
        texture_id: egui::TextureId,
        size: [u32; 2],
    },
    Pending,
    Failed,
}

#[derive(Clone, Copy)]
enum PlaceholderKind {
    Empty,
    Pending,
    Loading,
    Failed,
}

pub fn viewport_intersects(viewport: Rect, item_rect: Rect, vertical_overscan: f32) -> bool {
    if !viewport.is_positive() || !item_rect.is_positive() {
        return false;
    }
    let expanded = Rect::from_min_max(
        Pos2::new(viewport.min.x, viewport.min.y - vertical_overscan),
        Pos2::new(viewport.max.x, viewport.max.y + vertical_overscan),
    );
    expanded.intersects(item_rect)
}

pub fn fit_aspect_rect(bounds: Rect, intrinsic_size: [u32; 2]) -> Rect {
    let width = intrinsic_size[0].max(1) as f32;
    let height = intrinsic_size[1].max(1) as f32;
    let scale = (bounds.width() / width).min(bounds.height() / height);
    let target_size = Vec2::new(
        (width * scale).min(bounds.width()),
        (height * scale).min(bounds.height()),
    );
    Rect::from_center_size(bounds.center(), target_size)
}

fn paint_fitted_texture_rounded(
    ui: &egui::Ui,
    bounds: Rect,
    texture_id: egui::TextureId,
    intrinsic_size: [u32; 2],
    _corner_radius: f32,
) {
    let target = fit_aspect_rect(bounds, intrinsic_size);
    let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
    ui.painter().image(texture_id, target, uv, Color32::WHITE);
}

fn paint_placeholder(ui: &egui::Ui, rect: Rect, kind: PlaceholderKind) {
    let center = rect.center();
    let accent = match kind {
        PlaceholderKind::Empty => Color32::from_rgb(62, 62, 68),
        PlaceholderKind::Pending => Color32::from_rgb(76, 76, 84),
        PlaceholderKind::Loading => Color32::from_rgb(120, 92, 96),
        PlaceholderKind::Failed => Color32::from_rgb(96, 58, 58),
    };
    ui.painter()
        .circle_stroke(center, 13.0, Stroke::new(1.2, accent));
    let triangle = vec![
        Pos2::new(center.x - 4.0, center.y - 6.0),
        Pos2::new(center.x - 4.0, center.y + 6.0),
        Pos2::new(center.x + 6.0, center.y),
    ];
    ui.painter()
        .add(egui::Shape::convex_polygon(triangle, accent, Stroke::NONE));
}

fn paint_hover_outline_rounded(ui: &egui::Ui, rect: Rect, hovered: bool, corner_radius: f32) {
    let color = if hovered {
        Color32::from_rgb(95, 95, 105)
    } else {
        Color32::from_rgb(32, 32, 36)
    };
    ui.painter().rect_stroke(
        rect,
        corner_radius,
        Stroke::new(1.0, color),
        StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video_with_thumbnail(uri: &str) -> crate::search::Video {
        crate::search::Video {
            title: "Video".into(),
            channel: "Channel".into(),
            duration: "1:00".into(),
            url: "https://www.youtube.com/watch?v=video".into(),
            thumbnail: uri.into(),
        }
    }

    fn mark_ready(cache: &mut ThumbnailCache, uri: &str) {
        if let Some(entry) = cache.entries.get_mut(uri) {
            entry.state = EntryState::Ready { size: [320, 180] };
        }
    }

    #[test]
    fn progressive_video_retention_keeps_loaded_and_in_flight_thumbnails() {
        let ctx = egui::Context::default();
        let mut cache = ThumbnailCache::with_limits(4, 2, 1, Duration::ZERO);
        let now = Instant::now();
        cache.entries.insert(
            "https://img/shared.jpg".into(),
            CacheEntry {
                state: EntryState::Loading { started_at: now },
                last_visible_pass: 0,
                last_access_tick: 1,
            },
        );
        cache.entries.insert(
            "https://img/old.jpg".into(),
            CacheEntry {
                state: EntryState::Ready { size: [320, 180] },
                last_visible_pass: 0,
                last_access_tick: 2,
            },
        );
        let previous = vec![
            video_with_thumbnail("https://img/shared.jpg"),
            video_with_thumbnail("https://img/old.jpg"),
        ];
        let next = vec![video_with_thumbnail("https://img/shared.jpg")];

        cache.retain_for_videos(&ctx, &previous, &next);

        assert!(matches!(
            cache
                .entries
                .get("https://img/shared.jpg")
                .map(|entry| entry.state),
            Some(EntryState::Loading { .. })
        ));
        assert!(!cache.entries.contains_key("https://img/old.jpg"));
        assert!(
            cache
                .retired_in_flight
                .iter()
                .all(|item| item.uri != "https://img/shared.jpg")
        );
        assert_eq!(cache.in_flight_count(), 1);
    }

    #[test]
    fn gradual_loading_respects_per_pass_and_concurrency_caps() {
        let mut cache = ThumbnailCache::with_limits(6, 2, 1, Duration::from_millis(50));
        let t0 = Instant::now();

        cache.begin_pass();
        cache.record_candidate("https://img/1.jpg", true, 0.0);
        cache.record_candidate("https://img/2.jpg", true, 98.0);
        cache.record_candidate("https://img/3.jpg", true, 196.0);

        let (admitted_1, evicted_1) = cache.schedule_pass(t0);
        assert_eq!(admitted_1, vec!["https://img/1.jpg"]);
        assert!(evicted_1.is_empty());
        assert_eq!(cache.in_flight_count(), 1);
        assert_eq!(cache.retained_count(), 1);

        // Antes do intervalo mínimo (45-50ms), nenhuma nova miniatura deve iniciar.
        let (admitted_early, _) = cache.schedule_pass(t0 + Duration::from_millis(10));
        assert!(admitted_early.is_empty());

        // Após o intervalo, a segunda miniatura inicia, atingindo MAX_CONCURRENT_LOADS = 2.
        let t1 = t0 + Duration::from_millis(60);
        let (admitted_2, _) = cache.schedule_pass(t1);
        assert_eq!(admitted_2, vec!["https://img/2.jpg"]);
        assert_eq!(cache.in_flight_count(), 2);

        // Com 2 carregamentos em andamento, o terceiro aguarda até um terminar.
        let t2 = t1 + Duration::from_millis(60);
        let (admitted_blocked, _) = cache.schedule_pass(t2);
        assert!(admitted_blocked.is_empty());

        // Quando o primeiro conclui, o terceiro é admitido.
        mark_ready(&mut cache, "https://img/1.jpg");
        let (admitted_3, _) = cache.schedule_pass(t2);
        assert_eq!(admitted_3, vec!["https://img/3.jpg"]);
        assert_eq!(cache.retained_count(), 3);
    }

    #[test]
    fn retention_limit_evicts_least_recently_visible_offscreen_entries() {
        let mut cache = ThumbnailCache::with_limits(3, 2, 2, Duration::ZERO);
        let now = Instant::now();

        // Passo 1: carrega 3 miniaturas visíveis (atinge o limite de 3).
        cache.begin_pass();
        cache.record_candidate("https://img/1.jpg", true, 0.0);
        cache.record_candidate("https://img/2.jpg", true, 100.0);
        let (admitted, _) = cache.schedule_pass(now);
        assert_eq!(admitted.len(), 2);
        mark_ready(&mut cache, "https://img/1.jpg");
        mark_ready(&mut cache, "https://img/2.jpg");

        cache.record_candidate("https://img/3.jpg", true, 200.0);
        let (admitted, evicted) = cache.schedule_pass(now);
        assert_eq!(admitted, vec!["https://img/3.jpg"]);
        assert!(evicted.is_empty());
        mark_ready(&mut cache, "https://img/3.jpg");
        assert_eq!(cache.retained_count(), 3);

        // Passo 2: usuário rola para baixo; 2 e 3 continuam visíveis, 1 saiu da tela e 4 entra.
        cache.begin_pass();
        for uri in ["https://img/2.jpg", "https://img/3.jpg"] {
            let tick = cache.bump_tick();
            let pass = cache.pass_index;
            if let Some(entry) = cache.entries.get_mut(uri) {
                entry.last_visible_pass = pass;
                entry.last_access_tick = tick;
            }
        }
        cache.record_candidate("https://img/4.jpg", true, 300.0);

        let (admitted, evicted) = cache.schedule_pass(now);
        assert_eq!(admitted, vec!["https://img/4.jpg"]);
        assert_eq!(evicted, vec!["https://img/1.jpg"]);
        assert_eq!(cache.retained_count(), 3);
        assert!(!cache.entries.contains_key("https://img/1.jpg"));
        assert!(cache.entries.contains_key("https://img/2.jpg"));
        assert!(cache.entries.contains_key("https://img/3.jpg"));
        assert!(cache.entries.contains_key("https://img/4.jpg"));
    }

    #[test]
    fn reset_and_timeout_safely_track_retired_in_flight_requests() {
        let mut cache = ThumbnailCache::with_limits(4, 2, 2, Duration::ZERO);
        let t0 = Instant::now();

        cache.begin_pass();
        cache.record_candidate("https://img/inflight.jpg", true, 0.0);
        let (admitted, _) = cache.schedule_pass(t0);
        assert_eq!(admitted, vec!["https://img/inflight.jpg"]);

        let forgotten = cache.drain_all_for_reset(t0);
        assert_eq!(forgotten, vec!["https://img/inflight.jpg"]);
        assert_eq!(cache.retained_count(), 0);

        // Varreduras subsequentes continuam esquecendo a URI em voo até expirar o janela segura.
        let swept = cache.sweep_expired(t0 + Duration::from_secs(1));
        assert_eq!(swept, vec!["https://img/inflight.jpg"]);
    }

    #[test]
    fn repeated_resets_keep_retired_slots_and_block_same_uri_until_expiry() {
        let mut cache = ThumbnailCache::with_limits(4, 2, 2, Duration::ZERO);
        let now = Instant::now();
        cache.begin_pass();
        cache.record_candidate("a", true, 0.0);
        cache.record_candidate("b", true, 1.0);
        assert_eq!(cache.schedule_pass(now).0.len(), 2);
        cache.drain_all_for_reset(now);
        for second in 1..5 {
            cache.begin_pass();
            cache.record_candidate("a", true, 0.0);
            cache.record_candidate("c", true, 1.0);
            assert!(
                cache
                    .schedule_pass(now + Duration::from_secs(second))
                    .0
                    .is_empty()
            );
            cache.drain_all_for_reset(now + Duration::from_secs(second));
        }
        assert_eq!(cache.retired_in_flight.len(), 2);
        cache.sweep_expired(now + IN_FLIGHT_TIMEOUT);
        cache.begin_pass();
        cache.record_candidate("a", true, 0.0);
        assert_eq!(cache.schedule_pass(now + IN_FLIGHT_TIMEOUT).0, vec!["a"]);
    }

    #[test]
    fn timed_out_failed_entry_keeps_retired_slot_after_reset() {
        let mut cache = ThumbnailCache::with_limits(1, 1, 1, Duration::ZERO);
        let now = Instant::now();
        cache.begin_pass();
        cache.record_candidate("a", true, 0.0);
        cache.schedule_pass(now);
        cache.sweep_expired(now + IN_FLIGHT_TIMEOUT);
        assert_eq!(cache.entries["a"].state, EntryState::Failed);
        cache.begin_pass();
        cache.record_candidate("b", true, 0.0);
        assert!(cache.schedule_pass(now + IN_FLIGHT_TIMEOUT).0.is_empty());
        cache.drain_all_for_reset(now + IN_FLIGHT_TIMEOUT);
        cache.record_candidate("a", true, 0.0);
        assert!(cache.schedule_pass(now + IN_FLIGHT_TIMEOUT).0.is_empty());
        cache.sweep_expired(now + IN_FLIGHT_TIMEOUT * 2);
        assert_eq!(
            cache.schedule_pass(now + IN_FLIGHT_TIMEOUT * 2).0,
            vec!["a"]
        );
    }

    #[test]
    fn loading_entries_are_not_evicted_and_interval_applies_within_pass() {
        let mut cache = ThumbnailCache::with_limits(2, 2, 2, Duration::from_millis(50));
        let now = Instant::now();
        cache.begin_pass();
        cache.record_candidate("a", true, 0.0);
        cache.record_candidate("b", true, 1.0);
        assert_eq!(cache.schedule_pass(now).0, vec!["a"]);
        assert_eq!(
            cache.schedule_pass(now + Duration::from_millis(50)).0,
            vec!["b"]
        );
        cache.begin_pass();
        assert!(cache.evict_one().is_none());
        assert_eq!(cache.in_flight_count(), 2);
    }

    #[test]
    fn full_visible_cache_does_not_cycle_finished_images() {
        let mut cache = ThumbnailCache::with_limits(1, 1, 1, Duration::ZERO);
        let now = Instant::now();
        cache.begin_pass();
        cache.record_candidate("a", true, 0.0);
        cache.schedule_pass(now);
        mark_ready(&mut cache, "a");
        cache.record_candidate("b", true, 1.0);
        assert!(cache.schedule_pass(now).0.is_empty());
        assert!(cache.entries.contains_key("a"));
        cache.begin_pass();
        cache.record_candidate("b", true, 1.0);
        assert_eq!(cache.schedule_pass(now).0, vec!["b"]);
    }

    #[test]
    fn geometry_helpers_preserve_fixed_slot_and_aspect_ratio() {
        let viewport = Rect::from_min_max(Pos2::new(0.0, 100.0), Pos2::new(800.0, 600.0));
        let visible_card = Rect::from_min_size(Pos2::new(10.0, 150.0), THUMBNAIL_SIZE);
        let overscan_card = Rect::from_min_size(Pos2::new(10.0, 680.0), THUMBNAIL_SIZE);
        let far_card = Rect::from_min_size(Pos2::new(10.0, 850.0), THUMBNAIL_SIZE);

        assert!(viewport_intersects(viewport, visible_card, 0.0));
        assert!(!viewport_intersects(viewport, overscan_card, 0.0));
        assert!(viewport_intersects(
            viewport,
            overscan_card,
            VIEWPORT_OVERSCAN_PX
        ));
        assert!(!viewport_intersects(
            viewport,
            far_card,
            VIEWPORT_OVERSCAN_PX
        ));

        let fitted = fit_aspect_rect(visible_card, [320, 180]);
        assert!((fitted.width() - THUMBNAIL_WIDTH).abs() < 0.01);
        assert!((fitted.height() - THUMBNAIL_HEIGHT).abs() < 0.01);
    }
}
