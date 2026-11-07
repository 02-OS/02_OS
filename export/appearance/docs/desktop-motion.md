# Desktop motion

02_OS uses Dash to Dock 109, Flourish 1.2.0 and Compiz Windows Effect 31 on
GNOME Shell 50. The packages are vendored in
`profile/airootfs/usr/share/gnome-shell/extensions`; their download URLs, archive
SHA-256 hashes, licenses and local patches are in
[desktop-motion-vendor.json](desktop-motion-vendor.json).

## Defaults

The centered bottom dock keeps its 48 px icons, favorites, running indicators,
window previews and existing hover background fade. Flourish adds 1.20× hover
magnification, a 3 px lift, 160 ms easing, two neighboring icons and dynamic
spacing. Launching an app adds one bounce, about 21 px over 450 ms. Urgent apps
use a short wiggle. Press motion is disabled.

Dragging a normal window or dialog adds a spring deformation. The Compiz
Realistic preset uses friction 3.5, spring constant 3.8, mass 70, speed divisor
12 and a 6×6 deformation mesh. Resize and maximize effects are disabled.
Pop Shell keeps its existing floating default and `Super+Y` tiling shortcut.
Its active-window outline is disabled and transparent. The local Pop Shell
patch stops the border timer from showing an outline after it is disabled.

The local renderer patch clears depth before drawing the flat spring mesh.
This prevents the window content from disappearing in virtual/offscreen
rendering targets, as reproduced in the accelerated QEMU VM.

Both motion extensions honor GNOME's animation setting. The Compiz patch also
clears an active deformation when animations are disabled or the session locks.
The settings are defaults, so users can override them.

## Controls

Run these commands inside the GNOME session:

```bash
# Turn all desktop animations off, or restore them with true.
gsettings set org.gnome.desktop.interface enable-animations false

# Disable an effect independently; use `enable` to restore it.
gnome-extensions disable flourish@orsso.github.io
gnome-extensions disable compiz-windows-effect@hermes83.github.com

# Open the upstream preference panels.
gnome-extensions prefs flourish@orsso.github.io
gnome-extensions prefs compiz-windows-effect@hermes83.github.com

# Remove the active-window outline, including any outline already on screen.
gsettings set org.gnome.shell.extensions.pop-shell active-hint false
gsettings set org.gnome.shell.extensions.pop-shell hint-color-rgba 'rgba(0,0,0,0)'
```

On Wayland, sign out and back in after replacing extension code. Changes made
only to a live VM are temporary and are lost on reboot. The repository changes
are included by the next normal `./build.sh`; an existing ISO is not modified.

## Source changes and existing images

Committing or pushing this source preserves the implementation on GitHub. It
does not update a running VM or the ISO used to launch it. The locally tested
`02_OS-2026.10.01-x86_64.iso` predates the motion changes: a fresh boot has Dash
to Dock 106, neither motion extension, and Pop Shell's orange outline enabled.
Changes applied inside that live session are in memory and disappear when the
VM shuts down.

To include the changes in a fresh live session, check out the commit containing
this implementation and run `./build.sh` from the repository root. This is the
supported build path and needs Docker, Cargo and the documented build space;
see the [build instructions](../README.md#building). Launch the newly generated
ISO from `out/` and verify that all four extensions appear as enabled and active:

```bash
gnome-extensions list --details
gsettings get org.gnome.shell.extensions.pop-shell active-hint
gsettings get org.gnome.shell.extensions.pop-shell hint-color-rgba
```

The outline checks should return `false` and `'rgba(0,0,0,0)'`. The dock version
should be 109, Flourish 1.2.0, and Compiz Windows Effect 31. No rebuilt ISO was
produced during the motion implementation; validation used the previous ISO
with the source changes applied to its temporary live filesystem.

## Build and installation

`root/customize_airootfs.sh` compiles both the global GLib schemas and each
extension's local schemas with `--strict`, then updates dconf. `02os-provision`
requires and copies all four desktop extensions and their global schemas to an
installed system, and compiles local schemas there too.

New live images use GDM for the native GNOME login screen and screen locking.
Autologin is disabled: sign in with the existing `live` account and password
`live`. GNOME Shell 50 creates its screen shield only when GDM's authentication
service is available; registering a Wayland session through greetd alone does
not provide native locking. The live GDM configuration and live service
activation are excluded from installed systems, which keep their account and
display-manager policy.

The compatibility helper `02os-configure-greetd` still derives a graphical PAM
session stack from Arch's `system-login` and changes only greetd's session
include. It remains available for existing greetd sessions; GDM uses its own
distribution PAM stack.

## Updating the packages

1. Download the exact archives recorded in the vendor manifest and verify their
   SHA-256 hashes before extraction. For a version update, check GNOME 50 support
   in the package metadata and update the manifest deliberately.
2. Replace each complete extension directory, retaining its license and assets.
   Restore the local Dash to Dock CSS block and the documented Compiz patches.
   Keep the Pop Shell border timer patch when updating that existing extension.
   Flourish uses internal dock APIs, so update and test the dock and Flourish
   together.
3. Copy the extension schema XML files into the global schema directory. Keep
   these copies identical; generate compiled schemas during the build.
4. Run the profile and relative-import tests:

   ```bash
   python3 -m unittest discover -s scripts/tests -p test_os_profile.py -v
   python3 -m unittest discover -s scripts/tests -p test_dash_to_dock_imports.py -v
   ```

5. In a GNOME 50 VM, test hover transitions, app launch, drag and release,
   maximize/unmaximize, overview, closing a dragged window, extension disable
   and animation disable. Back up the guest extension folders, schema files and
   user dconf before activation. Restore those files and dconf if the desktop
   fails to start.

## VM verification

Verified on 2026-10-06 with GNOME Shell 50.5 and accelerated virtio graphics:
18 profile/import checks passed and 59 motion extension JavaScript modules
parsed successfully. The Pop Shell border timer also passed a regression check
that turns the outline off after its timer starts and verifies it stays hidden.

Hover reaches 1.20× and returns to rest, one launch bounce peaks at 21 px, moving windows
deform and settle, and window content stays visible during deformation.
Resize/maximize remain normal. Animation disable, extension disable, overview,
closing a moved window and `Super+Y` tiling were exercised. The dock also kept
correct geometry at 1920×1080 with scales 1 and 2; the original mode was restored.

The tested greetd live session had no ScreenSaver service and did not respond
to logind's lock request, so an actual lock/unlock cycle could not be exercised
there. The extension guards and cleanup for locked sessions are implemented;
verify the lock cycle in an installed GNOME session with its locker available.
