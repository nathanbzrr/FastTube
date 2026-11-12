# Fronteira de estado assíncrono, autenticação e sessão

## Escopo e método

Este documento descreve o estado observado em `src/main.rs` e propõe uma extração incremental sem mudar comportamento. Consultei Graphify: `FastTube` é o nó `src_main_fasttube` (origem `src/main.rs:823`, grau 278); também consultei `HomeFetchMessage` e `SearchFetchMessage`. O grafo marca algumas relações com `INFERRED`; confirmei os fluxos relevantes no fonte, que prevalece. Linhas abaixo referem-se ao snapshot analisado.

## Campos e acessos por grupo

### Async comum

Não existe um campo monolítico `async`. O protocolo usa `Receiver<T>`, `process::Cancellation`, ids de requisição e mensagens tipadas (`src/main.rs:733-775`). `poll_async_tasks` coordena os pollers na ordem reauth, rating, home, quality, search, canal e subconsultas (`3644-3655`); `ui` chama-o por frame (`12471-12483`). Workers usam `std::thread::spawn`, mpsc e `ctx.request_repaint()` (ex.: `start_search`, `3419-3436`). `on_exit` cancela workers pendentes (`12441-12469`).

### Search

Campos: `query`, `videos` (`824-825`), `search`, `search_cancel` (`831-832`), `search_results_mode` (`896`), `search_request_id`, `active_search_request_id` (`850-851`). `message` (`889`) é estado geral da UI, não exclusivamente Search.

Acessos principais: `begin_search_request`, `complete_search_request` (`3345-3392`); `start_search` (`3419-3436`); `poll_search` (`3860`); `apply_search_results` (`2308`); `handle_header_home`/`header_search_ui` (`4193`, `4215`); `search_results_ui` (`10313`). `start_browser_feed` cancela e invalida busca ao trocar o contexto (`3438-3451`). O request id e sua validação são essenciais para descartar resultados obsoletos.

### Home/feed

Campos: `home_search`, `home_cancel` (`833-834`); `home_playlist_urls`, `home_title`, `home_limit_reached`, `subscription_channels`, `subscription_channels_loaded`, `subscription_channels_error`, `pending_home_items` (`842-849`); `initial_public_home_started` (`843`), uma trava de inicialização/ciclo de vida, não transporte.

`HomeFetchMessage` envia `Progress(SubscriptionProgress)` e `Finished(Option<TokenSession>, Result<HomeFeed, String>)` (`769-775`). `start_browser_feed` (`3438-3553`) cancela tarefas anteriores, fecha canal, clona sessão OAuth e busca feed. Inscrições e playlists Google usam token OAuth/refresh e podem iniciar `authorize` quando interativo; playlists públicas recorrem a cookies via `session::youtube_cookies`. `poll_home` (`3735-3838`) aplica progresso e retorno. Outros consumidores: `start_public_home` (`3394-3405`), `cancel_login` (`3620-3629`), `cancel_home_feed` (`3631-3642`), `partition_home_feed_videos` (`9972`), `home_grid_ui` (`10019`), `browse_home_content_ui` (`11629`).

### Channel

Estado da página/consulta principal: `channel_page`, `channel_view_open`, `channel_tab`, `channel_search`, `channel_cancel`, `channel_request_id`, `active_channel_request_id`, `channel_loading_target`, `channel_last_target`, `channel_error`, `channel_video_limit`, `channel_from_cache` (`852-863`).

Subconsultas (`864-888`):
- Mais vídeos: `channel_more_videos_search`, `channel_more_videos_cancel`, `active_channel_more_videos_request_id`, `channel_more_videos_error`, `channel_videos_has_more`, `channel_videos_reached_max`.
- Playlists: `channel_playlists`, `channel_playlists_loaded`, `channel_playlists_search`, `channel_playlists_cancel`, `active_channel_playlists_request_id`, `channel_playlists_error`, `channel_selected_playlist`.
- Vídeos de playlist: `channel_playlist_videos`, `channel_playlist_videos_loaded`, `channel_playlist_videos_search`, `channel_playlist_videos_cancel`, `active_channel_playlist_videos_request_id`, `channel_playlist_videos_error`.
- Sobre: `channel_about`, `channel_about_loaded`, `channel_about_search`, `channel_about_cancel`, `active_channel_about_request_id`, `channel_about_error`.

