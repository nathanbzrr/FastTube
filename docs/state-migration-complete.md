# Migração de estado por domínio concluída

A migração estrutural de `FastTube` foi concluída em 12/11/2026. A coordenação de fluxos permanece em `FastTube`, mas os dados de cada área passaram a residir em subestados próprios.

## Resultado

- `FastTube` passou de 148 campos para 20.
- Oito campos transversais permanecem na raiz.
- Doze raízes de domínio agrupam os outros 140 valores.
- Nenhum campo antigo foi mantido em paralelo.
- As rotas de estado foram atualizadas no código de produção e nos testes.
- Nenhum estado duplicado foi introduzido.

Campos transversais mantidos na raiz:

- `video_metadata`
- `selected_metadata`
- `thumbnails`
- `message`
- `selected`
- `scrolling`
- `in_app_ui_pass`
- `player` no Windows

Domínios criados em [`src/app_state.rs`](../src/app_state.rs):

| Domínio | Responsabilidade |
| --- | --- |
| `BrowseFeedState` | busca, catálogo, filtros, cache e Home |
| `FeedPreferencesState` | preferências de inscrições e filtros |
| `ChannelState` | canal, paginação, playlists e aba Sobre |
| `AuthState` | sessão, reautenticação e avaliações |
| `PlaybackState` | player lógico, qualidade, overlay e diagnóstico |
| `SettingsState` | tema, preferências e buffer |
| `HistoryState` | histórico e checkpoint de persistência |
| `QueueState` | fila, correlação e persistência |
| `LibraryState` | biblioteca, coleções e marcadores |
| `BackupState` | exportação, prévia e importação |
| `NavigationUiState` | seção, modais e navegação lateral |
| `WatchState` | extras da tela de reprodução |

## Evidência no Graphify

Antes da migração:

- 1.297 nós.
- 3.679 relações.
- 68 comunidades.
- `FastTube` com 278 relações, incluindo 42 referências de tipos.

Depois da migração:

- 1.327 nós.
- 3.775 relações.
- 71 comunidades.
- `FastTube` com 255 relações, incluindo apenas 19 referências de tipos.

A redução de 42 para 19 referências diretas de tipos mostra que as dependências de dados saíram do coordenador e passaram aos subestados. O número de relações de métodos permaneceu em 234.

## Validação

- `cargo fmt --check`: passou.
- `cargo check`: passou.
- 164 testes aprovados.
- 0 falhas.
- 1 teste ignorado por depender de Chromium.
- 35 warnings preexistentes, nenhum novo.
- Workspace limpo após o commit.

A estabilização foi executada pelo GPT-6 Luna high no subagente de validação da migração.

## Arquivos

- [`src/main.rs`](../src/main.rs): estado raiz e coordenação.
- [`src/app_state.rs`](../src/app_state.rs): definições dos subestados.
- [`src/main_tests.rs`](../src/main_tests.rs): testes adaptados às rotas de domínio.
- [`state-migration-map.md`](state-migration-map.md): inventário completo e decisões.
- [`state-migration-async-auth.md`](state-migration-async-auth.md): análise de async/autenticação.
- [`state-migration-discovery-navigation.md`](state-migration-discovery-navigation.md): análise de descoberta/navegação.

## Próxima etapa possível

Os estados agora podem ser passados individualmente para funções que não precisam do aplicativo inteiro. A próxima evolução é reduzir `&mut self` nos helpers de UI e persistência, usando empréstimos menores de `self.feed`, `self.channels`, `self.playback` e `self.history`.