#!/usr/bin/env bash
# Install appearance and the boot animation into profile/airootfs.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
# shellcheck source=resolve-components.sh
source "${script_dir}/resolve-components.sh"

variant="${APPEARANCE_VARIANT:-01}"
if [[ ! "${variant}" =~ ^(01|02|03)$ ]]; then
  echo "ERROR: APPEARANCE_VARIANT must be 01, 02, or 03." >&2
  exit 1
fi

appearance="$(resolve_one APPEARANCE_DIR apply.sh vendor/appearance appearance)"
boot="$(resolve_one BOOT_ANIMATION_DIR install-into.sh vendor/boot-animation boot-animation)"
agent="$(resolve_one AGENT_RUNTIME_DIR Cargo.toml vendor/agent-runtime agent-runtime)"
overlay="${repo_root}/profile/airootfs"

"${appearance}/apply.sh" "${variant}" "${overlay}"
"${boot}/install-into.sh" "${overlay}"
mkdir -p "${overlay}/usr/lib/systemd/user"
cp -a "${agent}/packaging/02-agentd.service" "${overlay}/usr/lib/systemd/user/02-agentd.service"

if ! cmp -s "${boot}/02-logo.svg" "${appearance}/base/02-logo.svg"; then
  echo "ERROR: boot and appearance 02-logo.svg files differ." >&2
  exit 1
fi

wallpaper="${overlay}/usr/share/backgrounds/02os/desktop.jpg"
halo="${overlay}/usr/share/backgrounds/02os/portal-halo.png"
if python3 -c 'import numpy, PIL' >/dev/null 2>&1; then
  python3 "${boot}/render_lock_field.py" --wallpaper "${wallpaper}" --output "${halo}"
else
  echo "Pillow or numpy is missing. Keeping the committed greeter field."
fi

echo "Staged appearance ${variant} and the boot animation."
