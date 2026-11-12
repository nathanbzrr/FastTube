# Medições e Validação Portátil — Pós-Refatoração e Limpeza de Avisos

Este documento registra os resultados exatos das verificações portáteis (Etapa C) e das medições finais de processo e rede (Etapa D) executadas após a refatoração de UI, limpeza integral de avisos de compilação/Clippy e atualização do grafo estrutural do Graphify.

## 1. Ambiente e artefatos medidos

- **Sistema operacional**: Windows 11 Home Insider Preview 64-bit (versão `10.0.26220`, build `26220`, `12` processadores lógicos).
- **Build portátil GNU**: `cargo +stable-x86_64-pc-windows-gnu` via [`scripts/build-portable.ps1`](../scripts/build-portable.ps1) com `--target x86_64-pc-windows-gnu` e `--target-dir target\gnu-portable-build`.
- **Bundle final gerado em `D:\Fastube\target\portable-bundle-validation`** (contendo exclusivamente os 3 binários abaixo; sem arquivos de configuração, credenciais ou dados de usuário):
  - `fasttube.exe`: `33.475.675` bytes (`31,92 MiB`)
  - `yt-dlp.exe`: `108.406` bytes (`0,10 MiB`, versão `2026.08.19`)
  - `libmpv-2.dll`: `121.944.064` bytes (`116,29 MiB`)
- **Isolamento**: a execução da Etapa D usou uma cópia temporária do bundle fora do repositório e `LOCALAPPDATA` isolado em `D:\Fastube\target\stage-d-results\localappdata`, com `WorkingDirectory` externo (`D:\T3Code-final\temporarios`). O diretório `%LOCALAPPDATA%\FastTube` do usuário manteve os mesmos timestamps de criação (`2026-11-07T16:33:37.6700000Z`) e última gravação (`2026-11-12T02:18:01.6280000Z`) antes e depois das medições.

## 2. Medição de processo (`ocioso`) — GUI portátil

Lançamento isolado do executável portátil `fasttube.exe` (PID `19940`), aguardando ~`3 s` de inicialização da janela `FastTube`, seguido de coleta via [`scripts/measure-process.ps1`](../scripts/measure-process.ps1) com `-ProcessId 19940 -Scenario 'ocioso' -SampleCount 15 -IntervalSeconds 1.0 -IncludeChildren $true`. Duração efetiva da coleta: `15,32 s` (`15` amostras). Encerramento via `CloseMainWindow()` com código de saída `0`.

| Métrica | Min | Média | Mediana | P95 | Máx. | Última |
|---|---:|---:|---:|---:|---:|---:|
| `WorkingSetPrivate` (MiB) | 100,38 | 107,44 | 109,20 | 109,20 | 109,20 | 100,38 |
| `WorkingSet64` (MiB) | 155,81 | 163,08 | 164,62 | 164,62 | 164,62 | 155,81 |
| `PeakWorkingSet64` (MiB) | 164,59 | 164,62 | 164,62 | 164,62 | 164,62 | 164,62 |
| `PrivateMemorySize64` (MiB) | 235,74 | 243,87 | 245,32 | 249,00 | 249,00 | 235,74 |
| CPU total (%) | 0 | 0 | 0 | 0 | 0 | 0 |
| CPU equivalente a 1 núcleo (%) | 0 | 0 | 0 | 0 | 0 | 0 |
| Threads | 13 | 16,2 | 17 | 17 | 17 | 13 |
| Handles | 370 | 371,67 | 372 | 372 | 372 | 370 |
| GPU total / 3D / Video Codec (%) | 0 | 0 | 0 | 0 | 0 | 0 |
| GPU memória dedicada (MiB) | 38,26 | 38,26 | 38,26 | 38,26 | 38,26 | 38,26 |
| GPU memória compartilhada (MiB) | 7,71 | 7,71 | 7,71 | 7,71 | 7,71 | 7,71 |

Observações do coletor: `0` processos filhos ativos durante `ocioso`; mapa de engines GPU ativos vazio (`GpuEngineMaxPercent` e `GpuCopyPercent` em `0`); nenhum arquivo foi gravado em `localappdata\FastTube` durante essa execução ociosa.

## 3. Medição de rede e baseline direto do `yt-dlp` (`ocioso` e `pesquisa`)

> [!WARNING]
> [`scripts/measure-network.ps1`](../scripts/measure-network.ps1) usa `NetworkInterface.GetIPStatistics()` e mede o **tráfego total da interface inteira (todos os processos da máquina)**, não por PID. Os números abaixo **não podem ser atribuídos exclusivamente ao FastTube**.

Cada coleta executou `5` amostras por interface com intervalo nominal de `1,0 s`:

| Cenário | Interface | Recebidos mín / média / máx (bytes/s) | Enviados mín / média / máx (bytes/s) |
|---|---|---|---|
| `ocioso` | `Ethernet` | 813,05 / 684.661,33 / 2.148.847,71 | 106,05 / 52.408,56 / 109.555,86 |
| `ocioso` | `Loopback Pseudo-Interface 1` | 0 / 0 / 0 | 0 / 0 / 0 |
| `pesquisa` | `Ethernet` | 35.439,52 / 165.527,90 / 431.634,27 | 10.467,70 / 16.843,17 / 32.321,17 |
| `pesquisa` | `Loopback Pseudo-Interface 1` | 0 / 0 / 0 | 0 / 0 / 0 |

Na janela do cenário `pesquisa`, a execução direta do `yt-dlp.exe` empacotado (`--dump-single-json --flat-playlist "ytsearch5:rust programming"`, sem passar pela caixa de busca da GUI) concluiu com código `0` em `2,205 s`, pico observado de working set de `4.587.520` bytes (`4,375 MiB`) e retorno de `1` objeto JSON de playlist com `5` entradas.

## 4. Separação entre verificações automatizadas e interações manuais

### Verificações automatizadas executadas (sem falhas)

- Toolchain GNU: `cargo fmt --check`, `cargo check --all-targets --locked` (`0` avisos), `cargo clippy --all-targets --locked` (`0` avisos) e `cargo test --locked` (`164` aprovados, `0` falhas, `1` ignorado).
- Scripts PowerShell: `test-measure-process.ps1` (executado em cópia temporária sem o probe opcional de processo `fasttube` existente), `test-measure-network.ps1` e `test-portable-package.ps1` aprovados.
- Teste ignorado: `session::tests::dedicated_browser_debug_interface_smoke -- --ignored` aprovado (`1/1`).
- Testes focados adicionais: `player::tests` (`2/2`), `queue::tests` (`8/8`), `backup::tests` (`3/3`) e import preview (`1/1`).
- E2E portátil isolado (Etapa C): `yt-dlp.exe --version` (`2026.08.19`), busca ao vivo (`2` vídeos), consulta de formatos (`116` formatos, com aviso padrão do `yt-dlp` sobre ausência de runtime JS externo), probe de API/criação/inicialização/término da `libmpv-2.dll`, abertura da janela `FastTube` fora da árvore de desenvolvimento e encerramento limpo (`0`).

### Interações não automatizadas

- Fluxo interativo de login Google OAuth no navegador dedicado.
- Interações manuais da GUI e cenários de medição **`pausado`**, **`reproducao`** e **`comparacao-navegador`** (dependem de reprodução de vídeo aberta pelo usuário e de comparação com navegador externo).
- Caminhos de erro direto de inicialização da `libmpv` em runtime (os 2 testes em [`src/player.rs`](../src/player.rs) cobrem `VideoWindow` e opções `libmpv`/`wid`).
