#!/usr/bin/env bash
# ==============================================================================
# 02_OS Release Orchestrator
# Builds the ISO, splits into upload chunks, generates checksums, and publishes
# release to GitHub using gh CLI.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
PROFILE_DIR="${REPO_ROOT}/profile"
# Tests set these so a stub build can write into a disposable directory.
# Unset, release uses this repo's out/ and build.sh.
OUT_DIR="${RELEASE_OUT_DIR:-${REPO_ROOT}/out}"
export OUT_DIR
BUILD_SCRIPT="${RELEASE_BUILD_SCRIPT:-${REPO_ROOT}/build.sh}"
python3 "${SCRIPT_DIR}/source-manifest.py" "${REPO_ROOT}" --require-clean > /dev/null

REUSE_ISO=0
FORCE_TAG=0
for arg in "$@"; do
    case "${arg}" in
        --reuse-iso) REUSE_ISO=1 ;;
        --force-tag) FORCE_TAG=1 ;;
        --force-rebuild) ;;
        *)
            echo "ERROR: unknown argument ${arg} (expected --reuse-iso, --force-tag, or --force-rebuild)" >&2
            exit 1
            ;;
    esac
done

# ISO_PATH is global. A fresh build must be the new name, or the single ISO
# whose bytes changed, and its completion manifest must match HEAD and bytes. Untouched older
# images in out/ are never selected. --reuse-iso stays count-strict.
select_fresh_iso() {
    declare -A before=()
    local iso name mtime
    if [[ -d "${OUT_DIR}" ]]; then
        while IFS= read -r iso; do
            [[ -n "${iso}" ]] || continue
            name="$(basename -- "${iso}")"
            before["${name}"]="$(stat -c '%y:%s:%i' -- "${iso}")"
        done < <(find "${OUT_DIR}" -maxdepth 1 -type f -name '02_OS-*.iso' -print | sort)
    fi

    echo "=== [Step 1/5] Building 02_OS Live ISO ==="
    "${BUILD_SCRIPT}"

    if [[ ! -d "${OUT_DIR}" ]]; then
        echo "ERROR: build did not create ${OUT_DIR}" >&2
        exit 1
    fi

    local -a new_isos=() changed_isos=()
    while IFS= read -r iso; do
        [[ -n "${iso}" ]] || continue
        name="$(basename -- "${iso}")"
        mtime="$(stat -c '%y:%s:%i' -- "${iso}")"
        if [[ -z "${before[${name}]+x}" ]]; then
            new_isos+=("${iso}")
        elif [[ "${before[${name}]}" != "${mtime}" ]]; then
            changed_isos+=("${iso}")
        fi
    done < <(find "${OUT_DIR}" -maxdepth 1 -type f -name '02_OS-*.iso' -print | sort)

    if [[ "${#new_isos[@]}" -eq 1 && "${#changed_isos[@]}" -eq 0 ]]; then
        ISO_PATH="${new_isos[0]}"
    elif [[ "${#new_isos[@]}" -eq 0 && "${#changed_isos[@]}" -eq 1 ]]; then
        ISO_PATH="${changed_isos[0]}"
    else
        echo "ERROR: could not identify the ISO produced by this build in ${OUT_DIR} (new: ${#new_isos[@]}, replaced: ${#changed_isos[@]})" >&2
        exit 1
    fi

    local head_now
    head_now="$(git -C "${REPO_ROOT}" rev-parse HEAD)"
    python3 "${SCRIPT_DIR}/iso-manifest.py" verify "${ISO_PATH}" "${head_now}" --source-root "${REPO_ROOT}"

}

