# Medições Reprodutíveis — Primeira Etapa do FastTube

> Este documento registra a arquitetura e as medições da primeira etapa. Para o comportamento atual da fila e os comandos de build, consulte o README; a fila agora é persistida e avança automaticamente.

Este documento descreve as ferramentas ([`scripts/measure-process.ps1`](../scripts/measure-process.ps1) e [`scripts/measure-network.ps1`](../scripts/measure-network.ps1)), a arquitetura real atual do código e o protocolo para medir o consumo de recursos de um processo `fasttube` já em execução no Windows **sem interrompê-lo, reiniciá-lo ou modificar executáveis em uso**, cobrindo memória privada/residente, CPU, subprocessos filhos e GPU (quando disponível via WDDM) por processo, além de tráfego total de rede por interface (com ressalvas explícitas de escopo), com registro manual dos cenários **`ocioso`**, **`pausado`**, **`reproducao`**, **`pesquisa`**, **`comparacao-navegador`** e **`nao-informado`**.

> [!IMPORTANT]
> As otimizações e configurações presentes no código atual (perfis de buffer, política `hwdec = auto-safe`, cache gradual de miniaturas, cache de resoluções, fila em memória e modo teatro) **não prometem nem garantem redução fixa de memória, CPU, GPU ou rede**. Os números reais registrados na Seção 4 referem-se exclusivamente à coleta não intrusiva da Etapa 1 no processo `PID 4204` (`nao-informado`) já em execução na máquina; nenhuma medição comparativa antes/depois foi inventada.

---

## 1. Arquitetura Real Inspecionada e Impacto nas Métricas

A inspeção dos módulos atuais — [`src/main.rs`](../src/main.rs), [`src/player.rs`](../src/player.rs), [`src/queue.rs`](../src/queue.rs), [`src/search.rs`](../src/search.rs), [`src/settings.rs`](../src/settings.rs), [`src/diagnostics.rs`](../src/diagnostics.rs), [`src/thumbnails.rs`](../src/thumbnails.rs) e [`src/history.rs`](../src/history.rs) — mostra os seguintes comportamentos que afetam diretamente as métricas:

