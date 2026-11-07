#!/usr/bin/env bash
set -euo pipefail

# 02_OS ISO build script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROFILE_DIR="${SCRIPT_DIR}/profile"
OUT_DIR="${SCRIPT_DIR}/out"
# /tmp is often a small tmpfs. Keep the rootfs work tree on disk.
WORK_DIR="${WORK_DIR:-/var/tmp/02_OS-build-work}"
PACMAN_CACHE_DIR="${PACMAN_CACHE_DIR:-/var/cache/pacman/pkg}"
MIN_FREE_GB="${MIN_FREE_GB:-20}"

HOST_UID="$(id -u)"
HOST_GID="$(id -g)"

# Drop the staged runtime on the way out, including after a failed build, so a
# later manual mkarchiso cannot package binaries that are no longer this build.
cleanup_staged_runtime() {
  rm -f -- \
    "${PROFILE_DIR}/airootfs/usr/bin/02" \
    "${PROFILE_DIR}/airootfs/usr/bin/02-agentd" \
    "${PROFILE_DIR}/airootfs/usr/bin/02agent" \
    "${PROFILE_DIR}/airootfs/usr/lib/02-agent/SOURCE_REVISION" \
    "${PROFILE_DIR}/airootfs/usr/lib/02-agent/BUILD_STAMP"
}
trap cleanup_staged_runtime EXIT

# shellcheck source=scripts/prepare-work-dir.sh
source "${SCRIPT_DIR}/scripts/prepare-work-dir.sh"
# Validates WORK_DIR before creating it, deleting a previous marked dir, or building.
prepare_work_dir

mkdir -p "${OUT_DIR}"

if [[ ! -d "${PACMAN_CACHE_DIR}" ]]; then
  PACMAN_CACHE_DIR="${SCRIPT_DIR}/.cache/pacman"
  mkdir -p "${PACMAN_CACHE_DIR}"
fi

# ./build.sh is the only supported image build. Stage from source, prove the
# staged binaries match that build, then stamp the tree mkarchiso will copy.
echo "Staging appearance (${APPEARANCE_VARIANT:-01}) and the boot animation..."
"${SCRIPT_DIR}/scripts/stage-components.sh"
echo "Building and staging 02 Agent Runtime (02, 02agent, 02-agentd)..."
"${SCRIPT_DIR}/scripts/build-agent-runtime.sh"
"${SCRIPT_DIR}/scripts/verify-staged-runtime.sh"
revision="$(git -C "${SCRIPT_DIR}" rev-parse HEAD)"
printf 'built_by=build.sh\nrevision=%s\n' "${revision}" \
  > "${PROFILE_DIR}/airootfs/usr/lib/02-agent/BUILD_STAMP"

echo "============================================================"
echo " Building 02_OS ISO via Docker (Arch Linux container)"
echo " Profile: ${PROFILE_DIR}"
echo " Output:  ${OUT_DIR}"
echo " Work:    ${WORK_DIR}"
echo " Cache:   ${PACMAN_CACHE_DIR}"
echo "============================================================"

docker run --rm --privileged \
  -e HOST_UID="${HOST_UID}" \
  -e HOST_GID="${HOST_GID}" \
  -e WORK_DIR="${WORK_DIR}" \
  -v "${PROFILE_DIR}:/02_OS" \
  -v "${OUT_DIR}:/out" \
  -v "${WORK_DIR}:${WORK_DIR}" \
  -v "${PACMAN_CACHE_DIR}:/var/cache/pacman/pkg" \
  archlinux:latest bash -c '
    set -euo pipefail
    trap "chown -R \"${HOST_UID}:${HOST_GID}\" /out \"${WORK_DIR}\" 2>/dev/null || true" EXIT
    pacman -Syu --noconfirm archiso
    mkarchiso -v -w "${WORK_DIR}" -o /out /02_OS
  '

echo ""
echo "============================================================"
echo " ISO Build Complete!"
echo " Output ISO directory: ${OUT_DIR}"
echo "============================================================"
ls -lh "${OUT_DIR}"
