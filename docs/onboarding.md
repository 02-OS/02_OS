# Onboarding

02_OS does not ask for an account. The agent runtime keeps the index, memories, approvals, and evidence on this machine.

## Install

Use the latest GitHub release of 02-OS/02_OS when you want a published image. To build one, use a machine with at least 40 GiB free and run `./build.sh` from a checkout of `master`. GitHub-hosted runners do not produce the image. Keep the older images dated `2026.09.26`, `2026.10.01`, and `2026.10.06`.

The installer command on the image is `02os-install`. Pass `--developer` only when you want gcc, make, pkgconf, gdb, and strace in addition to the desktop profile. Leaving the flag off keeps the current desktop package list.

## First commands

```bash
02 init
02 index
02 doctor
02 security
02 dashboard
```

`02 dashboard` prints a `http://127.0.0.1:8732` URL and serves one page. It refuses any other bind address. The page reads `~/.local/share/02-agent/status.json` when that file is a regular file and mode `0600`.

`02-agentd` is the user daemon. Its socket is `$XDG_RUNTIME_DIR/02agent.sock` when that directory exists, otherwise `~/.local/share/02-agent/02agent.sock`.

## Desktop

Pop Shell is the tiler. The developer keys are Super+T for the terminal (`kgx`), Super+F for Files, Super+I for Settings, Super+G to float a window, and Super+Y to toggle tiling. Super+L locks the screen.

The cursor theme is Capitaine. The boot animation is the Zero Portal. Neither was retuned in this run.

## Skills

A skill is a `SKILL.toml` inside a directory you choose. It has a name, a version, and a list of tools that the agent already exposes. A path that leaves that directory, a symlink, or an unknown tool is rejected. Loading a skill does not open a network connection.
