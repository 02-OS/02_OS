# Zero Portal boot transition

`02-logo.svg` is the approved vector master. The old turn-and-ripple theme and
its projected PNG frames have been removed. `generate_assets.py` now renders
one high-resolution transparent logo for the native `02-zero-portal` theme.
Pillow and pycairo are needed only to regenerate that asset, not to build an ISO.
The same command also refreshes the generated paths in GNOME's `motion.js`,
so the retained boot logo and the first reveal frame use identical geometry.
The revised master follows the supplied October 15 logo, including the 0's
flat lower edge and the rounded join into the shared baseline.

Plymouth fades the logo in over 0.25 seconds of real elapsed boot time, on a
smoother curve at 60 Hz, then holds it while the system starts. Refresh ticks
do not advance the timeline.
Real boot messages, disk-password prompts and questions retain their native
callbacks; shutdown/reboot never play the intro.

The `02-zero-portal@02os` GNOME Shell extension completes the transition in GDM:
the 2 slips behind the 0, and the 0 keeps its oval and expands from where it
sits to reveal the **live login screen** through its center. A transparent hole
in a decorative overlay reveals GNOME's own actors; no login screenshot or
imitation password field is used. The frame clock paces the reveal. The
easing has no bounce or overshoot, and the hole and the white rim ease out
instead of cutting off.

This extension runs only in the greeter session. An atomic marker under
`/run/02os-zero-portal`, created by the owned tmpfiles configuration, permits
one reveal per system boot even if GDM replaces its ephemeral account. The
mask is nonreactive and exits immediately on interaction, errors, reduced
motion, monitor changes, session changes or a bounded timeout. Authentication
and system controls remain owned by GNOME.

GDM's dconf profile enables only this greeter extension. Ordinary desktop
extensions and their user defaults remain separate. The provisioner copies
both boot stages, the greeter settings and the tmpfiles configuration into
installed systems. A new ISO must be built to include these changes.

Installed boot branding targets Archinstall 4.5. Its `on_install` hook installs
adapters on that installer instance before bootloader generation; a different
version or incompatible method signature fails explicitly. The title pass runs
immediately after generation and uses the current installation's BLS filenames,
GRUB local-kernel sections, or selected Limine/rEFInd configuration. Root and
encryption identities distinguish current entries from neighboring entries in
shared configurations. Foreign configurations and GRUB os-prober sections keep
their original contents.

EFISTUB uses a version-specific adapter of Archinstall's entry creation method
that supplies `02_OS` as the label. Disk, partition, kernel, initrd, command line,
and UKI handling remain the same; it never scans or renames existing firmware
entries. Recording tests compare the unmodified Archinstall 4.5 dispatch with
the adapter, without installing an OS or accessing disks or NVRAM:

```sh
python3 -m unittest discover -s scripts/tests -p 'test_installed_boot.py' -v
```

The adapter and its upstream recording fixture retain Archinstall's
`GPL-3.0-only` license. Source provenance, modifications and the full upstream
license are documented in [the fixture notes](../tests/fixtures/README.md).

```sh
python3 scripts/boot-animation/generate_assets.py
python3 scripts/boot-animation/render_lock_field.py
```

`check_native.c` exercises the actual Plymouth script plugin, including parse,
elapsed fade, repeated refresh, long-boot hold, prompts/messages, resizing,
shutdown and quit. Compile it against the matching installed Plymouth headers
and libraries. This verifies the native logo stage; the oval live reveal
requires a GNOME 50 VM check, including boot, interruption and greeter re-entry.

`portal-halo.png` is the greeter field: the desktop wallpaper under a dark
veil, with a soft cyan oval on the 0's resting point. The desktop itself
keeps the full wallpaper. The extension stylesheet keeps GNOME's real login and
password field and draws a cyan edge on that card. It loads only for the
greeter. The desktop wallpaper is separate, and an in-session lock screen
uses that wallpaper.