1. **Inicialização sob demanda e destruição do `libmpv` (`ocioso` vs. vídeo aberto)**:
   - Ao abrir o aplicativo, `FastTube::default()` ([`src/main.rs`](../src/main.rs#L539-L593)) inicia com `player: None`, carregando também as configurações de buffer, o histórico local e a fila vazia (`Queue::new(50)`) em memória. Nesse estado inicial ocioso, apenas a janela `eframe`/`egui` (backend `glow`, tamanho inicial `1100 × 760`, mínimo `640 × 480` em [`src/main.rs`](../src/main.rs#L3863-L3892)) e os módulos `fastframe-*` estão ativos.
   - `Player::new` ([`src/player.rs`](../src/player.rs#L164-L281)) só carrega `libmpv-2.dll` (ou `mpv-2.dll`) e cria a janela filha Win32 `STATIC` (`VideoWindow::new`, [`src/player.rs`](../src/player.rs#L58-L85)) quando o usuário abre um vídeo e `self.player.is_none()` em `open_video` ([`src/main.rs`](../src/main.rs#L1444-L1511)).
   - Enquanto um vídeo já está aberto, abrir outro vídeo ou avançar manualmente na fila reutiliza a mesma instância de `Player` (`loadfile <url> replace`).
   - Quando o usuário fecha o vídeo em `close_video` ([`src/main.rs`](../src/main.rs#L1513-L1539)) — pelos botões `✕ Fechar vídeo`, `⌂ Resultados`, `⌂ Início` ou clicando no título `FastTube` — `self.selected` e `self.player` passam a `None`, acionando `Drop for Player` ([`src/player.rs`](../src/player.rs#L407-L414)) e `Drop for VideoWindow` ([`src/player.rs`](../src/player.rs#L141-L148)), que executam `mpv_terminate_destroy` e `DestroyWindow`.

2. **Perfis de buffer configuráveis e necessidade de fechar o vídeo atual**:
   - Em [`src/settings.rs`](../src/settings.rs#L8-L59), `BufferProfile` define os valores de `demuxer-max-bytes`, `demuxer-max-back-bytes` e `cache-secs`:
     - `Small` (**Pequeno**): `32 MiB` / `8 MiB` / `15 s`
     - `Default` (**Padrão**, default): `64 MiB` / `16 MiB` / `30 s`
     - `Large` (**Grande**): `128 MiB` / `32 MiB` / `60 s`
     - `Custom` (**Personalizado**): lido de `%LOCALAPPDATA%\FastTube\buffer-settings.txt` (`1 custom <max_mib> <back_mib> <cache_seconds>`), validado com `1 <= max_mib <= 1024`, `back_mib <= max_mib` e `1 <= cache_seconds <= 3600`.
   - Em [`src/player.rs`](../src/player.rs#L245-L270), esses limites (junto com `hwdec = auto-safe` e `ytdl-format = bestvideo[height<=1080]+bestaudio/best[height<=1080]/best`) são aplicados via `mpv_set_option_string` **somente na criação do `Player`**.
   - Se o usuário alterar o perfil em **⚙ Configurações** com um vídeo já aberto (`apply_buffer_profile` e `has_pending_buffer_restart` em [`src/main.rs`](../src/main.rs#L1141-L1203)), o novo perfil é salvo no arquivo, mas **só entra em vigor após fechar o vídeo atual (`close_video`) e abrir o próximo vídeo**.
   - Enquanto a janela **⚙ Configurações** está aberta, `should_show_video_window` ([`src/main.rs`](../src/main.rs#L277-L279)) oculta temporariamente o `HWND` nativo (`SW_HIDE` em [`src/player.rs`](../src/player.rs#L95-L108)) para não sobrepor a janela do `egui`, mantendo o áudio e a decodificação ativos em segundo plano.

3. **Diagnóstico de decodificação efetiva (`hwdec` vs. `hwdec-current`) e incertezas**:
   - O módulo [`src/diagnostics.rs`](../src/diagnostics.rs#L47-L215) separa a política configurada (`hwdec`, padrão `"auto-safe"`) da decodificação efetiva reportada pelo `libmpv` em `hwdec-current`:
     - `EffectiveDecoding::HardwareActive(backend)`: confirmado apenas para backends explícitos conhecidos no Windows (`d3d11va`, `d3d11va-copy`, `dxva2`, `dxva2-copy`, `nvdec`, `nvdec-copy`, `vulkan`, `vulkan-copy`, `qsv`, `qsv-copy`, `cuda`, `cuda-copy`).
     - `EffectiveDecoding::Software`: quando `hwdec-current` é `"no"`.
     - `EffectiveDecoding::Unknown`: quando não há vídeo aberto, quando `hwdec-current` está vazio/indisponível, ou quando retorna strings de política (`"auto"`, `"auto-safe"`, `"yes"`) ou nome não reconhecido.
   - Em [`src/main.rs`](../src/main.rs#L139-L170), a leitura das propriedades (`hwdec`, `hwdec-current`, `video-codec`, `width`, `height`) ocorre **apenas quando há vídeo ativo e a janela de configurações está aberta**, limitada a 1 coleta por segundo (`DIAGNOSTICS_REFRESH_INTERVAL = 1s`).

4. **Cache de miniaturas (`ThumbnailCache`) e cache de resoluções (`RESOLUTION_CACHE`)**:
   - **Miniaturas** ([`src/thumbnails.rs`](../src/thumbnails.rs#L5-L483)): slots fixos de `160 × 90` pontos (`SizeHint` `320 × 180`), agendamento restrito à área visível + `110 px` de overscan vertical, retenção máxima de `12` miniaturas (`MAX_RETAINED_THUMBNAILS`), até `2` carregamentos simultâneos (`MAX_CONCURRENT_LOADS`), no máximo `1` novo carregamento por passagem de UI (`MAX_NEW_LOADS_PER_PASS`), intervalo mínimo de `45 ms` entre inícios (`LOAD_START_INTERVAL`) e timeout de `15 s` (`IN_FLIGHT_TIMEOUT`), com descarte via `ctx.forget_image`.
   - **Resoluções** ([`src/search.rs`](../src/search.rs#L6-L137)): `RESOLUTION_CACHE` retém em memória até `128` URLs por `5 minutos` (`RESOLUTION_CACHE_TTL`), evitando reexecutar `yt-dlp --dump-single-json` ao reabrir a mesma URL para montar o seletor de qualidade, embora o `libmpv` continue invocando o `yt-dlp` internamente a cada `loadfile`.

5. **Histórico local e privacidade**:
   - Em [`src/history.rs`](../src/history.rs#L10-L381) e [`src/main.rs`](../src/main.rs#L49-L56), o histórico é salvo somente em `%LOCALAPPDATA%\FastTube\history.json` (até `200` entradas, limite de `2 MiB`), aceitando apenas URLs de vídeo do YouTube com ID válido de 11 caracteres. Gravações periódicas em disco durante a reprodução são limitadas a cada `5 s` (com variação mínima de `1 s`), além de gravações imediatas em pausa, seek manual, avanço na fila, troca/fechamento de vídeo e encerramento do app.

6. **Fila de reprodução atual (em memória, avanço manual e limitações)**:
   - O módulo [`src/queue.rs`](../src/queue.rs#L1-L58) implementa `Queue`, uma fila FIFO exclusivamente em memória inicializada com limite de `50` vídeos (`DEFAULT_QUEUE_LIMIT = 50` em [`src/main.rs`](../src/main.rs#L64)).
   - Na interface ([`src/main.rs`](../src/main.rs#L1306-L1427) e [`src/main.rs`](../src/main.rs#L3027-L3208)), o usuário pode adicionar cartões de resultados com `+ Fila` (`enqueue_video`), reordenar itens (`↑`/`↓` via `reorder_queue`) e remover itens (`✕` via `remove_from_queue`). Ao atingir `50/50`, novas adições são rejeitadas sem perda dos itens já presentes (`Err(rejected)`). Ao atualizar resultados de busca (`apply_search_results` em [`src/main.rs`](../src/main.rs#L1277-L1304)), os metadados (`VideoMetadata`) dos vídeos presentes na fila são mantidos em memória.
   - **Avanço manual e ausência de autoplay**: o avanço ocorre somente por ação explícita do usuário nos botões `⏭ Próximo` / `⏭ Próximo da fila` (`request_queue_advance` -> `advance_queue_with_frame` em [`src/main.rs`](../src/main.rs#L1395-L1427)). Se `open_video` falhar ao iniciar o próximo item, o vídeo permanece na fila sem ser descartado. **Não há avanço automático ao término do vídeo (`eof-reached`)**, a fila não é persistida em disco ao fechar o FastTube, não há pré-carregamento de rede/resoluções para itens enfileirados e os cartões compactos da fila não carregam miniaturas.

7. **Modo teatro (`theater_mode`) e impacto no layout/geometria do player**:
   - Em [`src/main.rs`](../src/main.rs#L299-L369) e [`src/main.rs`](../src/main.rs#L1547-L1583), o modo teatro pode ser alternado pelo botão `▭ Teatro` / `▭ Normal` nos controles do player, pelo botão `▭ Modo teatro` / `▭ Sair do teatro` no painel lateral ou pela tecla `T` (quando fora da tela cheia e sem campo de texto focado).
   - Quando ativo com vídeo aberto, `should_show_navigation_panel` oculta o painel de navegação esquerdo, `compute_watch_columns_for_mode` força o layout empilhado (`WatchLayoutMode::Stacked`) ocupando toda a largura central disponível e `compute_player_size_with_theater` expande a superfície `16:9` do `HWND` nativo sem o limite horizontal de `1280 px` do modo normal (`max_height = (available_height - 150.0).max(200.0)`), mantendo metadados, fila e resultados/histórico logo abaixo em um `ScrollArea` vertical.
   - Como o modo teatro altera o tamanho do `HWND` do `libmpv` e oculta a barra lateral (mudando também a área visível de cartões e miniaturas abaixo do vídeo), o protocolo de medição requer manter o mesmo modo de visualização (normal ou teatro) ao comparar cenários.

8. **Taxas de atualização da interface (`request_repaint_after`)**:
   - Com vídeo aberto (`reproducao` ou `pausado`), `player_ui` ([`src/main.rs`](../src/main.rs#L2281)) agenda repaint a cada `250 ms` (`4 Hz`).
   - Com a janela de configurações aberta e vídeo ativo, `settings_window_ui` ([`src/main.rs`](../src/main.rs#L2206-L2208)) agenda repaint de `1 s` para atualizar o diagnóstico.
   - Enquanto há miniaturas em carregamento ou aguardando vaga, `ThumbnailCache::end_pass` ([`src/thumbnails.rs`](../src/thumbnails.rs#L341-L359)) agenda repaints entre `16 ms` e `80 ms` até estabilizar.

9. **Build GNU em diretório separado para preservar executáveis em uso**:
   - Para compilar ou testar alterações de código sem substituir `target\release\fasttube.exe` nem interromper um processo `fasttube` ativo, utilize `--target x86_64-pc-windows-gnu --target-dir target\gnu-check` (gera `target\gnu-check\x86_64-pc-windows-gnu\release\fasttube.exe`).

---

## 2. Ferramentas de Medição: `scripts/measure-process.ps1` e `scripts/measure-network.ps1`

### 2.1. Medição por Processo (`scripts/measure-process.ps1`)

O script [`scripts/measure-process.ps1`](../scripts/measure-process.ps1) coleta amostras periódicas de um processo Windows existente sem interrompê-lo. O script de validação [`scripts/test-measure-process.ps1`](../scripts/test-measure-process.ps1) verifica a exportação JSON/CSV/Markdown no próprio processo PowerShell e confirma de forma não intrusiva que um processo `fasttube` ativo permanece em execução após a leitura.

#### Métricas Coletadas por Processo (e O Que Não É Coletado)

| Categoria | Campo no Relatório | Fonte no Windows | Significado Técnico |
| :--- | :--- | :--- | :--- |
| **Memória Privada Residente** | `WorkingSetPrivateMiB` | `Win32_PerfFormattedData_PerfProc_Process.WorkingSetPrivate` | Páginas residentes na RAM física exclusivas do processo. Corresponde à coluna padrão **Memória (conjunto de trabalho privado ativo)** do Gerenciador de Tarefas. |
| **Memória Residente Total** | `WorkingSetMiB` / `PeakWorkingSetMiB` | `System.Diagnostics.Process.WorkingSet64` / `PeakWorkingSet64` | Total de RAM física mapeada no conjunto de trabalho do processo (inclui páginas compartilhadas de `libmpv-2.dll`, DLLs do Windows e drivers). |
| **Memória Privada Comitada** | `PrivateMiB` | `System.Diagnostics.Process.PrivateMemorySize64` | Total de memória virtual privada comitada (*Commit Size* / *Private Bytes*). |
| **Memória Paginada / Virtual** | `PagedMiB`, `VirtualMiB` *(nas amostras)* | `Process.PagedMemorySize64` / `VirtualMemorySize64` | Memória paginada e espaço de endereçamento virtual por amostra (JSON/CSV). |
| **CPU (Total da Máquina)** | `CpuTotalPercent` | Delta de `TotalProcessorTime` / `(DeltaWallSec * LogicalProcessors)` | Uso percentual da capacidade total de CPU da máquina (`0%` a `100%`), comparável ao Gerenciador de Tarefas. |
| **CPU (1 Núcleo Lógico)** | `CpuSingleCorePercent` | Delta de `TotalProcessorTime` / `DeltaWallSec` | Uso percentual equivalente a 1 thread lógico (`0%` a `N * 100%`). |
| **GPU — Motores (WDDM)** | `GpuEngineTotalPercent`, `GpuEngineMaxPercent`, `GpuVideoCodecPercent`, `Gpu3DPercent`, `GpuCopyPercent` | `Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine` | Utilização por tipo de motor da GPU associada ao `pid_<PID>_*` (inclui `3D`/`Compute`, `Copy` e `Video Decode`/`Video Codec Engine`). |
| **GPU — Memória (WDDM)** | `GpuDedicatedMiB`, `GpuSharedMiB`, `GpuLocalMiB` | `Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory` | VRAM dedicada (`DedicatedUsage`) e memória de GPU compartilhada (`SharedUsage`) alocadas pelo processo. |
| **Subprocessos Filhos** | `ChildProcessCount`, `ChildWorkingSetPrivateMiB`, `ChildWorkingSetMiB`, `ChildPrivateMiB` | `Win32_Process` (`ParentProcessId`) | Monitora processos filhos ativos no instante da amostra (ex.: `yt-dlp.exe`) separadamente do processo principal. |
| **Rede por Processo** | *(Não disponível em `measure-process.ps1`)* | *(Não exposto por `Process` / `PerfProc` padrão)* | **Não coletado por PID.** `measure-process.ps1` não mede bytes/s enviados ou recebidos, largura de banda nem contagem de conexões HTTP por processo. Para tráfego de interface (com escopo global da máquina), veja a Seção 2.2. |

Para cada métrica numérica resumida em `Summary`, `measure-process.ps1` calcula: **`Min`**, **`Avg` (média)**, **`Median` (mediana)**, **`P95`**, **`Max`** e **`Last`**.

### 2.2. Medição de Tráfego por Interface (`scripts/measure-network.ps1`)

Como o Windows não expõe contadores de rede por PID sem rastreamento ETW administrativo, o repositório disponibiliza complementarmente [`scripts/measure-network.ps1`](../scripts/measure-network.ps1) (e seu teste não intrusivo [`scripts/test-measure-network.ps1`](../scripts/test-measure-network.ps1)), que lê `System.Net.NetworkInformation.NetworkInterface.GetIPStatistics()` sem captura de pacotes.

> [!WARNING]
> **Escopo estrito por interface (não é por PID)**: [`scripts/measure-network.ps1`](../scripts/measure-network.ps1) mede o **tráfego total da interface de rede selecionada, incluindo outros processos e serviços do sistema**. Os valores coletados **não são filtrados por PID** e **não podem ser atribuídos ao FastTube**. Todo resultado e amostra registra explicitamente o campo `Scope`: `"Trafego total da interface, incluindo outros processos; nao e por PID e nao pode ser atribuido ao FastTube."`

#### Parâmetros e Campos de `scripts/measure-network.ps1`

- **Parâmetros aceitos** ([`scripts/measure-network.ps1`](../scripts/measure-network.ps1#L10-L23)):
  - `-InterfaceName <string[]>`: nomes exatos das interfaces a monitorar (ex.: `"Ethernet"`, `"Wi-Fi"`). Se omitido ou vazio, seleciona automaticamente todas as interfaces com `OperationalStatus -eq 'Up'`. Falha imediatamente se uma interface informada não existir, for ambígua ou desaparecer durante a coleta.
  - `-SampleCount <int>`: número de intervalos de amostragem (`1..3600`, padrão `10`).
  - `-IntervalSeconds <double>`: duração em segundos de cada intervalo (`0.1..3600`, padrão `1`).
  - `-Scenario <string>`, `-CustomScenarioLabel <string>`, `-VideoUrl <string>`, `-Resolution <string>`, `-Notes <string>`: metadados manuais gravados no objeto raiz e repetidos em cada amostra.
  - `-OutputJson <string>` e `-OutputCsv <string>`: caminhos opcionais para exportar o objeto completo em JSON (`SchemaVersion = "1.0"`) e as amostras em CSV (os dois caminhos devem ser distintos).
  - `-Quiet`: suprime o `Write-Warning` inicial com o aviso de escopo no terminal (mantendo `Scope` no retorno e nos arquivos exportados).
- **Campos por amostra (`Samples`)** ([`scripts/measure-network.ps1`](../scripts/measure-network.ps1#L76-L95)):
  - `Sample`, `InterfaceName`, `InterfaceId`, `StartUtc`, `EndUtc`, `ElapsedSeconds` (medido com `Stopwatch`), `ReceivedBytesStart`, `ReceivedBytesEnd`, `SentBytesStart`, `SentBytesEnd`, `ReceivedBytesPerSecond`, `SentBytesPerSecond`, `Scenario`, `CustomScenarioLabel`, `VideoUrl`, `Resolution`, `Notes` e `Scope`.

#### Exemplo Real de Uso de `scripts/measure-network.ps1`

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\measure-network.ps1 `
  -InterfaceName "Ethernet" `
  -SampleCount 15 `
  -IntervalSeconds 1.0 `
  -Scenario reproducao `
  -CustomScenarioLabel "perfil-padrao-1080p" `
  -VideoUrl "https://www.youtube.com/watch?v=dQw4w9WgXcQ" `
  -Resolution "1080p" `
  -Notes "Perfil Padrao (64/16 MiB, 30s), modo normal, sem outros downloads intencionais" `
  -OutputJson .\target\measurements\rede-reproducao.json `
  -OutputCsv .\target\measurements\rede-reproducao.csv
```

Para validar os contadores reais de interface, cálculos de taxa, metadados e exportação JSON/CSV em diretório temporário sem alterar configurações de rede ou processos, execute:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\test-measure-network.ps1
```

---

## 3. Protocolo Reprodutível por Cenário (Registro Manual)

Como o FastTube não possui servidor IPC, flags de linha de comando de automação ou exportação externa de telemetria, o operador deve colocar manualmente a janela do FastTube no cenário desejado e informar `-Scenario` (e usar `-Notes` para registrar o perfil de buffer efetivamente ativo no player, o estado de `hwdec-current` exibido em **⚙ Configurações**, se o **modo teatro** está ativo ou desativado e quantos itens existem na **fila em memória**).

### Pré-requisitos
1. Certifique-se de que o FastTube já está aberto em execução. Caso haja mais de uma instância aberta (por exemplo, a instância instalada do usuário e um binário de teste compilado em `target\gnu-check\x86_64-pc-windows-gnu\release\fasttube.exe`), passe `-ProcessId <PID>` em [`scripts/measure-process.ps1`](../scripts/measure-process.ps1) para medir o processo desejado sem encerrar o outro.
2. Se você alterou o perfil de buffer (`Pequeno`, `Padrão`, `Grande` ou `Personalizado`), **feche o vídeo atual primeiro** (`✕ Fechar vídeo`) antes de abrir o vídeo do teste, garantindo que `Player::new` seja recriado com o novo perfil.
3. Para comparabilidade entre execuções, utilize o **mesmo vídeo de referência**, a **mesma resolução** (ex.: `1080p` ou `Automática`), o **mesmo perfil de buffer**, o **mesmo modo de layout** (modo normal vs. modo teatro `T`) e o **mesmo tamanho de janela** (`1100 × 760` padrão, com a janela de configurações fechada durante a amostragem para manter o `HWND` do vídeo visível).

### 3.1. Cenário 1: `ocioso`
- **Estado manual na UI**: aplicativo aberto na tela inicial (ou após clicar em **Fechar vídeo** e aguardar 5 segundos para estabilização), sem pesquisa em andamento e sem vídeo carregado.
- **Comando**:
```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\measure-process.ps1 `
  -ProcessName fasttube `
  -Scenario ocioso `
  -SampleCount 15 `
  -IntervalSeconds 1.0 `
  -Notes "Janela padrao 1100x760, sem video carregado" `
  -OutputJson .\target\measurements\ocioso.json `
  -OutputCsv .\target\measurements\ocioso.csv `
  -OutputMarkdown .\target\measurements\relatorio-etapa1.md
```

### 3.2. Cenário 2: `reproducao`
- **Estado manual na UI**: abra o vídeo de referência, aguarde o término da consulta de resoluções (`yt-dlp` encerrado), confirme a resolução (ex.: `1080p`), verifique opcionalmente em **⚙ Configurações** o perfil de buffer ativo e o valor de `hwdec-current` (fechando o painel de configurações em seguida para restaurar o `HWND` de vídeo), defina se o teste será em modo normal ou modo teatro (`T`) e deixe o vídeo reproduzindo continuamente por pelo menos 5 a 10 segundos (ou mais, caso queira observar o preenchimento completo do cache de 15s/30s/60s) antes de disparar a coleta.
- **Comando**:
```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\measure-process.ps1 `
  -ProcessName fasttube `
  -Scenario reproducao `
  -VideoUrl "https://www.youtube.com/watch?v=<ID_DO_VIDEO>" `
  -Resolution "1080p" `
  -SampleCount 15 `
  -IntervalSeconds 1.0 `
  -Notes "Perfil Padrao (64/16 MiB, 30s), hwdec=auto-safe, modo normal" `
  -OutputJson .\target\measurements\reproducao.json `
  -OutputCsv .\target\measurements\reproducao.csv `
  -OutputMarkdown .\target\measurements\relatorio-etapa1.md `
  -AppendMarkdown
```

### 3.3. Cenário 3: `pausado`
- **Estado manual na UI**: com o mesmo vídeo carregado, pause a reprodução (`Espaço` ou `K`), aguarde 3 a 5 segundos para cessar a decodificação de quadros em trânsito e execute a coleta.
- **Comando**:
```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\measure-process.ps1 `
  -ProcessName fasttube `
  -Scenario pausado `
  -VideoUrl "https://www.youtube.com/watch?v=<ID_DO_VIDEO>" `
  -Resolution "1080p" `
  -SampleCount 15 `
  -IntervalSeconds 1.0 `
  -Notes "Video carregado em memoria e pausado" `
  -OutputJson .\target\measurements\pausado.json `
  -OutputCsv .\target\measurements\pausado.csv `
  -OutputMarkdown .\target\measurements\relatorio-etapa1.md `
  -AppendMarkdown
```

### 3.4. Modo Interativo e Comparação com Navegador (Opcional)
- Para escolher o cenário em um menu interativo no terminal antes da coleta, adicione `-PromptScenario`.
- Para medir um navegador multi-processo reproduzindo o mesmo vídeo (ex.: `msedge`, `chrome`, `firefox`), utilize `-AggregateMatchingProcesses`:
```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\measure-process.ps1 `
  -ProcessName msedge `
  -AggregateMatchingProcesses `
  -Scenario comparacao-navegador `
  -SampleCount 15 `
  -IntervalSeconds 1.0
```

---

## 4. Verificação Real Executada no Ambiente Atual (Etapa 1)

Durante a implementação inicial da ferramenta de medição, havia um processo `fasttube` já em execução na máquina (`PID 4204`, executável `C:\Users\nbeze\AppData\Local\Programs\FastTube\fasttube.exe`, iniciado em `2026-11-06T22:09:03-03:00`).

Em conformidade com as regras de integridade:
- O processo `PID 4204` **não foi interrompido, reiniciado nem manipulado**.
- Como o estado visual da janela do usuário não foi alterado nem inspecionado intrusivamente pelo agente, a coleta foi registrada com `-Scenario nao-informado` (não rotulamos artificialmente como `ocioso`, `pausado` ou `reproducao`, embora os contadores de hardware WDDM mostrem atividade real de decodificação de vídeo no momento da leitura).
- **Escopo exato desta medição**: os números abaixo refletem exclusivamente aquele binário pré-existente (`PID 4204`) no momento da leitura da Etapa 1. Não foram inventadas medições pós-alterações nem resultados comparativos entre perfis de buffer, cache de miniaturas, cache de resoluções, fila de reprodução, modo teatro ou navegadores.

### Ambiente Verificado
- **SO**: Microsoft Windows 11 Home Insider Preview (`10.0.26220`, Build `26220`)
- **CPU**: AMD Ryzen 5 5500X3D 6-Core Processor (`6 núcleos / 12 threads lógicos`)
- **RAM Visível**: `32716,7 MiB` (~32 GiB)
- **GPUs**: AMD Radeon RX 7600 (`Driver 32.0.32015.7029`); Parsec Virtual Display Adapter (`Driver 0.45.0.0`)
- **Ferramentas no PATH**: `rustc 1.99.0 (b940084d7 2026-09-28)`, `yt-dlp 2026.08.19`

### Resultados Reais Observados (`PID 4204`, Cenário: `nao-informado`, 5 amostras)

| Métrica | Mín | Média | Mediana | P95 | Máx |
| :--- | ---: | ---: | ---: | ---: | ---: |
| Memória Privada Residente (`WorkingSetPrivate`, MiB) | 308,56 | 308,88 | 308,88 | 309,22 | 309,22 |
| Memória Residente Total (`WorkingSet64`, MiB) | 395,41 | 395,78 | 395,79 | 396,13 | 396,13 |
| Memória Privada Comitada (`PrivateBytes`, MiB) | 636,05 | 636,36 | 636,36 | 636,67 | 636,67 |
| CPU Total da Máquina (%, 12 threads) | 0,70 | 1,18 | 1,19 | 1,45 | 1,45 |
| CPU Equivalente 1 Núcleo (%) | 8,43 | 14,18 | 14,33 | 17,35 | 17,35 |
| GPU Total Engines (%) | 16,00 | 22,20 | 23,00 | 26,00 | 26,00 |
| GPU Video Codec Engine (%) | 15,00 | 21,20 | 22,00 | 25,00 | 25,00 |
| GPU 3D Engine (%) | 1,00 | 1,00 | 1,00 | 1,00 | 1,00 |
| GPU VRAM Dedicada (`DedicatedUsage`, MiB) | 330,23 | 330,23 | 330,23 | 330,23 | 330,23 |
| GPU VRAM Compartilhada (`SharedUsage`, MiB) | 56,20 | 56,20 | 56,20 | 56,20 | 56,20 |
| Threads / Handles | 41 / 1476 | 41 / 1476 | 41 / 1476 | 41 / 1476 | 41 / 1476 |

---

## 5. Limitações Conhecidas

1. **Ausência de medição de rede por processo (`PID`) e escopo global por interface**:
   - [`scripts/measure-process.ps1`](../scripts/measure-process.ps1) **não mede consumo de rede por processo** (taxa de download/upload em bytes/s, volume transferido ou conexões HTTP abertas pelo `libmpv`, pelo `yt-dlp` ou pelo carregador de miniaturas `egui_extras`). O Windows não disponibiliza contadores de rede por PID em `System.Diagnostics.Process` nem em `Win32_PerfFormattedData_PerfProc_Process`; medir tráfego por processo exige rastreamento ETW (`NetEventSession` com privilégios elevados) ou ferramentas externas como o Monitor de Recursos (`resmon.exe`).
   - [`scripts/measure-network.ps1`](../scripts/measure-network.ps1) mede apenas os contadores acumulados da **interface de rede como um todo** (`NetworkInterface.GetIPStatistics()`). Esse tráfego **inclui outros processos da máquina, não é por PID e não pode ser atribuído ao FastTube**. Além disso, se a interface for reiniciada ou desaparecer durante a amostragem, o script aborta a coleta com erro explícito para evitar taxas negativas ou inconsistentes.
2. **Ausência de introspecção automática do cenário e do estado interno da UI**:
   - Os coletores externos não têm acesso direto às variáveis internas de [`src/main.rs`](../src/main.rs) (`self.selected`, `self.theater_mode`, `self.queue`, `self.active_player_buffer`, `self.decoding_diagnostics`) nem às propriedades internas do `libmpv` (`pause`, `hwdec-current`), e a janela `STATIC` filha do `libmpv` não altera o título da janela principal do `eframe`. Por isso, o rótulo do cenário depende do registro manual via `-Scenario`, `-CustomScenarioLabel` e `-Notes`.
3. **Incertezas entre política `hwdec`, `hwdec-current` e contadores WDDM de GPU**:
   - Configurar `hwdec = auto-safe` em [`src/player.rs`](../src/player.rs#L251) apenas solicita decodificação por hardware ao `libmpv`, sem garanti-la. O diagnóstico interno ([`src/diagnostics.rs`](../src/diagnostics.rs#L47-L127)) só confirma aceleração ativa quando `hwdec-current` reporta um backend conhecido e só é atualizado quando a janela **⚙ Configurações** está aberta com vídeo ativo.
   - Mesmo com aceleração por hardware ativa (ou atividade em `GpuVideoCodecPercent` via WDDM), modos `*-copy` e buffers de driver continuam utilizando RAM e VRAM; não se deve inferir redução automática de memória apenas pela presença de `hwdec`.
4. **Aplicação diferida dos perfis de buffer e crescimento gradual do cache do `libmpv`**:
   - Como `demuxer-max-bytes`, `demuxer-max-back-bytes` e `cache-secs` são definidos apenas em `Player::new` ([`src/player.rs`](../src/player.rs#L252-L254)), trocar o perfil em **⚙ Configurações** durante a reprodução **exige fechar o vídeo atual** antes de abrir o próximo para surtir efeito (trocar de vídeo ou avançar manualmente na fila com o player aberto reutiliza a instância existente).
   - Durante a reprodução, o `libmpv` preenche o buffer progressivamente até o limite do perfil ativo (`32/8 MiB` no Pequeno, `64/16 MiB` no Padrão, `128/32 MiB` no Grande, ou os valores do Personalizado). Amostras coletadas logo após abrir o vídeo refletirão um buffer ainda parcialmente preenchido.
5. **Limitações reais dos caches de miniaturas e de resoluções**:
   - O `ThumbnailCache` ([`src/thumbnails.rs`](../src/thumbnails.rs#L54-L483)) limita a retenção a `12` texturas e a admissão a `2` carregamentos simultâneos (`1` por passagem a cada `45 ms`), mas **não pode abortar downloads HTTP já iniciados no `egui_extras`** (apenas acompanha URIs aposentadas por `15 s` e chama `ctx.forget_image`), não despeja itens visíveis na tela ou em estado `Loading`, e `forget_image` não obriga o alocador a devolver páginas imediatamente ao SO.
   - O `RESOLUTION_CACHE` ([`src/search.rs`](../src/search.rs)) guarda apenas em memória até `128` URLs por `5 minutos` usando a string exata da URL, sem normalizar IDs equivalentes. Consultas simultâneas à mesma URL compartilham o resultado em voo; isso evita apenas chamadas de `yt-dlp` da UI, pois o `libmpv` ainda executa seu hook interno a cada `loadfile`.
6. **Limitações da fila de reprodução e do modo teatro**:
   - A fila ([`src/queue.rs`](../src/queue.rs#L3-L58)) vive apenas em memória (até `50` itens, perdida ao fechar o app), exige avanço manual (`⏭ Próximo`) sem avanço automático ao fim do vídeo (`eof-reached`), não faz pré-carregamento e só pode receber itens pelo botão `+ Fila` dos cartões de pesquisa.
   - O modo teatro (`theater_mode` em [`src/main.rs`](../src/main.rs#L299-L369)) vive apenas em memória na sessão atual, fica indisponível em tela cheia e altera o layout para coluna única empilhada (ocultando a navegação lateral e ampliando a área de vídeo).
7. **Disponibilidade e semântica dos contadores WDDM de GPU**:
   - Os contadores `Win32_PerfFormattedData_GPUPerformanceCounters_*` exigem driver WDDM 2.0+ e contadores de desempenho habilitados no Windows. Se estiverem ausentes, o script define `GpuAvailable = $false` e preenche os campos com `null`/`N/D`.
   - `GpuEngineTotalPercent` soma a utilização de todas as engines do processo (`3D` + `Video Codec` + `Copy`), podendo ultrapassar `100%` se motores independentes operarem em paralelo; por isso `GpuEngineMaxPercent`, `Gpu3DPercent`, `GpuVideoCodecPercent` e `GpuCopyPercent` são reportados separadamente.
8. **Latência das consultas WMI/CIM e subprocessos `yt-dlp` de curta duração**:
   - Consultas `Get-CimInstance` levam cerca de `0,4 s` a `0,9 s` por amostra no Windows PowerShell. O script compensa esse tempo no intervalo de espera e calcula CPU pelo delta exato do `Stopwatch`, mas intervalos efetivos menores que ~`0,6 s` são limitados pelo provedor WMI.
   - Subprocessos `yt-dlp.exe` que iniciarem e terminarem inteiramente entre duas amostras podem não aparecer na contagem pontual de processos filhos (`ChildProcessCount`).
