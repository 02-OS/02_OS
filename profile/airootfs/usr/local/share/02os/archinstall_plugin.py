# SPDX-License-Identifier: GPL-3.0-only
# archinstall 4.5 (packages.x86_64) loads Plugin() and calls
# on_install(installation) at the end of Installer.minimal_installation,
# before bootloader generation and desktop profile installation. Set kernel
# flags at that point so every generated bootloader/UKI retains them.
# on_genfstab runs after the desktop profile and after add_bootloader.
# Select our theme again in case the installer selected another Plymouth
# theme. Boot titles are changed only while generating this installation's
# entries, never by scanning a shared ESP or another OS's menu entries.
__archinstall__version__ = 4.5
PLYMOUTH_THEME = "02-zero-portal"
OS_TITLE = "02_OS"

# Beyond archinstall's gnome + gnome-tweaks profile. Read from the live image.
INSTALLED_PACKAGES = "/usr/local/share/02os/installed-packages.txt"


def _read_installed_packages(path):
    packages = []
    with open(path, encoding="utf-8") as handle:
        for raw in handle:
            line = raw.split("#", 1)[0].strip()
            if line:
                packages.append(line)
    return packages


class Plugin:
    def on_install(self, installation):
        # Plymouth owns /etc/plymouth/plymouthd.conf. Install it before the
        # provisioner writes our configuration, avoiding an unowned-file conflict.
        installation.add_additional_packages(["plymouth"])
        self._provision(installation)
        self._prepare_boot(installation)
        self._configure_boot_branding(installation)
        self._activate_theme(installation)

    def on_genfstab(self, installation):
        self._install_desktop_packages(installation)
        self._provision(installation)
        installation.arch_chroot("/usr/local/bin/02os-check-desktop")
        # The desktop and its official dependencies are installed by this point.
        # Run inside the target so browser payloads and dependency transactions
        # cannot accidentally install into the live system instead.
        self._install_browsers(installation)
        self._activate_theme(installation)

    def _install_browsers(self, installation):
        import subprocess

        helper = "/usr/local/bin/02os-install-browsers"
        target = str(installation.target)
        # This query reads only the manifest and target package database. A
        # repeated final hook must not restore hundreds of MB of spent payload.
        result = subprocess.run(["arch-chroot", "-S", target, helper, "--verify-only"], check=False)
        if result.returncode != 0:
            subprocess.check_call(["/usr/local/bin/02os-provision", "--browser-payload", target])
            installation.arch_chroot(helper)
        # A failed query/stage/transaction leaves payload available for diagnosis.
        # Cleanup occurs only after the exact installed versions are confirmed.
        self._cleanup_browser_payload(installation)

    def _cleanup_browser_payload(self, installation):
        import hashlib
        import json
        from pathlib import Path
        import re

        target = Path(str(installation.target)).resolve()
        if target == Path("/"):
            raise RuntimeError("Refusing to clean browser payload in the live system")
        repo = target / "usr/local/share/02os/browser-repo"
        manifest_path = repo / "manifest.json"
        if repo.resolve() != repo or manifest_path.is_symlink():
            raise RuntimeError("Refusing to clean browser payload through a symlink")
        manifest = json.loads(manifest_path.read_text())
        repository = manifest["repository"]
        files = [(package["filename"], package["sha256"]) for package in manifest["packages"]]
        files.extend((repository[name], repository[digest]) for name, digest in (
            ("filename", "sha256"), ("database_filename", "database_sha256")))
        owned = []
        # Validate every candidate before removing any file. Unrelated package
        # caches, source payload and files placed alongside the repo stay intact.
        for name, expected in files:
            if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9.+_\-]*", name):
                raise RuntimeError("Invalid browser payload filename during cleanup")
            if name == "manifest.json" or not isinstance(expected, str) or not re.fullmatch(r"[0-9a-f]{64}", expected):
                raise RuntimeError("Invalid browser payload manifest during cleanup")
            path = repo / name
            if path.is_symlink():
                raise RuntimeError(f"Refusing to remove browser payload symlink: {name}")
            if not path.exists():
                continue
            if not path.is_file():
                raise RuntimeError(f"Refusing to remove browser payload directory: {name}")
            digest = hashlib.sha256()
            with path.open("rb") as payload:
                for chunk in iter(lambda: payload.read(1024 * 1024), b""):
                    digest.update(chunk)
            if digest.hexdigest() != expected:
                raise RuntimeError(f"Browser payload changed; preserving files for diagnosis: {name}")
            owned.append(path)
        for path in owned:
            path.unlink()

    def _prepare_boot(self, installation):
        # archinstall's own Plymouth integration uses these installer-owned
        # lists. Keep the installed target's autodetect, keyboard, encryption,
        # LVM, and resume hooks; never copy the live archiso hook configuration.
        hooks = getattr(installation, "_hooks", None)
        parameters = getattr(installation, "_kernel_params", None)
        if not isinstance(hooks, list) or not isinstance(parameters, list):
            raise RuntimeError("Unsupported archinstall boot configuration")
        backend = "systemd" if "systemd" in hooks else "udev"
        if backend not in hooks:
            raise RuntimeError("Plymouth requires a systemd or udev initramfs")

        hooks = [hook for hook in hooks if hook != "plymouth"]
        insert_at = hooks.index(backend) + 1
        if "kms" in hooks:
            insert_at = max(insert_at, hooks.index("kms") + 1)
        for encryption in ("encrypt", "sd-encrypt"):
            if encryption in hooks and hooks.index(encryption) < insert_at:
                raise RuntimeError("Unsupported initramfs encryption hook ordering")
        hooks.insert(insert_at, "plymouth")
        installation._hooks = hooks
        for parameter in ("quiet", "splash"):
            if parameter not in parameters:
                parameters.append(parameter)

    def _activate_theme(self, installation):
        installation.arch_chroot(f"plymouth-set-default-theme {PLYMOUTH_THEME}")
        if not installation.mkinitcpio(["-P"]):
            raise RuntimeError("Could not generate the installed Plymouth initramfs")

    def _configure_boot_branding(self, installation):
        # on_install runs before guided.py calls add_bootloader. Archinstall
        # 4.5 has no firmware-label option or post-bootloader plugin hook, so
        # adapt this Installer instance only. Never patch its module globals
        # or rename existing Boot#### entries in NVRAM.
        if not hasattr(installation, "add_bootloader"):
            # Lightweight provisioning test doubles have no bootloader API.
            return
        if getattr(installation, "_02os_boot_branding", False):
            return

        from importlib.metadata import version
        from inspect import signature
        from types import MethodType

        release = version("archinstall")
        if release.split(".")[:2] != ["4", "5"]:
            raise RuntimeError(f"02_OS boot branding requires archinstall 4.5, found {release}")
        expected = {
            "add_bootloader": ("bootloader", "uki_enabled", "bootloader_removable", "plymouth"),
            "_add_efistub_bootloader": ("boot_partition", "root", "uki_enabled"),
        }
        for name, parameters in expected.items():
            method = getattr(installation, name, None)
            if not callable(method) or tuple(signature(method).parameters) != parameters:
                raise RuntimeError(f"Unsupported archinstall 4.5 bootloader API: {name}")

        original = installation.add_bootloader

        def add_bootloader(_installation, bootloader, uki_enabled=False,
                           bootloader_removable=False, plymouth=None):
            result = original(bootloader, uki_enabled, bootloader_removable, plymouth)
            self._rename_os_titles(_installation, uki_enabled, bootloader_removable)
            return result

        installation._add_efistub_bootloader = MethodType(_add_02os_efistub_bootloader, installation)
        installation.add_bootloader = MethodType(add_bootloader, installation)
        installation._02os_boot_branding = True

    def _rename_os_titles(self, installation, uki_enabled=False, bootloader_removable=False):
        from pathlib import Path

        root = Path(str(installation.target))
        selected = installation._helper_flags.get("bootloader")
        if selected not in {"systemd", "grub", "limine", "refind"}:
            return
        boot = installation._get_boot_partition()
        efi = installation._get_efi_partition()
        parameters = installation._get_kernel_params(installation._get_root())
        # These are the exact output paths used by Archinstall 4.5. Invoke
        # immediately after generation, before later custom commands can add
        # foreign entries to these files. Other configurations are untouched.
        candidates = []
        if selected == "systemd" and not uki_enabled:
            entries = root / boot.relative_mountpoint / "loader/entries"
            candidates = [entries / f"{installation.init_time}_{kernel}.conf"
                          for kernel in installation.kernels]
        elif selected == "grub":
            candidates = [root / (boot.mountpoint or Path("/boot")).relative_to("/") / "grub/grub.cfg"]
        elif selected == "limine":
            if efi is not None:
                directory = "BOOT" if bootloader_removable else "arch-limine"
                candidates = [root / efi.mountpoint.relative_to("/") / "EFI" / directory / "limine.conf"]
            else:
                candidates = [root / "boot/limine/limine.conf"]
        elif selected == "refind":
            separate = boot != efi and boot.dev_path != efi.dev_path
            directory = boot.mountpoint.relative_to("/") if separate else Path("boot")
            candidates = [root / directory / "refind_linux.conf"]
        for path in candidates:
            if not path.is_file() or path.is_symlink():
                continue
            original = path.read_text()
            updated = (retitle_grub_text(original) if selected == "grub" else
                       retitle_owned_boot_text(original, parameters, selected))
            if updated != original:
                path.write_text(updated)

    def _install_desktop_packages(self, installation):
        add = getattr(installation, "add_additional_packages", None)
        # Recording stubs have no archinstall Installer API.
        if add is None:
            return
        packages = _read_installed_packages(INSTALLED_PACKAGES)
        if not packages:
            raise RuntimeError("02_OS installed-system package list is empty")
        add(packages)

    def _provision(self, installation):
        import subprocess

        target = str(installation.target)
        subprocess.check_call(["/usr/local/bin/02os-provision", target])


