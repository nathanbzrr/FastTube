# Mapa de migração do estado FastTube

Data da análise: 2026-11-12. Escopo: pesquisa estática e Graphify; nenhum código de produção foi alterado e nenhum teste/validação foi executado.

## Conclusão

`FastTube` contém exatamente **148 campos** em `src/main.rs:823-973`. O grafo registra `FastTube` como o maior hub do projeto, com 278 arestas (`graphify-out/GRAPH_REPORT.md:71-72`), e distribui o código em comunidades claras de feed/recomendações, canal, fila, histórico, settings, player, diagnósticos, watch extras e miniaturas (`graphify-out/GRAPH_REPORT.md:100-180`). A separação recomendada mantém `FastTube` como coordenador de fluxos e move os dados para 12 estados de domínio; oito valores permanecem compartilhados na raiz durante esta etapa.

A migração de menor risco começa por preferências e persistência local, avança para estados operacionais coesos como canal e fila, e deixa `Auth`, `Playback`, `Browse/Search` e navegação para o fim. O ponto crítico é preservar a ordem dos efeitos colaterais nos fluxos de UI, persistência, polling de canais assíncronos e troca de vídeo; a segurança de tipos não garante equivalência comportamental quando campos que compartilham callbacks são movidos juntos.

O grafo foi gerado em `f9cca2b7` e o HEAD analisado é `18f8f18`. Os dois commits posteriores alteraram somente documentação, portanto `src/main.rs` é coerente com o grafo, embora o relatório ainda exiba o SHA antigo.

## Evidência Graphify e leitura direta

- Grafo: 1.297 nós, 3.679 arestas e 68 comunidades (`graphify-out/GRAPH_REPORT.md:7-8`).
- A consulta expandida do vocabulário encontrou `FastTube`, `Library`, `Player`, `Video`, `Channel`, settings, download, layout e campos conectados. A travessia confirmou arestas de `FastTube` para `ThumbnailCache` (`src/main.rs:830`), `FeedDerivedCache` (`:968`), `History` (`:914`), `PlaybackPreferences` (`:901`), `Queue` (`:953`), `WatchExtras` (`:829`) e `Player` (`:972`).
- O maior acoplamento não é apenas de tipos: `ui` referencia oito grupos de estado e `player_ui` referencia seis (`src/main.rs:12471`, `src/main.rs:5723`).
- `src/main.rs` tem 12.713 linhas e 270 declarações `fn`; o mapa abaixo evita transformar a migração de campos em uma reescrita simultânea de métodos.

## Estrutura concreta proposta

Criar `src/app_state/` e declarar `mod app_state;` em `main.rs`. Os módulos contêm somente dados e transições locais; fluxos que atravessam domínios continuam em `impl FastTube`.

```rust
// src/main.rs, após a migração
pub(crate) struct FastTube {
    // Compartilhado na raiz nesta etapa
    video_metadata: HashMap<String, VideoMetadata>,
    selected_metadata: Option<VideoMetadata>,
    thumbnails: ThumbnailCache,
    message: String,
    selected: Option<Video>,
    scrolling: fastframe_scroll::Scrolling,
    in_app_ui_pass: bool,
    #[cfg(windows)]
    player: Option<player::Player>,

    // Domínios
    feed: app_state::browse::BrowseFeedState,
    feed_preferences: app_state::feed_preferences::FeedPreferencesState,
    channels: app_state::channel::ChannelState,
    auth: app_state::auth::AuthState,
    playback: app_state::playback::PlaybackState,
    settings: app_state::settings::SettingsState,
    history: app_state::history::HistoryState,
    queue: app_state::queue::QueueState,
    library: app_state::library::LibraryState,
    backup: app_state::backup::BackupState,
    ui: app_state::navigation::NavigationUiState,
    watch: app_state::watch::WatchState,
}
```

```text
src/app_state/
  mod.rs
  browse.rs              BrowseFeedState, SearchJob, HomeFeedState, FeedDerivedCache
  feed_preferences.rs    FeedPreferencesState
  channel.rs             ChannelState e quatro subestados de jobs
  auth.rs                AuthState
  playback.rs            PlaybackState, ActivePlayback
  settings.rs            SettingsState, TrackedSettings<T>, BufferSettingsState
  history.rs             HistoryState, HistorySaveState
  queue.rs               QueueState
  library.rs             LibraryState
  backup.rs              BackupState, BackupPreview, BackupImportMode, ImportedUserData
  navigation.rs          NavigationUiState, BrowseSection
  watch.rs               WatchState
```

Os tipos de mensagem e enums auxiliares devem acompanhar o estado proprietário: `SearchFetchMessage`/`HomeFetchMessage` e `FeedDerivedCache` em `browse.rs`; `ChannelFetchMessage`, os outros quatro tipos de mensagem e `ChannelTab` em `channel.rs`; `ChannelTarget` continua em `search.rs` como tipo de dom?nio compartilhado; `ActivePlayback` em `playback.rs`; `BackupImportMode`, `BackupPreview` e `ImportedUserData` em `backup.rs`; `BrowseSection` em `navigation.rs`.

### Contratos de dados

