# Pull request testing and release readiness

Every pull request runs three jobs: `check` (Rust formatting, strict Clippy, and
the workspace tests), `os-profile` (ISO-overlay validation), and `runtime-smoke`
(the built CLI and MCP process against disposable repositories).

Configure the repository ruleset or branch protection for `main` (or `master`
where it remains the default branch) to require
**all three check names** before merging. Workflows running on a pull request
do not themselves prevent a merge. Keep the existing review requirement and
require branches to be current with the protected branch if that matches the project policy.
The test PR does not change repository settings.

## Running the checks locally

```bash
cd components/02-agent
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
cd ../..
python3 -m unittest discover -s scripts/tests -v
```

Install `desktop-file-utils` and the packages providing `glib-compile-schemas`
and `dconf` before running the profile and installer checks. On Ubuntu these
are `desktop-file-utils`, `libglib2.0-bin` and `dconf-cli`; the installer regression
uses the real `dconf compile` to verify regenerated desktop defaults.
Set `AGENT_BINARY` to an absolute path to a
freshly built `02` executable when using a different Cargo target directory.
Unix-socket Rust tests need an environment that permits temporary sockets.
The smoke tests isolate XDG storage, use synthetic data and temporary Git
repositories, and never run the installer or root customization.

## Coverage now

- Initialization, full indexing, and repeated incremental indexing.
- Removing obsolete symbols on source replacement and file deletion.
- Private index permissions and excluded sensitive filenames.
- MCP stdio initialize, tool discovery, tool execution, notifications,
  malformed JSON, unknown tools, and ping.
- A real daemon process, private Unix socket, watch registration, and MCP
  routing to a watched repository distinct from the process working directory.
- Named dataset ingestion, repeated text ingestion, and JSON/DOT exports.
- Nonzero errors outside a repository.
- Shell and Python syntax, package manifest consistency, SVG/schema XML,
  strict GLib schema and dconf database compilation, desktop entries, executable permission
  mappings, and the runtime alias.
- Browser verification without a transaction, target-only archive staging,
  success cleanup, failure retention, repeated hooks, and symlink rejection.
- Pop Shell title changes on Wayland/X11 and maximization API contracts for
  the declared GNOME versions, using semantic API fixtures.
- Build profile copying with bytecode exclusion, preserved offline payload,
  symlink and work-directory guards, and concurrent build rejection.
- Greeter account filtering and the live boot dependency ordering.
- Completed-index state, repeat initialization, untracked content, nested ignores,
  daemon startup after automatic registration, and persistent watch opt-outs.
- Actual-source build manifests, cross-checkout work locking, clean-release
  provenance, and GNOME/Mutter/Archinstall compatibility rejection.

These checks complement the existing workspace tests. They are not evidence
that an ISO boots or that an installed system works.

## Regression tests required with audit fixes

For each issue, add a test that fails before its fix and passes afterward.
Use harmless fixture markers rather than personal files or live credentials.

- Repository boundaries: symlinked files and config, absolute paths, `..`
  evidence paths, and file replacement between validation and opening.
- Privacy: sensitive content must stay redacted in FTS, symbols, graph export,
  CLI context, and MCP context; tracked-but-ignored sensitive files must still
  trigger the security audit.
- Index lifecycle: content A → B → A, rename/delete/re-add, `#` in filenames,
  two datasets sharing a commit, disabled docs/tests, and exact ignore rules.
- Unicode and budgeting: non-ASCII comments, malformed source, long single
  lines, small/zero budgets, and limits on the serialized context package.
- Memory evidence: match the exact file and symbol, remove a function without
  deleting its file, and exclude invalidated/stale memories from verified
  context.
- Daemon: first startup, restart, untracked-only changes, edits within an
  untracked directory, concurrent watch registrations, and a failed sync
  followed by a successful retry. Poll for a state change with a deadline
  instead of relying on fixed sleeps.
- CLI contracts: every successful `--json` command emits one parseable JSON
  document, and subprocess failure/missing records return a nonzero exit code.
- Build safety: reject dangerous work paths using a mocked Docker command and
  disposable directories, and verify extension imports resolve inside the
  staged profile.

## ISO and installed-system acceptance gates

Run these on a dedicated disposable VM runner, because the ISO build requires
privileged Docker. Never run the installer against the runner's host disks.

Use the canonical installed-VM test credentials in
[`VM_TESTING.md`](VM_TESTING.md) and [`scripts/vm-test-credentials.json`](../scripts/vm-test-credentials.json).
Update reused test snapshots to that password before testing authentication.

1. Build using `./build.sh` from the exact commit. That is the only supported
   image build: it records the commit, checks the staged runtime against the
   source build, and refuses a direct `mkarchiso` that lacks that stamp.
   Record the package manifest and checksum the ISO.
2. Boot BIOS and UEFI with both a software-rendered GPU and accelerated graphics.
   Require login within a bounded deadline and capture serial logs and the
   journal as artifacts on failure.
3. Check `systemctl --failed`, the user daemon, GNOME Shell, enabled extensions,
   dock, launcher, terminal, network/DNS, audio, and package mirrors.
4. Test the live-user password, root lock, sudo authentication, default SSH
   policy, and index directory/database permissions in the built image.
5. Install to a disposable virtual disk, remove the live ISO, and reboot the
   disk. Assert the resulting desktop, branding, runtime, services, and normal
   account privileges match the distribution's promises.
6. Perform repeated boot/shutdown, update/reboot, sleep/resume where supported,
   and a long-running indexing workload. Publish measured pass/fail results
   by firmware/GPU configuration instead of inferring OS stability from unit
   tests.

Run quick validation on every PR. Make ISO/boot tests required before merging
changes to packaging, boot/session scripts, systemd units, or build/release
scripts once a dedicated runner is available. Add a periodic full VM run to
catch rolling Arch package changes, and require the complete VM suite before
publishing a release. Add dependency-advisory and secret scanners as separate
jobs with a documented policy for exceptions.

The repository-owned Docker/QEMU acceptance runner is documented in
[`scripts/vm/README.md`](../scripts/vm/README.md). It creates standalone test
disks, verifies ISO completion manifests, captures native screenshots and audio,
and checks the sole guest disk's serial/size before invoking the supported
installer. BIOS live boots and UEFI installation are separate recorded runs.
