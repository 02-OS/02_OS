# 02 Agent Runtime roadmap: memory engine and decision sidecar

Phases 2–5 are specified here and are not implemented yet. Phase 1 is the foundation that follows this document.

The runtime is synchronous Rust, SQLite via `rusqlite` (WAL, `busy_timeout`, FTS5), XDG paths under `~/.local/share/02-agent/repos/<repo-id>/`, secrets filtered by `SecretPattern`, errors as `AgentError`. There is no async runtime and no network client. Those facts constrain every phase.

## What already maps

### Memory engine (A)

| Cognee idea | Already in the tree | Gap |
| --- | --- | --- |
| Ingest sources | `02 index` scans with `RepoScanner`, respects `.gitignore`, `RepoConfig`, and `SecretPattern`, then writes `files` + FTS5 | No dataset, no content-hashed `DataPoint`, no history / session / pasted-text ingest |
| Cognify | `CodeExtractor` builds symbols and `calls` / `imports` / `tests` references. `02 index --full` and the incremental path both persist them | No pipeline, no chunks, no general graph, no git `touched-in-commit` edges |
| Memify | `StalenessEngine` compares symbol fingerprints to the working tree and sets memory `Fresh` / `Degraded` / `Stale`. `Invalidated` is preserved | Memories are JSONL claims, not graph nodes. No duplicate merge, summary nodes, triplet index, or prune |
| Search | `02 search` is FTS5 BM25. `02 symbol`, `02 deps`, `02 tests` are exact lookups | No `SearchType`, no graph neighborhood, no triplets, no RRF hybrid, no budget on search |
| Remember / recall | `02 memory list|inspect|verify` and MCP `memory_get` / `memory_write` / `memory_status` | No add+cognify composition, no auto-routed recall, no outcome-driven improve, no scoped forget |
| Graph | `symbol_references` is a flat edge table. Context scoring walks callers one hop in process | No node table, no recursive CTE, no export |
| Budget | `ContextCompiler` + `BudgetManager` (`--budget`, default 8000) | Search and future judgments do not go through it yet |
| Provenance | `EvidenceItem` has file, symbols, commit, optional fingerprint. `IndexedFile` has hash and git blob id | No byte range, source kind, version, or content-hash identity |

`EvidenceMemory` stays the architectural-claim record. It is not replaced by `DataPoint`.

### Decision sidecar (B)

Nothing in the tree judges, gates, or records outcomes. The pieces to reuse later:

- Evidence selection and token budgets: `agent-context`.
- Staleness, test linkage, diff size: `StalenessEngine`, `find_tests_for_symbol`, `GitRepo::get_diff_files` / `get_diff_hunks`.
- Memory confidence: `EvidenceMemory.confidence` and status.
- Secret handling and doctor/security reports: `agent-core::SecretPattern`, `02 doctor`, `02 security`.
- MCP tool registration: `agent-mcp` `list_tools` / `call_tool`. Resources (`02://packs/...`) do not exist yet. The server only implements `initialize`, `ping`, `tools/list`, and `tools/call`.

`agent-judge` is a new crate in Phase 3, not before.

## Schema today

`agent-index` `CURRENT_SCHEMA_VERSION` is `1`. `init_schema` runs `CREATE TABLE IF NOT EXISTS` and inserts the version only when the table is empty. It never upgrades. Each repo has its own `index.sqlite` (so `files.relative_path UNIQUE` is per database, not global). Memories are `memories.jsonl` beside that file, not SQL.

Open path already sets `foreign_keys`, `journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=5000`.

## Crate and module layout

No new crate until Phase 3.

```
agent-core/src/graph_model.rs     DataPoint, Provenance, Dataset, NodeSet, Session,
                                  Ontology, SourceKind, EdgeKind, Task, Pipeline
agent-index/src/schema.rs         v1 SQL kept, v2 tables, migrate()
agent-index/src/graph.rs          upsert nodes/edges, recursive CTE, export JSON/DOT/Mermaid
agent-index/src/datapoint.rs      content-hash dedup, version + supersedes lookup
agent-memory/src/pipeline.rs      add_pipeline, cognify_pipeline, TaskContext, TaskRegistry
agent-cli                         add, cognify, graph export; index delegates
agent-mcp                         tools add, cognify, graph_export
```

`agent-context` is not modified in Phase 1. Budgeted search and “past decisions inside `02 context`” are Phases 2 and 4. Touching the compiler now would mix those behaviors into the foundation commit.

`agent-parser` and `agent-git` stay as they are. Cognify calls them.

Phase 3 adds workspace member `crates/agent-judge`. Phase 2 adds `EmbeddingProvider` and `VectorStore` behind a cargo feature that is off by default. Those traits are not declared in Phase 1, because an unused trait fails `cargo clippy -- -D warnings`.

## Schema version 2 (in place, no drops)

