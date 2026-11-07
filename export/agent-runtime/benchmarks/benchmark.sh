#!/usr/bin/env bash
# ==============================================================================
# 02 Agent Runtime Performance Benchmark Suite
# Measures latency and throughput for key runtime subsystems.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
AGENT_BIN="${REPO_ROOT}/profile/airootfs/usr/bin/02"

if [[ ! -f "${AGENT_BIN}" ]]; then
    echo "ERROR: ${AGENT_BIN} not found. Running build-agent-runtime.sh first..."
    "${REPO_ROOT}/scripts/build-agent-runtime.sh"
fi

echo "============================================================"
echo " 02 Agent Runtime Performance Benchmarks"
echo " Target Binary: ${AGENT_BIN}"
echo " Repository:    ${REPO_ROOT}"
echo "============================================================"

# Benchmark 1: Repository Status Latency
echo -n "1. Repository Status Latency: "
START_NS=$(date +%s%N)
"${AGENT_BIN}" status >/dev/null
END_NS=$(date +%s%N)
DIFF_MS=$(( (END_NS - START_NS) / 1000000 ))
echo "${DIFF_MS} ms"

# Benchmark 2: Incremental Index Latency (Zero Changes)
echo -n "2. Incremental Index Latency (Zero-change check): "
START_NS=$(date +%s%N)
"${AGENT_BIN}" index >/dev/null
END_NS=$(date +%s%N)
DIFF_MS=$(( (END_NS - START_NS) / 1000000 ))
echo "${DIFF_MS} ms"

# Benchmark 3: BM25 Full-Text Search Latency
echo -n "3. FTS5 BM25 Search Latency ('profiledef'): "
START_NS=$(date +%s%N)
"${AGENT_BIN}" search "profiledef" --limit 10 >/dev/null
END_NS=$(date +%s%N)
DIFF_MS=$(( (END_NS - START_NS) / 1000000 ))
echo "${DIFF_MS} ms"

# Benchmark 4: AST Symbol Query Latency
echo -n "4. AST Symbol Lookup Latency ('iso_name'): "
START_NS=$(date +%s%N)
"${AGENT_BIN}" symbol "iso_name" >/dev/null
END_NS=$(date +%s%N)
DIFF_MS=$(( (END_NS - START_NS) / 1000000 ))
echo "${DIFF_MS} ms"

# Benchmark 5: Symbol Dependency & Test Graph Traversal
echo -n "5. Symbol Call Graph Traversal ('rotate_refresh_token'): "
START_NS=$(date +%s%N)
"${AGENT_BIN}" deps "rotate_refresh_token" >/dev/null 2>&1 || true
END_NS=$(date +%s%N)
DIFF_MS=$(( (END_NS - START_NS) / 1000000 ))
echo "${DIFF_MS} ms"

# Benchmark 6: Context Compiler Pipeline & Budgeting
echo -n "6. Context Compiler Execution ('iso name and profile configuration' --budget 4000): "
START_NS=$(date +%s%N)
"${AGENT_BIN}" context "iso name and profile configuration" --budget 4000 >/dev/null
END_NS=$(date +%s%N)
DIFF_MS=$(( (END_NS - START_NS) / 1000000 ))
echo "${DIFF_MS} ms"

# Benchmark 7: MCP JSON-RPC Stdio Handshake & Tool Call Roundtrip
echo -n "7. MCP JSON-RPC Stdio Roundtrip (initialize + repository_status): "
START_NS=$(date +%s%N)
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"repository_status","arguments":{}}}\n' | "${AGENT_BIN}" mcp >/dev/null
END_NS=$(date +%s%N)
DIFF_MS=$(( (END_NS - START_NS) / 1000000 ))
echo "${DIFF_MS} ms"

# Benchmark 8: Full Repository Re-index (400+ files, 1500+ symbols, 10000+ refs)
echo -n "8. Full Re-index Latency (--full): "
START_NS=$(date +%s%N)
"${AGENT_BIN}" index --full >/dev/null
END_NS=$(date +%s%N)
DIFF_MS=$(( (END_NS - START_NS) / 1000000 ))
echo "${DIFF_MS} ms"

# Benchmark 9: Dataset ingest
echo -n "9. Add (worktree,docs) Latency: "
START_NS=$(date +%s%N)
"${AGENT_BIN}" add --source worktree,docs >/dev/null
END_NS=$(date +%s%N)
DIFF_MS=$(( (END_NS - START_NS) / 1000000 ))
echo "${DIFF_MS} ms"

# Benchmark 10: Deterministic cognify
echo -n "10. Cognify Latency: "
START_NS=$(date +%s%N)
"${AGENT_BIN}" cognify >/dev/null
END_NS=$(date +%s%N)
DIFF_MS=$(( (END_NS - START_NS) / 1000000 ))
echo "${DIFF_MS} ms"

# Benchmark 11: Graph export
echo -n "11. Graph Export (JSON) Latency: "
START_NS=$(date +%s%N)
"${AGENT_BIN}" graph export --format json >/dev/null
END_NS=$(date +%s%N)
DIFF_MS=$(( (END_NS - START_NS) / 1000000 ))
echo "${DIFF_MS} ms"

echo "============================================================"
echo " All Benchmarks Completed Successfully."
echo "============================================================"
