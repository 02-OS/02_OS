# 02_OS

`02_OS` is an **agent-native developer operating system**. It pairs a refined **GNOME** desktop environment with an OS-native repository intelligence service (**02 Agent Runtime**) designed specifically for autonomous coding agents (OpenAI Codex, Claude Code, OpenCode, Gemini CLI, Cursor).

Built with `archiso` in a privileged Docker container. This tree builds a public alpha live image. It is an experiment for developers, not a production operating system.

---

## The 02 Agent Runtime

Rather than another AI chatbot, `02_OS` embeds a deterministic, Git-aware repository context compiler and Model Context Protocol (MCP) server directly into the operating system.

```
Coding Agents (Codex / Claude / OpenCode)
                |
           MCP Protocol (JSON-RPC 2.0)
                |
                v
      +-------------------+
      | 02 Agent Runtime  | <--> 02-agentd (systemd user daemon)
      +-------------------+
        |        |        |
     symbols   graph   memories
        |        |        |
        v        v        v
     Tree-sitter AST + libgit2 Diffs + SQLite WAL
        |
        v
  Staleness Engine (Symbol Fingerprints)
        |
        v
  Context Compiler (Budget-Optimized Evidence)
```

### Core Capabilities

- **100% Local-First**: Zero cloud telemetry, no external accounts, no cloud databases, no mandatory LLM APIs.
- **Git-Aware Truth**: Real-time diff tracking and symbol-level staleness detection.
- **Tree-sitter AST Evidence Graph**: Multi-language symbol parsing (Rust, Python, C/C++, Bash, JS/TS) with callers, callees, imports, and tests.
- **Architectural Memory**: Git-anchored verified facts with automated confidence degradation.
- **Token Budget Context Compiler**: Compiles high-density evidence packages tailored to LLM context windows (`--budget <N>`).
- **Standard MCP Server**: Vendor-neutral JSON-RPC 2.0 interface supporting all modern agent tools.

### Quick Commands

```bash
# Initialize repository index
02 init

# Inspect repository status, git HEAD, and indexed symbols
02 status

# Incremental re-index and automatic background watch registration
02 index

# Disable background indexing until explicitly re-enabled
02 unwatch
02 watch

# Compile a task-focused evidence package within an 8,000 token budget
02 context "fix refresh-token rotation race" --budget 8000

# Launch stdio MCP server for agent integration
02 mcp

# Configure coding agents
02 connect claude --write
02 connect codex
02 connect opencode
02 connect cursor --write
02 connect gemini --write
02 connect zed --write

# System & security diagnostics
02 doctor
02 security
```

For complete documentation:
- [User & CLI Reference](docs/agent-runtime.md)
- [Architecture Specification](docs/agent-runtime-architecture.md)
- [Security & Isolation Model](docs/agent-runtime-security.md)

---

## Look and feel

| Piece | What you get |
| :--- | :--- |
| **Desktop Environment** | GNOME Shell 51 + Pop Shell (optional tiling toggle with `Super+Y`) |
| **Dock** | Dash to Dock + Flourish — Centered dock with smooth hover magnification and a single app launch bounce |
| **Window motion** | Gentle jelly effect while dragging windows; honors GNOME's animation setting |
| **Theme** | adw-gtk3-dark, Capitaine cursors, Inter 11 |
| **Icons** | **02-OS** — Custom handcrafted glassmorphic vector icon theme (70+ scalable SVGs) |
| **Terminal** | GNOME Console (`org.gnome.Console`) |
| **File Manager** | Nautilus (`org.gnome.Nautilus`) |
| **Audio / Net** | PipeWire + NetworkManager (no duplicate network stacks) |
| **Agent Runtime** | `02` CLI (alias `02agent`) + `02-agentd` user daemon pre-installed and enabled |

The live image boots to GNOME's native login screen. Sign in as **`live`** with
password **`live`**; use the same password to unlock the session or for `sudo`.
The desktop runs as the unprivileged live account. Earlier live images used
autologin; new builds require sign-in after the boot animation.

For disposable installed-OS VM tests, use the shared credentials documented in
[VM testing](docs/VM_TESTING.md).

See [Desktop motion](docs/desktop-motion.md) for settings, package versions and maintenance instructions.
Chrome is pinned to the bottom dock. Brave is available in Show Applications
alongside the other apps; open that grid with the nine-dot button or **Super+A**.
Both browsers are native, preinstalled applications with matching 02-OS icons.
Each image build resolves the latest stable vendor packages, verifies their
signed repository metadata and downloads, and records the exact versions in
`/usr/local/share/02os/browser-repo/manifest.json`. The installer uses those same
packages for the installed desktop. Browser updates enter through a new image
build; the build-time manifest records a snapshot rather than promising that an
old image remains current. See [browser packaging](scripts/browser-packages/README.md).
Image customization only verifies the browsers already installed by pacstrap.
The installer stages browser archives only when the target needs them and
removes that staged payload after success, retaining the version manifest.
These effects ship in new builds from this source. The previously tested
`2026.10.01` ISO predates them; pushing source changes does not update an existing ISO.

---

## Keybinds

| Shortcut | Action |
| :--- | :--- |
| **Alt+F4** / **Super+Q** | Close focused window |
| **Super+Y** | Toggle Pop Shell window tiling (floating by default) |
| **Super+M** | Toggle maximize window |
| **Super+,** | Minimize window |
| **Super+V** | Toggle notification / message tray |
| **Super** | Overview / App Launcher |
| **Super+1..4** | Switch workspaces |

---

## Performance & Optimization

