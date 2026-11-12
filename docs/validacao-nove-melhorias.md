# Validação das sete melhorias — 11/11/2026

A rodada foi executada no workspace `D:\Fastube`, sem medições de desempenho. A toolchain GNU descrita anteriormente foi reutilizada: WinLibs GCC-only x86_64, GCC 16.2.0, MinGW-w64 14.0.0, POSIX/SEH, MSVCRT, extraída em `target/toolchains/winlibs-gcc-16.2.0-r3`. O Rust foi usado nas toolchains `stable-x86_64-pc-windows-msvc` para rustfmt e `stable-x86_64-pc-windows-gnu` para compilar/testar o alvo `x86_64-pc-windows-gnu`. Os compiladores C e seletores de linker foram configurados apenas no processo de validação.

## Testes adicionados

- Fila: migração de v1, preservação de IDs em v2 e remoção da ocorrência exata após reordenação com URLs duplicadas.
- Backup/importação: rollback de arquivo existente e remoção de arquivo criado durante importação; recuperação de um ponto pendente simulando interrupção antes do commit.
- Biblioteca e histórico: mesclagem sem perda dos registros locais válidos; histórico conserva progresso local maior e incorpora entrada importada ausente.
- Transcrição: índice de cue ativa em limites, sobreposição e lacunas.
- Diagnósticos: próximos passos acionáveis sem refletir domínio ou parâmetros da URL fornecida.

## Comandos executados

A configuração usada no PowerShell para cada comando GNU foi:

```powershell
$bin = Join-Path (Get-Location) 'target\toolchains\winlibs-gcc-16.2.0-r3\mingw64\bin'
$env:PATH = "$bin;$env:PATH"
$env:CC_x86_64_pc_windows_gnu = Join-Path $bin 'gcc.exe'
$env:CXX_x86_64_pc_windows_gnu = Join-Path $bin 'g++.exe'
$env:AR_x86_64_pc_windows_gnu = Join-Path $bin 'ar.exe'
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $bin 'gcc.exe'
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_AR = Join-Path $bin 'ar.exe'
```

Os comandos abaixo foram executados na raiz do projeto, usando `target/seven-improvements-validation` como `target-dir` isolado:

```powershell
cargo +stable-x86_64-pc-windows-msvc fmt
cargo +stable-x86_64-pc-windows-msvc fmt --check
cargo +stable-x86_64-pc-windows-gnu check --target x86_64-pc-windows-gnu --target-dir target/seven-improvements-validation
cargo +stable-x86_64-pc-windows-gnu test --target x86_64-pc-windows-gnu --target-dir target/seven-improvements-validation
cargo +stable-x86_64-pc-windows-gnu build --release --target x86_64-pc-windows-gnu --target-dir target/seven-improvements-validation
```

`fmt` e `fmt --check` passaram. `cargo check` passou com avisos já existentes de imports/variáveis não usados e código morto; sem erros de compilação. `cargo test` descobriu 162 testes: 161 passaram, 0 falharam e 1 foi ignorado. A suíte inicial encontrou um teste antigo que esperava 52 px quando `compute_header_height` retorna 56 px; a expectativa do teste foi alinhada ao valor implementado e a rodada final passou. O build release passou; executável gerado em `target/seven-improvements-ui-validation/x86_64-pc-windows-gnu/release/fasttube.exe`.

## Complemento da validação de UI — 11/11/2026

- Três testes headless com `egui::Context::run_ui` exercitam a abertura/renderização da prévia de importação, o estado do guia de atalhos por F1/Esc e a renderização da aba de transcrição sem panic.
- A prévia permanece pendente, com modo inicial `Merge`, sem `pending_import` e sem alteração de biblioteca, histórico ou tamanho da fila antes de confirmar.
- O tratamento de F1/Esc foi isolado em helper estreito, preservando o bloqueio quando egui está capturando teclado.
- `cargo +stable-x86_64-pc-windows-msvc fmt` e `fmt --check`: passaram.
- `cargo +stable-x86_64-pc-windows-gnu check --target x86_64-pc-windows-gnu --target-dir target/seven-improvements-ui-validation`: passou.
- `cargo +stable-x86_64-pc-windows-gnu test --target x86_64-pc-windows-gnu --target-dir target/seven-improvements-ui-validation`: 162 descobertos, 161 passaram, 0 falharam, 1 ignorado.
- `cargo +stable-x86_64-pc-windows-gnu build --release --target x86_64-pc-windows-gnu --target-dir target/seven-improvements-ui-validation`: passou.
- Smoke nativo oculto do executável release: `WaitForInputIdle(15000)` retornou `True`; o processo permaneceu aberto por 3 segundos e foi encerrado pelo teste. Dados isolados em `target/seven-improvements-ui-validation/smoke-localappdata`.
- Não foram feitas medições de desempenho nem consultas externas durante a rodada.

## Limitações

- O smoke test confirma criação da janela e permanência inicial, não valida reprodução com libmpv/dispositivo nem experiência visual via inspeção de pixels.
- A inicialização padrão dispara carregamento automático do feed público. O smoke test não adicionou chamadas de rede, mas não usou isolamento de rede; portanto não se pode afirmar que o processo não tentou acesso externo. Não houve novo lançamento após constatar esse comportamento.
- Os testes headless verificam produção de formas egui e estado do app; não validam conteúdo textual por pixel nem interação de mouse em cada opção.
- As verificações cobrem o comportamento das funções focadas, mas não simulam falhas físicas de disco em cada etapa de gravação.
- `cargo check`, testes e build emitem avisos de código não usado fora do escopo dos testes desta rodada; foram mantidos sem alterações amplas.
- Nenhuma medição de desempenho foi realizada.