def retitle_boot_text(text):
    """Retitle an owned generated entry; preserve arguments and paths."""
    import re

    label = re.compile(r"^(\s*(?:menuentry|submenu)\s+)(['\"])(.*?)\2")
    lines = []
    for line in text.splitlines(keepends=True):
        body = line.lstrip(" \t")
        title = body.startswith("title ") or body.startswith("title\t")
        match = label.match(line)
        if match:
            start, end = match.span(3)
            line = line[:start] + match.group(3).replace("Arch Linux", OS_TITLE, 1) + line[end:]
        elif (title or body.startswith("/Arch Linux") or body.startswith('"Arch Linux')) and "Arch Linux" in line:
            line = line.replace("Arch Linux", OS_TITLE, 1)
        lines.append(line)
    return "".join(lines)


def retitle_grub_text(text):
    """Only GRUB's local-kernel generators belong to this installation."""
    import re

    marker = re.compile(r"^### (BEGIN|END) /etc/grub\.d/(10_linux|15_uki) ###\s*$")
    owned = False
    lines = []
    for line in text.splitlines(keepends=True):
        match = marker.match(line)
        if match:
            owned = match.group(1) == "BEGIN"
        lines.append(retitle_boot_text(line) if owned else line)
    return "".join(lines)


def retitle_owned_boot_text(text, kernel_parameters, kind):
    """Preserve foreign stanzas even if a generated config includes them."""
    import re
    import shlex

    identity_keys = {"root", "rootflags", "cryptdevice", "rd.luks.name", "rd.luks.uuid"}
    identity = {parameter for parameter in kernel_parameters
                if parameter.partition("=")[0] in identity_keys}

    def owns(command_line):
        if not identity:
            return False
        try:
            tokens = set(shlex.split(command_line))
        except ValueError:
            return False
        actual = {token for token in tokens if token.partition("=")[0] in identity_keys}
        return actual == identity

    lines = text.splitlines(keepends=True)
    if kind == "systemd":
        options = next((line.split(None, 1)[1] for line in lines
                        if line.lstrip().startswith(("options ", "options\t"))), "")
        return retitle_boot_text(text) if owns(options) else text
    if kind == "refind":
        entry = re.compile(r'^\s*"[^"]*"\s+"([^"]*)"')
        return "".join(retitle_boot_text(line) if (match := entry.match(line)) and owns(match.group(1))
                       else line for line in lines)
    if kind == "limine":
        sections = []
        for line in lines:
            if line.lstrip().startswith("/") or not sections:
                sections.append([])
            sections[-1].append(line)
        result = []
        for section in sections:
            options = next((line.split(":", 1)[1] for line in section
                            if line.lstrip().startswith("cmdline:")), "")
            block = "".join(section)
            result.append(retitle_boot_text(block) if owns(options) else block)
        return "".join(result)
    return text