Mensagens: `ChannelFetchMessage`, `ChannelMoreVideosFetchMessage`, `ChannelPlaylistsFetchMessage`, `ChannelPlaylistVideosFetchMessage`, `ChannelAboutFetchMessage` (`738-767`). Métodos: `cancel_channel_subrequests`/`cancel_channel_loading` (`2238-2307`), `open_channel`/`open_channel_with_options` (`2494-2593`), `start_channel_*`, `cancel_channel_*`, `complete_channel_*` (`2494-3100`), `close_channel_view` (`3084-3093`), `poll_channel*` (`3880-4014`), mais a UI de canal (`channel_section_ui` e abas). A resposta principal valida request id e alvo (`complete_channel_request`, `3067-3081`). Nova busca fecha/invalida canal (`3359-3371`); feed também fecha canal (`3451`).

### Quality

`quality`, `qualities`, `quality_search`, `quality_cancel`, `quality_error` (`944-948`). `quality` é seleção do playback, não tarefa; `qualities` é resultado consumido pela UI. `poll_quality` (`3840-3859`), início em `open_video` (`4015` em diante; atribuição receiver/cancel `4068-4079`), `close_video` (`4102-4121`), `player_ui`/seletor (`5723`, `6085-6152`), limpeza `clear_visual_caches` (`3407-3416`) e `on_exit` (`12441-12469`) acessam o grupo.

### Rating

`video_ratings`, `rating_request`, `rating_cancel` (`839-841`). `start_video_rating` exclui operação concorrente, exige sessão e copia `session.access_token` (`3577-3604`). `poll_rating` aplica/remove cache e limpa tarefa em sucesso, erro ou desconexão (`3700-3733`). Cartão consulta cache e pendência (`video_card_actions_ui`, `6919-6920`); `set_oauth_session(None)` limpa ratings (`3606-3618`). O id do vídeo só retorna no resultado `(String, Option<VideoRating>)`; o estado ativo não tem request id próprio.

### Reauth e sessão

Campos: `reauth`, `reauth_cancel`, `session_authenticated`, `oauth_session`, `oauth_restore_attempted` (`835-843`). `restore_session_state` restaura/descarta sessão com escopo incompatível (`975-987`); `Default` deriva `session_authenticated` de `oauth_session.is_some()` e inicializa flags (`1061-1093`). `start_reauth`/`cancel_reauth` (`3555-3575`), `poll_reauth` (`3657-3698`) e `set_oauth_session` (`3606-3618`) coordenam autorização e persistência; login/logout UI também usa os campos (`4423`, `4618`, `4658`, `4695`, `4843`, `4917-4918`, `5057`, `11340`). `ui` aplica restauração/home inicial (`12477-12483`).

`oauth_session` também é emprestada para comentários, não só aos workers (`10446`, `10589-10746`), além de feed e rating. Logo, extraí-la exige adaptar esses consumidores. `initial_public_home_started` pertence mais naturalmente ao lifecycle da home/UI.

## Dependências entre grupos; token e sessão

1. Search e Home substituem-se na navegação: `begin_search_request` cancela home; `start_browser_feed` cancela search, limpa seu id e resultados. Ambos fecham/inicializam estado de canal (`3345-3373`, `3438-3458`).
2. Canal principal e subconsultas são tarefas separadas, mas dependem do mesmo `ChannelTarget`; fechar/mudar alvo cancela subconsultas e impede aplicação de respostas antigas (`2238-2307`, `3067-3093`).
3. Home é o fluxo que devolve `TokenSession` ao thread de UI. `start_browser_feed` clona a sessão, atualiza essa cópia via `refresh_if_needed`, e `Finished` devolve a sessão renovada (`3482-3512`). `poll_home` passa `token` a `set_oauth_session` tanto no sucesso quanto no erro (`3774-3819`). Não descartar a sessão quando o fetch falha.
4. Rating copia só `access_token` (`3591`), sem devolver sessão renovada; reauth roda separado. Centralizar refresh exigiria decidir concorrência entre workers.
5. Cookies de YouTube para playlists públicas (`3545-3548`) não são `oauth::TokenSession` e não devem ser confundidos.
6. Logout remove sessão persistida e cacheia a invalidação limpando `video_ratings`. Comentários compartilham a sessão fora da fronteira dos pollers.

## Estruturas propostas

