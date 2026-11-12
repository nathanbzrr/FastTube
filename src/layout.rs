use eframe::egui;

const SIDE_BY_SIDE_WATCH_MIN_WIDTH: f32 = 920.0;
const WATCH_PAGE_GAP: f32 = 24.0;
const WATCH_PAGE_MARGIN: f32 = 16.0;
const WATCH_RECOMMENDATIONS_WIDTH: f32 = 402.0;
const COMPACT_WATCH_CONTROLS_MAX_WIDTH: f32 = 760.0;
const COMPACT_NAVIGATION_MIN_VIEWPORT_WIDTH: f32 = 760.0;
const TWO_ROW_HEADER_MAX_WIDTH: f32 = 760.0;
const SINGLE_COLUMN_BROWSE_MAX_WIDTH: f32 = 680.0;
pub(crate) const COMPACT_NAVIGATION_WIDTH: f32 = 72.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HeaderLayoutMode {
    SingleRow,
    TwoRows,
}

pub(crate) fn compute_header_layout_mode(viewport_width: f32) -> HeaderLayoutMode {
    if viewport_width < TWO_ROW_HEADER_MAX_WIDTH {
        HeaderLayoutMode::TwoRows
    } else {
        HeaderLayoutMode::SingleRow
    }
}

pub(crate) fn compute_header_height(viewport_width: f32, fullscreen: bool) -> f32 {
    if fullscreen {
        return 0.0;
    }
    match compute_header_layout_mode(viewport_width) {
        HeaderLayoutMode::SingleRow => 56.0,
        HeaderLayoutMode::TwoRows => 96.0,
    }
}

pub(crate) fn should_show_compact_navigation(
    viewport_width: f32,
    navigation_open: bool,
    fullscreen: bool,
    theater_mode: bool,
) -> bool {
    !navigation_open
        && !fullscreen
        && !theater_mode
        && viewport_width >= COMPACT_NAVIGATION_MIN_VIEWPORT_WIDTH
}

pub(crate) fn compute_home_grid_columns(available_width: f32) -> usize {
    if available_width < SINGLE_COLUMN_BROWSE_MAX_WIDTH {
        1
    } else {
        (((available_width + 16.0) / 320.0).floor() as usize).clamp(1, 4)
    }
}

pub(crate) fn compute_continue_watching_columns(available_width: f32) -> usize {
    if available_width < SINGLE_COLUMN_BROWSE_MAX_WIDTH {
        1
    } else {
        (((available_width + 16.0) / 260.0).floor() as usize).clamp(1, 4)
    }
}

pub(crate) fn watch_controls_are_compact(video_width: f32) -> bool {
    video_width < COMPACT_WATCH_CONTROLS_MAX_WIDTH
}

