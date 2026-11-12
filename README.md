# FastTube

Aplicativo nativo para Windows com interface egui/eframe, reprodução embutida via libmpv, consultas de vídeo pelo yt-dlp e descoberta local e pública do YouTube.

## Requisitos

- Rust 1.98 ou superior e toolchain Windows x86-64 (MSVC ou GNU).
- `yt-dlp` disponível no `PATH` ou junto do executável portátil.
- `libmpv-2.dll` ou `mpv-2.dll` e as DLLs auxiliares da distribuição usada.
- Para recursos de conta, um cliente OAuth de aplicativo desktop válido. Recursos públicos e dados locais funcionam sem login.

### Configurar OAuth

1. Crie um cliente OAuth do tipo **Desktop app** no Google Cloud Console.
2. Ative a YouTube Data API v3 para o projeto.
3. Salve o JSON baixado como `%LOCALAPPDATA%\FastTube\oauth-client.json`.
4. Abra o FastTube, clique em **Entrar** e conclua a autorização no navegador.
5. Se o acesso expirar, entre novamente; o FastTube não inclui tokens ou cookies em backups.

Consulte `scripts\build-portable.ps1` para montar uma distribuição. O empacotador não baixa dependências nem inclui credenciais. Respeite as licenças de todos os binários redistribuídos.

## Recursos

- Início público sem login, com descoberta por temas e Feed FastTube explicável. O ranking combina histórico, fila e vídeo em contexto, aplica decaimento temporal, evita excesso de um mesmo canal e mostra o principal motivo de cada sugestão.
- Página **Explorar** com atalhos para tendências, música, jogos, podcasts, transmissões ao vivo, mixes e novidades.
- Inscrições e playlists pela YouTube Data API após login Google. A Home pública e os recursos locais continuam disponíveis quando a cota ou a conexão falham.
- **Gostei** e **Não gostei** enviam a avaliação à conta Google conectada pela YouTube Data API. O estado visual fica disponível durante a sessão.
- Busca de vídeos e reprodução no libmpv. O yt-dlp resolve streams; disponibilidade, formatos e duração dependem do YouTube e do vídeo.
- Shorts usam um palco vertical dedicado, com player centralizado e detalhes do vídeo ao lado em janelas largas.
- Fila FIFO separada e persistida em `queue.json`, limitada a 50 itens. Cada ocorrência recebe uma identidade estável, inclusive quando o mesmo vídeo aparece mais de uma vez. O item só sai da fila após o libmpv confirmar a entrada exata que carregou; falhas e substituições manuais preservam a fila.
- Histórico local com retomada. A Home apresenta até oito vídeos com progresso e permite marcar um vídeo concluído.
- Biblioteca local independente de OAuth em `library.json`, organizada em prateleiras para Assistir depois e Favoritos, além de coleções nomeadas e marcadores com notas.
- Legendas consultadas sob demanda. A aba **Transcrição** pesquisa o WebVTT, acompanha o trecho atual, centraliza os trechos conforme o vídeo avança, permite saltar para qualquer trecho e oferece tamanho de fonte e alto contraste. Limites: 2 MiB por arquivo e 20.000 trechos; o vídeo não é baixado.
- Capítulos oferecidos pelo libmpv e marcadores locais navegáveis durante a reprodução.
- Filtros na página de envios recentes das inscrições: ocultar assistidos, Shorts ou lives, filtrar um canal e ordenar por data quando o yt-dlp fornecer a data.
- Configurações mostram os interesses aprendidos localmente, o volume atual de metadados e um comando para liberar texturas e resoluções em cache sem apagar vídeos ou dados pessoais.
- Mini player redimensiona a janela atual e a mantém no topo. A renderização e os controles continuam na mesma instância; sair restaura o tamanho anterior da janela.
- Diagnóstico classifica problemas de login, cota, conexão e reprodução/dependências, mostra próximos passos específicos e copia um relatório redigido sem texto bruto de erro, URLs, tokens, cookies ou caminhos.
- Backup JSON versão 1 exporta biblioteca e marcadores, fila, histórico e preferências. A importação valida o arquivo, apresenta uma prévia e permite **mesclar** ou **substituir**. Na mesclagem, a biblioteca local prevalece, o histórico mantém o maior progresso e a fila respeita o limite de 50 itens. OAuth, tokens e cookies nunca entram no backup; o arquivo é limitado a 8 MiB.
- Interface alinhada ao YouTube em desktop e janelas estreitas: cabeçalho compacto, rail de 72 px, navegação lateral expansível, filtros com rolagem horizontal e margens de conteúdo consistentes.
- Acessibilidade: foco de teclado mais visível, controles com área de interação ampliada, legenda com alto contraste e guia de atalhos em **F1**.