Composição ilustrativa, preferindo extração por etapas:

```rust
struct AsyncTasks {
    search: SearchState,
    home: HomeState,
    channel: ChannelState,
    quality: QualityLookupState,
    rating: RatingState,
    reauth: ReauthState,
}
struct SearchState { /* query/results + receiver, cancel, request ids */ }
struct HomeState { /* feed receiver/cancel + progresso e resultados */ }
struct ChannelState { /* alvo/página + pedido principal e subpedidos */ }
struct QualityLookupState { /* opções, receiver, cancelamento, erro */ }
struct RatingState { /* cache, receiver, cancelamento */ }
struct ReauthState { /* receiver, cancelamento */ }
struct AuthSessionState {
    session_authenticated: bool,
    oauth_session: Option<oauth::TokenSession>,
    oauth_restore_attempted: bool,
}
```

Os comentários representam os campos enumerados acima, não propõem colapsar estados incompatíveis. Mantenha `quality` selecionado no playback. Se reduzir o risco for prioritário, primeiro mova só receivers/cancel tokens/ids e deixe resultados e campos de apresentação no `FastTube`; isso limita os consumidores de UI alterados. Não introduza `JoinHandle` sem requisito: o modelo atual cancela cooperativamente e descarta receiver, sem join.

## Ordem sugerida

1. Extrair `ReauthState`, `RatingState` e `AuthSessionState`; preservar `set_oauth_session` como fronteira única de escrita/persistência, limpeza de ratings e acessos a comentários.
2. Extrair Search, preservando request generation e cancelamentos cruzados Search/Home/Channel.
3. Extrair consulta de Quality, mantendo seleção junto ao playback.
4. Extrair Channel por partes: consulta principal, more-videos, playlists, playlist-videos e about.
5. Extrair Home por último, junto de `HomeFetchMessage`, pois combina navegação, progresso parcial, login interativo e sessão renovada.
6. Adaptar `poll_async_tasks`, `ui`, `on_exit` e só depois considerar mover resultados/campos de UI para os novos owners. Não misturar com mudança de OAuth, refresh ou modelo de threads.

## Riscos

- Resposta obsoleta aplicada se request id, alvo ou ordem de invalidação mudarem.
- Limpar receiver sem cancelar `Cancellation` deixa worker ativo; shutdown precisa cancelar todos os tokens.
- Home transmite progresso parcial e pode manter resultados parciais após cancelamento/erro; a ordem de limpeza é observável.
- Perder `TokenSession` retornada junto de erro perde refresh/persistência.
- `session_authenticated` dirige UI e boot; mantê-lo coerente com `oauth_session` e erro de persistência.
- Rating não inclui geração de sessão. Um retorno atrasado pode atualizar cache após logout/reauth; decidir explicitamente se a migração adicionará identidade de sessão.
- Login interativo do feed e `reauth` são workers diferentes; não os fundir sem modelar ambos.
- Comentários leem `oauth_session` diretamente; mudanças de ownership precisam cobri-los.
- Misturar resultado de transporte com UI/playback amplia refactor e pode complicar borrows de egui.
- Tratar `Disconnected`, `Empty`, erro e sucesso como equivalentes pode eliminar mensagens/limpezas distintas.

## Critérios de aceite

- Cada campo extraído tem owner único; consumidores de UI e métodos listados leem/escrevem esse owner, sem cópias divergentes.
- Workers preservam cancelamento cooperativo, resultado, repaint e tratamento de desconexão; saída cancela todos.
- Resposta antiga de Search/Channel não sobrepõe contexto atual; Channel valida request id e target.
- Subconsultas de canal continuam independentes; mudança/fechamento impede contaminação por resposta tardia.
- Home mantém progresso, limite, resultados parciais/cancelamento e aplica a sessão retornada tanto em sucesso quanto em erro.
- Restauração/escopo, login, logout, persistência e limpeza de avaliações permanecem coerentes; comentários continuam recebendo a sessão correta.
- Rating continua serializado e token-bound; definir tratamento de resultado após mudança de sessão.
- Lookup de resolução continua ligado ao ciclo de vida do vídeo; seleção não vira transporte assíncrono.
- Nenhuma validação ou teste foi executado nesta análise. Para implementação futura, `AGENTS.md` exige delegar essa etapa a GPT-6 Luna high ou Gemini 3.8 Flash high.