```rust
pub struct FeedPreferencesState {
    pub values: feed_prefs::FeedPreferences, // consolida os cinco campos atuais
    pub path: Option<PathBuf>,
}

pub struct TrackedSettings<T> {
    pub value: T,
    pub path: Option<PathBuf>,
    pub loaded_path: Option<PathBuf>,
    pub error: Option<String>,
}

pub struct SettingsState {
    pub buffer: BufferSettingsState,
    pub playback: TrackedSettings<PlaybackPreferences>,
    pub theme: TrackedSettings<Theme>,
    pub applied_theme: Option<Theme>,
}

pub struct HistoryState {
    pub data: History,
    pub path: Option<PathBuf>,
    pub loaded_path: Option<PathBuf>,
    pub read_failed: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub dirty: bool,
    pub save: HistorySaveState,
}

pub struct ChannelState {
    pub page: Option<ChannelPage>,
    pub view_open: bool,
    pub tab: ChannelTab,
    pub next_request_id: u64,
    pub loading_target: Option<ChannelTarget>,
    pub last_target: Option<ChannelTarget>,
    pub error: Option<String>,
    pub video_limit: usize,
    pub from_cache: bool,
    pub videos: ChannelVideosState,
    pub playlists: ChannelPlaylistsState,
    pub playlist_detail: ChannelPlaylistDetailState,
    pub about: ChannelAboutState,
}
```

`ChannelVideosState` agrupa `rx`, `cancel`, `active_request_id`, `more_rx`, `more_cancel`, `active_more_request_id`, `more_error`, `has_more` e `reached_max`. Os subestados de playlists, detalhe e sobre seguem a mesma regra: payload, flags de carregamento, erro, receptor e cancelamento permanecem no mesmo proprietário. Isso impede que um request antigo de uma aba limpe o estado de outra (`cancel_channel_subrequests`, `src/main.rs:2238`; polling em `src/main.rs:3880-4013`).

`BrowseFeedState` agrupa `query`, `videos`, geração, filtro, cache e dois jobs: busca e Home/inscrições. O job contém receptor, cancelamento, próximo ID e ID ativo. `home` contém itens pendentes, título, playlists, limite e estado das inscrições. O inventário abaixo fixa todas as correspondências.

## Campos que devem permanecer na raiz

| Campo | Linha | Motivo concreto |
| --- | ---: | --- |
| `video_metadata` | 827 | Cache compartilhado por feed, canal, fila e watch; preenchido em `apply_search_results` e `apply_channel_page` (`src/main.rs:2338-2380`, `2430-2437`). |
| `selected_metadata` | 828 | Derivado de `selected` e consultado por `metadata_for` (`src/main.rs:2183-2211`). |
| `thumbnails` | 830 | Cache global de imagens do egui usado por Home, canal, player e biblioteca; `clear_visual_caches` o mantém junto de `videos` (`src/main.rs:3407-3410`). |
| `message` | 890 | Canal de status global escrito por feed, auth, fila, player, settings e biblioteca. |
| `selected` | 891 | Chave de orquestração entre feed, metadados, watch extras, player, histórico e fila. |
| `scrolling` | 892 | Serviço de layout do egui sem domínio de negócio (`src/main.rs:12486`). |
| `in_app_ui_pass` | 965 | Guarda de reentrância do ciclo de UI e adiamento da fila (`src/main.rs:3337-3341`, `12484-12680`). |
| `player` | 972 | Handle FFI não clonável com ponteiro bruto, DLL e surface OpenGL (`src/player.rs:551-563`), acionado por player, diagnóstico, fila e histórico. |

`player` deve permanecer na raiz até a última fase. Movê-lo antes de estabilizar fila, histórico e diagnóstico troca um acoplamento por uma cadeia de empréstimos entre `PlaybackState` e o coordenador. `selected` e os caches globais também evitam duplicar a mesma identidade/propriedade em vários estados.

## Inventário completo dos 148 campos

Distribuição: 8 compartilhados na raiz + 140 em subestados.

| Destino | Campos |
| --- | ---: |
| `Root` | 8 |
| `BackupState` | 6 |
| `LibraryState` | 5 |
| `SettingsState` | 13 |
| `FeedPreferencesState` | 6 |
| `HistoryState` | 10 |
| `ChannelState` | 37 |
| `QueueState` | 12 |
| `AuthState` | 8 |
| `PlaybackState` | 16 |
| `NavigationUiState` | 6 |
| `BrowseFeedState` | 20 |
| `WatchState` | 1 |
| **Total** | **148** |
### Root compartilhado (8)

| Linha | Campo atual | Tipo | Destino proposto |
| ---: | --- | --- | --- |
| 827 | `video_metadata` | `HashMap<String, VideoMetadata>` | `self.video_metadata` |
| 828 | `selected_metadata` | `Option<VideoMetadata>` | `self.selected_metadata` |
| 830 | `thumbnails` | `ThumbnailCache` | `self.thumbnails` |
| 890 | `message` | `String` | `self.message` |
| 891 | `selected` | `Option<Video>` | `self.selected` |
| 892 | `scrolling` | `fastframe_scroll::Scrolling` | `self.scrolling` |
| 965 | `in_app_ui_pass` | `bool` | `self.in_app_ui_pass` |
| 972 | `player` | `Option<player::Player>` | `self.player` |

### BackupState (6)

