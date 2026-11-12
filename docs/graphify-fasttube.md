# Graphify no FastTube

O Graphify mantém um mapa consultável do código em `graphify-out/`. Ele ajuda a localizar dependências entre os módulos, rastrear fluxos entre tipos e funções e estimar o impacto de uma alteração. Os resultados do grafo orientam a investigação; o código-fonte continua sendo a fonte de verdade para detalhes de implementação.

## Estado atual

O grafo estrutural foi gerado com:

```powershell
graphify extract . --code-only --cargo
graphify cluster-only . --no-label
```

Resultado atual: `1297` nós, `3679` relações e `68` comunidades. O build inicial possuía `1219` nós, `3436` relações e `52` comunidades. A execução estrutural usa AST e não consome tokens de LLM. A documentação não faz parte desse grafo porque foi usada a opção `--code-only`.

Arquivos principais:

- `graphify-out/graph.html`: grafo interativo.
- `graphify-out/GRAPH_REPORT.md`: relatório de comunidades, hubs, conexões inferidas e lacunas.
- `graphify-out/GRAPH_TREE.html`: visão hierárquica por arquivo e símbolo.
- `graphify-out/callflow.html`: diagramas de fluxo de chamadas em Mermaid.
- `graphify-out/graph.json`: dados brutos para consultas e exportações.

Abra as visualizações com:

```powershell
Start-Process .\graphify-out\graph.html
Start-Process .\graphify-out\GRAPH_TREE.html
Start-Process .\graphify-out\callflow.html
```

## Fluxo recomendado

Comece pelos hubs arquiteturais:

```powershell
graphify god-nodes --top 25
```

Depois escolha a consulta conforme a pergunta:

```powershell
# Explicar um componente e suas ligações
graphify explain "RecommendationEngine" --all

# Descobrir o impacto provável de uma mudança
graphify affected "QueuePlaybackCorrelation" --depth 3

# Traçar o menor caminho entre dois conceitos
graphify path "src/queue.rs::Queue" "src/player.rs::Player" --undirected

# Explorar uma área com filtros de relação
graphify query "playback queue cancellation" `
  --seed src_queue_queue `
  --seed src_player_player `
  --context calls `
  --context references `
  --depth 2 `
  --budget 2000
```

O grafo extrai relações reais do Rust e também produz algumas relações semânticas. Confirme toda relação marcada como `INFERRED` no código antes de tomar uma decisão arquitetural.

## Consultas úteis para o FastTube

Pesquisa, canais e recomendações:

```powershell
graphify explain "ChannelTarget" --all
graphify explain "Video" --all
graphify explain "RecommendationEngine" --all
graphify query "channel search metadata cache pagination" --depth 2 --budget 1800
```

Fila, reprodução e libmpv:

```powershell
graphify explain "Queue" --all
graphify explain "Player" --all
graphify affected "QueuePlaybackCorrelation" --depth 3
graphify path "src/queue.rs::Queue" "src/player.rs::Player" --undirected
```

OAuth, diagnósticos e armazenamento local:

```powershell
graphify explain "TokenSession" --all
graphify explain "FailureGuidance" --all
graphify query "OAuth quota network diagnostics cancellation" --depth 2 --budget 1800
```

Documentos e scripts de medição não estão no grafo estrutural atual. Para relacioná-los semanticamente ao código, configure um backend de LLM e execute um build completo. Isso pode consumir tokens.

## Como resolver ambiguidades

O mesmo nome pode existir na interface e em um módulo. Por isso, `Queue` retorna mais de um nó. Use o resultado de `graphify explain`, `graphify path` ou `graphify query` para obter o `id` completo, ou qualifique com o arquivo:

```powershell
graphify path "src/queue.rs::Queue" "src/player.rs::Player" --undirected
```

Se uma consulta trouxer nós demais, reduza `--depth`, limite o `--budget` e use `--seed` com IDs exatos. Para perguntas sobre dependências de uma alteração, prefira `affected`; para ligação entre dois pontos conhecidos, prefira `path`.

## Atualização

Depois de alterações comuns no Rust:

```powershell
graphify update .
```

Depois de remover módulos, funções ou arquivos e quando o grafo ficar maior que o estado atual:

```powershell
graphify update . --force
```

Depois de uma alteração estrutural ampla, se as comunidades ficarem desatualizadas:

```powershell
graphify cluster-only . --no-label
graphify tree --output graphify-out/GRAPH_TREE.html --root . --label FastTube
graphify export callflow-html . --output graphify-out/callflow.html --lang auto
```

Durante uma sessão ativa de desenvolvimento:

```powershell
graphify watch .
```

O diretório `target*` e `graphify-out/` estão excluídos por `.graphifyignore`. O checkout atual não é um repositório Git, então os hooks automáticos do Graphify não se aplicam. Em um checkout Git, eles podem ser instalados depois com `graphify hook install`.
## Consultas registradas

- [`consulta-graphify-fasttube.md`](consulta-graphify-fasttube.md): diagnóstico do núcleo `FastTube`, acoplamentos, pontos de propagação e evolução estrutural.
- [`refatoracao-paralela-ui.md`](refatoracao-paralela-ui.md): resultado da rodada paralela, commits, modelos e próximos alvos.