if [[ "${REUSE_ISO}" -eq 1 ]]; then
    if [[ ! -d "${OUT_DIR}" ]]; then
        echo "ERROR: --reuse-iso requires ${OUT_DIR}" >&2
        exit 1
    fi
    mapfile -t ISO_CANDIDATES < <(find "${OUT_DIR}" -maxdepth 1 -type f -name "02_OS-*.iso" -print | sort)
    if [[ "${#ISO_CANDIDATES[@]}" -ne 1 ]]; then
        echo "ERROR: --reuse-iso needs exactly one 02_OS-*.iso in ${OUT_DIR} (found ${#ISO_CANDIDATES[@]})" >&2
        exit 1
    fi
    ISO_PATH="${ISO_CANDIDATES[0]}"
    HEAD_NOW="$(git -C "${REPO_ROOT}" rev-parse HEAD)"
    python3 "${SCRIPT_DIR}/iso-manifest.py" verify "${ISO_PATH}" "${HEAD_NOW}" --source-root "${REPO_ROOT}"
    echo "Reusing ISO ${ISO_PATH} built from ${HEAD_NOW}."
else
    select_fresh_iso
fi

if [[ -z "${ISO_PATH:-}" || ! -f "${ISO_PATH}" ]]; then
    echo "ERROR: ISO file not found in ${OUT_DIR}" >&2
    exit 1
fi

ISO_NAME="$(basename "${ISO_PATH}")"
ISO_VERSION="$(echo "${ISO_NAME}" | sed -E 's/^02_OS-(.*)-x86_64\.iso$/\1/')"
TAG_NAME="v${ISO_VERSION}"
RELEASE_TITLE="02_OS v${ISO_VERSION}"

echo "============================================================"
echo " Starting 02_OS Release: ${RELEASE_TITLE} (${TAG_NAME})"
echo " Repo: ${REPO_ROOT}"
echo " ISO:  ${ISO_PATH} ($(du -h "${ISO_PATH}" | cut -f1))"
echo "============================================================"
echo "Selected ISO: ${ISO_PATH}"

# RELEASE_DRY_RUN=1 stops before split, git tag, and gh.
if [[ "${RELEASE_DRY_RUN:-}" == "1" ]]; then
    echo "RELEASE_DRY_RUN=1: skipping split, tag, and GitHub publish"
    exit 0
fi

# Step 2: Generate Checksums & Split ISO
echo "=== [Step 2/5] Splitting ISO into 1.4GB Parts & Computing Checksums ==="
cd "${OUT_DIR}"

# Full ISO checksum
echo "Calculating sha256 for ${ISO_NAME}..."
sha256sum "${ISO_NAME}" > "02_OS.sha256"

# Remove any old split parts
rm -f "${ISO_NAME}.part"* "02_OS-parts.sha256"

# Split into 1400M chunks (partaa, partab, ...)
echo "Splitting ${ISO_NAME} into 1400MB parts..."
split -b 1400M "${ISO_NAME}" "${ISO_NAME}.part"

# Parts checksum
echo "Calculating sha256 for split parts..."
sha256sum "${ISO_NAME}.part"* > "02_OS-parts.sha256"

ls -lh "${OUT_DIR}"

# Step 3: Git Tag
echo "=== [Step 3/5] Creating and Pushing Git Tag ${TAG_NAME} ==="
cd "${REPO_ROOT}"
REMOTE_TAG="$(git ls-remote origin "refs/tags/${TAG_NAME}")"
if [[ -n "${REMOTE_TAG}" ]]; then
    echo "Remote tag ${TAG_NAME} already exists:"
    echo "${REMOTE_TAG}"
    if [[ "${FORCE_TAG}" -ne 1 ]]; then
        echo "ERROR: refusing to replace ${TAG_NAME}. Pass --force-tag to overwrite it." >&2
        exit 1
    fi
fi
if git rev-parse -q --verify "refs/tags/${TAG_NAME}" >/dev/null; then
    if [[ "${FORCE_TAG}" -ne 1 ]]; then
        echo "ERROR: local tag ${TAG_NAME} already exists. Pass --force-tag to replace it." >&2
        exit 1
    fi
    git tag -d "${TAG_NAME}"
fi

git tag -a "${TAG_NAME}" -m "Release ${RELEASE_TITLE}"
if [[ "${FORCE_TAG}" -eq 1 && -n "${REMOTE_TAG}" ]]; then
    git push origin "${TAG_NAME}" --force
else
    git push origin "${TAG_NAME}"
fi