Fresh databases create v1 and v2 tables and store version `2`. An existing version `1` file keeps every current row. Migration only adds tables and indexes, then sets `schema_version` to `2`.

New tables, all keyed by `repo_id` with `ON DELETE CASCADE` from `repositories`:

- `datasets` — `id`, `repo_id`, `name`, `created_at`, `UNIQUE(repo_id, name)`. One default dataset per repo, name `default`, id `dataset:<repo_id>:default`.
- `datapoints` — `id` (content hash, primary key), `dataset_id`, `version`, `kind`, `content_hash`, `content`, `source_kind`, `commit_id`, `path`, `start_byte`, `end_byte`, `symbol_fingerprint`, `confidence`, `created_at`, `updated_at`. Index on `(dataset_id, path, kind, version)`.
- `graph_nodes` — `id`, `dataset_id`, `kind`, `label`, `datapoint_id`.
- `graph_edges` — `src_id`, `dst_id`, `kind`, `payload`, `UNIQUE(dataset_id, src_id, dst_id, kind)`.
- `node_sets` and `node_set_members` — free tags.
- `sessions` — `id`, `dataset_id`, `label`, `ephemeral`, `created_at`.

Edge kinds stored as text: `calls`, `imports`, `tests`, `touched-in-commit`, `owns`, `decided-by`, `supersedes`. The last two are accepted and unused until Phase 4, so that phase does not need another migration to insert them.

`symbol_references` remains the source of truth for `02 deps` and `02 tests`. Cognify projects those rows into `graph_edges`. It does not rewrite the reference table.

Ontology is `.02agent/ontology.toml` next to `RepoConfig`, not a SQL table, so it stays with the repo. Absent file means the built-in edge list above. A present file restricts which edge kinds cognify may write.

## Phase 1 behavior

### Types

`DataPoint` is a versioned, content-hashed record. Identity is SHA-256 of `dataset_id`, `kind`, `path` (or empty), and content bytes. Re-adding the same bytes is a no-op. A new hash for the same path inserts `version + 1` and a `supersedes` edge to the previous id.

`Provenance` is commit, path, byte range, symbol fingerprint, and `SourceKind` (`Worktree`, `GitHistory`, `Document`, `SessionNote`, `PastedText`).

`Task` is a synchronous trait over a `TaskContext` (repo root, git repo, open `IndexDatabase`, dataset, config). `Pipeline` is an ordered list of those tasks plus a typed report at the boundary (`AddReport`, `CognifyReport`). Built-in stages are also generic `Task<I, O>` values wired by the two pipeline functions. A `TaskRegistry` on the context accepts extra `Box<dyn Task>` stages. This is the composable hook the prompt asks for without an HList crate.

### `02 add`

Sources:

- `worktree` (default): `RepoScanner` output, skipping secrets. One file datapoint per indexed file, incremental by content hash.
- `docs`: documentation files (`FileKind::Documentation`, `docs/**`, `*ADR*`). Included in the default run together with the worktree.
- `history`: last 20 commits from `GitRepo::get_recent_commits`, as commit datapoints, plus path links for `touched-in-commit` during cognify. Opt-in.
- `session`: a row in `sessions` plus a note datapoint. Requires `--text`.
- `text`: a pasted datapoint. Requires `--text`.

`--dataset` selects or creates a named dataset. `--full` ignores the hash short-circuit and rewrites datapoints. `--json` matches the rest of the CLI.

### `02 cognify`

Deterministic. No model trait and no feature flag.

1. Ensure symbols and references exist by running the same extractor `02 index` uses, only for files whose hash changed since the last symbol write (or all of them with `--full`).
2. Upsert a graph node per file datapoint and per symbol.
3. Project `symbol_references` into `calls`, `imports`, and `tests` edges. `owns` edges run from each file node to its symbols.
4. Chunk source and docs into 80-line windows with 10 lines of overlap. Each chunk is a datapoint (byte range set) and an `owns` edge from the file.
5. For `history`, add `touched-in-commit` edges from commit nodes to file nodes using the commit’s changed paths.
6. Drop edges whose kind the ontology disallows.

Secret files never become datapoints, chunks, or nodes.

### `02 graph export`

`--format json|dot|mermaid`, optional `--output`. JSON is `{ "nodes": [...], "edges": [...] }`. DOT is a digraph. Mermaid is `flowchart LR`. Node labels are escaped.

### `02 index`

Same flags (`--full`, global `--json`) and the same stdout fields (`files_updated`, totals, staleness, duration). Implementation calls worktree+docs add, then cognify, then the existing `StalenessEngine::evaluate`. It does not print a second report.

### MCP

`add`, `cognify`, and `graph_export`, each with optional `repo_path` like the other tools. Existing tool names and schemas stay. `graph_export` returns the document as text.

### Traversal

