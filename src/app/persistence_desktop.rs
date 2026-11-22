use crate::*;

impl FastTube {
    #[cfg(not(test))]
    pub(crate) fn default_user_data_paths() -> io::Result<Vec<(&'static str, PathBuf)>> {
        let history_path = history::history_path()?;
        Ok(vec![
            ("library", library::path()?),
            ("queue", history_path.with_file_name("queue.json")),
            ("history", history_path),
            ("followed", followed_channels::path()?),
            ("playback", settings::playback_path()?),
            ("theme", settings::theme_path()?),
            ("feed", feed_prefs::path()?),
            ("buffer", settings::settings_path()?),
            ("prefs", settings::app_preferences_path()?),
        ])
    }

    #[cfg(not(test))]
    pub(crate) fn recover_interrupted_import() -> Option<String> {
        let paths = Self::default_user_data_paths().ok()?;
        let root = paths.first()?.1.parent()?.to_path_buf();
        let refs: Vec<_> = paths
            .iter()
            .map(|(name, path)| (*name, path.as_path()))
            .collect();
        match backup::recover_pending_import(&root, &refs) {
            Ok(true) => Some(
                "Uma importação interrompida foi desfeita antes da abertura; os dados anteriores foram restaurados."
                    .into(),
            ),
            Ok(false) => None,
            Err(error) => Some(format!(
                "Foi encontrado um ponto de restauração de importação, mas ele não pôde ser aplicado: {error}"
            )),
        }
    }

    pub(crate) fn persist_queue(&mut self) {
        self.queue.data.set_repeat_mode(self.queue.repeat_mode);
        self.queue.persist(&self.persistence);
    }

    pub(crate) fn poll_persistence_errors(&mut self) {
        if let Some(fp) = self
            .persistence
            .last_fingerprint(persistence::PersistenceSlot::History)
        {
            self.history.loaded_fingerprint = Some(fp);
        }
        if let Some(fp) = self
            .persistence
            .last_fingerprint(persistence::PersistenceSlot::Library)
        {
            self.library.loaded_fingerprint = Some(fp);
        }
        if let Some(fp) = self
            .persistence
            .last_fingerprint(persistence::PersistenceSlot::Queue)
        {
            self.queue.loaded_fingerprint = Some(fp);
        }
        for err in self.persistence.poll_detailed_errors() {
            match err.slot {
                persistence::PersistenceSlot::History => {
                    if err.corrupt_file {
                        self.history.read_failed = true;
                    }
                    self.history.error = Some(err.message.clone());
                }
                persistence::PersistenceSlot::Library => {
                    if err.corrupt_file {
                        self.library.read_failed = true;
                    }
                    self.library.error = Some(err.message.clone());
                }
                persistence::PersistenceSlot::Queue => {
                    if err.corrupt_file {
                        self.queue.read_failed = true;
                    }
                    self.queue.persistence_error = Some(err.message.clone());
                }
            }
            self.toasts.error(err.message);
        }
    }

    pub(crate) fn flush_persistence(&mut self) {
        self.persistence.flush();
        self.poll_persistence_errors();
    }

    pub(crate) fn load_initial_settings() -> (BufferProfile, Option<PathBuf>, Option<String>) {
        match settings::settings_path() {
            Ok(path) => match Self::load_settings_from_path(&path) {
                Ok(profile) => (profile, Some(path), None),
                Err(error) => (BufferProfile::default(), Some(path), Some(error)),
            },
            Err(error) => (
                BufferProfile::default(),
                None,
                Some(format!(
                    "Não foi possível localizar o arquivo de configurações ({error}). Usando o perfil Padrão (64/16 MiB, 30s)."
                )),
            ),
        }
    }

    pub(crate) fn load_settings_from_path(path: &Path) -> Result<BufferProfile, String> {
        settings::load(path).map_err(|error| {
            format!(
                "Erro ao ler as configurações em '{}' ({error}). Usando o perfil Padrão (64/16 MiB, 30s).",
                path.display()
            )
        })
    }

    pub(crate) fn load_initial_playback_settings()
    -> (PlaybackPreferences, Option<PathBuf>, Option<String>) {
        if cfg!(test) {
            return (PlaybackPreferences::default(), None, None);
        }
        match settings::playback_path() {
            Ok(path) => match app_state::load_playback_settings_from_path(&path) {
                Ok(preferences) => (preferences, Some(path), None),
                Err(error) => (PlaybackPreferences::default(), Some(path), Some(error)),
            },
            Err(error) => (
                PlaybackPreferences::default(),
                None,
                Some(format!(
                    "Erro ao ler preferências de reprodução ({error}); usando 1× e qualidade automática."
                )),
            ),
        }
    }

    pub(crate) fn ensure_playback_settings_synchronized(&mut self) {
        self.settings.ensure_playback_synchronized();
        let before = self.settings.prefs.loaded_path.clone();
        self.settings.ensure_prefs_synchronized();
        if before != self.settings.prefs.loaded_path {
            self.sync_yt_dlp_cache_dir();
            self.sync_disk_image_cache();
        }
    }

    pub(crate) fn apply_playback_preferences(&mut self, preferences: PlaybackPreferences) {
        self.settings.apply_playback_preferences(preferences);
    }

