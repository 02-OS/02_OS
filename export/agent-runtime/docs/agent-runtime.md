# 02 Agent Runtime User & Developer Guide

The **02 Agent Runtime** is an OS-native repository intelligence service built into **02_OS**. Rather than treating AI as a chat application, 02_OS embeds a Git-aware, AST-analyzing evidence compiler directly into the operating system for autonomous coding agents (Codex, Claude Code, OpenCode, Cursor, Gemini CLI, etc.).

---

## Key Features

- **100% Local-First**: Zero external cloud databases, zero mandatory LLM API keys, zero telemetry. All source code and metadata remain on the machine.
- **Git-Aware Intelligence**: Synchronizes with Git HEAD, tracks working tree diffs, parses modified hunks, and maps edits to code symbols.
- **Tree-Sitter AST Evidence Graph**: Indexes functions, methods, classes, variables, and cross-file references (callers, callees, imports, tests).
- **Automated Staleness Engine**: Evaluates whether code modifications degrade confidence or invalidate previously established memories.
- **Context Compiler**: Compiles task-focused evidence packages under strict token budget limits (`--budget <N>`).
- **Standard MCP Server**: Vendor-neutral JSON-RPC Model Context Protocol (2024-11-05) support over stdio and Unix domain sockets.
- **Systemd User Daemon**: Background service (`02-agentd`) with live repository watching.

---

## Quickstart

The primary command is `02` (with `02agent` available as an alias).

### 1. Initialize a Repository

Navigate to any Git repository and initialize the 02 Agent index:

```bash
cd /path/to/project
02 init
```

This creates an isolated index in `~/.local/share/02-agent/repos/<repo-id>/index.sqlite`.

### 2. Check Repository Status

```bash
02 status
```

Example output:
```text
02 Agent Runtime: Repository Status
  Repository:       my-project
  Root Path:        /home/user/my-project
  Repo ID:          4cc6e55ea8bbb0ca
  Branch:           main
  HEAD Commit:      0ac4d4f0a977c8ef5185d6662e09f6232faf1b92
  Working Tree:     Clean
  Indexed Files:    408 (source: 107, tests: 6, docs: 1, manifests: 8)
  Symbols Parsed:   1531 symbols, 9535 references
  Memories:         2 total (2 fresh, 0 degraded, 0 stale)
  Index Freshness:  Fresh (synced with HEAD)
```

### 3. Incremental Indexing

Index newly added or modified files in milliseconds:

```bash
02 index
```

To force a complete re-index of all files from scratch:

```bash
02 index --full
```

---

## Context Compiler

Compile the exact evidence package needed for a coding agent to solve a task within a strict token budget:

```bash
02 context "fix refresh-token rotation race" --budget 4000
```

For machine-readable JSON output:

```bash
02 context "fix refresh-token rotation race" --json
```

---

## Coding Agent Integration (MCP)

### Launch Stdio MCP Server

```bash
02 mcp
```

### Connect Coding Agents

Generate or automatically install MCP configurations for supported coding agents:

```bash
# Print configuration for OpenAI Codex
02 connect codex

# Install configuration directly into Claude Desktop / Claude Code
02 connect claude --write

# Print configuration for OpenCode
02 connect opencode

# Install into Cursor, Gemini CLI, or Zed
02 connect cursor --write
02 connect gemini --write
02 connect zed --write
```

Codex, Claude, OpenCode, Cursor, and Gemini CLI use this configuration shape. Default paths are `~/.config/codex/mcp.json`, `~/.config/Claude/claude_desktop_config.json`, `~/.config/opencode/mcp.json`, `~/.cursor/mcp.json`, and `~/.gemini/antigravity-cli/mcp_config.json`.

```json
{
  "mcpServers": {
    "02": {
      "command": "02",
      "args": ["mcp"]
    }
  }
}
```

Zed is written into `~/.config/zed/settings.json` under `context_servers` (`source: custom`).

---

## Symbol Exploration & Memories

```bash
# Look up symbol definition
02 symbol rotate_refresh_token

# Inspect call graph and callers/callees
02 deps rotate_refresh_token

# Discover test suites exercising a symbol
02 tests rotate_refresh_token

# List architectural memories
02 memory list

# Verify freshness of a memory against current Git HEAD
02 memory verify mem_archiso_profile
```

---

## System Diagnostics

```bash
# Run system privilege, storage, and runtime diagnostics
02 doctor

# Audit repository for exposed secrets or credentials
02 security
```

---

## Background Daemon (`02-agentd`)

`02-agentd` runs as a systemd user service (`02-agentd.service`).

- Socket path: `$XDG_RUNTIME_DIR/02agent.sock` (fallback `~/.local/share/02-agent/02agent.sock`)
- Socket permissions: `0600` (restricted to current user)
- Automatically restarts on failure (`Restart=on-failure`, `RestartSec=3s`)
- Watches registered repositories and keeps indices fresh in the background
