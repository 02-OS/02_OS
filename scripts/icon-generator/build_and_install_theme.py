#!/usr/bin/env python3
import os
import sys
import shutil
import subprocess
import xml.etree.ElementTree as ET

# Import icon definition modules
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, SCRIPT_DIR)
from icons_apps import get_app_icons
from icons_browsers import BROWSER_ICON_ALIASES, get_browser_icons
from icons_places import get_places_icons
from icons_devices_categories import get_devices_icons, get_categories_icons, get_mimetypes_icons
from icons_status_actions import get_status_icons, get_actions_icons

REPO_ROOT = os.path.abspath(os.path.join(SCRIPT_DIR, "..", ".."))

TARGET_PATHS = [
    os.path.join(REPO_ROOT, "profile", "airootfs", "usr", "share", "icons", "02-OS"),
]
if "--install-user" in sys.argv or "--install" in sys.argv:
    TARGET_PATHS.append(os.path.expanduser("~/.local/share/icons/02-OS"))


INDEX_THEME = """[Icon Theme]
Name=02-OS
Comment=02_OS Premium Glassmorphic Vector Icon Theme
Inherits=Papirus-Dark,Adwaita,hicolor
Example=02os-menu

Directories=scalable/apps,scalable/places,scalable/devices,scalable/categories,scalable/mimetypes,scalable/status,scalable/actions

[scalable/apps]
Context=Applications
Size=64
MinSize=16
MaxSize=512
Type=Scalable

[scalable/places]
Context=Places
Size=64
MinSize=16
MaxSize=512
Type=Scalable

[scalable/devices]
Context=Devices
Size=64
MinSize=16
MaxSize=512
Type=Scalable

[scalable/categories]
Context=Categories
Size=64
MinSize=16
MaxSize=512
Type=Scalable

[scalable/mimetypes]
Context=MimeTypes
Size=64
MinSize=16
MaxSize=512
Type=Scalable

[scalable/status]
Context=Status
Size=64
MinSize=16
MaxSize=512
Type=Scalable

[scalable/actions]
Context=Actions
Size=64
MinSize=16
MaxSize=512
Type=Scalable
"""

def wrap_svg(defs, body, width=512, height=512):
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="100%" height="100%">
  <defs>
    <!-- Ambient Drop Shadow -->
    <filter id="shadow" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="18" stdDeviation="20" flood-color="#000000" flood-opacity="0.45"/>
    </filter>
    <filter id="folder-shadow" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="14" stdDeviation="16" flood-color="#000000" flood-opacity="0.40"/>
    </filter>
    <filter id="pocket-shadow" x="-15%" y="-15%" width="130%" height="130%">
      <feDropShadow dx="0" dy="8" stdDeviation="10" flood-color="#000000" flood-opacity="0.32"/>
    </filter>
    <filter id="glyph-shadow" x="-25%" y="-25%" width="150%" height="150%">
      <feDropShadow dx="0" dy="8" stdDeviation="10" flood-color="#000000" flood-opacity="0.36"/>
    </filter>
    <filter id="glow" x="-30%" y="-30%" width="160%" height="160%">
      <feGaussianBlur stdDeviation="12" result="blur"/>
      <feComposite in="SourceGraphic" in2="blur" operator="over"/>
    </filter>

    <!-- Glass Specular Rim Highlight -->
    <linearGradient id="specular-rim" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ffffff" stop-opacity="0.50"/>
      <stop offset="25%" stop-color="#ffffff" stop-opacity="0.18"/>
      <stop offset="70%" stop-color="#ffffff" stop-opacity="0.04"/>
      <stop offset="100%" stop-color="#ffffff" stop-opacity="0.12"/>
    </linearGradient>

    <!-- Top Gloss Sheen Overlay -->
    <linearGradient id="top-gloss" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ffffff" stop-opacity="0.32"/>
      <stop offset="40%" stop-color="#ffffff" stop-opacity="0.06"/>
      <stop offset="100%" stop-color="#ffffff" stop-opacity="0.0"/>
    </linearGradient>
    {defs}
  </defs>
  {body}