def _add_02os_efistub_bootloader(installation, boot_partition, root, uki_enabled=False):
    # Derived from Archinstall 4.5, by the Archinstall contributors (GPL-3.0-only):
    # https://github.com/archlinux/archinstall/blob/4.5/archinstall/lib/installer.py
    # Full upstream file SHA-256:
    # 8a891c129a4baf266f60c0fe9cbbcc38b22c4e895c6708b3fabef0aea7abdea3
    # Modified 2026-10-08: per-instance adapter using the 02_OS firmware label.
    # Preserve package installation, hardware check, partition, loader, kernel
    # command line, UKI handling, and per-kernel creation. The upstream license
    # is retained in scripts/tests/fixtures/LICENSE.archinstall in this source tree.
    from archinstall.lib.command import SysCommand
    from archinstall.lib.disk.utils import get_parent_device_path
    from archinstall.lib.exceptions import HardwareIncompatibilityError
    from archinstall.lib.hardware import SysInfo

    installation.pacman.strap("efibootmgr")
    if not SysInfo.has_uefi():
        raise HardwareIncompatibilityError
    if not uki_enabled:
        loader = "/vmlinuz-{kernel}"
        entries = (r"initrd=\initramfs-{kernel}.img", *installation._get_kernel_params(root))
        cmdline = [" ".join(entries)]
    else:
        loader = "/EFI/Linux/arch-{kernel}.efi"
        cmdline = []
    parent_dev_path = get_parent_device_path(boot_partition.safe_dev_path)
    command = (
        "efibootmgr", "--create", "--disk", str(parent_dev_path),
        "--part", str(boot_partition.partn), "--label", OS_TITLE + " ({kernel})",
        "--loader", loader, "--unicode", *cmdline, "--verbose",
    )
    for kernel in installation.kernels:
        SysCommand([argument.format(kernel=kernel) for argument in command])
    installation._helper_flags["bootloader"] = "efistub"
