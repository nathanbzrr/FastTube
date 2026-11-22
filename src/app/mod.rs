mod auth_sync;
mod channels_playlists;
mod feeds;
mod persistence_desktop;
mod playback;
mod shell_modals;
mod watch_actions;

#[cfg(test)]
pub(crate) use persistence_desktop::{RepaintScheduleInput, compute_repaint_interval};
#[cfg(test)]
pub(crate) use playback::resolve_screenshot_dir_from;
pub(crate) use playback::{build_screenshot_filename, screenshot_dir_candidates};
pub(crate) use watch_actions::{
    ShareTargetKind, build_youtube_embed_code, build_youtube_share_url, format_share_timestamp,
    format_subscribers_pt, open_external_url, paint_share_target_icon, parse_share_timestamp_secs,
};