| Linha | Campo atual | Tipo | Destino proposto |
| ---: | --- | --- | --- |
| 934 | `backup_path_text` | `String` | `self.backup.path_text` |
| 935 | `backup_status` | `Option<String>` | `self.backup.status` |
| 936 | `backup_confirm_import` | `bool` | `self.backup.confirm_import` |
| 937 | `backup_import_mode` | `BackupImportMode` | `self.backup.import_mode` |
| 938 | `backup_preview` | `Option<BackupPreview>` | `self.backup.preview` |
| 939 | `pending_import` | `Option<ImportedUserData>` | `self.backup.pending_import` |

### LibraryState (5)

| Linha | Campo atual | Tipo | Destino proposto |
| ---: | --- | --- | --- |
| 921 | `library` | `library::Library` | `self.library.data` |
| 922 | `library_path` | `Option<PathBuf>` | `self.library.path` |
| 923 | `library_error` | `Option<String>` | `self.library.error` |
| 926 | `bookmark_note` | `String` | `self.library.bookmark_note` |
| 927 | `collection_name` | `String` | `self.library.collection_name` |

### SettingsState (13)

| Linha | Campo atual | Tipo | Destino proposto |
| ---: | --- | --- | --- |
| 900 | `buffer_profile` | `BufferProfile` | `self.settings.buffer.profile` |
| 901 | `playback_preferences` | `PlaybackPreferences` | `self.settings.playback.value` |
| 902 | `playback_settings_path` | `Option<PathBuf>` | `self.settings.playback.path` |
| 903 | `loaded_playback_settings_path` | `Option<PathBuf>` | `self.settings.playback.loaded_path` |
| 904 | `playback_settings_error` | `Option<String>` | `self.settings.playback.error` |
| 905 | `theme` | `Theme` | `self.settings.theme.value` |
| 906 | `theme_settings_path` | `Option<PathBuf>` | `self.settings.theme.path` |
| 907 | `loaded_theme_settings_path` | `Option<PathBuf>` | `self.settings.theme.loaded_path` |
| 908 | `theme_settings_error` | `Option<String>` | `self.settings.theme.error` |
| 909 | `applied_theme` | `Option<Theme>` | `self.settings.applied_theme` |
| 911 | `settings_path` | `Option<PathBuf>` | `self.settings.buffer.path` |
| 912 | `settings_error` | `Option<String>` | `self.settings.buffer.error` |
| 913 | `settings_notice` | `Option<String>` | `self.settings.buffer.notice` |

### FeedPreferencesState (6 campos atuais; 2 campos finais)

| Linha | Campo atual | Tipo | Destino proposto |
| ---: | --- | --- | --- |
| 928 | `feed_hide_watched` | `bool` | `self.feed_preferences.values.hide_watched` |
| 929 | `feed_hide_shorts` | `bool` | `self.feed_preferences.values.hide_shorts` |
| 930 | `feed_hide_lives` | `bool` | `self.feed_preferences.values.hide_lives` |
| 931 | `feed_newest_first` | `bool` | `self.feed_preferences.values.newest_first` |
| 932 | `feed_channel` | `Option<String>` | `self.feed_preferences.values.channel` |
| 933 | `feed_prefs_path` | `Option<PathBuf>` | `self.feed_preferences.path` |

### HistoryState (10)

| Linha | Campo atual | Tipo | Destino proposto |
| ---: | --- | --- | --- |
| 914 | `history` | `History` | `self.history.data` |
| 915 | `history_path` | `Option<PathBuf>` | `self.history.path` |
| 916 | `loaded_history_path` | `Option<PathBuf>` | `self.history.loaded_path` |
| 917 | `history_read_failed` | `bool` | `self.history.read_failed` |
| 918 | `history_error` | `Option<String>` | `self.history.error` |
| 919 | `history_notice` | `Option<String>` | `self.history.notice` |
| 920 | `history_dirty` | `bool` | `self.history.dirty` |
| 940 | `last_history_save_at` | `Option<Instant>` | `self.history.save.last_at` |
| 941 | `last_saved_position_ms` | `Option<u64>` | `self.history.save.last_position_ms` |
| 942 | `last_saved_duration_ms` | `Option<Option<u64>>` | `self.history.save.last_duration_ms` |
### ChannelState (37)

