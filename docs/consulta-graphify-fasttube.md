# Consulta Graphify: núcleo de `FastTube`

Consulta realizada no grafo estrutural do FastTube em 11/11/2026. O objetivo foi identificar os principais pontos de acoplamento, orientar refatorações pequenas e registrar o efeito observado no grafo.

## Estado atual do grafo

Após todas as refatorações de interface realizadas até 12/11/2026:

- 1.297 nós, 3.679 relações e 68 comunidades.
- `FastTube` possui 278 relações: 234 de métodos, 42 referências de tipos, 1 de conteúdo e 1 de implementação.
- A estrutura `FastTube` declara 148 campos.
- As duas implementações de `FastTube` contêm 231 métodos identificados antes da implementação de `eframe::App`.
- A estrutura e as implementações ocupam aproximadamente 11 mil linhas de `src/main.rs`.

Evolução do grafo:

| Momento | Nós | Relações | Comunidades | Relações de `FastTube` |
| --- | ---: | ---: | ---: | ---: |
| Build inicial | 1.219 | 3.436 | 52 | 207 |
| Após dividir `poll_async_tasks` | 1.236 | 3.369 | 60 | 217 |
| Após dividir `settings_panel_ui` | 1.244 | 3.402 | 71 | 225 |
| Após dividir `navigation_ui` | 1.249 | 3.443 | 67 | 230 |
| Após dividir `browse_ui` | 1.253 | 3.464 | 65 | 234 |
| Após dividir a aba de playlists | 1.255 | 3.478 | 66 | 236 |
| Rodada paralela completa | 1.297 | 3.679 | 68 | 278 |

Os métodos de interface ainda mais extensos são:

| Método | Linhas aproximadas |
| --- | ---: |
| `player_ui` | 758 |
| `watch_page_ui` | 423 |
| `navigation_ui` | 392 |
| `browse_ui` | 390 |
| `channel_playlists_tab_ui` | 367 |
| `channel_section_ui` | 347 |
| `subtitles_ui` | 300 |

## Leitura estrutural

`FastTube` funciona como coordenador central de quatro grandes áreas:

1. Descoberta e navegação: busca, Home, canais, playlists, seleção de vídeo e filtros.
2. Reprodução: player, libmpv, telemetria, histórico, preferências e diagnóstico de decodificação.
3. Dados locais: fila, biblioteca, backup, importação e restauração.
4. Interface e ciclo do aplicativo: navegação, atalhos, janelas, temas, configurações e polling de tarefas assíncronas.

O acoplamento aparece principalmente no estado compartilhado. Existem pares de `Receiver` e `Cancellation` para busca, Home, reautenticação, avaliações, canais, playlists e qualidade. Os identificadores de requisição, mensagens de erro e estados de carregamento continuam armazenados em `FastTube`.

## Refatorações aplicadas

### Tarefas assíncronas

`poll_async_tasks` passou de aproximadamente 338 linhas para uma sequência explícita de 11 linhas. A ordem original foi preservada e o processamento foi dividido em:

- `poll_reauth`
- `poll_rating`
- `poll_home`
- `poll_quality`
- `poll_search`
- `poll_channel`
- `poll_channel_more_videos`
- `poll_channel_playlists`
- `poll_channel_playlist_videos`
- `poll_channel_about`

### Painel de configurações

`settings_panel_ui` passou de aproximadamente 524 linhas para 15 linhas. Ele agora coordena oito blocos independentes:

- `settings_panel_diagnostic_ui`
- `settings_panel_feed_ui`
- `settings_panel_backup_ui`
- `settings_panel_appearance_ui`
- `settings_panel_player_ui`
- `settings_panel_buffer_ui`
- `settings_panel_storage_status_ui`
- `settings_panel_decoding_diagnostics_ui`

Os maiores helpers resultantes são `settings_panel_buffer_ui` com aproximadamente 161 linhas, `settings_panel_backup_ui` com 131 e `settings_panel_decoding_diagnostics_ui` com 69.

### Navegação lateral

`navigation_ui` passou de aproximadamente 392 linhas para 19. A rolagem permanece no método principal e a renderização foi distribuída em cinco helpers:

