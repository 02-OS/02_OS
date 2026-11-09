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

## Next modules

1. Teach `compile` to order files with `rank` after a fixture locks the order the current tests expect.
2. Write decayed fact rows into `memory_index` without making SQL the memory authority.
3. Reject MCP self-approval. Keep `agt` as the process that checks an args hash.
4. Ingest an agt evidence package only when the digest matches, and reject an empty contract or an applied ask.
5. Show `completed_index_state.phase` on a GNOME Shell 51 session.
6. Add an off-by-default embedding feature that still falls back to FTS5.