pub(crate) fn should_show_video_window(has_selected_video: bool, settings_open: bool) -> bool {
    has_selected_video && !settings_open
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WatchLayoutMode {
    SideBySide,
    Stacked,
    Short,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WatchColumns {
    pub(crate) mode: WatchLayoutMode,
    pub(crate) primary_width: f32,
    pub(crate) secondary_width: f32,
    pub(crate) gap: f32,
}

pub(crate) fn compute_watch_columns(available_width: f32) -> WatchColumns {
    compute_watch_columns_for_mode(available_width, false)
}

pub(crate) fn compute_watch_columns_for_mode(available_width: f32, theater: bool) -> WatchColumns {
    let available_width = available_width.max(1.0);
    if !theater && available_width >= SIDE_BY_SIDE_WATCH_MIN_WIDTH {
        let secondary_width = (available_width * 0.24).clamp(360.0, WATCH_RECOMMENDATIONS_WIDTH);
        let primary_width = (available_width - secondary_width - WATCH_PAGE_GAP).max(460.0);
        let adjusted_secondary = (available_width - primary_width - WATCH_PAGE_GAP).max(280.0);
        WatchColumns {
            mode: WatchLayoutMode::SideBySide,
            primary_width,
            secondary_width: adjusted_secondary,
            gap: WATCH_PAGE_GAP,
        }
    } else {
        WatchColumns {
            mode: WatchLayoutMode::Stacked,
            primary_width: available_width,
            secondary_width: available_width,
            gap: 0.0,
        }
    }
}

/// Distribui a página de reprodução em um grid próximo ao YouTube: margens
/// laterais, coluna de vídeo limitada pela altura e recomendações com largura útil.
pub(crate) fn compute_watch_page_columns(
    available_width: f32,
    available_height: f32,
    theater: bool,
) -> WatchColumns {
    if theater || available_width < SIDE_BY_SIDE_WATCH_MIN_WIDTH {
        return compute_watch_columns_for_mode(available_width, theater);
    }

    let available_width = available_width.max(1.0);
    let content_width = (available_width - 2.0 * WATCH_PAGE_MARGIN).max(1.0);
    let secondary_width = (content_width * 0.34).clamp(320.0, WATCH_RECOMMENDATIONS_WIDTH);
    let player_width = ((available_height - 70.0).max(180.0) * 16.0 / 9.0)
        .min((content_width - secondary_width - WATCH_PAGE_GAP).max(460.0));
    WatchColumns {
        mode: WatchLayoutMode::SideBySide,
        primary_width: player_width,
        secondary_width,
        gap: WATCH_PAGE_GAP,
    }
}

pub(crate) const fn watch_page_margin() -> f32 {
    WATCH_PAGE_MARGIN
}

pub(crate) fn compute_player_size(
    column_width: f32,
    available_height: f32,
    fullscreen: bool,
    mode: WatchLayoutMode,
) -> egui::Vec2 {
    compute_player_size_with_theater(column_width, available_height, fullscreen, false, mode)
}

pub(crate) fn compute_player_size_with_theater(
    column_width: f32,
    available_height: f32,
    fullscreen: bool,
    theater: bool,
    mode: WatchLayoutMode,
) -> egui::Vec2 {
    let column_width = column_width.max(160.0);
    let available_height = available_height.max(160.0);
    if mode == WatchLayoutMode::Short && !fullscreen && !theater {
        let portrait_height = (available_height - 80.0).clamp(320.0, 720.0);
        let portrait_width = (portrait_height * 9.0 / 16.0).min(column_width).max(180.0);
        return egui::vec2(portrait_width, portrait_width * 16.0 / 9.0);
    }
    let max_height = if fullscreen {
        (available_height - 110.0).max(120.0)
    } else if theater {
        (available_height - 150.0).max(200.0)
    } else {
        match mode {
            WatchLayoutMode::SideBySide => (available_height - 70.0).max(180.0),
            WatchLayoutMode::Stacked => (available_height * 0.72).clamp(180.0, 720.0),
            WatchLayoutMode::Short => (available_height - 80.0).clamp(320.0, 720.0),
        }
    };
    let max_width = if fullscreen || theater {
        f32::INFINITY
    } else {
        1280.0
    };
    let width = column_width
        .min(max_width)
        .min(max_height * 16.0 / 9.0)
        .max(160.0);
    let height = width * 9.0 / 16.0;
    egui::vec2(width, height)
}

pub(crate) fn should_show_navigation_panel(
    navigation_open: bool,
    fullscreen: bool,
    has_selected_video: bool,
    theater_mode: bool,
) -> bool {
    navigation_open && !fullscreen && !(has_selected_video && theater_mode)
}

pub(crate) fn compute_header_search_layout(available_width: f32, reserve_right: f32) -> (f32, f32) {
    let usable = (available_width - reserve_right).max(120.0);
    let search_button_and_gap = 50.0;
    let max_input = (usable - search_button_and_gap).max(80.0);
    let input_width = max_input.clamp(80.0, 640.0);
    let leftover = (usable - input_width - search_button_and_gap).max(0.0);
    let leading_space = (leftover * 0.42).min(360.0);
    (leading_space, input_width)
}

pub(crate) fn compute_navigation_width(viewport_width: f32) -> f32 {
    if viewport_width < 760.0 { 132.0 } else { 160.0 }
}

pub(crate) fn compute_youtube_sidebar_width(viewport_width: f32) -> f32 {
    if viewport_width < 760.0 {
        168.0
    } else if viewport_width < 1280.0 {
        216.0
    } else {
        240.0
    }
}