- `navigation_primary_ui`
- `navigation_subscriptions_ui`
- `navigation_shortcuts_ui`
- `navigation_utilities_ui`
- `navigation_now_playing_ui`

A ordem visual e os fluxos de Início, Shorts, Inscrições, Explorar, inscrições carregadas, histórico, playlists, fila, biblioteca, configurações, logout e controles do vídeo ativo foram preservados.

### Descoberta e biblioteca

`browse_ui` passou de aproximadamente 390 para 68 linhas. O roteamento entre canal, Explorar, Inscrições, Home, busca, histórico, fila e biblioteca permaneceu no método principal. Quatro helpers passaram a cuidar de:

- avisos de configurações, histórico e notificações;
- cabeçalho de Inscrições;
- biblioteca, coleções e marcadores;
- conteúdo final da Home e estados vazios.

O maior helper resultante é `browse_library_ui`, com aproximadamente 231 linhas. Ele preserva criação de coleções, persistência da biblioteca, abertura de vídeos e navegação por marcadores.

### Playlists públicas do canal

`channel_playlists_tab_ui` passou de aproximadamente 367 para 35 linhas. O dispatcher consulta a playlist selecionada e delega para:

- `channel_playlist_detail_ui`, com estados de carregamento, erro, vazio, grade de vídeos, voltar, cancelar e atualizar;
- `channel_playlists_list_ui`, com carregamento, erro, vazio, cards de playlists, thumbnail, contagem e abertura de vídeos.

A listagem retorna `None` diretamente, como no fluxo original, porque apenas abre a playlist selecionada. O detalhe preserva a propagação de `Option<Video>`.

O total de linhas não diminuiu. O corpo foi distribuído entre funções menores. O grau de `FastTube` passou de 207 para 236 porque cada novo método também é representado como uma relação no AST. Esse crescimento é mecânico; a contagem de conexões deve orientar a navegação e não servir como nota isolada de qualidade.

A baseline e as validações passaram com 164 testes aprovados, nenhuma falha e um teste ignorado por depender de Chromium externo. Nenhum warning novo foi introduzido.

## Relações que exigem confirmação

O Graphify marcou onze referências de `FastTube` com confiança 0,55:

- `Cancellation`
- `TokenSession`
- `VideoRating`
- `ChannelPlaylist`
- `WatchExtras`
- `Library`
- `HomeChannel`
- `ChannelAboutInfo`
- `Player`

Essas relações são úteis para navegação, mas devem ser confirmadas no campo ou no fluxo correspondente antes de orientar uma refatoração.

A ligação `Queue → FastTube` é extraída diretamente. O caminho `Queue → FastTube → Player` contém uma inferência e não deve ser tratado como chamada direta de fila para player.

## Onde uma alteração tende a propagar

Use estas consultas antes de modificar fluxos centrais:

```powershell
graphify affected "FastTube" --depth 2
graphify affected "QueuePlaybackCorrelation" --depth 3
graphify explain "Player" --all
graphify path "src/queue.rs::Queue" "src/player.rs::Player" --undirected
```

Para mudanças em busca ou canais:

```powershell
graphify explain "ChannelTarget" --all
graphify explain "ChannelPage" --all
graphify affected "ChannelTarget" --depth 3
```

Para mudanças em recomendações:

```powershell
graphify explain "RecommendationEngine" --all
graphify explain "FeedFilter" --all
graphify affected "RecommendationEngine" --depth 3
```

## Rodada paralela

Todos os candidatos selecionados receberam uma passagem de refatoração em seis frentes isoladas. O resumo detalhado, commits, modelos e a lista de métodos que ainda merecem uma segunda passagem estão em [`refatoracao-paralela-ui.md`](refatoracao-paralela-ui.md).

O próximo alvo é uma segunda passagem em `player_ui`, agora com 613 linhas, seguida por `watch_page_ui`, com 321.

## Limitações

- O grafo atual é estrutural (`--code-only`); a documentação não participa das relações.
- As comunidades têm nomes genéricos ou derivados de hubs porque a rotulagem por LLM não foi executada.
- Relações com confiança inferior a 1 orientam a investigação, mas não substituem a leitura do código-fonte.