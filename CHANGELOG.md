# Changelog

## Unreleased

Pull requests merged in this run:

- 02_OS #121 opens a current catalog without taking a write lock.
- 02_OS #122 adds `daemon/status`, a private `status.json`, and the session extension `02-agent-status@02os`.
- 02_OS #123 adds desktop automation that stays off until a private consent file says `allow`. A kill file refuses every call. The live accessibility backend returns an error.
- 02_OS #124 binds Super+G to float, Super+I to Settings, and leaves Super+L on the screensaver. Pop Shell stays the tiler.
- 02_OS #125 installs the developer package list only when `02os-install --developer` is passed. It adds the bootc scaffold, the dispatch-only image workflow, and the published sha256 sidecar check.
- 02_OS #126 loads `SKILL.toml` and stores a capability token only as a hash.
- 02_OS #127 serves `02 dashboard` on `127.0.0.1`.
- 02_OS #128 adds GitHub issue and pull request templates and the `docs/site/` page source. Nothing was deployed.
- agt #2 lets a local policy file only tighten an allow.
- agt #3 adds baseline, acceptance, and property checks. Fixture trees stay byte-for-byte.
- agt #4 records runs, reads an Agent Runtime evidence package, and appends a private audit log.

After #125 merged, `df -BG` on `/dev/nvme0n1p2` showed 468G total, 441G used, and 4G available. That is under the 40 GiB host check, so `./build.sh` was not run. No new ISO path was produced. The retained images `2026.09.26`, `2026.10.01`, and `2026.10.06` were not deleted. The image was not booted.

The product-notes commit that adds this file is the last pull request of the run.
