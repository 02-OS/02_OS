# Agent runtime: ranking, facts, and local routing

This note describes the modules added beside the schema 5 catalog. `memories.jsonl` is still the evidence-memory record. `compile` is still the path that redacts a context package and enforces its token budget.

## Facts

`agent-memory` stores git-anchored facts in `facts.sqlite` next to the memory log. A fact names a commit, a path, and either a diff line or a parsed symbol. The claim does not copy hunk text. Secret-named paths are skipped. Confidence decays with a 30-day half-life and does not fall below `0.05` or rise on its own. `fact_decay` writes that value back. Readers also see the decay that has accrued since the last write.

## Ranking

`score_file` adds five signals:

| Signal | Value |
| --- | --- |
| Exact symbol match | `+10` each, capped at 3 |
| Partial symbol match | `+7` each, capped at 3 |
| Test proximity | `6` for a matching test file, `4` when a test references a symbol in the file, `2` for a farther test link |
| Recency | `+1` per recent touching commit, capped at 4 |
| Architectural importance | `5` for `main.rs`, `lib.rs`, or `mod.rs`; `3` for a manifest; `1` for documentation |
| Stale index | `-3` |

`allocate` keeps higher scores first and still accepts a later item that fits the leftover budget. `ContextCompiler::rank` builds those scores from the index. `ContextCompiler::compile` is unchanged.

## Tools

`nav_tools` adds 29 MCP tools for files, symbols, tests, git history, import and call edges, context dump, ranking, workspace discovery, and facts. `run_tests` accepts one filter of letters, numbers, `_`, `:`, `-`, and `.`, then runs `cargo test --offline --locked <filter>` in the repository. It does not pass the filter through a shell. Output is truncated and redacted. The run stops after 20 seconds.

## Workspace

`discover_workspace` returns the repository at the root and child directories that contain `.git`, up to 32. A child whose canonical path leaves the root is omitted.

## Benchmark skeleton

`benchkit` records elapsed time, token efficiency, and context recall. The timing sample is ignored. It is not a release gate.

## Model routing

`ModelRouter` names a Unix socket or a loopback HTTP port. The default route is absent. `https` hosts, non-loopback hosts, and relative sockets are rejected. This crate does not open the socket and does not load model weights. The model process stays outside the runtime.

## Ordering, projection, and the trust boundary

`order_compiled_files` keeps the compile score as the first key. The fixture in `order.rs` locks that: a file scored 20 stays ahead of a file whose rank total is 100. Rank breaks ties, then the path does. `compile` calls it after `rank`.

Decayed facts are copied into `memory_index` with ids prefixed by `fact:`. `jsonl_offset` stays null. `memories.jsonl` is still the evidence record.

`admit_approval` rejects an invoker that is also the approver, and it rejects source `mcp`. A stored allow row has to come from `agt-cli`, `gnome`, or `02-cli` and carry a 64-character hex args hash. This runtime does not recompute that hash. The MCP server has no approve tool; a call named `approve` returns an error before it opens the repository.

`ingest_evidence` stores a package only when `package_digest` is the SHA-256 of `package_json`, the contract digest is non-empty, and the outcome is not an applied ask. A second copy of the same digest is a no-op. A different digest for the same run is rejected.

`02 index` publishes `idle`, `indexing`, `complete`, or `stale` to `~/.local/share/02-agent/index-phase`. The user-session extension `02-index-phase@02os` shows that line on GNOME Shell 51. It is separate from the login portal. A live Shell session was not started here.

`02-agentd` answers `daemon/status` with that phase, the watched repository count, and approval rows already stored. It writes the same object to `~/.local/share/02-agent/status.json` mode `0600`. `daemon/decide` records a decision only when `confirmed_hash` matches the displayed `args_hash`, and it always stores source `gnome`. It does not recompute the hash. The session extension `02-agent-status@02os` reads the file and sends that decision. A live Shell session was not started for this extension either.

`02-desktop` is a separate MCP process and is not started by the session. `desktop-consent` must be mode `0600` and contain `allow`. `desktop-kill` refuses every tool call. The session menu writes that kill file. The process probes a local session bus and then returns an error: it does not link an AT-SPI toolkit, send input events, or open a network connection. Tests use an in-memory accessibility tree.

The `embeddings` feature is off by default. With the feature on, `semantic_hits` still returns nothing, including when `file_vectors` has rows, and `search_content` uses FTS5. No model weights are loaded.

## Still open

The foundation contract C1–C8 is not closed. In particular, this change does not pick the canonical line, replace the published ISO, or prove an agt process checked a live args hash. The next modules are a score-table fixture for the primary compile weights, writers that do not treat `memory_index` as memory, an agt ingest path outside this process, and a model process that can fill `file_vectors` without entering this binary.