| Linha | Campo atual | Tipo | Destino proposto |
| ---: | --- | --- | --- |
| 853 | `channel_page` | `Option<ChannelPage>` | `self.channels.page` |
| 854 | `channel_view_open` | `bool` | `self.channels.view_open` |
| 855 | `channel_tab` | `ChannelTab` | `self.channels.tab` |
| 856 | `channel_search` | `Option<Receiver<ChannelFetchMessage>>` | `self.channels.videos.rx` |
| 857 | `channel_cancel` | `Option<process::Cancellation>` | `self.channels.videos.cancel` |
| 858 | `channel_request_id` | `u64` | `self.channels.next_request_id` |
| 859 | `active_channel_request_id` | `Option<u64>` | `self.channels.videos.active_request_id` |
| 860 | `channel_loading_target` | `Option<ChannelTarget>` | `self.channels.loading_target` |
| 861 | `channel_last_target` | `Option<ChannelTarget>` | `self.channels.last_target` |
| 862 | `channel_error` | `Option<String>` | `self.channels.error` |
| 863 | `channel_video_limit` | `usize` | `self.channels.video_limit` |
| 864 | `channel_from_cache` | `bool` | `self.channels.from_cache` |
| 865 | `channel_more_videos_search` | `Option<Receiver<ChannelMoreVideosFetchMessage>>` | `self.channels.videos.more_rx` |
| 866 | `channel_more_videos_cancel` | `Option<process::Cancellation>` | `self.channels.videos.more_cancel` |
| 867 | `active_channel_more_videos_request_id` | `Option<u64>` | `self.channels.videos.active_more_request_id` |
| 868 | `channel_more_videos_error` | `Option<String>` | `self.channels.videos.more_error` |
| 869 | `channel_videos_has_more` | `bool` | `self.channels.videos.has_more` |
| 870 | `channel_videos_reached_max` | `bool` | `self.channels.videos.reached_max` |
| 871 | `channel_playlists` | `Vec<search::ChannelPlaylist>` | `self.channels.playlists.items` |
| 872 | `channel_playlists_loaded` | `bool` | `self.channels.playlists.loaded` |
| 873 | `channel_playlists_search` | `Option<Receiver<ChannelPlaylistsFetchMessage>>` | `self.channels.playlists.rx` |
| 874 | `channel_playlists_cancel` | `Option<process::Cancellation>` | `self.channels.playlists.cancel` |
| 875 | `active_channel_playlists_request_id` | `Option<u64>` | `self.channels.playlists.active_request_id` |
| 876 | `channel_playlists_error` | `Option<String>` | `self.channels.playlists.error` |
| 877 | `channel_selected_playlist` | `Option<search::ChannelPlaylist>` | `self.channels.playlist_detail.selected` |
| 878 | `channel_playlist_videos` | `Vec<SearchResult>` | `self.channels.playlist_detail.items` |
| 879 | `channel_playlist_videos_loaded` | `bool` | `self.channels.playlist_detail.loaded` |
| 880 | `channel_playlist_videos_search` | `Option<Receiver<ChannelPlaylistVideosFetchMessage>>` | `self.channels.playlist_detail.rx` |
| 881 | `channel_playlist_videos_cancel` | `Option<process::Cancellation>` | `self.channels.playlist_detail.cancel` |
| 882 | `active_channel_playlist_videos_request_id` | `Option<u64>` | `self.channels.playlist_detail.active_request_id` |
| 883 | `channel_playlist_videos_error` | `Option<String>` | `self.channels.playlist_detail.error` |
| 884 | `channel_about` | `Option<search::ChannelAboutInfo>` | `self.channels.about.info` |
| 885 | `channel_about_loaded` | `bool` | `self.channels.about.loaded` |
| 886 | `channel_about_search` | `Option<Receiver<ChannelAboutFetchMessage>>` | `self.channels.about.rx` |
| 887 | `channel_about_cancel` | `Option<process::Cancellation>` | `self.channels.about.cancel` |
| 888 | `active_channel_about_request_id` | `Option<u64>` | `self.channels.about.active_request_id` |
| 889 | `channel_about_error` | `Option<String>` | `self.channels.about.error` |

### QueueState (12)

| Linha | Campo atual | Tipo | Destino proposto |
| ---: | --- | --- | --- |
| 953 | `queue` | `Queue` | `self.queue.data` |
| 954 | `queue_path` | `Option<PathBuf>` | `self.queue.path` |
| 955 | `queue_read_failed` | `bool` | `self.queue.read_failed` |
| 956 | `queue_persistence_error` | `Option<String>` | `self.queue.persistence_error` |
| 957 | `queue_eof_handled` | `bool` | `self.queue.eof_handled` |
| 958 | `queue_error` | `Option<String>` | `self.queue.error` |
| 959 | `queue_notice` | `Option<String>` | `self.queue.notice` |
| 960 | `last_rejected_video` | `Option<Video>` | `self.queue.last_rejected_video` |
| 961 | `pending_queue_video` | `Option<Video>` | `self.queue.pending_video` |
| 962 | `pending_queue_entry_id` | `Option<u64>` | `self.queue.pending_entry_id` |
| 963 | `queue_correlation` | `QueuePlaybackCorrelation` | `self.queue.correlation` |
| 964 | `pending_queue_advance` | `bool` | `self.queue.pending_advance` |
### AuthState (8)

| Linha | Campo atual | Tipo | Destino proposto |
| ---: | --- | --- | --- |
| 835 | `reauth` | `Option<Receiver<Result<oauth::TokenSession, String>>>` | `self.auth.reauth_rx` |
| 836 | `reauth_cancel` | `Option<process::Cancellation>` | `self.auth.reauth_cancel` |
| 837 | `session_authenticated` | `bool` | `self.auth.authenticated` |
| 838 | `oauth_session` | `Option<oauth::TokenSession>` | `self.auth.session` |
| 839 | `video_ratings` | `HashMap<String, oauth::VideoRating>` | `self.auth.ratings` |
| 840 | `rating_request` | `Option<Receiver<Result<(String, Option<oauth::VideoRating>), String>>>` | `self.auth.rating_rx` |
| 841 | `rating_cancel` | `Option<process::Cancellation>` | `self.auth.rating_cancel` |
| 842 | `oauth_restore_attempted` | `bool` | `self.auth.restore_attempted` |

### PlaybackState (16; `player` e `selected` permanecem na raiz)

