# Release checklist

Run these from a clean checkout before merging a release pull request. `PATH` starts with `/usr/bin:/bin`.

```bash
cargo fmt --check --manifest-path components/02-agent/Cargo.toml
cargo clippy --offline --locked --workspace --all-targets --manifest-path components/02-agent/Cargo.toml -- -D warnings
cargo test --locked --workspace --manifest-path components/02-agent/Cargo.toml
cargo build --locked --workspace --manifest-path components/02-agent/Cargo.toml
AGENT_BINARY=components/02-agent/target/debug/02 python3 -m unittest discover -s scripts/tests
```

`cargo test` does not leave `target/debug/02`. The Python suite needs the binary from `cargo build`.

## Timing samples

`indexing_and_ranking_skeleton` and `benchmark_budget_pruning` stay ignored. They are not a release gate. The bounded check that does run is `efficiency_and_recall_are_bounded`. Do not remove `#[ignore]` from the two timing tests to make a release look faster or slower.

## Image

- Confirm free space with `df -BG` before `./build.sh`. Skip the build when less than 40 GiB is free, and write down the number, the device, and that the build did not run.
- Do not delete the ISOs dated `2026.09.26`, `2026.10.01`, or `2026.10.06`.
- Do not boot an image as part of this checklist unless a machine is actually reserved for it. Record boot-not-run when it was not booted.
- Do not publish a boot-time measurement from this checklist.
- `.github/workflows/image.yml` is `workflow_dispatch` only. A pull request must not start an image build.
- `scripts/release.sh` tags and uploads. Do not pass `--force-tag` unless the intent is to replace an existing tag. This run did not invoke `scripts/release.sh`.
- A published sha256 sidecar is optional. When the file exists it must match the ISO digest and the ISO name.

## Appearance and network

- The Capitaine string stays `capitaine-cursors` in the dconf defaults and the GTK settings.
- `02-zero-portal@02os` stays the shell motion. Do not retune it in a release that is not about that motion.
- Do not change DNS or MX for `02os.xyz`.
- Do not add telemetry or a mandatory account.

## Scope that stays open

A green checklist does not close the foundation contract C1–C8. It does not prove a live GNOME Shell 51 session. It does not prove a booted ISO.
