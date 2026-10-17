#!/usr/bin/env bash
# shellcheck disable=SC2034

iso_name="02_OS"
iso_label="02_OS_$(date --date="@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y%m)"
iso_publisher="02_OS <https://github.com/csy20/02_OS>"
iso_application="02_OS Live"
iso_version="$(date --date="@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y.%m.%d)"
install_dir="arch"
buildmodes=('iso')
bootmodes=('bios.syslinux'
           'uefi.systemd-boot')
pacman_conf="pacman.conf"
airootfs_image_type="squashfs"
airootfs_image_tool_options=('-comp' 'zstd' '-Xcompression-level' '15' '-b' '1M')
bootstrap_tarball_compression=('zstd' '-c' '-T0' '--auto-threads=logical' '--long' '-19')
file_permissions=(
  ["/etc/polkit-1/rules.d"]="0:0:750"
  ["/etc/polkit-1/rules.d/10-live-power.rules"]="0:0:644"
  ["/etc/sudoers.d/01-live"]="0:0:440"
  ["/root"]="0:0:750"
  ["/root/.gnupg"]="0:0:700"
  ["/root/customize_airootfs.sh"]="0:0:755"
  ["/usr/local/bin/choose-mirror"]="0:0:755"
  ["/usr/local/bin/Installation_guide"]="0:0:755"
  ["/usr/local/bin/livecd-sound"]="0:0:755"
  ["/usr/local/bin/02os-gnome-session"]="0:0:755"
  ["/usr/local/bin/02os-configure-greetd"]="0:0:755"
  ["/usr/local/bin/pop-shell-shortcuts"]="0:0:755"
  ["/usr/local/bin/02os-ensure-live-user"]="0:0:755"
  ["/usr/local/bin/02os-install"]="0:0:755"
  ["/usr/local/bin/02os-provision"]="0:0:755"
  ["/usr/local/bin/02os-install-browsers"]="0:0:755"
  ["/usr/local/bin/pop-launcher"]="0:0:755"
  ["/usr/local/bin/02os-check-runtime"]="0:0:755"
  ["/usr/local/bin/02os-check-desktop"]="0:0:755"
  ["/usr/local/share/02os/archinstall_plugin.py"]="0:0:644"
  ["/usr/bin/02"]="0:0:755"
  ["/usr/bin/02agent"]="0:0:755"
  ["/usr/bin/02-agentd"]="0:0:755"
  ["/usr/lib/systemd/user/02-agentd.service"]="0:0:644"
  ["/usr/lib/02-agent/prepare-storage.sh"]="0:0:755"
  ["/usr/lib/systemd/user-generators/02-agent-storage"]="0:0:755"
  ["/usr/lib/tmpfiles.d/02os-zero-portal.conf"]="0:0:644"
  ["/usr/share/gnome-shell/extensions/02-zero-portal@02os"]="0:0:755"
  ["/usr/share/gnome-shell/extensions/02-zero-portal@02os/extension.js"]="0:0:644"
  ["/usr/share/gnome-shell/extensions/02-zero-portal@02os/motion.js"]="0:0:644"
  ["/usr/share/gnome-shell/extensions/02-zero-portal@02os/metadata.json"]="0:0:644"
  ["/usr/share/gnome-shell/extensions/02-zero-portal@02os/stylesheet.css"]="0:0:644"
  ["/usr/share/backgrounds/02os/portal-halo.png"]="0:0:644"
)