| Linha | Campo atual | Tipo | Destino proposto |
| ---: | --- | --- | --- |
| 893 | `fullscreen` | `bool` | `self.playback.fullscreen` |
| 894 | `theater_mode` | `bool` | `self.playback.theater` |
| 899 | `video_window_visible` | `bool` | `self.playback.window_visible` |
| 910 | `active_player_buffer` | `Option<BufferProfile>` | `self.playback.active_buffer` |
| 924 | `mini_player` | `bool` | `self.playback.mini_player` |
| 925 | `mini_player_restore_size` | `Option<egui::Vec2>` | `self.playback.mini_player_restore_size` |
| 943 | `active_playback` | `Option<ActivePlayback>` | `self.playback.active` |
| 944 | `quality` | `Option<u32>` | `self.playback.quality` |
| 945 | `qualities` | `Vec<u32>` | `self.playback.qualities` |
| 946 | `quality_search` | `Option<Receiver<Result<Vec<u32>, String>>>` | `self.playback.quality_rx` |
| 947 | `quality_cancel` | `Option<process::Cancellation>` | `self.playback.quality_cancel` |
| 948 | `quality_error` | `Option<String>` | `self.playback.quality_error` |
| 949 | `seek_drag` | `Option<f64>` | `self.playback.seek_drag` |
| 950 | `overlay_last_activity` | `Instant` | `self.playback.overlay_last_activity` |
| 951 | `decoding_diagnostics` | `DecodingDiagnostics` | `self.playback.diagnostics` |
| 952 | `last_diagnostics_poll_at` | `Option<Instant>` | `self.playback.last_diagnostics_poll_at` |

### NavigationUiState (6)

| Linha | Campo atual | Tipo | Destino proposto |
| ---: | --- | --- | --- |
| 895 | `navigation_open` | `bool` | `self.ui.navigation_open` |
| 896 | `search_results_mode` | `bool` | `self.ui.search_results_mode` |
| 897 | `settings_open` | `bool` | `self.ui.settings_open` |
| 898 | `shortcuts_open` | `bool` | `self.ui.shortcuts_open` |
| 969 | `sidebar_subscriptions_expanded` | `bool` | `self.ui.sidebar_subscriptions_expanded` |
| 970 | `browse_section` | `BrowseSection` | `self.ui.browse_section` |
### BrowseFeedState (20)

| Linha | Campo atual | Tipo | Destino proposto |
| ---: | --- | --- | --- |
| 824 | `query` | `String` | `self.feed.query` |
| 825 | `videos` | `Vec<Video>` | `self.feed.videos` |
| 826 | `feed_data_generation` | `u64` | `self.feed.generation` |
| 831 | `search` | `Option<Receiver<SearchFetchMessage>>` | `self.feed.search.rx` |
| 832 | `search_cancel` | `Option<process::Cancellation>` | `self.feed.search.cancel` |
| 833 | `home_search` | `Option<Receiver<HomeFetchMessage>>` | `self.feed.home.rx` |
| 834 | `home_cancel` | `Option<process::Cancellation>` | `self.feed.home.cancel` |
| 843 | `initial_public_home_started` | `bool` | `self.feed.home.initial_public_started` |
| 844 | `home_playlist_urls` | `Vec<String>` | `self.feed.home.playlist_urls` |
| 845 | `home_title` | `Option<String>` | `self.feed.home.title` |
| 846 | `home_limit_reached` | `bool` | `self.feed.home.limit_reached` |
| 847 | `subscription_channels` | `Vec<search::HomeChannel>` | `self.feed.home.subscription_channels` |
| 848 | `subscription_channels_loaded` | `bool` | `self.feed.home.subscription_channels_loaded` |
| 849 | `subscription_channels_error` | `Option<String>` | `self.feed.home.subscription_channels_error` |
| 850 | `pending_home_items` | `Vec<SearchResult>` | `self.feed.home.pending_items` |
| 851 | `search_request_id` | `u64` | `self.feed.search.next_request_id` |
| 852 | `active_search_request_id` | `Option<u64>` | `self.feed.search.active_request_id` |
| 966 | `feed_filter` | `FeedFilter` | `self.feed.filter` |
| 967 | `feed_cache_dirty` | `bool` | `self.feed.cache_dirty` |
| 968 | `feed_cache` | `FeedDerivedCache` | `self.feed.cache` |

### WatchState (1)

| Linha | Campo atual | Tipo | Destino proposto |
| ---: | --- | --- | --- |
| 829 | `watch_extras` | `watch_extras::WatchExtras` | `self.watch.extras` |
## Dependências cruzadas

As chamadas abaixo são as fronteiras que precisam continuar explícitas no coordenador durante a migração.

