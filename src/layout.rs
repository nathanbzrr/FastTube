use eframe::egui;

const SIDE_BY_SIDE_WATCH_MIN_WIDTH: f32 = 1000.0;
const WATCH_PAGE_GAP: f32 = 24.0;
const WATCH_PAGE_MARGIN: f32 = 24.0;
const WATCH_RECOMMENDATIONS_WIDTH: f32 = 402.0;
const COMPACT_WATCH_CONTROLS_MAX_WIDTH: f32 = 760.0;
const COMPACT_NAVIGATION_MIN_VIEWPORT_WIDTH: f32 = 760.0;
pub(crate) const PERSISTENT_SIDEBAR_MIN_VIEWPORT_WIDTH: f32 = 1312.0;
const TWO_ROW_HEADER_MAX_WIDTH: f32 = 760.0;
pub(crate) const SINGLE_COLUMN_BROWSE_MAX_WIDTH: f32 = 680.0;
pub(crate) const COMPACT_NAVIGATION_WIDTH: f32 = 72.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewportBreakpoint {
    Compact,
    Medium,
    Wide,
}

pub(crate) fn viewport_breakpoint(viewport_width: f32) -> ViewportBreakpoint {
    if viewport_width < COMPACT_NAVIGATION_MIN_VIEWPORT_WIDTH {
        ViewportBreakpoint::Compact
    } else if viewport_width < PERSISTENT_SIDEBAR_MIN_VIEWPORT_WIDTH {
        ViewportBreakpoint::Medium
    } else {
        ViewportBreakpoint::Wide
    }
}

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

#[cfg(test)]
pub(crate) fn compute_header_height(viewport_width: f32, fullscreen: bool) -> f32 {
    if fullscreen {
        return 0.0;
    }
    match compute_header_layout_mode(viewport_width) {
        HeaderLayoutMode::SingleRow => 56.0,
        HeaderLayoutMode::TwoRows => 96.0,
    }
}

#[cfg(test)]
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

pub(crate) fn should_show_compact_navigation_for_page(
    viewport_width: f32,
    navigation_open: bool,
    fullscreen: bool,
    has_selected_video: bool,
    theater_mode: bool,
) -> bool {
    if fullscreen || has_selected_video || theater_mode {
        return false;
    }
    match viewport_breakpoint(viewport_width) {
        ViewportBreakpoint::Compact => false,
        ViewportBreakpoint::Medium => true,
        ViewportBreakpoint::Wide => !navigation_open,
    }
}

pub(crate) fn should_show_navigation_drawer(
    viewport_width: f32,
    navigation_open: bool,
    fullscreen: bool,
    has_selected_video: bool,
) -> bool {
    navigation_open
        && !fullscreen
        && (has_selected_video || viewport_breakpoint(viewport_width) != ViewportBreakpoint::Wide)
}