- **Single DE Architecture**: Pruned conflicting Hyprland / XFCE packages, leaving a clean, lean GNOME stack with no portal conflicts.
- **Build-Time Schema Compilation**: GLib schemas and dconf databases are precompiled during ISO build, saving boot time and RAM overlayfs space.
- **Clean User Management**: System users (`greeter`, `polkitd`, `dbus`) preserved via Arch packages and `sysusers.d`.
- **Deduplicated Skeleton**: Unified dotfiles in `/etc/skel` as the single source of truth.
- **Agent runtime staging**: `./build.sh` compiles `02` (alias `02agent`) and `02-agentd`, checks the staged binaries match that build, and then runs `mkarchiso`. Those binaries are not committed.
- **zstd** squashfs + initramfs (faster decompression and boot).
- **zram** swap with tuned swappiness.

---

## Repository layout

```
02_OS/
├── README.md
├── build.sh                   # Main ISO build orchestrator
├── 02-OS-icons-preview.html   # Visual preview gallery for 02-OS icon pack
├── components/
│   └── 02-agent/              # 02 Agent Runtime (Rust Workspace)
│       ├── Cargo.toml
│       ├── crates/
│       │   ├── agent-core/    # Identifiers, config, XDG paths, secrets
│       │   ├── agent-git/     # libgit2 discovery, diff hunks, status
│       │   ├── agent-parser/  # Tree-sitter AST symbol & reference graph
│       │   ├── agent-index/   # SQLite WAL + FTS5 full-text indexing
│       │   ├── agent-memory/  # Git-anchored memories & staleness engine
│       │   ├── agent-context/ # Multi-signal context compiler & budgeter
│       │   ├── agent-mcp/     # Model Context Protocol (2024-11-05) server
│       │   ├── agent-daemon/  # 02-agentd systemd user daemon & watcher
│       │   └── agent-cli/     # 02 CLI tool (alias 02agent)
│       └── benchmarks/        # Automated performance benchmark suite
├── docs/
│   ├── agent-runtime.md       # User guide & command reference
│   ├── agent-runtime-architecture.md # Architecture specification
│   └── agent-runtime-security.md     # Security & threat model
├── profile/
│   ├── profiledef.sh          # ISO build configuration and file permissions
│   ├── packages.x86_64        # Curated package manifest
│   ├── pacman.conf
│   └── airootfs/              # Live filesystem overlay:
│       ├── etc/dconf/         # Desktop defaults and keybindings
│       ├── etc/gdm/           # Native GNOME login screen; live autologin disabled
│       ├── etc/skel/          # User default profile and GTK settings
│       ├── etc/systemd/user/  # Enabled user services (02-agentd.service)
│       ├── etc/sysusers.d/    # Declarative live user creation
│       ├── root/              # Build-time customization script
│       ├── usr/bin/           # Staged by ./build.sh from source; not committed
│       ├── usr/lib/systemd/user/ # 02-agentd systemd service definition
│       └── usr/share/icons/02-OS/ # Glassmorphic vector icon theme
├── scripts/
│   ├── build-agent-runtime.sh # Agent runtime build and staging script
│   └── icon-generator/        # Python generator suite for the 02-OS icon pack
└── out/                       # Generated ISO images
```

---

## Building

`./build.sh` is the only supported way to build a 02_OS image. It compiles the agent runtime, checks that the staged `02` and `02-agentd` binaries match that build, and then runs the image build in Docker. The agent binaries are not in git. A manual `mkarchiso` of this profile is not a 02_OS image: the image build refuses agent binaries that were not just stamped by `./build.sh`.

Builds sharing a checkout or work directory are serialized. Each build copies the prepared profile
into its owned work directory, excludes host Python bytecode, and mounts that
copy read-only for mkarchiso. The copy and mkarchiso work tree remain available
there for diagnosis until the next build reclaims the marked work directory.
The image contains a source snapshot manifest. Its completion sidecar records
that source identity, the resolved package inventory, and the ISO SHA-256/size.
Development builds may include uncommitted source; releases require a clean
checkout and an artifact built from the matching clean source snapshot.
GNOME Shell/Mutter major 51 and Archinstall 4.5 are checked during image
customization. Unsupported combinations fail the build or installer preflight.

```bash
./build.sh
```

The ISO is written to `out/`. The build host needs Python 3.11 or newer. A
successful build atomically writes an adjacent `.manifest.json` containing its
source revision, size and SHA-256, alongside the compatibility `.commit` file.
`scripts/release.sh` verifies both revision and image bytes before selecting an
ISO for publication. Legacy images with only a commit stamp must be rebuilt
before using `--reuse-iso`; existing images and historical evidence are preserved.

---

## Test in QEMU

```bash
sudo apt update && sudo apt install -y qemu-system-x86
qemu-system-x86_64 -enable-kvm -m 4G -cdrom out/02_OS-*.iso -boot d
```

Accelerated rendering in QEMU:
`-device virtio-vga-gl -display gtk,gl=on`

---

## Install

From the live desktop, launch **Install 02_OS**. That starts `archinstall` with the 02_OS plugin. Choose the GNOME desktop profile. The plugin copies the 02 desktop profile, agent binaries, user daemon, and branding onto the new system after the base install and again after the desktop packages are installed.

The installer guides you through partitioning, username creation, and bootloader configuration.
The installed desktop omits the live-only installer shortcut. Live boot does
not wait for Ethernet carrier or an automatic mirror refresh; NetworkManager
and systemd-resolved handle networking once a connection is available.

---

## Sign and flash

```bash
gpg --detach-sign --armor out/02_OS-*.iso
sha256sum out/02_OS-*.iso > out/02_OS.sha256
sudo dd if=out/02_OS-*.iso of=/dev/sdX bs=4M status=progress oflag=sync
```
