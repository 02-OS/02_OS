# Migration

## Catalog

The catalog schema authority is `schema_version`. The current version is 5. A catalog that is already at version 5 opens without a write transaction. Do not bump the schema for a feature that does not migrate rows. A newer schema than the binary knows is refused before any DDL.

## Memory

`memories.jsonl` remains the evidence-memory record. `facts.sqlite` is a sidecar. Projections in `memory_index` use `fact:` ids and a null `jsonl_offset`. Writers must not treat that projection as the memory itself.

Context compilation still uses the numeric weights, redaction, secret-path exclusion, and the token budget. Rank is a tie-break after the compile score, then path.

Search uses FTS5. The embeddings feature stays off. `semantic_hits` returns no rows even when `file_vectors` has rows.

## Approvals and evidence

Stored approvals have no pending column. A status read does not approve. A GNOME-sourced decision is inserted only when the confirmed hash matches the displayed args hash, ignoring ASCII case. The source is forced to `gnome`. An MCP approver is rejected.

An evidence package is accepted only when its digest matches. An empty contract and an applied ask are rejected.

## Desktop and install

Existing installs keep Pop Shell, Capitaine, and the Zero Portal. New keybindings are Super+G and Super+I. Super+L is still the screensaver. Super+T and Super+F are unchanged.

An install that does not pass `--developer` does not gain the developer package list. Browser packaging is the same pipeline as before.

## Images

This run did not replace a published download. The pages in `docs/site/` point at the latest GitHub release. The three retained ISO dates stay on disk. Building a replacement requires 40 GiB free and a local `./build.sh`. `scripts/release.sh` was not run. Do not pass `--force-tag` to replace an existing tag unless that replacement is the intended release.

## agt

agt is a separate repository. A policy file may only tighten an allow into `require_approval` or `deny`. It does not replace `Decide()`. Approving a run still uses `agt approve RUN COMMAND --args-hash HASH`. agt checks that hash. It does not recompute it during approval.