`IndexDatabase::neighbors(dataset, start, depth)` uses `WITH RECURSIVE` and refuses depth above 8. Default depth for callers is 2. Cycle-safe via a visited set in the CTE.

### Tests and benchmark

- Open a temp file initialized with the v1 SQL and one `files` row, then `IndexDatabase::open`. The row remains, version is 2, `graph_nodes` exists.
- Adding the same file twice does not create a second datapoint. Changing the bytes does, with a `supersedes` edge.
- Cognify on a tiny git repo produces a `calls` or `owns` edge that traversal returns.
- Export contains the symbol name in all three formats.
- A `.env` file is not a datapoint.
- `02 index` library path still updates `files` and runs staleness (covered by the pipeline test, not a new CLI harness).

`benchmarks/benchmark.sh` gains three timings after the existing eight: `02 add`, `02 cognify`, and `02 graph export --format json`. The script already builds via `scripts/build-agent-runtime.sh` when the staged binary is missing. No new benchmark dependency.

Phase 1 is done only when `cargo fmt`, `cargo clippy --workspace -- -D warnings`, and `cargo test --workspace` pass offline from `components/02-agent`.

## Deviations from the prompt

1. Pipelines are synchronous. The workspace has no async runtime. Adding tokio for `Task` would be a new convention.
2. Heterogeneous typed stages are wired explicitly inside `add_pipeline` and `cognify_pipeline`. A registry of `Task` objects covers custom stages. A typestate HList is not worth a crate.
3. `SearchType`, vector search, and `EmbeddingProvider` wait until Phase 2. Declaring them now is unused code under `-D warnings`.
4. `agent-context` is unchanged in Phase 1. Search budgeting and decision inclusion belong to Phases 2 and 4.
5. `EvidenceMemory` stays JSONL. Moving it into SQLite would be a second store migration and would rewrite `02 memory`. DataPoints are a new table, not a replacement.
6. Ontology is `.02agent/ontology.toml`, beside `config.toml`, not a SQL blob.
7. `02 add` with no `--source` ingests the working tree and docs, not git history. History is `--source history` and capped at 20 commits. Full blob replay of every revision is out of scope.
8. `decided-by` rows are not written in Phase 1. The edge kind is legal so Phase 4 does not migrate again.
9. Sessions are not auto-deleted. Nothing in Phase 1 runs an unscoped delete. Expiry is a column; `forget` in Phase 2 is the only deleter.
10. The `files` table is not rebuilt. Its `UNIQUE` path constraint stays. New tables use composite keys.
11. Graph export and cognify read the projection in `graph_edges`. `02 deps` / `02 tests` keep reading `symbol_references`, so those commands cannot change behavior.
12. Phase 1 is based on `fix/open-issues-16-27` (PR #28), which holds the current runtime fixes, rather than an older `master`.

## Later phases

**Phase 2.** `memify` (merge duplicate nodes, summary/rule nodes, triplet table, staleness prune). `search` for `SYMBOLS`, `CHUNKS`, `GRAPH_NEIGHBORHOOD`, `TRIPLETS`, `MEMORIES`, and `HYBRID` with reciprocal rank fusion, clipped by `ContextCompiler` / `--budget`. `remember`, `recall`, `improve`, `forget` with an explicit scope. CLI and MCP names match. `VectorStore` brute-force cosine and `EmbeddingProvider` behind a default-off feature. Retrieval uses FTS5 when the feature is off.

**Phase 3.** New crate `agent-judge`. `Choice` and `Score` judgments, versioned TOML packs (built-in plus `$XDG_CONFIG_HOME/02-agent/packs`), MCP resources `02://packs/<name>`. Evidence bundles reuse `agent-context`; truncated evidence cannot be `auto`. Pure policy function to `auto | review | escalate` from config. Default `HeuristicJudge` uses the symbol graph, staleness, tests, diff size, and memory confidence. SQLite ledger and `02 budget` hard stops. CLI `02 judge choose|score|review|gate` and MCP `judge_choose`, `judge_score`, `judge_review`, `judge_gate`.

**Phase 4.** `02 outcome record` / `record_outcome` and `02 outcomes report`. Each judgment and outcome becomes a `DataPoint` with `decided-by` / `supersedes` edges. `improve` moves confidence from those outcomes. `02 context` may include a past decision only when it is fresh and inside the budget. Optional `local-model` and `remote-jev` features, and `llm-extract`, all off by default. `RemoteJevJudge` reads its key through `SecretPattern` paths. `02 doctor` and `02 security` print a clear warning when `remote-jev` is compiled in and configured. Reflex controllers stay last and are skipped if the rest is not solid.

**Phase 5.** `docs/agent-runtime-memory.md`, `docs/agent-runtime-judgment.md`, architecture and security updates (threat model includes the optional network backends), a README section, and an offline MCP smoke test: remember, recall, judge_gate, record_outcome, improve.
