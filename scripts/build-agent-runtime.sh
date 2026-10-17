#!/usr/bin/env bash
# ==============================================================================
# 02 Agent Runtime Build Script
# Compiles 02 Agent Runtime (02agent, 02-agentd) and stages into ArchISO airootfs.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
AGENT_DIR="${REPO_ROOT}/components/02-agent"
DEST_DIR="${REPO_ROOT}/profile/airootfs/usr/bin"

echo "=== [02 Agent Runtime] Compiling Release Binaries ==="

if ! command -v cargo >/dev/null 2>&1; then
    echo "ERROR: 'cargo' is required to build the 02 Agent Runtime." >&2
    echo "Install Rust via 'pacman -S rust' or 'rustup'." >&2
    exit 1
fi

# cargo metadata honors CARGO_TARGET_DIR and .cargo/config target-dir.
# Build, test, and the copy below all use that directory — not a hardcoded
# components/02-agent/target path that may be stale or absent.
read_cargo_target_dir() {
    local manifest="$1"
    local meta target_dir
    meta="$(cargo metadata --format-version 1 --no-deps --manifest-path "${manifest}")" \
        || { echo "ERROR: cargo metadata failed for ${manifest}" >&2; return 1; }
    if command -v python3 >/dev/null 2>&1; then
        target_dir="$(printf '%s' "${meta}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')" \
            || { echo "ERROR: could not parse cargo target_directory" >&2; return 1; }
    else
        target_dir="$(printf '%s' "${meta}" | sed -n 's/.*"target_directory"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
        target_dir="${target_dir//\\\//\/}"
    fi
    [[ -n "${target_dir}" ]] || { echo "ERROR: cargo metadata did not report target_directory" >&2; return 1; }
    printf '%s\n' "${target_dir%/}"
}

TARGET_DIR="$(read_cargo_target_dir "${AGENT_DIR}/Cargo.toml")"
echo "Cargo target directory: ${TARGET_DIR}"

# Build release binaries. Do not override CARGO_TARGET_DIR here; metadata and
# these invocations share the caller's cargo configuration.
cargo build --locked --release --manifest-path "${AGENT_DIR}/Cargo.toml"

echo "=== [02 Agent Runtime] Running Verification Tests ==="
cargo test --locked --manifest-path "${AGENT_DIR}/Cargo.toml"

echo "=== [02 Agent Runtime] Staging Binaries into airootfs ==="
mkdir -p "${DEST_DIR}"

cp -f "${TARGET_DIR}/release/02" "${DEST_DIR}/02"
ln -sf "02" "${DEST_DIR}/02agent"
cp -f "${TARGET_DIR}/release/02-agentd" "${DEST_DIR}/02-agentd"

chmod 755 "${DEST_DIR}/02" "${DEST_DIR}/02-agentd"

# Strip debug symbols if strip is present to conserve squashfs space
if command -v strip >/dev/null 2>&1; then
    echo "Stripping binaries..."
    strip "${DEST_DIR}/02" "${DEST_DIR}/02-agentd" || true
fi

PROV_DIR="${REPO_ROOT}/profile/airootfs/usr/lib/02-agent"
mkdir -p "${PROV_DIR}"
revision="unknown"
if git_rev="$(git -C "${REPO_ROOT}" rev-parse HEAD 2>/dev/null)" && [[ -n "${git_rev}" ]]; then
    revision="${git_rev}"
fi
digest_02="$(sha256sum "${DEST_DIR}/02" | awk 'NR==1 { print $1 }')"
digest_daemon="$(sha256sum "${DEST_DIR}/02-agentd" | awk 'NR==1 { print $1 }')"
printf 'revision=%s\nsha256=%s\nsha256_02=%s\nsha256_02_agentd=%s\n' \
  "${revision}" "${digest_02}" "${digest_02}" "${digest_daemon}" > "${PROV_DIR}/SOURCE_REVISION"

echo "Staged binaries:"
ls -lh "${DEST_DIR}/02" "${DEST_DIR}/02agent" "${DEST_DIR}/02-agentd"
echo "Provenance: ${PROV_DIR}/SOURCE_REVISION"

echo "=== [02 Agent Runtime] Build & Staging Complete ==="