| Fronteira | Evidência atual | Efeito para a separação |
| --- | --- | --- |
| Feed/Search ↔ História, Fila, Vídeo selecionado e metadados | `ensure_feed_cache` lê `history`, `queue`, `selected` e `video_metadata` para montar `UserProfile` e gravar `feed_cache` (`src/main.rs:2388-2415`). `apply_search_results` agrega metadados da fila, canal e vencedor atual (`:2337-2382`). | `BrowseFeedState::cache` não pode depender de uma cópia própria de histórico/fila/seleção. A recomputação continua em `FastTube` com parâmetros explícitos. |
| Feed/Search ↔ Canal | `begin_search_request` fecha a visão e cancela o request de canal (`src/main.rs:3359-3370`). `apply_search_results` poda miniaturas considerando página e playlist de canal (`:2314-2363`). | A busca deve chamar uma operação estreita de `ChannelState`, sem receber o estado inteiro de Browse. |
| Canal ↔ Metadados e História | `apply_channel_page` injeta metadados no cache global e atualiza `selected_metadata` (`src/main.rs:2430-2437`). `video_for_history_entry` procura vídeos em feed, página de canal e playlist (`:2018-2058`). | Histórico continua devolvendo/consultando `Video` por uma função de catálogo compartilhada; não deve possuir cópia do feed. |
| Auth ↔ Browse/Home | `start_browser_feed` clona a sessão antes de enviar o job (`src/main.rs:3482-3552`). `poll_home` atualiza `session_authenticated`, persiste a sessão e aplica o feed (`:3774-3787`). O início automático ocorre em `ui` (`:12477-12482`). | `AuthState` e `BrowseFeedState` devem se comunicar por comandos/resultados; `poll_home` permanece coordenado por `FastTube`. |
| Auth ↔ Watch extras e avaliações | `poll_reauth` limpa comentários e reinicia a carga quando a aba está ativa (`src/main.rs:3677-3688`); avaliação executa com `oauth_session` (`:3577-3603`). | `WatchState` não deve guardar token; recebe `Option<&TokenSession>` enquanto durar a chamada. |
| Settings ↔ Playback e diagnóstico | `apply_buffer_profile` altera o perfil e consulta `active_player_buffer` (`src/main.rs:2060-2122`); `open_video` aplica preferências ao libmpv (`:4023-4051`); `settings_open` controla diagnóstico e janela (`:2150-2180`). | Manter `active_player_buffer` em Playback e o perfil desejado em Settings. Extrair os dois juntos provocaria um estado redundante. |
| Settings ↔ Feed | `apply_buffer_profile` chama `invalidate_feed_cache` (`src/main.rs:2089`). | Essa chamada deve sobreviver como evento explícito do coordenador, mesmo que o motivo funcional seja revisto depois. |
| História ↔ Feed e Playback | `record_active_playback_in_memory` e `save_history_to_disk` dependem de `active_playback` e invalidam/consultam o feed (`src/main.rs:1640-1675`, `1568-1617`). | `HistoryState` persiste seu dado; `ActivePlayback` continua em Playback; o coordenador passa uma amostra imutável à gravação. |
| Fila ↔ Player, Playback, Feed e História | `advance_queue_at` amostra player/histórico, ativa o próximo vídeo, limpa qualidade e incrementa a correlação (`src/main.rs:3167-3211`). `queue_auto_advance_ready` combina fila, selecionado e amostra viva (`:3326-3334`). | Não mover `advance_queue_at` para `QueueState`; ela é orquestração. Mover apenas dados de espera/correlação e métodos de mutação da própria `Queue`. |
| Playback ↔ Watch extras | `ui` sincroniza extras com `selected` (`src/main.rs:12473-12475`) e `close_video` limpa a URL (`:4102-4104`). | Uma transição explícita `WatchState::sync_selected(Option<&Video>)` deve substituir leituras diretas dispersas. |
| Backup ↔ todos os dados persistentes | `export_user_data` lê library, queue, history, playback, theme, buffer e feed (`src/main.rs:7275-7313`). `import_user_data` cria restore point e substitui todos os domínios (`:7517-7615`). | Backup é o maior mutador cruzado; migrar por último entre os persisters e manter a sequência write → commit → replace. |
| Navegação/UI ↔ todos | `ui` decide player, atalhos, quebra de vídeo, overlays e fechamento de settings (`src/main.rs:12471-12683`); `player_ui` escreve biblioteca, fila, playback e mensagens. | Não decompor o loop de UI em métodos que tomem `&mut self` simultaneamente. Visibilidade e navegação devem usar snapshots pequenos. |

### Chamadas que devem permanecer como coordenação de `FastTube`

- `apply_search_results` (`src/main.rs:2308`): combina feed, canal, fila, cache e seleção.
- `ensure_feed_cache` (`:2388`): combina recomendação, histórico, fila e seleção.
- `poll_async_tasks` (`:3644`): ordem fixa de reauth, rating, home, quality, busca e canais.
- `open_video` (`:4015`) e `close_video` (`:4102`): player, settings, histórico, qualidade, watch extras, navegação e janela.
- `advance_queue_at`, `advance_queue_with_frame` e `handle_queue_player_event` (`:3167`, `:3215`, `:3288`): fila, player, histórico e UI.
- `export_user_data`, `analyze_import`, `import_user_data` (`:7275`, `:7455`, `:7517`): atravessam todos os estados persistidos.
- `ui` (`:12471`) e `player_ui` (`:5723`): permanecem finos coordenadores, mesmo depois da extração de blocos puros.
## Ordem de migração de menor risco

