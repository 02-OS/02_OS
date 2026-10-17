#!/usr/bin/env bash
set -euo pipefail

# Build-time customization for 02_OS ISO.
# A manual mkarchiso stops here unless ./build.sh staged a matching runtime.

# pacstrap leaves /etc/os-release as a symlink to the filesystem package's
# vendor file. Copying the profile over that symlink can update only one path.
# Install the profile identity as a regular file at both paths.
install_02os_identity() {
  local root="${1%/}"
  local identity etc_path vendor_path path
  identity="$(mktemp)"
  etc_path="${root}/etc/os-release"
  vendor_path="${root}/usr/lib/os-release"
  if [[ ! -f "${etc_path}" ]]; then
    echo "ERROR: ${etc_path} is missing" >&2
    rm -f "${identity}"
    return 1
  fi
  cat -- "${etc_path}" > "${identity}"
  if ! grep -qx 'NAME="02_OS"' "${identity}" \
    || ! grep -qx 'PRETTY_NAME="02_OS"' "${identity}" \
    || grep -q 'Arch Linux' "${identity}"; then
    echo "ERROR: ${etc_path} is not the 02_OS identity" >&2
    rm -f "${identity}"
    return 1
  fi
  for path in "${etc_path}" "${vendor_path}"; do
    if [[ -d "${path}" ]]; then
      echo "ERROR: ${path} is a directory" >&2
      rm -f "${identity}"
      return 1
    fi
  done
  mkdir -p -- "$(dirname -- "${etc_path}")" "$(dirname -- "${vendor_path}")"
  rm -f -- "${etc_path}" "${vendor_path}"
  install -m 644 "${identity}" "${vendor_path}"
  install -m 644 "${identity}" "${etc_path}"
  rm -f "${identity}"
}

/usr/local/bin/02os-check-runtime
/usr/local/bin/02os-check-desktop
/usr/local/bin/02os-check-desktop --installer

echo "==> Verifying preinstalled Chrome and Brave..."
# mkarchiso has already installed packages.x86_64 with pacstrap. Do not start a
# second transaction in this chroot, which has no copied build-host keyring.
/usr/local/bin/02os-install-browsers --verify-only

echo "==> Installing the 02_OS identity..."
install_02os_identity /

echo "==> Configuring greetd to register a Wayland login session..."
/usr/local/bin/02os-configure-greetd

echo "==> Compiling GLib schemas in /usr/share/glib-2.0/schemas..."
glib-compile-schemas --strict /usr/share/glib-2.0/schemas
while IFS= read -r -d '' schema_dir; do
  glib-compile-schemas --strict "${schema_dir}"
done < <(find /usr/share/gnome-shell/extensions -type d -name schemas -print0)

echo "==> Updating dconf system database..."
dconf update

echo "==> Generating icon theme cache for 02-OS..."
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/02-OS || true
fi

echo "==> Enforcing polkit rules directory permissions..."
if [[ -d /etc/polkit-1/rules.d ]]; then
  chmod 750 /etc/polkit-1/rules.d
  chmod 644 /etc/polkit-1/rules.d/* 2>/dev/null || true
fi

echo "==> Locking the root account for the live environment..."
passwd -l root 2>/dev/null || true

echo "==> Uncommenting HTTPS mirrors in /etc/pacman.d/mirrorlist..."
if [[ -f /etc/pacman.d/mirrorlist ]]; then
  sed -i -E 's/^#(Server = https:)/\1/' /etc/pacman.d/mirrorlist 2>/dev/null || true
  if ! grep -q '^Server = https' /etc/pacman.d/mirrorlist 2>/dev/null; then
    sed -i -E 's/^#(Server =)/\1/' /etc/pacman.d/mirrorlist 2>/dev/null || true
  fi
fi