# Step 4: Prepare Release Notes
echo "=== [Step 4/5] Preparing Release Notes ==="
NOTES_FILE="${OUT_DIR}/RELEASE_NOTES.md"
cat > "${NOTES_FILE}" <<EOF
# 02_OS v${ISO_VERSION} — Agent-Native Developer Operating System

Welcome to the **02_OS v${ISO_VERSION}** release!

This release introduces the **02 Agent Runtime** — an operating-system-level repository intelligence service and Model Context Protocol (MCP) server built directly into 02_OS for autonomous coding agents (Codex, Claude Code, OpenCode, Gemini CLI, Cursor).

### What's New

- **02 Agent Runtime (\`02\` & \`02-agentd\`)**:
  - **100% Local-First**: Zero telemetry, no cloud DBs, no external LLM dependencies.
  - **Git-Aware Truth**: Real-time diff tracking and symbol-level staleness detection.
  - **Tree-sitter AST Evidence Graph**: Multi-language symbol parsing (Rust, Python, C/C++, Bash, JS/TS) with callers, callees, imports, and tests.
  - **Architectural Memory**: Git-anchored verified facts with automated confidence degradation.
  - **Token Budget Context Compiler**: Compiles high-density evidence packages tailored to LLM context windows (\`02 context "<task>" --budget 8000\`).
  - **Standard MCP Server**: Vendor-neutral JSON-RPC 2.0 interface supporting 15 agent tools (\`02 mcp\`, \`02 connect\`).
  - **Pre-installed Background User Daemon**: \`02-agentd\` enabled via \`systemd --user\`.
- **Refined GNOME Desktop**:
  - GNOME Shell 51 + Pop Shell window tiling (\`Super+Y\`).
  - Centered floating Dash-to-Dock with pinned terminal and installer.
  - Custom handcrafted **02-OS Glassmorphic Vector Icon Theme** (70+ scalable SVGs).
  - Precompiled GLib schemas and dconf database for faster boot and minimal RAM footprint.
- **archinstall**:
  - Live session user \`live\` (password \`live\`) with an unprivileged desktop session.
  - Launch "Install 02_OS" (which runs the provisioner). Choose the GNOME desktop profile.
- **Modern Linux Audio & Networking**:
  - PipeWire audio stack with automatic live unmuting.
  - Clean NetworkManager stack.

---

### Joining Split Parts

To reassemble the full ISO image after downloading all parts:

\`\`\`bash
cat ${ISO_NAME}.part* > ${ISO_NAME}
\`\`\`

---

### Verification

Verify the reassembled ISO image integrity:

\`\`\`bash
sha256sum -c 02_OS.sha256
\`\`\`

Or verify the split parts individually:

\`\`\`bash
sha256sum -c 02_OS-parts.sha256
\`\`\`
EOF

# Step 5: Publish Release via gh CLI
echo "=== [Step 5/5] Publishing GitHub Release with Assets ==="
ASSETS=(
    "${OUT_DIR}/02_OS.sha256"
    "${OUT_DIR}/02_OS-parts.sha256"
)

for part in "${OUT_DIR}/${ISO_NAME}.part"*; do
    if [[ -f "${part}" ]]; then
        ASSETS+=("${part}")
    fi
done

echo "Uploading assets to release ${TAG_NAME}:"
printf '  %s\n' "${ASSETS[@]}"

if gh release view "${TAG_NAME}" >/dev/null 2>&1; then
    echo "Release ${TAG_NAME} already exists on GitHub, updating..."
    gh release upload "${TAG_NAME}" "${ASSETS[@]}" --clobber
    gh release edit "${TAG_NAME}" --title "${RELEASE_TITLE}" --notes-file "${NOTES_FILE}"
else
    echo "Creating new GitHub release ${TAG_NAME}..."
    gh release create "${TAG_NAME}" "${ASSETS[@]}" \
        --title "${RELEASE_TITLE}" \
        --notes-file "${NOTES_FILE}" \
        --latest
fi

echo "============================================================"
echo " 02_OS Release ${TAG_NAME} Published Successfully!"
echo " URL: $(gh release view "${TAG_NAME}" --json url -q .url)"
echo "============================================================"