| Ordem | Batch | Campos movidos | Por que vem aqui | Condição de parada do batch |
| ---: | --- | ---: | --- | --- |
| 0 | Criar `app_state/` e os tipos vazios/constructores | 0 | Prepara módulos sem alterar um único acesso. | Nenhum campo duplicado; `FastTube` ainda compila pelo processo autorizado. |
| 1 | `FeedPreferencesState` | 6 → 2 | O domínio já possui `feed_prefs::FeedPreferences`; consolidar os cinco valores atuais e o caminho é quase mecânico. | UI de filtros, save e backup leem apenas `feed_preferences.values`. |
| 2 | `BackupState` | 6 | Estado quase exclusivo das funções de exportação/importação; não participa do polling. | `export_user_data`, `analyze_import` e `import_user_data` usam somente o subestado para campos de backup; ordem de replace preservada. |
| 3 | `LibraryState` | 5 | Dados e rascunhos ficam juntos; há métodos de persistência dedicados e apenas três callbacks de UI. | `save_library`, bookmarks, coleções e backup usam `self.library.*`. |
| 4 | `SettingsState` | 13 | Os métodos `load/ensure/apply` já são coesos; a divisão `buffer` + `TrackedSettings<T>` elimina pares path/error soltos. | Default preserva os três arquivos carregados em `FastTube::default` (`src/main.rs:989-1224`). |
| 5 | `HistoryState` | 10 | Persistência, dirty e cadência estão no mesmo fluxo; apenas feed/playback leem seus dados. | `ensure/save/record/clear/remove` passam a usar um único owner; active playback continua fora. |
| 6 | `ChannelState` | 37 | Muitos campos, porém a maior parte dos métodos só toca canal; os cinco jobs têm ciclo de vida próprio. | Payload, receptor, cancelamento e IDs de cada request pertencem ao mesmo subestado; busca ainda pode cancelar canal por método público. |
| 7 | `QueueState` | 12 | O `Queue` já encapsula a coleção; restam correlação, persistência e avisos. | UI e persistência não escrevem `correlation/pending_*` fora do estado. Autoadvance continua coordenado. |
| 8 | `AuthState` + `WatchState` | 8 + 1 | Trocam mensagens, sessão e comentários; migrar juntos evita uma fase intermediária com dois owners da sessão. | `poll_reauth`, rating e comentários recebem snapshots de sessão; token não é copiado para `WatchState`. |
| 9 | `PlaybackState`, mantendo `player` e `selected` na raiz | 16 | Separa janela/qualidade/diagnóstico/active playback antes do FFI. | `Player` ainda é único owner na raiz; `ActivePlayback` não é duplicado em histórico. |
| 10 | `BrowseFeedState`, mantendo os caches globais na raiz | 20 | É o maior acoplamento, mas já pode usar Channel/History/Queue/Auth em APIs estreitas. | Busca e Home recebem/entregam mensagens; `FeedDerivedCache` é recomputado com referências externas. |
| 11 | `NavigationUiState` | 6 | Baixo dado, alta frequência em UI; depois da estabilização reduz apenas flags de apresentação. | `ui` usa um snapshot de navegação antes de chamadas mutáveis; nenhuma flag duplica estado de domínio. |

O batch 0 não deve introduzir aliases como `FastTube::queue()` que devolvam `&mut QueueState` enquanto o campo antigo ainda existe. Aliases temporários multiplicam empréstimos. A migração deve ser um move único por grupo e usar o compilador, por meio do subagente autorizado, para revelar cada uso restante.

A ordem não é uma estimativa de tamanho do diff: `ChannelState` tem 37 campos, mas poucas fronteiras semânticas; `NavigationUiState` tem seis e todas as 25 funções identificadas a cruzam com outro domínio. Por isso, agrupar jobs de canal cedo é mais seguro do que mover quatro flags globais cedo.
## Riscos de Rust: borrow, ownership e lifetime

1. **Player e ponteiros FFI.** `player::Player` guarda `handle: *mut c_void`, DLLs, callbacks e surface OpenGL (`src/player.rs:551-563`). Não derive `Clone`, `Serialize` ou `Default` para o estado que o contiver. Mantê-lo na raiz reduz a chance de mudar ordem de drop e de mover um tipo não `Send` para jobs.

2. **Borrow no cache derivado.** `ensure_feed_cache` hoje usa `&mut self.feed_cache` e ao mesmo tempo consulta `self.history`, `self.queue` e `self.selected` (`src/main.rs:2388-2415`). Depois da migração, prefira uma função livre com parâmetros explícitos, ou calcule `history_len`, `queue_len`, `selected_url` e `profile` antes do único trecho mutável. Não introduza `self.feed.ensure(&mut self)`.

3. **Polling de canais.** Os métodos fazem `receiver.try_recv()` por referência e depois chamam `complete_*`, que muta o mesmo estado (`src/main.rs:3880-4013`). Extraia `Option<Result<Message, Error>>` e libere a referência ao receptor antes de mutar payload, IDs, flags e erros. O mesmo padrão já aparece em `poll_reauth` (`:3658`) e `poll_home` (`:3738`).

4. **Mensagens antigas.** Os request IDs são a proteção contra resultados atrasados. Ao encapsular jobs, receptor, cancelamento e ID ativo devem viajar juntos. Separar `*_request_id` deles cria uma janela para um resultado antigo limpar o estado novo, justamente o que `begin_channel_request` e `complete_*` evitam.

5. **UI com `&mut self`.** `ui` referencia oito domínios e `player_ui` seis (`src/main.rs:12471`, `:5723`). Dentro de closures egui, não mantenha `&mut`, por exemplo, `self.settings` enquanto chama `self.open_video`. Clone/take apenas o valor pequeno necessário, como já ocorre em `clear_visual_caches` com `std::mem::take(&mut self.videos)` (`:3408`).

6. **Lifetime de `metadata_for`.** A função devolve `&VideoMetadata` escolhido entre `video_metadata`, `selected_metadata`, página de canal e playlist (`src/main.rs:2183-2211`). O retorno continua ligado ao borrow de `FastTube`. Nunca armazene essa referência em um estado de domínio nem a converta em `'static`; clone no ponto que precisa sobreviver a mutações, como `open_video` faz em `:4086`.