A análise visual que orientou estas decisões está em [`docs/analise-youtube-ui.md`](docs/analise-youtube-ui.md).

## Atalhos

| Teclas | Ação |
| --- | --- |
| `F1` | Abrir ou fechar o guia de atalhos |
| `Tab` / `Shift+Tab` | Percorrer controles |
| `Enter` ou `Espaço` | Ativar o controle focado |
| `Alt+1` | Início |
| `Alt+2` | Explorar |
| `Alt+3` | Histórico |
| `Alt+4` | Fila |
| `Alt+5` | Biblioteca |
| `Espaço` ou `K` | Reproduzir ou pausar |
| `←` / `→` | Voltar ou avançar 10 segundos |
| `M` | Silenciar |
| `T` | Alternar modo teatro |
| `F` | Alternar tela cheia |
| `Esc` | Fechar tela cheia, painel ou guia aberto |

## Escopo e dados

O FastTube é um cliente desktop independente inspirado nos fluxos de descoberta e reprodução do YouTube; ele não se apresenta como cópia oficial. A consulta de páginas do YouTube é centralizada no `yt-dlp`, sem captura de HTML interno ou coleta indiscriminada do site. Recursos de conta usam OAuth e podem ficar indisponíveis por cota, permissão ou política do provedor. Histórico, fila, biblioteca e perfil de recomendação são locais.

## Armazenamento e recuperação

Dados comuns ficam em `%LOCALAPPDATA%\FastTube`. Fila, histórico, biblioteca e configurações são gravados por arquivo temporário e substituição atômica. Arquivos inválidos são preservados e geram aviso.

Antes de uma importação, o FastTube copia os arquivos afetados para `.fasttube-import-recovery`. Se uma gravação falhar, o rollback é executado imediatamente; se o aplicativo for encerrado no meio do processo, a restauração é concluída na próxima abertura, antes da leitura dos dados.

## Desenvolvimento e validação

Consulte [`docs/validacao-nove-melhorias.md`](docs/validacao-nove-melhorias.md) para comandos e detalhes da toolchain usada nas validações.

Para evitar sobrescrever executáveis em uso, use um target separado:

```powershell
cargo +stable-x86_64-pc-windows-gnu fmt --check
cargo +stable-x86_64-pc-windows-gnu check --target x86_64-pc-windows-gnu --target-dir target\task-validation
cargo +stable-x86_64-pc-windows-gnu test --target x86_64-pc-windows-gnu --target-dir target\task-validation
```

Os scripts `scripts\test-portable-package.ps1`, `scripts\test-measure-process.ps1` e `scripts\test-measure-network.ps1` têm objetivos separados. Medições de desempenho ficam para a etapa final do projeto.
## Mapa do código com Graphify

O projeto possui um grafo estrutural consultável do Rust em `graphify-out/`. Para navegar pelos módulos e rastrear impactos:

```powershell
graphify god-nodes --top 20
graphify explain "Queue" --all
graphify affected "QueuePlaybackCorrelation" --depth 3
Start-Process .\graphify-out\graph.html
```

Consulte [`docs/graphify-fasttube.md`](docs/graphify-fasttube.md) para o fluxo completo, consultas específicas do FastTube e atualização do grafo.
