# Launch outline

02_OS is a local-first GNOME system. `ID_LIKE=arch` stays. There is no telemetry and no mandatory account. The operating system name stays 02_OS.

## What this run shipped

- The agent daemon reports index phase, watched repositories, and stored approvals. It does not approve from a status read.
- The session panel shows that status. A confirmed decision must repeat the displayed args hash.
- Desktop automation is off by default and fail-closed. CI uses a fake accessibility tree.
- Pop Shell remains the tiler. Super+T opens the terminal, Super+F opens Files, Super+I opens Settings, Super+G toggles floating, and Super+Y toggles tiling. Super+L stays the screensaver.
- `02os-install --developer` is the only path that adds gcc, make, pkgconf, gdb, and strace. The default install stays the desktop profile.
- A skill file names a version and known tools. A capability token is stored as a hash.
- `02 dashboard` reads the local status file on `127.0.0.1`.
- The public page source is `docs/site/`. Publishing those pages is a separate step. This run did not change DNS or MX for 02os.xyz.

agt, in its own repository, can tighten an allow, check baseline and acceptance results, and append a private audit log.

## What this run did not ship

The host disk had 4 GiB free of 468 GiB after the image pull request merged. The threshold for one `./build.sh` run is 40 GiB, so the build was skipped. There is no new ISO. The retained images `2026.09.26`, `2026.10.01`, and `2026.10.06` are still the older files on disk. Nothing was booted.

A live GNOME Shell 51 session was not exercised on hardware. The foundation contract items C1–C8 stay open. The two timing samples inside `cargo test` stay ignored. They are not a launch gate.

The Capitaine cursor string and the Zero Portal motion file were not edited.