7. **Settings e janela do player.** `refresh_player_diagnostics` toma `&self.player` e passa funções que mutam diagnóstico/último poll (`src/main.rs:2150-2164`); `sync_player_window_visibility` toma `&mut player` (`:2174-2180`). Mantenha `settings_open` legível por valor antes dessas chamadas e evite método em `SettingsState` que receba `&mut FastTube`.

8. **Histórico e active playback.** `save_history_to_disk` muta histórico e depois lê `active_playback` (`src/main.rs:1608-1616`). A API segura é `HistoryState::save_with_sample(&HistorySample, now)`: primeiro copie posição/duração para um DTO pequeno, sem manter `&mut ActivePlayback` durante I/O.

9. **Closures e drenagem de fila.** `handle_queue_player_event` muta correlação, pending IDs, `Queue` e mensagens no mesmo match (`src/main.rs:3288-3320`). Extraia a decisão da correlação para um valor (`QueueLoadResult`) e aplique-a depois; não guarde `&mut self.queue.correlation` durante `self.queue.data.remove_entry`.

10. **Construção não trivial.** `Instant::now`, arquivos carregados, erros iniciais e branches `cfg(test)` estão em `FastTube::default` (`src/main.rs:989-1224`). Settings, History, Queue e Playback precisam de constructores que recebam esses valores; `#[derive(Default)]` indiscriminado apagaria estado carregado ou criaria defaults divergentes.

11. **Drop order.** `Player`, `ThumbnailCache`, `WatchExtras` e os receivers têm Drop/cancelamento implícito. Um move entre structs altera a ordem de drop. Nesta fase, manter esses tipos na raiz ou em wrappers simples evita mudanças no encerramento; `on_exit` também deve continuar cancelando os jobs (`src/main.rs:12450-12469`).

12. **`#[cfg(windows)]`.** O campo `player` e os fluxos em `open_video`/`close_video` são condicionais (`src/main.rs:971-972`, `:4015-4135`). Se `PlaybackState` receber código dependente, preserve a cfg no campo e nos blocos, sem criar tipos Windows-only que quebrem os demais builds.

13. **Borrows disjuntos podem desaparecer em métodos auxiliares.** Rust aceita hoje ler `self.history` e mutar `self.feed_cache` no mesmo escopo porque são campos distintos. Se a migração transformar isso em `self.feed.recompute(self.history...)`, o borrow de `self` pode conflitar. Funções livres, parâmetros explícitos e DTOs pequenos são a forma de manter os borrows disjuntos.

## Estratégia incremental sem quebrar comportamento

1. **Primeiro mover dados, depois comportamento.** Os métodos continuam em `impl FastTube`; apenas a expressão de acesso muda (`self.theme` vira `self.settings.theme.value`). Isso mantém ordem de mutações e reduz o diff semântico.
2. **Um domínio por commit.** Não misturar um batch de estado com extração de UI ou mudança de mensagem/erro. O mapa preserva a correspondência dos nomes antigos para facilitar revisão.
3. **Mover tipos dependentes com o dono.** Channel jobs e mensagens, ActivePlayback, FeedDerivedCache e backup models nunca ficam órfãos em `main.rs`.
4. **Sem estado espelhado.** Ao mover um grupo, apagar imediatamente os campos antigos. Manter ambos e sincronizá-los cria duas fontes de verdade e esconde acessos não migrados.
5. **APIs estreitas entre domínios.** Preferir `ChannelState::cancel_all()`, `FeedPreferencesState::snapshot()`, `HistoryState::save(sample, now)` e `WatchState::sync_selected(video)` a passar `&mut FastTube` para um estado.
6. **Snapshots para UI.** Antes de renderizar, copiar flags pequenas (`selected.is_some()`, `settings_open`, `navigation_open`, `theater_mode`) para variáveis locais. Isso preserva a independência de empréstimos sem clones grandes.
7. **Ordem dos polls congelada.** `poll_async_tasks` mantém a sequência atual de `src/main.rs:3644-3654` até todos os jobs estarem encapsulados. Qualquer mudança de ordem entra em commit separado, com justificativa.
8. **Persistência após mutação em memória.** Em settings, theme, feed, history, library e queue, preservar a regra atual de atualizar o valor, tentar salvar e registrar erro/notice sem reverter silenciosamente.
9. **Importação transacional.** Preservar `ImportRestorePoint::create`, escrita em ordem, rollback e `commit` em `src/main.rs:7544-7589`; a refatoração deve trocar apenas os caminhos de acesso aos campos.
10. **Gate por batch.** A implementação deve parar quando ocorrer mudança de comportamento observável, erro de borrow ou divergência de ordem. A compilação, formatação e testes de cada batch devem ser executados exclusivamente por subagente GPT-6 Luna high ou Gemini 3.8 Flash high, conforme `AGENTS.md`; esta pesquisa não executou esses gates.

## Critério para concluir a separação

A migração estrutural está concluída quando nenhum estado de domínio escreve campos privados uns dos outros diretamente, `FastTube` conserva apenas os oito compartilhados e os fluxos coordenadores acima, e todos os jobs assíncronos mantêm receptor, cancelamento, ID e payload sob o mesmo owner. A extração de métodos por domínio é uma segunda etapa: comece por `SettingsState` e `HistoryState`, depois `ChannelState`/`QueueState`; deixe `apply_search_results`, player e importação no coordenador até que suas fronteiras estejam estáveis.