</svg>"""

def squircle_card(bg_id, inner_art):
    return f"""
  <!-- Base Squircle with Shadow -->
  <rect x="36" y="36" width="440" height="440" rx="104" ry="104" fill="url(#{bg_id})" filter="url(#shadow)"/>

  <!-- Top Glass Sheen -->
  <path d="M 36 140 C 36 82, 82 36, 140 36 L 372 36 C 430 36, 476 82, 476 140 L 476 210 C 360 240, 150 240, 36 210 Z" fill="url(#top-gloss)"/>

  <!-- Center Hero Artwork -->
  <g filter="url(#glyph-shadow)">
    {inner_art}
  </g>

  <!-- Specular Rim Border -->
  <rect x="36" y="36" width="440" height="440" rx="104" ry="104" fill="none" stroke="url(#specular-rim)" stroke-width="2.5"/>
"""

def generate_browser_icons_at(base_dir):
    """Update only browser art and aliases, preserving all other theme files."""
    apps_dir = os.path.join(base_dir, "scalable", "apps")
    os.makedirs(apps_dir, exist_ok=True)
    for name, (defs, background, art) in get_browser_icons().items():
        path = os.path.join(apps_dir, f"{name}.svg")
        with open(path, "w") as icon:
            icon.write(wrap_svg(defs, squircle_card(background, art)))
    for name, target in BROWSER_ICON_ALIASES.items():
        path = os.path.join(apps_dir, name)
        if os.path.lexists(path):
            os.remove(path)
        os.symlink(target, path)
    print(f"Generated Chrome and Brave icons at: {apps_dir}")

def generate_theme_at(base_dir):
    print(f"Generating 02-OS theme at: {base_dir}")
    os.makedirs(base_dir, exist_ok=True)

    # Write index.theme
    with open(os.path.join(base_dir, "index.theme"), "w") as f:
        f.write(INDEX_THEME)

    # 1. Apps
    apps_dir = os.path.join(base_dir, "scalable", "apps")
    os.makedirs(apps_dir, exist_ok=True)
    apps = get_app_icons()
    for name, (d, bg, art) in apps.items():
        svg_content = wrap_svg(d, squircle_card(bg, art))
        filepath = os.path.join(apps_dir, f"{name}.svg")
        with open(filepath, "w") as f:
            f.write(svg_content)

    # Keep OS branding faithful to the approved boot logo, without a menu card.
    with open(os.path.join(REPO_ROOT, "scripts", "boot-animation", "02-logo.svg")) as f:
        logo_svg = f.read()
    logo_root = ET.fromstring(logo_svg)
    for dimension, size in (("width", "96"), ("height", "88")):
        logo_svg = logo_svg.replace(
            f'{dimension}="{logo_root.attrib[dimension]}"', f'{dimension}="{size}"', 1)
    logo_path = os.path.join(apps_dir, "02os-logo.svg")
    if os.path.islink(logo_path):
        os.remove(logo_path)
    with open(logo_path, "w") as f:
        f.write(logo_svg)

    # Symlinks for Apps
    app_symlinks = {
        **BROWSER_ICON_ALIASES,
        "start-here.svg": "02os-menu.svg",
        "distributor-logo.svg": "02os-logo.svg",
        "archlinux-logo.svg": "02os-logo.svg",
        "org.xfce.thunar.svg": "thunar.svg",
        "system-file-manager.svg": "thunar.svg",
        "nautilus.svg": "thunar.svg",
        "org.gnome.Nautilus.svg": "thunar.svg",
        "file-manager.svg": "thunar.svg",
        "org.gnome.Console.svg": "kitty.svg",
        "terminal.svg": "kitty.svg",
        "utilities-terminal.svg": "kitty.svg",
        "xfce4-terminal.svg": "kitty.svg",
        "xterm.svg": "kitty.svg",
        "firefox-developer-edition.svg": "firefox.svg",
        "web-browser.svg": "firefox.svg",
        "internet-web-browser.svg": "firefox.svg",
        "org.pulseaudio.pavucontrol.svg": "pavucontrol.svg",
        "multimedia-volume-control.svg": "pavucontrol.svg",
        "volume-control.svg": "pavucontrol.svg",
        "system-software-install.svg": "02os-install.svg",
        "archinstall.svg": "02os-install.svg",
        "calamares.svg": "02os-install.svg",
        "fuzzel.svg": "02os-spotlight.svg",
        "system-search.svg": "02os-spotlight.svg",
        "preferences-system-search.svg": "02os-spotlight.svg",
        "search.svg": "02os-spotlight.svg",
        "notifications.svg": "swaync.svg",
        "preferences-desktop-notification-bell.svg": "swaync.svg",
        "notification.svg": "swaync.svg",
        "blueman-manager.svg": "blueman.svg",
        "bluetooth.svg": "blueman.svg",
        "preferences-system-bluetooth.svg": "blueman.svg",
        "network-wireless.svg": "nm-connection-editor.svg",
        "preferences-system-network.svg": "nm-connection-editor.svg",
        "network.svg": "nm-connection-editor.svg",
        "accessories-screenshot.svg": "02os-screenshot.svg",
        "applets-screenshooter.svg": "02os-screenshot.svg",
        "grim.svg": "02os-screenshot.svg",
        "slurp.svg": "02os-screenshot.svg",
        "screenshot.svg": "02os-screenshot.svg",
        "system-info.svg": "fastfetch.svg",
        "utilities-system-monitor.svg": "fastfetch.svg",
        "gnome-system-monitor.svg": "fastfetch.svg",
        "preferences-system.svg": "gnome-control-center.svg",
        "systemsettings.svg": "gnome-control-center.svg",
        "settings.svg": "gnome-control-center.svg",
        "org.gnome.tweaks.svg": "gnome-tweaks.svg",
        "preferences-desktop.svg": "gnome-tweaks.svg",
        "ark.svg": "xarchiver.svg",
        "utilities-file-archiver.svg": "xarchiver.svg",
        "file-roller.svg": "xarchiver.svg",
        "mousepad.svg": "accessories-text-editor.svg",
        "text-editor.svg": "accessories-text-editor.svg",
        "neovim.svg": "accessories-text-editor.svg",
        "nvim.svg": "accessories-text-editor.svg",
        "gedit.svg": "accessories-text-editor.svg",
        "calc.svg": "accessories-calculator.svg",
        "gnome-calculator.svg": "accessories-calculator.svg",
        "evince.svg": "document-viewer.svg",
        "xreader.svg": "document-viewer.svg",
        "image-viewer.svg": "multimedia-photo-viewer.svg",
        "viewnior.svg": "multimedia-photo-viewer.svg",
        "eog.svg": "multimedia-photo-viewer.svg",
        "gthumb.svg": "multimedia-photo-viewer.svg",
        "audio-player.svg": "multimedia-player.svg",
        "video-player.svg": "multimedia-player.svg",
        "mpv.svg": "multimedia-player.svg",
        "vlc.svg": "multimedia-player.svg",
        "hyprlock.svg": "system-lock-screen.svg",
        "lock.svg": "system-lock-screen.svg",
        "help-browser.svg": "system-help.svg",
        "02os-guide.svg": "system-help.svg",
        "system-software-update.svg": "software-properties.svg"
    }
    for src, target in app_symlinks.items():
        link_path = os.path.join(apps_dir, src)
        if os.path.exists(link_path) or os.path.islink(link_path):
            os.remove(link_path)
        os.symlink(target, link_path)

    # 2. Places
    places_dir = os.path.join(base_dir, "scalable", "places")
    os.makedirs(places_dir, exist_ok=True)
    places = get_places_icons()
    for name, (d, body) in places.items():
        svg_content = wrap_svg(d, body)
        with open(os.path.join(places_dir, f"{name}.svg"), "w") as f:
            f.write(svg_content)

    places_symlinks = {
        "user-home.svg": "folder-home.svg",
        "user-desktop.svg": "folder-desktop.svg",
        "network-server.svg": "folder-remote.svg",
        "trashcan_empty.svg": "user-trash.svg",
        "trashcan_full.svg": "user-trash-full.svg"
    }
    for src, target in places_symlinks.items():
        link_path = os.path.join(places_dir, src)
        if os.path.exists(link_path) or os.path.islink(link_path):
            os.remove(link_path)
        os.symlink(target, link_path)

    # 3. Devices
    devices_dir = os.path.join(base_dir, "scalable", "devices")
    os.makedirs(devices_dir, exist_ok=True)
    devices = get_devices_icons()
    for name, (d, bg, art) in devices.items():
        svg_content = wrap_svg(d, squircle_card(bg, art))
        with open(os.path.join(devices_dir, f"{name}.svg"), "w") as f:
            f.write(svg_content)

    devices_symlinks = {
        "drive-harddisk-system.svg": "drive-harddisk.svg",
        "media-flash.svg": "drive-removable-media.svg",
        "drive-optical.svg": "media-optical.svg",
        "video-display.svg": "computer.svg"
    }
    for src, target in devices_symlinks.items():
        link_path = os.path.join(devices_dir, src)
        if os.path.exists(link_path) or os.path.islink(link_path):
            os.remove(link_path)
        os.symlink(target, link_path)

    # 4. Categories
    cat_dir = os.path.join(base_dir, "scalable", "categories")
    os.makedirs(cat_dir, exist_ok=True)
    categories = get_categories_icons()
    for name, (d, bg, art) in categories.items():
        svg_content = wrap_svg(d, squircle_card(bg, art))
        with open(os.path.join(cat_dir, f"{name}.svg"), "w") as f:
            f.write(svg_content)

    # 5. Mimetypes
    mime_dir = os.path.join(base_dir, "scalable", "mimetypes")
    os.makedirs(mime_dir, exist_ok=True)
    mimetypes = get_mimetypes_icons()
    for name, (d, body) in mimetypes.items():
        svg_content = wrap_svg(d, body)
        with open(os.path.join(mime_dir, f"{name}.svg"), "w") as f:
            f.write(svg_content)

    # 6. Status
    status_dir = os.path.join(base_dir, "scalable", "status")
    os.makedirs(status_dir, exist_ok=True)
    statuses = get_status_icons()
    for name, (d, body) in statuses.items():
        svg_content = wrap_svg(d, body)
        with open(os.path.join(status_dir, f"{name}.svg"), "w") as f:
            f.write(svg_content)

    # 7. Actions
    actions_dir = os.path.join(base_dir, "scalable", "actions")
    os.makedirs(actions_dir, exist_ok=True)
    actions = get_actions_icons()
    for name, (d, body) in actions.items():
        svg_content = wrap_svg(d, body)
        with open(os.path.join(actions_dir, f"{name}.svg"), "w") as f:
            f.write(svg_content)

    # Run gtk-update-icon-cache if available
    try:
        subprocess.run(["gtk-update-icon-cache", "-f", "-t", base_dir], check=True, capture_output=True)
        print(f"Icon cache successfully generated for {base_dir}")
    except Exception as e:
        print(f"Note: gtk-update-icon-cache notice: {e}")

def main():
    failed = []
    for target in TARGET_PATHS:
        try:
            if "--browsers-only" in sys.argv:
                generate_browser_icons_at(target)
            else:
                generate_theme_at(target)
        except Exception as err:
            failed.append(target)
            print(f"Error generating at {target}: {err}", file=sys.stderr)
            continue
        print(f"Theme generation complete for {target}")
    if failed:
        print(
            "ERROR: required theme write failed for: " + ", ".join(failed),
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
