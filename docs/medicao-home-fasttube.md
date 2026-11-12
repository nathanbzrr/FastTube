# Medição de memória — FastTube (Home)

## Escopo e método

- Data: 11/11/2026; Windows 11 Home Insider Preview, build 26220.
- Cenário: janela inicial ociosa, sem busca manual nem vídeo carregado. Processo iniciado apenas para esta coleta: PID `7048`.
- Build Release GNU com a toolchain WinLibs GCC 16.2.0 portátil em `target/toolchains/winlibs-gcc-16.2.0-r3`; `--target-dir target/task-home-measurement` manteve `target/debug` e o target padrão fora do build.
- `LOCALAPPDATA` do processo apontou para `target/task-home-measurement/localappdata`. A inicialização criou apenas `FastTube/history.json` (219 bytes) nessa pasta isolada; nenhum dado real do usuário foi usado. A sessão OAuth isolada estava ausente e não houve login.
- O smoke ficou ativo, foi medido pelo PID exato e depois encerrado. A amostragem usou `scripts/measure-process.ps1`, 15 amostras, intervalo solicitado de 1 s, incluindo processos filhos.

## Processo

Valores do resumo do script (MiB; `Min/Avg/Median/P95/Max/Last`):

| Métrica | Min | Média | Mediana | P95 | Máx. | Última |
|---|---:|---:|---:|---:|---:|---:|
| Working set privado residente | 113,96 | 113,96 | 113,96 | 113,96 | 113,96 | 113,96 |
| Working set residente total | 168,80 | 168,80 | 168,80 | 168,80 | 168,80 | 168,80 |
| Pico de working set reportado | 177,62 | 177,62 | 177,62 | 177,62 | 177,62 | 177,62 |
| Private Bytes | 182,91 | 182,91 | 182,91 | 182,91 | 182,91 | 182,91 |

Foram coletadas 15 amostras em 21,20 s de duração efetiva do script. O tempo excedeu os 15 s nominais: a coleta usa consultas WMI/CIM e alguns intervalos observados foram mais longos que 1 s. Portanto, a duração reportada é a do script, não um tempo exato de inicialização do app.

## Artefatos e build

- Executável: `target/task-home-measurement/x86_64-pc-windows-gnu/release/fasttube.exe` — 33.172.366 bytes (31,64 MiB).
- DLL runtime copiada para o mesmo diretório isolado: `libmpv-2.dll` — 121.944.064 bytes (116,29 MiB); não incluída no tamanho do executável.
- Medições brutas: `target/task-home-measurement/process.json`, `process.csv`, `network.json` e `network.csv`.
- Comando de build: `cargo +stable-x86_64-pc-windows-gnu build --release --target x86_64-pc-windows-gnu --target-dir target/task-home-measurement`, com `PATH`, `CC_x86_64_pc_windows_gnu`, `CXX_x86_64_pc_windows_gnu`, `AR_x86_64_pc_windows_gnu`, `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER` e `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_AR` apontando para `target/toolchains/winlibs-gcc-16.2.0-r3/mingw64/bin`.
- Build concluído com sucesso; o compilador emitiu 22 avisos já presentes na compilação (imports/variáveis não usados e dead code).

## Rede e limites

`scripts/measure-network.ps1` foi executado com 5 amostras de 1 s. Na interface `Ethernet`, a taxa global variou entre 11.218,87 e 1.726.326,59 bytes/s recebidos (média 379.109,73) e entre 1.186,43 e 15.563,29 bytes/s enviados (média 4.685,64). A pseudo-interface Loopback reportou 0 bytes/s em todas as amostras.

Esses contadores cobrem a interface inteira, incluindo outros processos e serviços, e **não podem ser atribuídos ao FastTube**. Não foi usada URL, reprodução, credencial ou OAuth. A medição de rede serve apenas como observação do tráfego total simultâneo.

Não havia baseline comparável nesta execução. Os valores descrevem somente este binário, máquina e cenário ocioso; não demonstram redução ou economia relativa de RAM. Não foi feita comparação com outro build nem com navegador.