    pub(crate) fn load_initial_theme_settings() -> (Theme, Option<PathBuf>, Option<String>) {
        if cfg!(test) {
            return (Theme::default(), None, None);
        }
        match settings::theme_path() {
            Ok(path) => match app_state::load_theme_settings_from_path(&path) {
                Ok(theme) => (theme, Some(path), None),
                Err(error) => (Theme::default(), Some(path), Some(error)),
            },
            Err(error) => (
                Theme::default(),
                None,
                Some(format!(
                    "Erro ao ler tema da interface ({error}); usando o tema Escuro padrão."
                )),
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn ensure_theme_settings_synchronized(&mut self) {
        self.settings.ensure_theme_synchronized();
    }

    pub(crate) fn system_is_dark(&self) -> bool {
        if let Some(observer) = &self.desktop_win11.theme_observer {
            return observer.is_dark();
        }
        #[cfg(test)]
        if let Some(override_dark) = self.desktop_win11.test_system_dark_override {
            return override_dark;
        }
        if cfg!(test) {
            true
        } else {
            win11::read_windows_apps_use_dark_theme()
        }
    }

    pub(crate) fn resolved_theme(&self) -> settings::ResolvedTheme {
        self.settings.theme.value.resolve(self.system_is_dark())
    }

    pub(crate) fn effective_theme(&self) -> Theme {
        self.resolved_theme().to_concrete_theme()
    }

    pub(crate) fn is_dark_theme(&self) -> bool {
        self.resolved_theme().is_dark()
    }

    pub(crate) fn sync_theme_visuals(&mut self, ctx: &egui::Context) {
        let system_dark = self.system_is_dark();
        self.settings.sync_theme_visuals(ctx, system_dark);
    }

    pub(crate) fn apply_theme(&mut self, theme: Theme) {
        self.settings.apply_theme(theme);
    }

    pub(crate) fn load_initial_app_preferences()
    -> (settings::AppPreferences, Option<PathBuf>, Option<String>) {
        if cfg!(test) {
            return (settings::AppPreferences::default(), None, None);
        }
        match settings::app_preferences_path() {
            Ok(path) => match app_state::load_app_preferences_from_path(&path) {
                Ok(prefs) => (prefs, Some(path), None),
                Err(error) => (settings::AppPreferences::default(), Some(path), Some(error)),
            },
            Err(error) => (
                settings::AppPreferences::default(),
                None,
                Some(format!(
                    "Erro ao ler preferências gerais ({error}); usando os padrões."
                )),
            ),
        }
    }

    pub(crate) fn sync_yt_dlp_cache_dir(&self) {
        if cfg!(test) && self.settings.prefs.value.cache_dir_override.is_none() {
            return;
        }
        if let Ok(effective) =
            settings::effective_cache_dir(self.settings.prefs.value.cache_dir_override.as_deref())
        {
            search::set_yt_dlp_cache_dir(Some(settings::yt_dlp_cache_subdir(&effective)));
        }
    }

    pub(crate) fn disk_image_cache_for_override(
        override_dir: Option<&str>,
    ) -> Option<std::sync::Arc<thumbnails::DiskImageCache>> {
        if cfg!(test) && override_dir.is_none() {
            return None;
        }
        settings::effective_cache_dir(override_dir)
            .ok()
            .map(|effective| {
                std::sync::Arc::new(thumbnails::DiskImageCache::new(
                    effective.join("thumbnails"),
                    thumbnails::DEFAULT_DISK_CACHE_MAX_BYTES,
                ))
            })
            .or_else(|| {
                if cfg!(test) {
                    None
                } else {
                    thumbnails::DiskImageCache::default_production()
                }
            })
    }

    pub(crate) fn sync_disk_image_cache(&mut self) {
        let disk_cache = Self::disk_image_cache_for_override(
            self.settings.prefs.value.cache_dir_override.as_deref(),
        );
        self.thumbnails.set_disk_cache(disk_cache.clone());
        self.avatars.set_disk_cache(disk_cache);
    }

    pub(crate) fn apply_app_preferences(&mut self, next: settings::AppPreferences) {
        let prev = self.settings.prefs.value.clone();
        self.settings.apply_app_preferences(next);
        self.sync_yt_dlp_cache_dir();
        if prev.cache_dir_override != self.settings.prefs.value.cache_dir_override {
            self.sync_disk_image_cache();
        }
        if prev.hide_shorts_in_home != self.settings.prefs.value.hide_shorts_in_home
            || prev.pause_recommendations != self.settings.prefs.value.pause_recommendations
            || prev.muted_topics != self.settings.prefs.value.muted_topics
            || prev.muted_channels != self.settings.prefs.value.muted_channels
            || prev.pinned_topics != self.settings.prefs.value.pinned_topics
        {
            self.invalidate_feed_cache();
        }
        #[cfg(windows)]
        if prev.audio_normalization != self.settings.prefs.value.audio_normalization
            && let Some(player) = &self.player
        {
            let _ = player.apply_audio_filters(
                self.settings.prefs.value.audio_normalization,
                self.playback.audio_eq,
            );
        }
        #[cfg(windows)]
        if prev.video_codec_preference != self.settings.prefs.value.video_codec_preference
            && let Some(player) = &self.player
        {
            let format = player::ytdl_format_for_codec_pref(
                self.settings.playback.value.max_height,
                self.settings.prefs.value.video_codec_preference,
            );
            let _ = player.set("options/ytdl-format", &format);
        }
        #[cfg(windows)]
        if prev.caption_style != self.settings.prefs.value.caption_style
            && let Some(player) = &self.player
        {
            let _ = player.apply_caption_style(&self.settings.prefs.value.caption_style);
        }
        #[cfg(windows)]
        if (prev.ultrawide_zoom_mode != self.settings.prefs.value.ultrawide_zoom_mode
            || (prev.ultrawide_custom_zoom - self.settings.prefs.value.ultrawide_custom_zoom).abs()
                > f32::EPSILON)
            && let Some(player) = &self.player
        {
            let _ = player.set_ultrawide_zoom(
                self.settings.prefs.value.ultrawide_zoom_mode,
                self.settings.prefs.value.ultrawide_custom_zoom,
            );
        }
        self.downloads
            .manager
            .set_max_concurrent(self.settings.prefs.value.download_max_concurrent);
        self.downloads.manager.default_options.embed_subs =
            self.settings.prefs.value.download_embed_subs;
        self.downloads.manager.default_options.embed_chapters =
            self.settings.prefs.value.download_embed_chapters;
        self.downloads.manager.default_options.embed_thumbnail =
            self.settings.prefs.value.download_embed_thumbnail;
        if self.selected.is_none() {
            self.reset_decoding_diagnostics();
        }
    }

    pub(crate) fn start_download_video(
        &mut self,
        video: Video,
        format: downloads::DownloadFormat,
        ctx: &egui::Context,
    ) {
        let dir = downloads::DownloadManager::effective_download_dir(
            self.settings.prefs.value.download_dir_override.as_deref(),
        );
        let title = video.title.clone();
        self.downloads.manager.enqueue(video, format, dir, ctx);
        self.persist_downloads_state();
        let msg = format!(
            "{}: {title}",
            self.pick("Download adicionado à fila", "Download added to queue")
        );
        self.downloads.notice = Some(msg.clone());
        self.toasts.info(msg);
    }

    pub(crate) fn poll_downloads(&mut self, ctx: &egui::Context) {
        if let Some((_id, result)) = self.downloads.manager.poll(ctx) {
            match result {
                Ok(()) => {
                    let msg = self
                        .pick("Download concluído.", "Download completed.")
                        .to_owned();
                    self.downloads.notice = Some(msg.clone());
                    self.toasts.success(msg);
                }
                Err(err) => {
                    let msg = format!(
                        "{}: {err}",
                        self.pick("Falha no download", "Download failed")
                    );
                    self.downloads.notice = Some(msg.clone());
                    self.toasts.warning(msg);
                }
            }
            self.persist_downloads_state();
        }
    }

    pub(crate) fn start_ytdlp_check(&mut self, ctx: &egui::Context) {
        if self.settings.ytdlp_task.is_running() {
            return;
        }
        self.settings.ytdlp_status = Some((
            true,
            self.pick(
                "Consultando versão mais recente do yt-dlp no GitHub...",
                "Checking latest yt-dlp version on GitHub...",
            )
            .into(),
        ));
        self.settings
            .ytdlp_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                app_state::YtDlpUpdaterMessage::Checked(updater::check_for_update(&cancel))
            });
    }

    pub(crate) fn start_ytdlp_update(
        &mut self,
        download_url: String,
        install_path: PathBuf,
        version: String,
        ctx: &egui::Context,
    ) {
        if self.settings.ytdlp_task.is_running() {
            return;
        }
        self.settings.ytdlp_status = Some((
            true,
            format!(
                "{} ({version})...",
                self.pick(
                    "Baixando e instalando yt-dlp",
                    "Downloading and installing yt-dlp"
                )
            ),
        ));
        self.settings
            .ytdlp_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                let result = updater::download_and_install(&download_url, &install_path, &cancel);
                app_state::YtDlpUpdaterMessage::Updated { version, result }
            });
    }

    pub(crate) fn poll_ytdlp_updater(&mut self) {
        match self.settings.ytdlp_task.poll() {
            Some(app_state::YtDlpUpdaterMessage::Checked(Ok(info))) => {
                let msg = if info.update_available {
                    format!(
                        "{}: {} -> {}",
                        self.pick("Atualização disponível", "Update available"),
                        info.current_version.as_deref().unwrap_or("?"),
                        info.latest_version
                    )
                } else {
                    format!(
                        "{} ({})",
                        self.pick("yt-dlp já está atualizado", "yt-dlp is up to date"),
                        info.latest_version
                    )
                };
                self.settings.ytdlp_check = Some(info);
                self.settings.ytdlp_status = Some((true, msg));
            }
            Some(app_state::YtDlpUpdaterMessage::Checked(Err(err))) => {
                self.settings.ytdlp_status = Some((false, err));
            }
            Some(app_state::YtDlpUpdaterMessage::Updated { version, result }) => match result {
                Ok(path) => {
                    if let Some(check) = &mut self.settings.ytdlp_check {
                        check.current_version = Some(version.clone());
                        check.update_available = false;
                        check.install_path = path.clone();
                    }
                    self.settings.ytdlp_status = Some((
                        true,
                        format!(
                            "{} ({version}): {}",
                            self.pick("yt-dlp atualizado", "yt-dlp updated"),
                            path.display()
                        ),
                    ));
                }
                Err(err) => {
                    self.settings.ytdlp_status = Some((false, err));
                }
            },
            None => {}
        }
    }

    pub(crate) fn maybe_auto_check_app_updates(&mut self, ctx: &egui::Context) {
        if self.settings.app_update_auto_checked || !self.settings.prefs.value.auto_check_updates {
            return;
        }
        self.settings.app_update_auto_checked = true;
        self.start_app_update_check(true, ctx);
    }

    pub(crate) fn open_settings_tab(&mut self, tab: app_state::SettingsTab, _ctx: &egui::Context) {
        self.settings.active_tab = tab;
        self.ui.settings_open = true;
        self.sync_player_window_visibility();
    }

    pub(crate) fn start_app_update_check(&mut self, silent: bool, ctx: &egui::Context) {
        if self.settings.app_update_task.is_running() {
            return;
        }
        if !silent {
            self.settings.app_update_status = Some((
                true,
                self.pick(
                    "Consultando release mais recente do FastTube no GitHub...",
                    "Checking latest FastTube release on GitHub...",
                )
                .into(),
            ));
        }
        self.settings
            .app_update_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                let result = updater::check_fasttube_update(&cancel);
                app_state::AppUpdaterMessage::Checked { silent, result }
            });
    }

    pub(crate) fn start_app_update_download(&mut self, ctx: &egui::Context) {
        if self.settings.app_update_task.is_running() {
            return;
        }
        let Some(check) = self.settings.app_update_check.clone() else {
            return;
        };
        if !check.update_available {
            return;
        }
        self.settings.app_update_prepared = None;
        self.settings.app_update_status = Some((
            true,
            format!(
                "{} (v{})...",
                self.pick(
                    "Baixando e verificando SHA-256 da atualização",
                    "Downloading and verifying update SHA-256",
                ),
                check.latest_version
            ),
        ));
        self.settings
            .app_update_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                let result = updater::download_and_prepare_app_update(&check, &cancel);
                app_state::AppUpdaterMessage::Prepared(result)
            });
    }

    pub(crate) fn apply_prepared_app_update(&mut self, ctx: &egui::Context) {
        let Some(prepared) = self.settings.app_update_prepared.clone() else {
            return;
        };
        match updater::launch_app_update_helper(&prepared) {
            Ok(()) => {
                self.settings.app_update_status = Some((
                    true,
                    format!(
                        "{} (v{})...",
                        self.pick(
                            "Aplicando atualização e reiniciando",
                            "Applying update and restarting",
                        ),
                        prepared.version
                    ),
                ));
                self.desktop.tray_quit_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(err) => {
                self.settings.app_update_status = Some((false, err.clone()));
                self.toasts.error(err);
            }
        }
    }

    pub(crate) fn active_app_update_available(&self) -> Option<&updater::AppUpdateCheck> {
        let check = self.settings.app_update_check.as_ref()?;
        if !check.update_available {
            return None;
        }
        if self
            .settings
            .prefs
            .value
            .skipped_app_update_version
            .as_deref()
            .is_some_and(|skipped| skipped == check.latest_version)
        {
            return None;
        }
        Some(check)
    }

    pub(crate) fn poll_app_updater(&mut self, ctx: &egui::Context) {
        match self.settings.app_update_task.poll() {
            Some(app_state::AppUpdaterMessage::Checked { silent, result }) => match result {
                Ok(info) => {
                    if info.update_available {
                        self.settings.app_update_banner_dismissed = false;
                        let msg = format!(
                            "{}: v{} -> v{}",
                            self.pick(
                                "Nova versão do FastTube disponível",
                                "New FastTube version available"
                            ),
                            info.current_version,
                            info.latest_version
                        );
                        self.settings.app_update_status = Some((true, msg.clone()));
                        let skipped = self
                            .settings
                            .prefs
                            .value
                            .skipped_app_update_version
                            .as_deref()
                            == Some(info.latest_version.as_str());
                        if !skipped {
                            self.toasts.info(msg);
                        }
                    } else if !silent {
                        self.settings.app_update_status = Some((
                            true,
                            format!(
                                "{} (v{})",
                                self.pick(
                                    "O FastTube já está na versão mais recente",
                                    "FastTube is already up to date",
                                ),
                                info.current_version
                            ),
                        ));
                    }
                    self.settings.app_update_check = Some(info);
                }
                Err(err) => {
                    if !silent {
                        self.settings.app_update_status = Some((false, err));
                    }
                }
            },
            Some(app_state::AppUpdaterMessage::Prepared(result)) => match result {
                Ok(prepared) => {
                    self.settings.app_update_prepared = Some(prepared.clone());
                    self.settings.app_update_status = Some((
                        true,
                        format!(
                            "{} v{}.",
                            self.pick(
                                "Atualização verificada e pronta para instalar:",
                                "Update verified and ready to install:",
                            ),
                            prepared.version
                        ),
                    ));
                    self.apply_prepared_app_update(ctx);
                }
                Err(err) => {
                    self.settings.app_update_status = Some((false, err.clone()));
                    self.toasts.error(err);
                }
            },
            None => {}
        }
    }

    pub(crate) fn poll_async_tasks(&mut self, ctx: &egui::Context) {
        self.poll_reauth(ctx);
        self.poll_rating();
        self.poll_subscriptions(ctx);
        self.poll_my_channel(ctx);
        self.poll_remote_playlists(ctx);
        self.poll_playlists_sync(ctx);
        self.poll_playlist_remove_action(ctx);
        self.poll_watch_later_sync(ctx);
        self.poll_active_playlist_detail_sync(ctx);
        self.poll_watch_mix_sync(ctx);
        self.poll_playlist_action(ctx);
        self.poll_report(ctx);
        self.poll_subscription_channels();
        self.poll_home_batch(ctx);
        self.poll_home(ctx);
        self.poll_innertube_home(ctx);
        self.poll_innertube_subscriptions(ctx);
        self.poll_liked_videos();
        self.poll_history_sync(ctx);
        self.poll_remote_watch_tracking();
        self.poll_quality();
        self.poll_stream_manifest(ctx);
        self.poll_watch_next(ctx);
        self.poll_innertube_comments();
        self.poll_comment_mutation();
        self.poll_zero_quota_feedback();
        self.poll_live_watch(ctx);
        self.poll_live_chat_replay(ctx);
        self.poll_notifications_inbox();
        self.poll_explore_hub(ctx);
        self.poll_shorts_sequence(ctx);
        self.maybe_auto_activate_subtitle(ctx);
        self.maybe_start_deferred_quality_fallback(ctx);
        self.poll_search(ctx);
        self.poll_search_page(ctx);
        self.poll_search_suggestions(ctx);
        self.poll_channel(ctx);
        self.poll_channel_more_videos();
        self.poll_channel_continuation();
        self.poll_channel_sorted_videos();
        self.poll_channel_tab_feeds();
        self.poll_channel_search();
        self.poll_channel_playlists(ctx);
        self.poll_channel_playlist_videos();
        self.poll_channel_about();
        self.poll_sponsorblock();
        self.poll_local_followed_feed(ctx);
        self.poll_downloads(ctx);
        self.poll_remote_playlist(ctx);
        self.poll_community_extras();
        self.poll_ytdlp_updater();
        self.poll_app_updater(ctx);
        self.poll_cache_picker(ctx);
        self.poll_download_dir_picker(ctx);
        self.poll_backup_picker(ctx);
        self.poll_oauth_picker(ctx);
        self.poll_storage_background_jobs(ctx);
        self.poll_persistence_errors();
        self.poll_deferred_fonts(ctx);
    }

    pub(crate) fn poll_deferred_fonts(&mut self, ctx: &egui::Context) {
        let outcome = self.deferred_fonts_rx.as_ref().map(Receiver::try_recv);
        match outcome {
            Some(Ok(fonts)) => {
                ctx.set_fonts(fonts);
                self.deferred_fonts_rx = None;
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.deferred_fonts_rx = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn user_data_paths(&self) -> io::Result<Vec<(&'static str, PathBuf)>> {
        let required = |path: &Option<PathBuf>, name| {
            path.clone().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("caminho de dados indisponível: {name}"),
                )
            })
        };
        let playback_path = required(&self.settings.playback.path, "preferências de reprodução")?;
        let prefs_path = self
            .settings
            .prefs
            .path
            .clone()
            .unwrap_or_else(|| playback_path.with_file_name("app-preferences.json"));
        let followed_path = self
            .followed_channels
            .path
            .clone()
            .unwrap_or_else(|| playback_path.with_file_name("followed-channels.json"));
        Ok(vec![
            ("library", required(&self.library.path, "biblioteca")?),
            ("queue", required(&self.queue.path, "fila")?),
            ("history", required(&self.history.path, "histórico")?),
            ("followed", followed_path),
            ("playback", playback_path),
            ("theme", required(&self.settings.theme.path, "tema")?),
            (
                "feed",
                required(&self.feed_preferences.path, "filtros do feed")?,
            ),
            ("buffer", required(&self.settings.buffer.path, "buffer")?),
            ("prefs", prefs_path),
        ])
    }

    pub(crate) fn export_user_data(&mut self) {
        let path_text = self.backup.path_text.trim();
        if path_text.is_empty() {
            self.backup.status = Some("Informe o caminho do arquivo de backup.".into());
            return;
        }
        let path = PathBuf::from(path_text);
        let history_json = serde_json::json!({
            "version": 1,
            "entries": self.history.data.entries().iter().map(|entry| {
                let mut obj = serde_json::Map::new();
                obj.insert("video_id".into(), serde_json::json!(entry.video_id));
                obj.insert("title".into(), serde_json::json!(entry.title));
                obj.insert("channel".into(), serde_json::json!(entry.channel));
                obj.insert("position_ms".into(), serde_json::json!(entry.position_ms));
                obj.insert("duration_ms".into(), serde_json::json!(entry.duration_ms));
                obj.insert("watched_at_ms".into(), serde_json::json!(entry.watched_at_ms));
                if let Some(v) = entry.is_short {
                    obj.insert("is_short".into(), serde_json::json!(v));
                }
                if let Some(v) = entry.is_podcast {
                    obj.insert("is_podcast".into(), serde_json::json!(v));
                }
                if let Some(v) = entry.is_music {
                    obj.insert("is_music".into(), serde_json::json!(v));
                }
                if let Some(v) = &entry.date_label {
                    obj.insert("date_label".into(), serde_json::json!(v));
                }
                if let Some(v) = &entry.view_count_text {
                    obj.insert("view_count_text".into(), serde_json::json!(v));
                }
                if let Some(v) = &entry.thumbnail_url {
                    obj.insert("thumbnail_url".into(), serde_json::json!(v));
                }
                if let Some(v) = entry.channel_verified {
                    obj.insert("channel_verified".into(), serde_json::json!(v));
                }
                serde_json::Value::Object(obj)
            }).collect::<Vec<_>>()
        });
        let backup = backup::Backup {
            library: serde_json::to_value(&self.library.data).unwrap_or_default(),
            queue: self.queue.data.to_backup_value(),
            history: history_json,
            preferences: serde_json::json!({
                "speed_quarters": self.settings.playback.value.speed_quarters,
                "max_height": self.settings.playback.value.max_height,
                "theme": self.settings.theme.value.as_str(),
                "buffer_max_mib": self.settings.buffer.profile.values().max_mib,
                "buffer_back_mib": self.settings.buffer.profile.values().back_mib,
                "buffer_cache_seconds": self.settings.buffer.profile.values().cache_seconds,
                "feed_hide_watched": self.feed_preferences.values.hide_watched,
                "feed_hide_shorts": self.feed_preferences.values.hide_shorts,
                "feed_hide_lives": self.feed_preferences.values.hide_lives,
                "feed_newest_first": self.feed_preferences.values.newest_first,
                "feed_channel": self.feed_preferences.values.channel,
                "app_preferences": self.settings.prefs.value,
                "followed_channels": self.followed_channels.data,
            }),
        };
        self.backup.status = Some(match backup::save(&path, &backup) {
            Ok(()) => "Backup exportado com fila versionada e preferências atuais.".into(),
            Err(error) => format!("Falha ao exportar backup: {error}"),
        });
    }

    pub(crate) fn read_imported_user_data(&self) -> io::Result<ImportedUserData> {
        let path_text = self.backup.path_text.trim();
        if path_text.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "informe o caminho do arquivo de backup",
            ));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(Path::new(path_text))?
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        let data = backup::decode(&bytes)?;
        for field in [
            "speed_quarters",
            "max_height",
            "theme",
            "buffer_max_mib",
            "buffer_back_mib",
            "buffer_cache_seconds",
        ] {
            if data.preferences.get(field).is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "preferências incompletas no backup",
                ));
            }
        }
        let library: library::Library = serde_json::from_value(data.library)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "biblioteca inválida"))?;
        library
            .validate()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "biblioteca inválida"))?;
        let queue = Queue::from_backup_value(data.queue, DEFAULT_QUEUE_LIMIT)?;
        let values = data
            .history
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "histórico ausente"))?;
        let mut history = History::default();
        if values.len() > history::MAX_ENTRIES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "histórico excede o limite",
            ));
        }
        for value in values.iter().rev() {
            let entry = HistoryEntry {
                video_id: value["video_id"].as_str().unwrap_or_default().into(),
                title: value["title"].as_str().unwrap_or_default().into(),
                channel: value["channel"].as_str().unwrap_or_default().into(),
                position_ms: value["position_ms"].as_u64().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "posição inválida")
                })?,
                duration_ms: value["duration_ms"].as_u64(),
                watched_at_ms: value["watched_at_ms"].as_u64(),
                is_short: value["is_short"].as_bool(),
                is_podcast: value["is_podcast"].as_bool(),
                is_music: value["is_music"].as_bool(),
                date_label: value["date_label"].as_str().map(str::to_owned),
                view_count_text: value["view_count_text"].as_str().map(str::to_owned),
                published_time_text: value["published_time_text"].as_str().map(str::to_owned),
                thumbnail_url: value["thumbnail_url"].as_str().map(str::to_owned),
                channel_verified: value["channel_verified"].as_bool(),
            };
            history.record(entry).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "entrada de histórico inválida")
            })?;
        }
        let speed = data.preferences["speed_quarters"]
            .as_u64()
            .and_then(|value| u8::try_from(value).ok())
            .unwrap_or(4);
        let height = data.preferences["max_height"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok());
        let playback_preferences = PlaybackPreferences {
            speed_quarters: speed,
            max_height: height,
        };
        let buffer_profile = BufferProfile::Custom(BufferSettings {
            max_mib: data.preferences["buffer_max_mib"]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(64),
            back_mib: data.preferences["buffer_back_mib"]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(16),
            cache_seconds: data.preferences["buffer_cache_seconds"]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(30),
        });
        buffer_profile
            .validate()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "buffer inválido"))?;
        let theme = data.preferences["theme"]
            .as_str()
            .and_then(Theme::parse)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tema inválido"))?;
        let feed = feed_prefs::FeedPreferences {
            hide_watched: data.preferences["feed_hide_watched"]
                .as_bool()
                .unwrap_or(false),
            hide_shorts: data.preferences["feed_hide_shorts"]
                .as_bool()
                .unwrap_or(false),
            hide_lives: data.preferences["feed_hide_lives"]
                .as_bool()
                .unwrap_or(false),
            newest_first: data.preferences["feed_newest_first"]
                .as_bool()
                .unwrap_or(false),
            channel: data.preferences["feed_channel"].as_str().map(str::to_owned),
        };
        if feed
            .channel
            .as_ref()
            .is_some_and(|channel| channel.len() > 4096)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "canal de filtro inválido",
            ));
        }
        if !(1..=8).contains(&speed)
            || !matches!(height, None | Some(360 | 480 | 720 | 1080 | 1440 | 2160))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "preferências inválidas",
            ));
        }
        let app_preferences = match data.preferences.get("app_preferences") {
            Some(value) if !value.is_null() => {
                let parsed: settings::AppPreferences = serde_json::from_value(value.clone())
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "preferências do app inválidas")
                    })?;
                let normalized = parsed.normalized();
                normalized
                    .validate()
                    .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))?;
                normalized
            }
            _ => settings::AppPreferences::default(),
        };
        let followed_channels = match data.preferences.get("followed_channels") {
            Some(value) if !value.is_null() => {
                let parsed: followed_channels::FollowedChannels =
                    serde_json::from_value(value.clone()).map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "canais seguidos inválidos no backup",
                        )
                    })?;
                parsed
                    .validate()
                    .map_err(|msg| io::Error::new(io::ErrorKind::InvalidData, msg))?;
                parsed
            }
            _ => followed_channels::FollowedChannels::default(),
        };
        Ok(ImportedUserData {
            library,
            queue,
            history,
            followed_channels,
            playback_preferences,
            theme,
            buffer_profile,
            feed,
            app_preferences,
        })
    }

    pub(crate) fn analyze_import(&mut self) {
        let imported = match self.read_imported_user_data() {
            Ok(imported) => imported,
            Err(error) => {
                self.backup.pending_import = None;
                self.backup.preview = None;
                self.backup.confirm_import = false;
                self.backup.status = Some(format!(
                    "Importação cancelada; dados atuais preservados: {error}"
                ));
                return;
            }
        };
        let (_, library_merge) = self.library.data.merged(&imported.library);
        let (_, history_merge) = self.history.data.merged(&imported.history);
        let (_, followed_merge) = self
            .followed_channels
            .data
            .merged(&imported.followed_channels);
        let queue_space = DEFAULT_QUEUE_LIMIT.saturating_sub(self.queue.data.len());
        let queue_added = imported.queue.len().min(queue_space);
        let queue_skipped = imported.queue.len().saturating_sub(queue_added);
        self.backup.preview = Some(BackupPreview {
            path: PathBuf::from(self.backup.path_text.trim()),
            library_items: imported.library.item_count(),
            favorites: imported.library.favorites.len(),
            watch_later: imported.library.watch_later.len(),
            collections: imported.library.collections.len(),
            bookmarks: imported.library.bookmarks.len(),
            queue_items: imported.queue.len(),
            history_entries: imported.history.entries().len(),
            followed_channels: imported.followed_channels.len(),
            merge_added: library_merge.added
                + history_merge.added
                + queue_added
                + followed_merge.added,
            merge_conflicts: library_merge.conflicts
                + history_merge.conflicts
                + followed_merge.conflicts,
            merge_skipped: library_merge.skipped_for_limit
                + queue_skipped
                + followed_merge.skipped_for_limit,
            history_progress_updates: history_merge.progress_updated,
        });
        self.backup.pending_import = Some(imported);
        self.backup.import_mode = BackupImportMode::Merge;
        self.backup.confirm_import = true;
        self.backup.status =
            Some("Backup validado. Confira a prévia e escolha como aplicar os dados.".into());
    }

    pub(crate) fn merged_imported_user_data(&self, imported: ImportedUserData) -> ImportedUserData {
        let (library, _) = self.library.data.merged(&imported.library);
        let (history, _) = self.history.data.merged(&imported.history);
        let (followed_channels, _) = self
            .followed_channels
            .data
            .merged(&imported.followed_channels);
        let mut queue = Queue::new(DEFAULT_QUEUE_LIMIT);
        for video in self
            .queue
            .data
            .videos()
            .iter()
            .chain(imported.queue.videos())
            .cloned()
        {
            if queue.add(video).is_err() {
                break;
            }
        }
        ImportedUserData {
            library,
            queue,
            history,
            followed_channels,
            ..imported
        }
    }

    pub(crate) fn import_user_data(&mut self) {
        let Some(imported) = self.backup.pending_import.clone() else {
            self.analyze_import();
            return;
        };
        let target = match self.backup.import_mode {
            BackupImportMode::Replace => imported,
            BackupImportMode::Merge => self.merged_imported_user_data(imported),
        };
        let paths = match self.user_data_paths() {
            Ok(paths) => paths,
            Err(error) => {
                self.backup.status = Some(format!(
                    "Importação cancelada; dados atuais preservados: {error}"
                ));
                return;
            }
        };
        let Some(root) = paths.first().and_then(|(_, path)| path.parent()) else {
            self.backup.status =
                Some("Importação cancelada; a pasta de dados local não foi localizada.".into());
            return;
        };
        self.persistence.flush();
        let refs: Vec<_> = paths
            .iter()
            .map(|(name, path)| (*name, path.as_path()))
            .collect();
        let mut restore_point = match backup::ImportRestorePoint::create(root, &refs) {
            Ok(point) => point,
            Err(error) => {
                self.backup.status = Some(format!(
                    "Importação cancelada; não foi possível criar o ponto de restauração: {error}"
                ));
                return;
            }
        };
        let persist = (|| -> io::Result<()> {
            let path = |name| {
                paths
                    .iter()
                    .find(|(candidate, _)| *candidate == name)
                    .map(|(_, path)| path.as_path())
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, name))
            };
            library::save(path("library")?, &target.library)?;
            target.queue.save(path("queue")?)?;
            history::save(path("history")?, &target.history)?;
            followed_channels::save(path("followed")?, &target.followed_channels)?;
            settings::save_playback(path("playback")?, target.playback_preferences)?;
            settings::save_theme(path("theme")?, target.theme)?;
            feed_prefs::save(path("feed")?, &target.feed)?;
            settings::save(path("buffer")?, target.buffer_profile)?;
            settings::save_app_preferences(path("prefs")?, &target.app_preferences)?;
            Ok(())
        })();
        if let Err(error) = persist {
            let rollback = restore_point.restore();
            self.backup.status = Some(match rollback {
                Ok(true) => format!(
                    "Importação falhou ({error}); o ponto de restauração desfez as alterações parciais."
                ),
                Ok(false) => {
                    format!("Importação falhou ({error}); nenhum arquivo precisou ser restaurado.")
                }
                Err(restore_error) => format!(
                    "Importação falhou ({error}); a restauração automática também falhou: {restore_error}. O ponto será tentado novamente ao abrir o app."
                ),
            });
            return;
        }
        if let Err(error) = restore_point.commit() {
            self.backup.status = Some(format!(
                "Os arquivos foram gravados, mas não foi possível remover o ponto de restauração: {error}. A importação será desfeita na próxima abertura."
            ));
            return;
        }
        self.library.data = target.library;
        self.library.revision = self.library.revision.wrapping_add(1);
        self.library.read_failed = false;
        self.library.error = None;
        self.queue.data = target.queue;
        self.queue.read_failed = false;
        self.queue.persistence_error = None;
        self.history.data = target.history;
        self.history.read_failed = false;
        self.history.error = None;
        if let Some((_, lib_path)) = paths.iter().find(|(name, _)| *name == "library") {
            let fp = history::file_fingerprint(lib_path);
            self.library.loaded_fingerprint = fp;
            self.persistence.record_loaded_fingerprint(
                persistence::PersistenceSlot::Library,
                lib_path.clone(),
                fp,
            );
        }
        if let Some((_, queue_path)) = paths.iter().find(|(name, _)| *name == "queue") {
            let fp = history::file_fingerprint(queue_path);
            self.queue.loaded_fingerprint = fp;
            self.persistence.record_loaded_fingerprint(
                persistence::PersistenceSlot::Queue,
                queue_path.clone(),
                fp,
            );
        }
        if let Some((_, hist_path)) = paths.iter().find(|(name, _)| *name == "history") {
            let fp = history::file_fingerprint(hist_path);
            self.history.loaded_fingerprint = fp;
            self.persistence.record_loaded_fingerprint(
                persistence::PersistenceSlot::History,
                hist_path.clone(),
                fp,
            );
        }
        self.followed_channels.data = target.followed_channels;
        self.followed_channels.error = None;
        self.settings.playback.value = target.playback_preferences;
        self.settings.theme.value = target.theme;
        self.settings.applied_theme = None;
        self.settings.buffer.profile = target.buffer_profile;
        self.settings.prefs.value = target.app_preferences;
        self.settings.prefs.error = None;
        self.settings.cache_dir_input_synced = false;
        self.sync_yt_dlp_cache_dir();
        self.sync_disk_image_cache();
        if self.selected.is_none() {
            self.playback.autoplay = self.settings.prefs.value.default_autoplay;
            self.playback.loop_video = self.settings.prefs.value.default_loop;
            self.reset_decoding_diagnostics();
        }
        self.feed_preferences.values.hide_watched = target.feed.hide_watched;
        self.feed_preferences.values.hide_shorts = target.feed.hide_shorts;
        self.feed_preferences.values.hide_lives = target.feed.hide_lives;
        self.feed_preferences.values.newest_first = target.feed.newest_first;
        self.feed_preferences.values.channel = target.feed.channel;
        self.cancel_pending_queue_video();
        self.invalidate_feed_cache();
        self.backup.pending_import = None;
        self.backup.preview = None;
        self.backup.confirm_import = false;
        self.backup.status = Some(match self.backup.import_mode {
            BackupImportMode::Replace => {
                "Backup aplicado por substituição; os dados anteriores ficaram no ponto de restauração até a conclusão.".into()
            }
            BackupImportMode::Merge => {
                "Backup mesclado: itens ausentes foram adicionados, a biblioteca atual prevaleceu em conflitos e o histórico manteve o maior progresso.".into()
            }
        });
    }

    // -----------------------------------------------------------------------
    // Wave 2 — InnerTube Feeds, Pagination, Watch Next, Mutations & Manifests
    // -----------------------------------------------------------------------

    pub(crate) fn persist_downloads_state_at(&self, path: &Path) -> io::Result<()> {
        let now_unix = (history::current_unix_epoch_ms() / 1000) as i64;
        let records = self.downloads.manager.to_persisted_records(now_unix);
        downloads::save_download_records_to(path, &records)
    }

    pub(crate) fn persist_downloads_state(&mut self) {
        let Some(path) = self.downloads.history_path.clone() else {
            return;
        };
        let _ = self.persist_downloads_state_at(&path);
    }

    #[cfg(windows)]
    pub(crate) fn poll_player_frame_events(&mut self, ctx: &egui::Context) {
        if self.selected.is_none()
            && self
                .player
                .as_ref()
                .is_some_and(|p| p.should_teardown_idle(player::WARM_PLAYER_IDLE_TIMEOUT))
        {
            if let Some(player) = &mut self.player {
                player.set_visible(false);
            }
            self.player = None;
            self.playback.active_buffer = None;
            self.playback.active_hwdec = None;
            self.playback.frame_snapshot = None;
            self.playback.frame_events_polled_pass = None;
            return;
        }
        let pass_nr = ctx.cumulative_pass_nr();
        if let Some(player) = &self.player {
            let (events, poll_error) = player.poll_events();
            let snap = player.snapshot();
            self.playback.frame_snapshot = Some(snap);
            self.playback.frame_events_polled_pass = Some(pass_nr);
            for event in events {
                self.handle_queue_player_event(event);
            }
            if let Some(error) = poll_error {
                self.message = error;
                self.report_pending_queue_load_error();
            }
        } else {
            self.playback.frame_snapshot = None;
            self.playback.frame_events_polled_pass = None;
        }
    }

    pub(crate) fn has_active_async_jobs(&self) -> bool {
        self.feed.search.task.is_running()
            || self.feed.search_page_task.is_running()
            || self.feed.search_suggest.task.is_running()
            || self.feed.home.stream_task.is_running()
            || self.feed.home.batch_task.is_running()
            || self.feed.home.channels_task.is_running()
            || self.feed.home_innertube_task.is_running()
            || self.feed.subs_innertube_task.is_running()
            || self.feed.notifications_task.is_running()
            || self.feed.explore_task.is_running()
            || self.feed.shorts_task.is_running()
            || self.library.liked_videos.rx.is_some()
            || self.library.remote_playlists.rx.is_some()
            || self.library.remote_playlists.action_rx.is_some()
            || self.library.watch_later_remote.rx.is_some()
            || self.library.active_playlist_detail.rx.is_some()
            || self.watch.mix_rx.is_some()
            || self.watch.next_task.is_running()
            || self.watch.innertube_comments_task.is_running()
            || self.watch.feedback_task.is_running()
            || self.watch.comment_mutation_task.is_running()
            || self.watch.replay_chat_task.is_running()
            || self.history.remote.sync_rx.is_some()
            || self.history.remote.chip_rx.is_some()
            || self.channels.videos.task.is_running()
            || self.channels.videos.more_task.is_running()
            || self.channels.videos.continuation_task.is_running()
            || self.channels.videos.sorted_task.is_running()
            || self.channels.shorts.task.is_running()
            || self.channels.streams.task.is_running()
            || self.channels.playlists.task.is_running()
            || self.channels.playlist_detail.task.is_running()
            || self.channels.about.task.is_running()
            || self.channels.search.task.is_running()
            || self.auth.reauth_task.is_running()
            || self.auth.rating_task.is_running()
            || self.auth.sub_check_task.is_running()
            || self.auth.sub_toggle_task.is_running()
            || self.auth.my_channel_task.is_running()
            || self.auth.remote_playlists_task.is_running()
            || self.auth.playlist_action_task.is_running()
            || self.auth.report_task.is_running()
            || self.playback.quality_task.is_running()
            || self.playback.manifest_task.is_running()
            || self.watch.live.watch_rx.is_some()
            || self.watch.live.stream_rx.is_some()
            || self.watch.live.send_rx.is_some()
            || self.watch.sponsorblock.rx.is_some()
            || self.watch.extras.comments_loading.is_some()
            || self.watch.extras.innertube_comments_loading
            || self.watch.extras.subtitles_loading.is_some()
            || self.watch.extras.transcript_loading
            || self.watch.extras.is_posting_comment()
            || self.watch.extras.is_rating_any_comment()
            || self.settings.ytdlp_task.is_running()
            || self.settings.app_update_task.is_running()
            || self.followed_channels.feed_task.is_running()
            || self.downloads.manager.has_active()
            || self.queue.remote_playlist_task.is_running()
            || self.watch.community_task.is_running()
    }

    pub(crate) fn schedule_idle_aware_repaint(&self, ctx: &egui::Context) {
        #[cfg(windows)]
        let (video_actively_playing, native_hwnd_fallback_active) = self
            .player
            .as_ref()
            .map(|player| {
                let playing = if let Some(snap) =
                    self.playback.frame_snapshot.as_ref().filter(|s| s.observed)
                {
                    self.selected.is_some()
                        && snap.pause != Some(true)
                        && snap.eof_reached != Some(true)
                } else {
                    self.selected.is_some()
                        && player.flag_cstr(c"pause") != Some(true)
                        && player.flag_cstr(c"eof-reached") != Some(true)
                };
                let hwnd_fallback = playing && !player.is_render_api();
                (playing, hwnd_fallback)
            })
            .unwrap_or((false, false));
        #[cfg(not(windows))]
        let (video_actively_playing, native_hwnd_fallback_active) = (false, false);

        let seek_drag_active = self.playback.seek_drag.is_some();
        let (minimized, user_interacting) = ctx.input(|input| {
            let minimized =
                input.viewport().minimized.unwrap_or(false) || self.desktop.hidden_to_tray;
            let wheel_scrolled = input
                .raw
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::MouseWheel { .. }));
            let interacting = seek_drag_active
                || self.autoscroll.is_scrolling()
                || input.pointer.any_down()
                || input.pointer.is_moving()
                || input.smooth_scroll_delta != egui::Vec2::ZERO
                || wheel_scrolled;
            (minimized, interacting)
        });
        let overlay_auto_hide_active = video_actively_playing
            && self.playback.overlay_last_activity.elapsed().as_secs_f64()
                < overlay::OVERLAY_HIDE_DELAY_SECS;
        let thumbnails_loading =
            self.thumbnails.has_pending_work() || self.avatars.has_pending_work();
        let async_jobs_active = self.has_active_async_jobs();
        let next_toast_repaint = self.toasts.next_repaint_delay(Instant::now());

        if let Some(interval) = compute_repaint_interval(RepaintScheduleInput {
            minimized,
            video_actively_playing,
            native_hwnd_fallback_active,
            user_interacting,
            overlay_auto_hide_active,
            next_toast_repaint,
            thumbnails_loading,
            async_jobs_active,
        }) {
            ctx.request_repaint_after(interval);
        }
    }

    pub(crate) fn is_window_hidden_for_power(minimized: bool, hidden_to_tray: bool) -> bool {
        minimized || hidden_to_tray
    }

    pub(crate) fn hide_to_tray(&mut self, ctx: &egui::Context) {
        if !self.desktop.tray_active() {
            return;
        }
        self.desktop.hidden_to_tray = true;
        #[cfg(windows)]
        if let Some(player) = &self.player {
            player.set_window_hidden(true);
        }
        self.thumbnails.evict_all_gpu_textures(ctx);
        self.avatars.evict_all_gpu_textures(ctx);
        self.sync_player_window_visibility();
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        ctx.request_repaint();
    }

    pub(crate) fn show_from_tray(&mut self, ctx: &egui::Context) {
        self.desktop.hidden_to_tray = false;
        #[cfg(windows)]
        if let Some(player) = &self.player {
            player.set_window_hidden(false);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.sync_player_window_visibility();
        ctx.request_repaint();
    }

    pub(crate) fn handle_window_tray_lifecycle(&mut self, ctx: &egui::Context) {
        if !self.desktop.tray_active() {
            return;
        }
        let (close_requested, minimized) = ctx.input(|i| {
            (
                i.viewport().close_requested(),
                i.viewport().minimized.unwrap_or(false),
            )
        });
        if close_requested
            && self.settings.prefs.value.close_to_tray
            && !self.desktop.tray_quit_requested
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_to_tray(ctx);
            return;
        }
        if minimized && self.settings.prefs.value.minimize_to_tray && !self.desktop.hidden_to_tray {
            self.hide_to_tray(ctx);
        }
    }

    pub(crate) fn process_desktop_integrations(&mut self, ctx: &egui::Context) {
        self.desktop.autostart.poll();

        let tray_actions = self.desktop.poll_tray_actions();
        for action in tray_actions {
            match action {
                desktop::TrayAction::Show => {
                    self.show_from_tray(ctx);
                }
                desktop::TrayAction::ToggleVisibility => {
                    if self.desktop.hidden_to_tray {
                        self.show_from_tray(ctx);
                    } else {
                        self.hide_to_tray(ctx);
                    }
                }
                desktop::TrayAction::PlayPause => {
                    #[cfg(windows)]
                    {
                        let now = Instant::now();
                        if let Some(player) = &self.player {
                            let paused = player.property("pause").as_deref() == Some("yes");
                            let next = if paused { "no" } else { "yes" };
                            let _ = player.set("pause", next);
                        }
                        self.sample_player_before_transition(now);
                        self.flush_playback_history_at(now);
                    }
                }
                desktop::TrayAction::CheckUpdates => {
                    self.show_from_tray(ctx);
                    self.open_settings_tab(app_state::SettingsTab::General, ctx);
                    self.start_app_update_check(false, ctx);
                }
                desktop::TrayAction::Quit => {
                    self.desktop.tray_quit_requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        for command in self.desktop.drain_instance_commands() {
            self.handle_instance_command(command, ctx);
        }

        let actions = self.desktop.poll_now_playing_commands();
        for action in actions {
            match action {
                desktop::NowPlayingAction::Play => {
                    #[cfg(windows)]
                    {
                        let is_live = self.active_video_is_live();
                        let mut go_live = false;
                        if let Some(player) = &self.player {
                            let ended = player.property("eof-reached").as_deref() == Some("yes");
                            if ended && is_live {
                                go_live = true;
                            } else {
                                let res = if ended {
                                    player
                                        .seek(0.0, "absolute+exact")
                                        .and_then(|()| player.set("pause", "no"))
                                } else {
                                    player.set("pause", "no")
                                };
                                if let Err(err) = res {
                                    self.message = err;
                                } else if ended {
                                    self.record_manual_seek(0.0, None, Instant::now());
                                }
                            }
                        }
                        if go_live && let Some(video) = self.selected.clone() {
                            self.go_live_edge(&video);
                        }
                    }
                }
                desktop::NowPlayingAction::Pause => {
                    #[cfg(windows)]
                    {
                        let now = Instant::now();
                        if let Some(player) = &self.player
                            && let Err(err) = player.set("pause", "yes")
                        {
                            self.message = err;
                        }
                        self.sample_player_before_transition(now);
                        self.flush_playback_history_at(now);
                    }
                }
                desktop::NowPlayingAction::PlayPause => {
                    #[cfg(windows)]
                    {
                        let now = Instant::now();
                        let is_live = self.active_video_is_live();
                        let mut go_live = false;
                        if let Some(player) = &self.player {
                            let ended = player.property("eof-reached").as_deref() == Some("yes");
                            let paused = player.property("pause").as_deref() == Some("yes");
                            if ended && is_live {
                                go_live = true;
                            } else {
                                let res = if ended {
                                    player
                                        .seek(0.0, "absolute+exact")
                                        .and_then(|()| player.set("pause", "no"))
                                } else {
                                    player.set("pause", if paused { "no" } else { "yes" })
                                };
                                if let Err(err) = res {
                                    self.message = err;
                                } else if ended {
                                    self.record_manual_seek(0.0, None, now);
                                } else if !paused {
                                    self.sample_player_before_transition(now);
                                    self.flush_playback_history_at(now);
                                }
                            }
                        }
                        if go_live && let Some(video) = self.selected.clone() {
                            self.go_live_edge(&video);
                        }
                    }
                }
                desktop::NowPlayingAction::Stop => {
                    self.close_video(ctx);
                }
                desktop::NowPlayingAction::Next => {
                    self.trigger_next_video_action(self.playback.in_app_mini_player);
                }
                desktop::NowPlayingAction::Previous => {
                    #[cfg(windows)]
                    let pos = self
                        .player
                        .as_ref()
                        .and_then(|p| p.number("time-pos"))
                        .unwrap_or(0.0);
                    #[cfg(not(windows))]
                    let pos = 0.0;
                    if pos > 5.0 {
                        self.seek_to_seconds(0.0);
                    } else {
                        self.trigger_previous_video_action(
                            self.active_video_is_live(),
                            pos,
                            self.playback.in_app_mini_player,
                        );
                    }
                }
                desktop::NowPlayingAction::SeekByMs(delta_ms) => {
                    #[cfg(windows)]
                    {
                        let is_live = self.active_video_is_live();
                        let live_dvr = self
                            .playback
                            .active
                            .as_ref()
                            .is_some_and(|active| active.live_dvr);
                        if !is_live || live_dvr {
                            let mut go_live = false;
                            let mut seek_record: Option<(f64, Option<f64>)> = None;
                            if let Some(player) = &self.player {
                                let cur = player.number("time-pos").unwrap_or(0.0);
                                let duration = player.number("duration").filter(|&d| d > 0.0);
                                let delta_secs = delta_ms as f64 / 1000.0;
                                let target = (cur + delta_secs).max(0.0);
                                if is_live
                                    && let Some(dur) = duration
                                    && (dur <= 1.5 || target >= (dur - 1.5).max(0.0))
                                {
                                    go_live = true;
                                } else if is_live {
                                    let clamped = duration
                                        .map_or(target, |dur| target.min((dur - 1.5).max(0.0)));
                                    if let Err(err) = player.seek(clamped, "absolute+exact") {
                                        self.message = err;
                                    } else {
                                        seek_record = Some((clamped, duration));
                                    }
                                } else if let Err(err) = player.seek(delta_secs, "relative") {
                                    self.message = err;
                                } else {
                                    seek_record = Some((target, duration));
                                }
                            }
                            if go_live {
                                if let Some(video) = self.selected.clone() {
                                    self.go_live_edge(&video);
                                }
                            } else if let Some((pos, dur)) = seek_record {
                                self.record_manual_seek(pos, dur, Instant::now());
                            }
                        }
                    }
                }
                desktop::NowPlayingAction::SeekTo(target) => {
                    self.seek_to_seconds(target.as_secs_f64());
                }
                desktop::NowPlayingAction::SetVolume(vol) =>
                {
                    #[cfg(windows)]
                    if let Some(player) = &self.player {
                        let pct = (vol.clamp(0.0, 1.0) * 100.0).round();
                        if let Err(err) = player.set("volume", &format!("{pct:.0}")) {
                            self.message = err;
                        }
                    }
                }
                desktop::NowPlayingAction::Raise => {
                    self.show_from_tray(ctx);
                }
            }
        }

        let (snapshot, actively_playing) = if let Some(video) = self.selected.clone() {
            #[cfg(windows)]
            let (playing, ended, position_secs, duration_secs, volume_fraction) =
                if let Some(snap) = self.playback.frame_snapshot.as_ref().filter(|s| s.observed) {
                    let paused = snap.pause == Some(true);
                    let ended = snap.eof_reached == Some(true);
                    let pos = snap
                        .time_pos
                        .filter(|v| *v >= 0.0)
                        .or_else(|| {
                            self.playback
                                .active
                                .as_ref()
                                .map(|a| a.position_ms as f64 / 1000.0)
                        })
                        .unwrap_or(0.0);
                    let dur = snap.duration.filter(|v| *v > 0.0).or_else(|| {
                        self.playback
                            .active
                            .as_ref()
                            .and_then(|a| a.duration_ms)
                            .map(|ms| ms as f64 / 1000.0)
                    });
                    let vol = snap.volume.map(|v| (v / 100.0).clamp(0.0, 1.0));
                    (!paused && !ended, ended, pos, dur, vol)
                } else if let Some(player) = &self.player {
                    let paused = player.property("pause").as_deref() == Some("yes");
                    let ended = player.property("eof-reached").as_deref() == Some("yes");
                    let pos = player
                        .number("time-pos")
                        .filter(|v| *v >= 0.0)
                        .or_else(|| {
                            self.playback
                                .active
                                .as_ref()
                                .map(|a| a.position_ms as f64 / 1000.0)
                        })
                        .unwrap_or(0.0);
                    let dur = player.number("duration").filter(|v| *v > 0.0).or_else(|| {
                        self.playback
                            .active
                            .as_ref()
                            .and_then(|a| a.duration_ms)
                            .map(|ms| ms as f64 / 1000.0)
                    });
                    let vol = player.number("volume").map(|v| (v / 100.0).clamp(0.0, 1.0));
                    (!paused && !ended, ended, pos, dur, vol)
                } else {
                    (false, false, 0.0, None, None)
                };
            #[cfg(not(windows))]
            let (playing, ended, position_secs, duration_secs, volume_fraction) =
                (false, false, 0.0, None, None);

            let can_next = !self.queue.data.is_empty()
                || (self.is_watch_short_mode() && self.next_or_prev_short(true).is_some())
                || self.next_recommended_video().is_some();
            let can_previous = position_secs > 5.0
                || (self.is_watch_short_mode() && self.next_or_prev_short(false).is_some());

            (
                Some(desktop::NowPlayingSnapshot {
                    video_id: video.url,
                    title: video.title,
                    channel: video.channel,
                    thumbnail_url: video.thumbnail,
                    playing,
                    ended,
                    position_secs,
                    duration_secs,
                    volume_fraction,
                    can_next,
                    can_previous,
                }),
                playing,
            )
        } else {
            (None, false)
        };

        self.desktop.sync_now_playing(snapshot, ctx);

        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        let window_hidden =
            Self::is_window_hidden_for_power(minimized, self.desktop.hidden_to_tray);
        let keep_awake = actively_playing && self.settings.prefs.value.keep_awake_during_playback;
        let power_events = self.desktop.sync_power(keep_awake, window_hidden);
        for event in power_events {
            match event {
                fastframe_power::Event::Suspending(ack) => {
                    let now = Instant::now();
                    #[cfg(windows)]
                    if let Some(player) = &self.player
                        && player.property("pause").as_deref() != Some("yes")
                    {
                        let _ = player.set("pause", "yes");
                    }
                    #[cfg(windows)]
                    self.sample_player_before_transition(now);
                    self.flush_playback_history_at(now);
                    ack.done();
                }
                fastframe_power::Event::EndingSession(ack) => {
                    let now = Instant::now();
                    #[cfg(windows)]
                    self.sample_player_before_transition(now);
                    self.flush_playback_history_at(now);
                    self.desktop.release_keep_awake();
                    ack.done();
                }
                fastframe_power::Event::Resumed => {
                    ctx.request_repaint();
                }
            }
        }

        let update_version = self
            .active_app_update_available()
            .map(|u| u.latest_version.clone());
        let video_title = self.selected.as_ref().map(|v| v.title.clone());
        self.desktop.sync_tray(
            self.desktop.hidden_to_tray,
            self.selected.is_some(),
            actively_playing,
            self.ui_pt(),
            update_version.as_deref(),
            video_title.as_deref(),
        );
    }

    pub(crate) fn handle_instance_command(
        &mut self,
        command: desktop::InstanceCommand,
        ctx: &egui::Context,
    ) {
        self.show_from_tray(ctx);
        match command {
            desktop::InstanceCommand::Show => {}
            desktop::InstanceCommand::OpenUrl(url) => {
                let normalized = desktop::normalize_fasttube_protocol_url(&url).unwrap_or(url);
                if let Some(target) = command_palette::parse_direct_youtube_target(&normalized) {
                    self.open_direct_youtube_target(target, ctx);
                } else {
                    self.playback.pending_open_video = Some(Video {
                        title: normalized.clone(),
                        channel: String::new(),
                        url: normalized,
                        thumbnail: String::new(),
                        duration: String::new(),
                    });
                }
            }
            desktop::InstanceCommand::OpenSection(section) => {
                let target_section = match section.trim().to_ascii_lowercase().as_str() {
                    "home" => Some(BrowseSection::Home),
                    "subscriptions" => Some(BrowseSection::Subscriptions),
                    "history" => Some(BrowseSection::History),
                    "downloads" => Some(BrowseSection::Downloads),
                    "watch_later" | "watch-later" => Some(BrowseSection::WatchLater),
                    _ => None,
                };
                if let Some(browse_section) = target_section {
                    self.navigate_from_shortcut(ctx, browse_section);
                }
            }
        }
    }

    pub(crate) fn handle_dropped_input(
        &mut self,
        input: desktop::DroppedInput,
        ctx: &egui::Context,
    ) -> bool {
        match input {
            desktop::DroppedInput::YouTubeUrl(url) => {
                let normalized = desktop::normalize_fasttube_protocol_url(&url).unwrap_or(url);
                if let Some(target) = command_palette::parse_direct_youtube_target(&normalized) {
                    self.open_direct_youtube_target(target, ctx);
                } else {
                    self.playback.pending_open_video = Some(Video {
                        title: normalized.clone(),
                        channel: String::new(),
                        url: normalized,
                        thumbnail: String::new(),
                        duration: String::new(),
                    });
                }
                true
            }
            desktop::DroppedInput::LocalMediaFile(path) => {
                let fallback_title = self.pick("Mídia local", "Local media").to_owned();
                let title = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .unwrap_or(fallback_title);
                self.play_offline_download(&path, &title, "", ctx);
                true
            }
        }
    }

    pub(crate) fn handle_drag_and_drop(&mut self, ctx: &egui::Context) {
        let (hovered_files, dropped_files, content_rect) = ctx.input(|i| {
            (
                i.raw.hovered_files.clone(),
                i.raw.dropped_files.clone(),
                i.content_rect(),
            )
        });
        if !hovered_files.is_empty() {
            let label = self.pick(
                "Solte URL do YouTube ou arquivo de mídia",
                "Drop YouTube URL or media file",
            );
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drag_and_drop_overlay"),
            ));
            painter.rect_filled(content_rect, 0.0, Color32::from_black_alpha(150));
            let card_size = egui::vec2((content_rect.width() - 64.0).clamp(260.0, 460.0), 88.0);
            let card_rect = egui::Rect::from_center_size(content_rect.center(), card_size);
            let palette = self.palette();
            painter.rect_filled(card_rect, 14.0, palette.card_alt_bg);
            painter.rect_stroke(
                card_rect,
                14.0,
                egui::Stroke::new(2.0, palette.info_accent_fg),
                egui::StrokeKind::Inside,
            );
            painter.text(
                card_rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(16.0),
                palette.channel_fg,
            );
        }
        for dropped in dropped_files {
            let path = dropped.path();
            let classified = desktop::classify_dropped_path(path)
                .or_else(|| {
                    let allow_read = std::fs::metadata(path)
                        .map(|m| m.len() <= 64 * 1024)
                        .unwrap_or(true);
                    allow_read
                        .then(|| dropped.bytes().ok())
                        .flatten()
                        .and_then(|bytes| String::from_utf8(bytes).ok())
                        .and_then(|text| desktop::classify_dropped_text(&text))
                })
                .or_else(|| {
                    path.to_str()
                        .filter(|s| !s.is_empty())
                        .and_then(desktop::classify_dropped_text)
                });
            if let Some(input) = classified
                && self.handle_dropped_input(input, ctx)
            {
                break;
            }
        }
    }

    pub(crate) fn init_win11_integrations(&mut self, creation: &eframe::CreationContext<'_>) {
        #[cfg(all(windows, not(test)))]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};

            self.desktop_win11.theme_observer =
                Some(win11::SystemThemeObserver::start(&creation.egui_ctx));
            if let Ok(handle) = creation.window_handle()
                && let RawWindowHandle::Win32(win32) = handle.as_raw()
            {
                let hwnd = win32.hwnd.get();
                self.desktop_win11.hwnd = Some(hwnd);
                let (thumb_tx, thumb_rx) = mpsc::channel();
                if win11::install_window_subclass(hwnd, thumb_tx) {
                    self.desktop_win11.subclass_installed = true;
                    self.desktop_win11.thumb_rx = Some(thumb_rx);
                }
                let is_dark = self.is_dark_theme();
                let mica = self.settings.prefs.value.mica_backdrop;
                win11::apply_dwm_window_attributes(hwnd, is_dark, mica);
                self.desktop_win11.last_dwm_state = Some((is_dark, mica));
            }
            let lang = self.settings.prefs.value.language;
            if let Ok(exe) = std::env::current_exe() {
                let tasks = win11::default_jump_list_tasks(self.ui_pt());
                let _ = win11::register_jump_list(&exe, &tasks);
            }
            self.desktop_win11.last_jump_list_lang = Some(lang);
        }
        #[cfg(not(all(windows, not(test))))]
        {
            let _ = creation;
        }
    }

    pub(crate) fn sync_win11_runtime(&mut self, ctx: &egui::Context) {
        let is_dark = self.is_dark_theme();
        let mica = self.settings.prefs.value.mica_backdrop;
        if let Some(hwnd) = self.desktop_win11.hwnd {
            if self.desktop_win11.last_dwm_state != Some((is_dark, mica)) {
                win11::apply_dwm_window_attributes(hwnd, is_dark, mica);
                self.desktop_win11.last_dwm_state = Some((is_dark, mica));
            }
            let is_fullscreen =
                self.playback.fullscreen || ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            let snap_rect = if is_fullscreen {
                None
            } else {
                self.desktop_win11.maximize_button_rect.map(|rect| {
                    let ppp = ctx.pixels_per_point();
                    win11::SnapLayoutRect::new(
                        (rect.left() * ppp).round() as i32,
                        (rect.top() * ppp).round() as i32,
                        (rect.right() * ppp).round() as i32,
                        (rect.bottom() * ppp).round() as i32,
                    )
                })
            };
            win11::update_maximize_button_rect(hwnd, snap_rect);
        }

        let current_lang = self.settings.prefs.value.language;
        if self.desktop_win11.last_jump_list_lang != Some(current_lang) {
            #[cfg(all(windows, not(test)))]
            if let Ok(exe) = std::env::current_exe() {
                let tasks = win11::default_jump_list_tasks(self.ui_pt());
                let _ = win11::register_jump_list(&exe, &tasks);
            }
            self.desktop_win11.last_jump_list_lang = Some(current_lang);
        }

        let mut thumb_cmds = Vec::new();
        if let Some(rx) = &self.desktop_win11.thumb_rx {
            while let Ok(cmd) = rx.try_recv() {
                thumb_cmds.push(cmd);
            }
        }
        for cmd in thumb_cmds {
            match cmd {
                win11::TaskbarThumbCommand::Previous => {
                    #[cfg(windows)]
                    let pos = self
                        .player
                        .as_ref()
                        .and_then(|p| p.number("time-pos"))
                        .unwrap_or(0.0);
                    #[cfg(not(windows))]
                    let pos = 0.0;
                    if pos > 5.0 {
                        self.seek_to_seconds(0.0);
                    } else {
                        self.trigger_previous_video_action(
                            self.active_video_is_live(),
                            pos,
                            self.playback.in_app_mini_player,
                        );
                    }
                }
                win11::TaskbarThumbCommand::PlayPause => {
                    #[cfg(windows)]
                    {
                        let now = Instant::now();
                        let is_live = self.active_video_is_live();
                        let mut go_live = false;
                        if let Some(player) = &self.player {
                            let ended = player.property("eof-reached").as_deref() == Some("yes");
                            let paused = player.property("pause").as_deref() == Some("yes");
                            if ended && is_live {
                                go_live = true;
                            } else {
                                let res = if ended {
                                    player
                                        .seek(0.0, "absolute+exact")
                                        .and_then(|()| player.set("pause", "no"))
                                } else {
                                    player.set("pause", if paused { "no" } else { "yes" })
                                };
                                if let Err(err) = res {
                                    self.message = err;
                                } else if ended {
                                    self.record_manual_seek(0.0, None, now);
                                } else if !paused {
                                    self.sample_player_before_transition(now);
                                    self.flush_playback_history_at(now);
                                }
                            }
                        }
                        if go_live && let Some(video) = self.selected.clone() {
                            self.go_live_edge(&video);
                        }
                    }
                }
                win11::TaskbarThumbCommand::Next => {
                    self.trigger_next_video_action(self.playback.in_app_mini_player);
                }
            }
        }

        if let Some(hwnd) = self.desktop_win11.hwnd {
            let (progress_state, completed, total) = self.compute_taskbar_progress();
            self.desktop_win11
                .taskbar
                .set_progress(hwnd, progress_state, completed, total);

            let has_video = self.selected.is_some();
            #[cfg(windows)]
            let (is_playing, pos_secs) = if !has_video {
                (false, 0.0)
            } else if let Some(snap) = self.playback.frame_snapshot.as_ref().filter(|s| s.observed)
            {
                let playing = snap.pause != Some(true) && snap.eof_reached != Some(true);
                (playing, snap.time_pos.unwrap_or(0.0))
            } else if let Some(player) = &self.player {
                let paused = player.property("pause").as_deref() == Some("yes");
                let ended = player.property("eof-reached").as_deref() == Some("yes");
                (!paused && !ended, player.number("time-pos").unwrap_or(0.0))
            } else {
                (false, 0.0)
            };
            #[cfg(not(windows))]
            let (is_playing, pos_secs) = (false, 0.0);

            let can_next = has_video
                && (!self.queue.data.is_empty()
                    || self
                        .watch
                        .active_playlist
                        .as_ref()
                        .is_some_and(|pl| pl.has_next())
                    || (self.is_watch_short_mode() && self.next_or_prev_short(true).is_some())
                    || self.next_recommended_video().is_some());
            let can_prev = has_video
                && (pos_secs > 5.0
                    || !self.playback.video_back_stack.is_empty()
                    || self
                        .watch
                        .active_playlist
                        .as_ref()
                        .is_some_and(|pl| pl.has_previous())
                    || (self.is_watch_short_mode() && self.next_or_prev_short(false).is_some()));

            self.desktop_win11.taskbar.sync_thumb_toolbar(
                hwnd,
                has_video,
                is_playing,
                can_prev,
                can_next,
                self.ui_pt(),
            );
        }
    }

    fn compute_taskbar_progress(&self) -> (win11::TaskbarProgressState, u64, u64) {
        let mut active_count = 0u64;
        let mut running_count = 0u64;
        let mut paused_count = 0u64;
        let mut progress_sum = 0.0f64;
        let mut paused_sum = 0.0f64;
        for item in &self.downloads.manager.items {
            match &item.status {
                downloads::DownloadStatus::Running {
                    progress_percent, ..
                } => {
                    active_count += 1;
                    running_count += 1;
                    progress_sum += f64::from(progress_percent.clamp(0.0, 100.0));
                }
                downloads::DownloadStatus::Queued => {
                    active_count += 1;
                }
                downloads::DownloadStatus::Paused => {
                    paused_count += 1;
                    paused_sum += f64::from(
                        self.downloads
                            .manager
                            .paused_progress(item.id)
                            .clamp(0.0, 100.0),
                    );
                }
                downloads::DownloadStatus::Completed { .. }
                | downloads::DownloadStatus::Failed(_)
                | downloads::DownloadStatus::Cancelled => {}
            }
        }
        if running_count > 0 && active_count > 0 {
            let avg = (progress_sum / active_count as f64).clamp(0.0, 100.0);
            let completed = (avg * 10.0).round() as u64;
            if completed == 0 {
                (win11::TaskbarProgressState::Indeterminate, 0, 0)
            } else {
                (win11::TaskbarProgressState::Normal, completed, 1000)
            }
        } else if paused_count > 0 {
            let avg = (paused_sum / paused_count as f64).clamp(0.0, 100.0);
            let completed = (avg * 10.0).round() as u64;
            (win11::TaskbarProgressState::Paused, completed.max(1), 1000)
        } else if self.settings.app_update_task.is_running() {
            (win11::TaskbarProgressState::Indeterminate, 0, 0)
        } else {
            (win11::TaskbarProgressState::NoProgress, 0, 0)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RepaintScheduleInput {
    pub(crate) minimized: bool,
    pub(crate) video_actively_playing: bool,
    pub(crate) native_hwnd_fallback_active: bool,
    pub(crate) user_interacting: bool,
    pub(crate) overlay_auto_hide_active: bool,
    pub(crate) next_toast_repaint: Option<Duration>,
    pub(crate) thumbnails_loading: bool,
    pub(crate) async_jobs_active: bool,
}

pub(crate) fn compute_repaint_interval(input: RepaintScheduleInput) -> Option<Duration> {
    if input.minimized {
        return None;
    }
    if input.user_interacting {
        return Some(Duration::from_millis(16));
    }
    let mut interval: Option<Duration> = None;
    let mut consider = |candidate: Duration| {
        interval = Some(match interval {
            Some(current) => current.min(candidate),
            None => candidate,
        });
    };
    if input.video_actively_playing {
        consider(Duration::from_millis(100));
    }
    if input.overlay_auto_hide_active || input.thumbnails_loading {
        consider(Duration::from_millis(80));
    }
    if let Some(toast_delay) = input.next_toast_repaint {
        consider(toast_delay);
    }
    // Background workers call `ctx.request_repaint()` directly when sending results,
    // so `async_jobs_active` intentionally schedules zero polling timers.
    let _ = input.async_jobs_active;
    if input.native_hwnd_fallback_active {
        consider(Duration::from_millis(150));
    }
    interval
}
