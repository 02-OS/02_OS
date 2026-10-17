# Disposable image acceptance

Build the provided runner image once:

```bash
docker build -t 02os-acceptance-runner:local -f scripts/vm/Dockerfile scripts/vm
```

`runner.py` uses this local Docker image containing QEMU, OVMF, and EGL support.
Set `--runner-image` to use a different prepared runner image. It must contain
`/usr/bin/qemu-system-x86_64` and `/usr/bin/qemu-img`. The VM receives only its fresh
run directory, a read-only ISO, `/dev/kvm`, and the selected GPU render device
for accelerated graphics. It receives no host block device or host home share.
If the host's first DNS resolver cannot be reached from Docker, supply a
reachable upstream address with `prepare --dns ADDRESS`. QEMU's DNS forwarding
uses the container resolver; this option changes only the disposable runner.

Prepare a run with a completed new ISO and an empty evidence directory:

```bash
python3 scripts/vm/runner.py prepare --iso /absolute/out/02_OS-NEW-x86_64.iso \
  --runs-root /absolute/acceptance-runs --firmware uefi --graphics software
python3 scripts/vm/runner.py start --run /absolute/acceptance-runs/RUN --phase live
python3 scripts/vm/runner.py capture --run /absolute/acceptance-runs/RUN
python3 scripts/vm/runner.py snapshot --run /absolute/acceptance-runs/RUN
```

Use `--firmware bios` without the OVMF options for a BIOS live boot, or
`--graphics gl` for accelerated virtio graphics. UEFI defaults to the image's
matching OVMF templates; supply both `--firmware-code` and `--firmware-vars` to
select explicit matching templates. Capture the real GDM login and
sign in using the live account before checking desktop extension state. Guest
snapshots collect package versions, service state, extension state, networking,
audio sinks, and the portal result. They are evidence, not an automatic claim
that every visual or hardware acceptance check passed.
Some QEMU/virgl combinations report `no surface` for host screenshots after
the guest switches to accelerated scanout. In that case, use GNOME's
`Shift+Print` shortcut after login and retain the resulting image from the
guest's `Pictures/Screenshots` directory. Do not treat a missing host capture
as proof of a working or failed guest display.

For a fresh UEFI run, `install` loads the canonical installed-VM credentials,
hashes the password, verifies the only persistent guest disk's serial/size and
empty partition inventory, and runs an Archinstall dry run before installation.
It refuses reuse of a partially installed guest. Package signature checking
stays enabled. The host stops the VM if physical free space drops below 2 GiB.

```bash
python3 scripts/vm/runner.py install --run /absolute/acceptance-runs/RUN
python3 scripts/vm/runner.py stop --run /absolute/acceptance-runs/RUN
python3 scripts/vm/runner.py start --run /absolute/acceptance-runs/RUN --phase disk
python3 scripts/vm/runner.py capture --run /absolute/acceptance-runs/RUN
python3 scripts/vm/runner.py snapshot --run /absolute/acceptance-runs/RUN
```

Disk boot removes the ISO. Sign in using `scripts/vm-test-credentials.json`.
The VM exposes an Intel HDA duplex device and records guest playback in a
run-local WAV file. Keep audio files, screenshots, journals, and metadata with
their exact ISO checksum. Keep historical proof disks and captures intact.
The guarded installer currently supports the reviewed UEFI, unencrypted ext4
fixture; BIOS installation and encrypted-disk installation need separate
acceptance fixtures before those paths can be claimed as verified.
