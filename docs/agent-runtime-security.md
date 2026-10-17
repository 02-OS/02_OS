# 02 Agent Runtime Security & Isolation Model

## 1. Security Philosophy & Threat Model

The **02 Agent Runtime** is an OS-level developer service designed under strict **Local-First, Zero-Trust, and Least-Privilege** principles.

Because coding agents interact with proprietary source code and sensitive repository environments, the runtime guarantees:

1. **Zero External Data Exfiltration**: The runtime never initiates external network requests, transmits telemetry, or contacts cloud endpoints.
2. **Zero Provider Lock-In**: The runtime does not handle or store proprietary LLM API keys. It functions strictly as a local repository intelligence engine over standard MCP.
3. **Secret Isolation**: Sensitive files, private keys, and API tokens are detected and stripped before entering search indices or evidence packages.
4. **User-Service Sandboxing**: The daemon is a systemd user service. It warns when started as root and keeps running. Storage and the socket stay private to the owning uid.
5. **Multi-User IPC Isolation**: Inter-process communication occurs over Unix domain sockets restricted exclusively to the owning user (`0600`).

---

## 2. Secret Scanning & Exclusion Pipeline

The indexing engine (`agent-index`) skips exact secret filenames (`.env`, key material, `secrets.json`), `.env.*` names, and secret extensions. Source files whose names merely contain words like `secret` stay in the catalog. After a file is decoded as UTF-8, credential matches are replaced with `[REDACTED]` before FTS and file datapoints are written. The content hash still uses the raw bytes, so a later edit is detected.

### Filtered Patterns

| Category | Detection Regex / Rule | Action |
| :--- | :--- | :--- |
| **Private Keys** | `-----BEGIN (RSA\|OPENSSH\|EC\|PGP) PRIVATE KEY-----` | Exclude file / redact content |
| **AWS Credentials** | `AKIA[0-9A-Z]{16}`, AWS secret access key patterns | Redact match |
| **GitHub Tokens** | `ghp_[A-Za-z0-9_]{36}`, `github_pat_[A-Za-z0-9_]{82}` | Redact match |
| **Slack Tokens** | `xox[baprs]-[0-9A-Za-z]{10,48}` | Redact match |
| **Generic API Keys** | High-entropy strings matching `(api[_-]?key\|secret\|token)\s*[:=]\s*["'][A-Za-z0-9_\-]{20,}["']` | Redact match |
| **Environment Files** | `.env`, `.env.local`, `.env.*.local` | Excluded from text indexing |

Developers can audit their repository at any time:

```bash
02 security
```

---

## 3. Privilege Separation & Daemon Hardening

### Non-Root Enforcement

`02-agentd` is a user service. Its packaged unit excludes system accounts and GDM greeters. If the binary is started directly as UID 0 it prints a warning and continues. It does not exit solely because the uid is root. Socket mode `0600`, directory mode `0700`, and `SO_PEERCRED` still limit who can connect.

### Systemd User Sandboxing

In **02_OS**, `02-agentd` is managed as an unprivileged user service (`02-agentd.service`) under `systemd --user`:

```ini
[Unit]
ConditionUser=!@system
ConditionUser=!gdm-greeter

[Service]
Type=simple
ExecCondition=/usr/bin/sh -c 'case "$$1" in gdm|gdm-greeter|gdm-greeter-*) exit 1 ;; *) exit 0 ;; esac' sh %u
ExecStartPre=+/usr/lib/02-agent/prepare-storage.sh
ExecStart=/usr/bin/02-agentd
Restart=on-failure
RestartSec=3s
Environment=RUST_LOG=info

ProtectSystem=strict
ProtectHome=no
ReadWritePaths=%h/.local/share/02-agent %h/.cache/02-agent %h/.config/02-agent %t %h
NoNewPrivileges=true
```

- **`ProtectSystem=strict`**: Mounts the whole file system read-only, including `$HOME`. `ProtectHome=no` does not undo that.
- **`ExecStartPre=+/usr/lib/02-agent/prepare-storage.sh`**: The `+` prefix skips the sandbox and creates the data, cache, and config directories (mode `0700`) before the daemon starts. Those paths honor `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and `XDG_CONFIG_HOME`, with the usual defaults under `~/.local/share`, `~/.cache`, and `~/.config`. A missing directory is not papered over with a `-` prefix on `ReadWritePaths`.
- **`ProtectHome=no` and `ReadWritePaths=%h`**: Home stays visible and writable so repositories there can be indexed. Data, cache, config, and `%t` are listed as well. A user generator adds `ReadWritePaths` when those XDG directories sit outside `$HOME`.
- **Account checks**: The unit filters system UIDs and static/dynamic GDM greeter names. It does not order itself after the target that enables it.
- **`NoNewPrivileges=true`**: Disallows gaining new privileges via `setuid` binaries.

---

## 4. Socket Security & IPC Isolation

Communications between coding agents, `02` CLI, and `02-agentd` take place over a local Unix domain socket:

```
Primary:   $XDG_RUNTIME_DIR/02agent.sock
Fallback:  ~/.local/share/02-agent/02agent.sock
```

### Access Restrictions

- **File Permissions**: The socket is created with `0600` permissions (`-rw-------`). SQLite database, WAL, and SHM files are mode `0600`.
- **Directory Permissions**: Data, cache, config, and per-repository directories are initialized with `0700` (`drwx------`).
- **Peer Credential Checking**: `SO_PEERCRED` must report the same uid as the daemon. A failed lookup or a different uid drops the connection.

---

## 5. Live-User & OverlayFS Storage Safety

When running in the 02_OS live ISO environment:

- The live account is `live` with password `live`. `sudo` without a password is granted to `live` only. Members of `wheel` do not get `NOPASSWD`.
- The root account is locked (`passwd -l root`). `sshd` does not set `PermitRootLogin`. There is no `script=` hook under `/root`.
- Live detection uses `/run/archiso/bootmnt` and `/run/archiso/airootfs`. The account name `live` is not treated as proof of the live image.
- The root filesystem is a memory-backed OverlayFS (`cowspace`).
- **RAM Preservation**:
  - SQLite indexes operate with `PRAGMA synchronous = NORMAL;` and `PRAGMA cache_size = -64000;` (64 MB max memory cache).
  - Write-Ahead Logging (WAL) automatically checkpoints to prevent unbounded file growth in RAM.
  - Ignored directories (`target/`, `node_modules/`, `.git/objects/`, `build/`, `dist/`) are pruned prior to indexing, keeping RAM footprint minimal (typically < 30 MB per indexed repository).

---

## 6. Audit & Health Verification

Developers and security teams can verify the operational integrity of the agent runtime using the built-in diagnostic suite:

```bash
# Verify system environment, storage permissions, and live-user state
02 doctor

# Scan repository for credentials or insecure configuration
02 security
```