pub(crate) fn compute_home_grid_columns(available_width: f32) -> usize {
    if available_width < SINGLE_COLUMN_BROWSE_MAX_WIDTH {
        1
    } else {
        (((available_width + 16.0) / 320.0).floor() as usize).clamp(1, 5)
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
    if !theater && available_width >= 920.0 {
        let gap = 16.0;
        let secondary_width = (available_width * 0.34).clamp(320.0, 420.0);
        let primary_width = (available_width - secondary_width - gap).max(460.0);
        let adjusted_secondary = (available_width - primary_width - gap).max(320.0);
        WatchColumns {
            mode: WatchLayoutMode::SideBySide,
            primary_width,
            secondary_width: adjusted_secondary,
            gap,
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

pub(crate) fn compute_watch_page_columns(
    available_width: f32,
    available_height: f32,
    theater: bool,
) -> WatchColumns {
    let available_width = available_width.max(1.0);
    if theater {
        return WatchColumns {
            mode: WatchLayoutMode::Stacked,
            primary_width: available_width,
            secondary_width: available_width,
            gap: 0.0,
        };
    }

    let margin = if available_width < 680.0 {
        12.0
    } else if available_width < SIDE_BY_SIDE_WATCH_MIN_WIDTH {
        16.0
    } else {
        WATCH_PAGE_MARGIN
    };

    if available_width < SIDE_BY_SIDE_WATCH_MIN_WIDTH {
        let content_width = (available_width - 2.0 * margin).max(160.0);
        return WatchColumns {
            mode: WatchLayoutMode::Stacked,
            primary_width: content_width,
            secondary_width: content_width,
            gap: 0.0,
        };
    }

    let content_width = (available_width - 2.0 * margin).max(1.0);
    let secondary_width = (content_width * 0.32).clamp(300.0, WATCH_RECOMMENDATIONS_WIDTH);
    let max_primary_from_width = (content_width - secondary_width - WATCH_PAGE_GAP).max(480.0);
    let max_primary_from_height =
        ((available_height - 185.0).max(225.0) * 16.0 / 9.0).clamp(480.0, 1280.0);
    let player_width = max_primary_from_width.min(max_primary_from_height);
    let effective_secondary =
        (content_width - player_width - WATCH_PAGE_GAP).clamp(300.0, WATCH_RECOMMENDATIONS_WIDTH);
    WatchColumns {
        mode: WatchLayoutMode::SideBySide,
        primary_width: player_width,
        secondary_width: effective_secondary,
        gap: WATCH_PAGE_GAP,
    }
}

pub(crate) fn watch_page_side_margin(available_width: f32, columns: &WatchColumns) -> f32 {
    if columns.mode == WatchLayoutMode::SideBySide {
        let total = columns.primary_width + columns.gap + columns.secondary_width;
        ((available_width - total) * 0.5).max(16.0)
    } else if available_width > columns.primary_width {
        ((available_width - columns.primary_width) * 0.5).max(0.0)
    } else {
        0.0
    }
}

pub(crate) const fn watch_page_margin() -> f32 {
    WATCH_PAGE_MARGIN
}

#[cfg(test)]
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
        (available_height * 0.82)
            .min(available_height - 140.0)
            .max(200.0)
    } else {
        match mode {
            WatchLayoutMode::SideBySide => (available_height - 185.0).max(180.0),
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

#[cfg(test)]
pub(crate) fn should_show_navigation_panel(
    navigation_open: bool,
    fullscreen: bool,
    has_selected_video: bool,
    theater_mode: bool,
) -> bool {
    navigation_open && !fullscreen && !(has_selected_video && theater_mode)
}

pub(crate) fn should_show_persistent_navigation_panel(
    viewport_width: f32,
    navigation_open: bool,
    fullscreen: bool,
    has_selected_video: bool,
    theater_mode: bool,
) -> bool {
    navigation_open
        && !fullscreen
        && !has_selected_video
        && !theater_mode
        && viewport_width >= PERSISTENT_SIDEBAR_MIN_VIEWPORT_WIDTH
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

#[cfg(test)]
pub(crate) fn compute_navigation_width(viewport_width: f32) -> f32 {
    if viewport_width < 760.0 { 132.0 } else { 160.0 }
}

pub(crate) fn compute_youtube_sidebar_width(viewport_width: f32) -> f32 {
    if viewport_width < 480.0 { 216.0 } else { 240.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_breakpoint_classifies_boundaries() {
        assert_eq!(viewport_breakpoint(0.0), ViewportBreakpoint::Compact);
        assert_eq!(viewport_breakpoint(480.0), ViewportBreakpoint::Compact);
        assert_eq!(viewport_breakpoint(759.9), ViewportBreakpoint::Compact);
        assert_eq!(viewport_breakpoint(760.0), ViewportBreakpoint::Medium);
        assert_eq!(viewport_breakpoint(1024.0), ViewportBreakpoint::Medium);
        assert_eq!(
            viewport_breakpoint(PERSISTENT_SIDEBAR_MIN_VIEWPORT_WIDTH - 0.1),
            ViewportBreakpoint::Medium
        );
        assert_eq!(
            viewport_breakpoint(PERSISTENT_SIDEBAR_MIN_VIEWPORT_WIDTH),
            ViewportBreakpoint::Wide
        );
        assert_eq!(viewport_breakpoint(1920.0), ViewportBreakpoint::Wide);
    }

    #[test]
    fn viewport_breakpoint_matches_navigation_and_drawer_modes() {
        assert!(!should_show_compact_navigation_for_page(
            759.9, false, false, false, false
        ));
        assert!(should_show_compact_navigation_for_page(
            760.0, true, false, false, false
        ));
        assert!(should_show_compact_navigation_for_page(
            PERSISTENT_SIDEBAR_MIN_VIEWPORT_WIDTH,
            false,
            false,
            false,
            false
        ));
        assert!(!should_show_compact_navigation_for_page(
            PERSISTENT_SIDEBAR_MIN_VIEWPORT_WIDTH,
            true,
            false,
            false,
            false
        ));

        assert!(should_show_navigation_drawer(760.0, true, false, false));
        assert!(!should_show_navigation_drawer(
            PERSISTENT_SIDEBAR_MIN_VIEWPORT_WIDTH,
            true,
            false,
            false
        ));
        assert!(should_show_navigation_drawer(
            PERSISTENT_SIDEBAR_MIN_VIEWPORT_WIDTH,
            true,
            false,
            true
        ));
    }
}
