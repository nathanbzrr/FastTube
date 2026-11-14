# Graphify Code Graph

FastTube uses Graphify to maintain a local structural AST graph of the Rust codebase in `graphify-out/` (gitignored).

## Generate or Refresh the Graph

After cloning or making structural changes:

```powershell
graphify update . --force
graphify cluster-only . --no-label
graphify tree --output graphify-out/GRAPH_TREE.html --root . --label FastTube
graphify export callflow-html . --output graphify-out/callflow.html --lang auto
```

For incremental updates after routine Rust edits:

```powershell
graphify update .
```

## When to Use vs. Skip Graphify

To minimize token consumption and latency, decide per prompt whether to invoke `graphify`:

- **Use Graphify** when:
  - The prompt is conceptual or open-ended (e.g., *"where is X handled?"*, *"how does Y interact with Z?"*) -> use `graphify query "<terms>" --depth 2 --budget 1800`.
  - Analyzing impact before refactoring shared types or state -> use `graphify affected "<Symbol>" --depth 3` or `graphify path`.
  - Navigating `src/main.rs` (~16k lines) when the exact symbol or line range is unknown -> use `graphify explain "<Symbol>"`.
- **Skip Graphify** when:
  - The prompt already specifies the target file, struct, function, or UI label.
  - Making a localized edit, fixing a compiler/clippy diagnostic at a known line, or running tests.
  - A single targeted symbol search + `view_file` slice is sufficient.

## Useful Queries

Start from high-degree hubs, then narrow down to specific symbols or paths:

```powershell
graphify god-nodes --top 25
graphify explain "RecommendationEngine" --all
graphify affected "QueuePlaybackCorrelation" --depth 3
graphify path "src/queue.rs::Queue" "src/player.rs::Player" --undirected
graphify query "OAuth quota network diagnostics cancellation" --depth 2 --budget 1800
```

If a symbol name is ambiguous across UI and domain modules, qualify it with its file path (for example, `"src/queue.rs::Queue"`). Always verify any relationship labeled `INFERRED` directly in the source files.

## Generated Artifacts (`graphify-out/`)

- `graph.html` — interactive graph explorer
- `GRAPH_TREE.html` — hierarchical file and symbol view
- `callflow.html` — Mermaid call-flow diagrams
- `GRAPH_REPORT.md` — community and hub summary
- `graph.json` — raw graph data
