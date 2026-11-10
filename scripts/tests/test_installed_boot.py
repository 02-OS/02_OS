"""Installed Plymouth configuration without installing packages or an OS."""
import importlib.util
from enum import Enum
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from types import MethodType, ModuleType, SimpleNamespace
import unittest
from unittest.mock import Mock, patch


ROOT = Path(__file__).resolve().parents[2]
PLUGIN_PATH = ROOT / "profile/airootfs/usr/local/share/02os/archinstall_plugin.py"


class InstalledBootTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("02os_archinstall_test", PLUGIN_PATH)
        module = importlib.util.module_from_spec(spec)
        previous = sys.dont_write_bytecode
        try:
            sys.dont_write_bytecode = True
            spec.loader.exec_module(module)
        finally:
            sys.dont_write_bytecode = previous
        module.INSTALLED_PACKAGES = ROOT / "profile/airootfs/usr/local/share/02os/installed-packages.txt"
        module.DEVELOPER_PACKAGES = ROOT / "profile/airootfs/usr/local/share/02os/developer-packages.txt"
        os.environ.pop("O2OS_DEVELOPER", None)
        self.module = module
        self.plugin = module.Plugin()

    def installation(self, hooks, parameters=None):
        return SimpleNamespace(
            target="/mnt/disposable-target",
            _hooks=list(hooks),
            _kernel_params=list(parameters or []),
            add_additional_packages=Mock(),
            arch_chroot=Mock(),
            mkinitcpio=Mock(return_value=True),
        )

    def test_keeps_installed_encryption_lvm_resume_and_kernel_parameters(self):
        cases = [
            ["base", "udev", "autodetect", "microcode", "kms", "keyboard", "keymap consolefont", "block", "encrypt", "lvm2", "resume", "filesystems", "fsck"],
            ["base", "systemd", "autodetect", "microcode", "kms", "keyboard", "sd-vconsole", "block", "sd-encrypt", "filesystems", "fsck"],
        ]
        for hooks in cases:
            with self.subTest(backend=hooks[1]):
                parameters = ["root=UUID=target", "cryptdevice=UUID=encrypted:root", "resume=UUID=swap", "quiet"]
                target = self.installation(hooks, parameters)
                self.plugin._prepare_boot(target)
                self.plugin._prepare_boot(target)
                self.assertEqual([hook for hook in target._hooks if hook != "plymouth"], hooks)
                self.assertEqual(target._hooks.count("plymouth"), 1)
                self.assertEqual(target._hooks.index("plymouth"), target._hooks.index("kms") + 1)
                encryption = "encrypt" if "encrypt" in hooks else "sd-encrypt"
                self.assertLess(target._hooks.index("plymouth"), target._hooks.index(encryption))
                self.assertEqual(target._kernel_params, parameters + ["splash"])
                self.assertNotIn("archiso", target._hooks)

    def test_early_setup_precedes_image_generation_and_final_phase_reselects_theme(self):
        target = self.installation(["base", "udev", "kms", "filesystems"], ["root=UUID=target"])
        events = []
        self.plugin._provision = Mock(side_effect=lambda _target: events.append("copy-theme"))
        self.plugin._install_browsers = Mock(side_effect=lambda _target: events.append("install-browsers"))
        target.add_additional_packages.side_effect = lambda _packages: events.append("install-package")
        target.arch_chroot.side_effect = lambda command: events.append(
            "check-desktop" if command.endswith("02os-check-desktop") else "select-theme")

        def generate(flags):
            self.assertEqual(flags, ["-P"])
            self.assertIn("splash", target._kernel_params)
            self.assertIn("plymouth", target._hooks)
            events.append("regenerate-image")
            return True

        target.mkinitcpio.side_effect = generate
        self.plugin.on_install(target)
        # archinstall generates the bootloader after on_install returns.
        self.assertEqual(events, ["install-package", "copy-theme", "select-theme", "regenerate-image"])
        target.add_additional_packages.assert_called_once_with(["plymouth"])
        self.plugin.on_genfstab(target)
        self.assertEqual(events[-6:], ["install-package", "copy-theme", "check-desktop", "install-browsers", "select-theme", "regenerate-image"])
        desktop = self.module._read_installed_packages(self.module.INSTALLED_PACKAGES)
        self.assertEqual(target.add_additional_packages.call_args.args, (desktop,))
        self.assertEqual(target.arch_chroot.call_args.args, ("plymouth-set-default-theme 02-zero-portal",))

    def test_developer_packages_are_absent_unless_the_flag_is_set(self):
        target = self.installation(["base", "udev", "kms", "filesystems"])
        self.plugin._install_desktop_packages(target)
        desktop = self.module._read_installed_packages(self.module.INSTALLED_PACKAGES)
        developer = self.module._read_installed_packages(self.module.DEVELOPER_PACKAGES)
        self.assertTrue(developer)
        self.assertTrue(set(developer).isdisjoint(desktop))
        self.assertEqual(target.add_additional_packages.call_args.args, (desktop,))
        with patch.dict(os.environ, {"O2OS_DEVELOPER": "1"}):
            selected = self.installation(["base", "udev", "kms", "filesystems"])
            self.plugin._install_desktop_packages(selected)
        self.assertEqual(selected.add_additional_packages.call_args.args, (desktop + developer,))

    def test_plymouth_package_failure_precedes_provisioned_configuration(self):
        target = self.installation(["base", "udev", "kms", "filesystems"])
        self.plugin._provision = Mock()
        target.add_additional_packages.side_effect = RuntimeError("Plymouth installation failed")
        with self.assertRaisesRegex(RuntimeError, "Plymouth installation failed"):
            self.plugin.on_install(target)
        self.plugin._provision.assert_not_called()
        self.assertNotIn("plymouth", target._hooks)

    def test_browser_install_failure_aborts_final_installed_configuration(self):
        target = self.installation(["base", "udev", "kms", "filesystems"])
        self.plugin._provision = Mock()
        self.plugin._activate_theme = Mock()
        self.plugin._install_browsers = Mock(side_effect=RuntimeError("Browser package installation failed"))
        with self.assertRaisesRegex(RuntimeError, "Browser package"):
            self.plugin.on_genfstab(target)
        self.plugin._install_browsers.assert_called_once_with(target)
        self.plugin._activate_theme.assert_not_called()

    def browser_fixture(self):
        temporary = tempfile.TemporaryDirectory(prefix="02os-installed-browser-")
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        source = root / "live-repo"
        source.mkdir()
        target = self.installation(["base", "udev", "kms", "filesystems"])
        target.target = root / "target"
        repo = target.target / "usr/local/share/02os/browser-repo"
        repo.mkdir(parents=True)
        packages = []
        for name in ("google-chrome", "brave-bin"):
            filename = f"{name}-1-1-x86_64.pkg.tar.zst"
            payload = name.encode()
            (source / filename).write_bytes(payload)
            packages.append({"name": name, "version": "1-1", "filename": filename,
                             "sha256": hashlib.sha256(payload).hexdigest()})
        database = b"image browser database"
        for name in ("02os-browsers.db", "02os-browsers.db.tar.gz"):
            (source / name).write_bytes(database)
        digest = hashlib.sha256(database).hexdigest()
        manifest = {"format_version": 1, "architecture": "x86_64", "packages": packages,
                    "repository": {"name": "02os-browsers", "filename": "02os-browsers.db.tar.gz",
                                   "database_filename": "02os-browsers.db", "sha256": digest,
                                   "database_sha256": digest}}
        (source / "manifest.json").write_text(json.dumps(manifest))
        shutil.copyfile(source / "manifest.json", repo / "manifest.json")
        return target, source, repo

    def test_final_browser_stage_is_once_and_cleanup_preserves_manifest_source_and_user_files(self):
        target, source, repo = self.browser_fixture()
        cache = target.target / "var/cache/pacman/pkg/user-package.pkg.tar.zst"
        cache.parent.mkdir(parents=True)
        cache.write_bytes(b"unrelated cache")
        note = repo / "diagnostic.txt"
        note.write_text("preserve this")
        installed = False

        def query(command, **kwargs):
            self.assertEqual(command, ["arch-chroot", "-S", str(target.target),
                                       "/usr/local/bin/02os-install-browsers", "--verify-only"])
            return subprocess.CompletedProcess(command, 0 if installed else 1)

        def stage(command):
            self.assertEqual(command, ["/usr/local/bin/02os-provision", "--browser-payload", str(target.target)])
            for path in source.iterdir():
                shutil.copyfile(path, repo / path.name)

        def install(command):
            nonlocal installed
            self.assertEqual(command, "/usr/local/bin/02os-install-browsers")
            installed = True

        target.arch_chroot.side_effect = install
        with patch("subprocess.run", side_effect=query) as queries, \
                patch("subprocess.check_call", side_effect=stage) as stages:
            self.plugin._install_browsers(target)
            self.plugin._install_browsers(target)
        self.assertEqual(queries.call_count, 2)
        stages.assert_called_once()
        target.arch_chroot.assert_called_once()
        self.assertEqual(set(path.name for path in repo.iterdir()), {"manifest.json", "diagnostic.txt"})
        self.assertEqual((repo / "manifest.json").read_bytes(), (source / "manifest.json").read_bytes())
        self.assertEqual(len(list(source.iterdir())), 5)
        self.assertEqual(cache.read_bytes(), b"unrelated cache")
        self.assertEqual(note.read_text(), "preserve this")

    def test_failed_browser_install_preserves_staged_payload_for_diagnosis(self):
        target, source, repo = self.browser_fixture()

        def stage(_command):
            for path in source.iterdir():
                shutil.copyfile(path, repo / path.name)

        target.arch_chroot.side_effect = RuntimeError("browser transaction failed")
        with patch("subprocess.run", return_value=subprocess.CompletedProcess([], 1)), \
                patch("subprocess.check_call", side_effect=stage):
            with self.assertRaisesRegex(RuntimeError, "transaction failed"):
                self.plugin._install_browsers(target)
        for path in source.iterdir():
            self.assertEqual((repo / path.name).read_bytes(), path.read_bytes())

    def test_cleanup_rejects_changed_payload_without_removing_other_files(self):
        target, source, repo = self.browser_fixture()
        for path in source.iterdir():
            shutil.copyfile(path, repo / path.name)
        (repo / "02os-browsers.db").write_bytes(b"changed by someone else")
        with self.assertRaisesRegex(RuntimeError, "changed.*preserving"):
            self.plugin._cleanup_browser_payload(target)
        self.assertEqual(len(list(repo.iterdir())), 5)

    def test_cleanup_rejects_payload_symlink_and_live_root(self):
        target, source, repo = self.browser_fixture()
        database = repo / "02os-browsers.db"
        database.symlink_to(source / database.name)
        with self.assertRaisesRegex(RuntimeError, "symlink"):
            self.plugin._cleanup_browser_payload(target)
        self.assertTrue(database.is_symlink())
        self.assertTrue((source / database.name).is_file())
        target.target = "/"
        with self.assertRaisesRegex(RuntimeError, "live system"):
            self.plugin._cleanup_browser_payload(target)

    def test_repositions_existing_plymouth_before_encryption(self):
        target = self.installation(["base", "udev", "kms", "encrypt", "plymouth", "filesystems"])
        self.plugin._prepare_boot(target)
        self.assertEqual(target._hooks, ["base", "udev", "kms", "plymouth", "encrypt", "filesystems"])

    def test_rejects_unsupported_target_without_mutating_it(self):
        for hooks in (["base", "kms", "filesystems"], ["base", "udev", "encrypt", "kms", "filesystems"]):
            with self.subTest(hooks=hooks):
                target = self.installation(hooks, ["root=UUID=target"])
                with self.assertRaisesRegex(RuntimeError, "(requires|ordering)"):
                    self.plugin._prepare_boot(target)
                self.assertEqual(target._hooks, hooks)
                self.assertEqual(target._kernel_params, ["root=UUID=target"])

    def test_initramfs_failure_aborts_provisioning(self):
        target = self.installation(["base", "udev", "kms", "filesystems"])
        self.plugin._provision = Mock()
        target.mkinitcpio.return_value = False
        with self.assertRaisesRegex(RuntimeError, "initramfs"):
            self.plugin.on_install(target)


class BootTitleTests(unittest.TestCase):
    setUp = InstalledBootTests.setUp
    installation = InstalledBootTests.installation

    def boot_target(self, root):
        boot = SimpleNamespace(mountpoint=Path("/boot"), relative_mountpoint=Path("boot"),
                               safe_dev_path=Path("/dev/example1"), dev_path=Path("/dev/example1"), partn=1)
        efi = SimpleNamespace(mountpoint=Path("/efi"), relative_mountpoint=Path("efi"), dev_path=Path("/dev/example2"))
        target = self.installation(["base", "udev", "kms", "filesystems"])
        target.target = root
        target.init_time = "2026-10-08_01-02-03"
        target.kernels = ["linux"]
        target._helper_flags = {}
        target._get_boot_partition = lambda: boot
        target._get_efi_partition = lambda: efi
        target._get_root = lambda: object()
        target._get_kernel_params = lambda _root: ["root=UUID=current", "quiet", "splash"]

        def efistub(_target, boot_partition, root, uki_enabled=False):
            raise AssertionError("Unexpected real EFISTUB operation in text fixture")

        def generate(_target, bootloader, uki_enabled=False, bootloader_removable=False, plymouth=None):
            for rel, contents in _target.generated.items():
                path = root / rel
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(contents)
            _target._helper_flags["bootloader"] = bootloader

        target._add_efistub_bootloader = MethodType(efistub, target)
        target.add_bootloader = MethodType(generate, target)
        return target

    def test_generated_bls_filename_does_not_rename_shared_esp_neighbors(self):
        with tempfile.TemporaryDirectory(prefix="02os-title-") as tmp:
            root = Path(tmp)
            target = self.boot_target(root)
            own = f"boot/loader/entries/{target.init_time}_linux.conf"
            foreign = "title Arch Linux (neighbor)\nlinux /EFI/neighbor/linux\noptions root=UUID=foreign quiet\n"
            paths = ["boot/loader/entries/older_linux.conf", "efi/loader/entries/neighbor.conf"]
            for rel in paths:
                path = root / rel
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(foreign)
            target.generated = {own: "title Arch Linux (linux)\nlinux /vmlinuz-linux\noptions root=UUID=current quiet splash\n"}
            with patch("importlib.metadata.version", return_value="4.5"):
                self.plugin._configure_boot_branding(target)
            target.add_bootloader("systemd")
            self.assertIn("title 02_OS (linux)", (root / own).read_text())
            for rel in paths:
                self.assertEqual((root / rel).read_bytes(), foreign.encode())

    def test_foreign_stanzas_in_generated_grub_limine_and_refind_are_unchanged(self):
        cases = {
            "grub": (
                "boot/grub/grub.cfg",
                "### BEGIN /etc/grub.d/10_linux ###\n"
                "menuentry 'Arch Linux' --id 'Arch Linux ID must not change' {\n"
                " linux /vmlinuz-linux root=UUID=current quiet splash\n}\n"
                "submenu 'Advanced options for Arch Linux' {\n}\n"
                "### END /etc/grub.d/10_linux ###\n",
                "### BEGIN /etc/grub.d/30_os-prober ###\n"
                "menuentry 'Arch Linux (on /dev/sdb2)' --id 'neighbor' {\n"
                " linux /vmlinuz-linux root=UUID=foreign\n}\n"
                "### END /etc/grub.d/30_os-prober ###\n",
            ),
            "limine": (
                "efi/EFI/arch-limine/limine.conf",
                "/Arch Linux (linux)\n protocol: linux\n path: boot():/vmlinuz-linux\n"
                " cmdline: root=UUID=current quiet splash\n",
                "/Arch Linux (neighbor)\n protocol: linux\n path: uuid(foreign):/vmlinuz-linux\n"
                " cmdline: root=UUID=foreign quiet splash\n",
            ),
            "refind": (
                "boot/refind_linux.conf",
                '"Arch Linux (linux)" "root=UUID=current quiet splash initrd=\\initramfs-linux.img"\n',
                '"Arch Linux (neighbor)" "root=UUID=foreign quiet splash initrd=\\neighbor.img"\n',
            ),
        }
        for kind, (rel, own, foreign) in cases.items():
            with self.subTest(kind=kind), tempfile.TemporaryDirectory(prefix="02os-title-") as tmp:
                root = Path(tmp)
                target = self.boot_target(root)
                # A recording generator preserves the foreign stanza in the
                # very same file. Both it and separate neighbor configs must
                # survive the title pass byte-for-byte.
                target.generated = {rel: own + foreign}
                neighbor = root / "efi/EFI/neighbor/limine.conf"
                neighbor.parent.mkdir(parents=True)
                neighbor.write_text(foreign)
                with patch("importlib.metadata.version", return_value="4.5"):
                    self.plugin._configure_boot_branding(target)
                target.add_bootloader(kind)
                actual = (root / rel).read_bytes()
                self.assertIn(foreign.encode(), actual)
                self.assertIn(b"02_OS", actual)
                self.assertEqual(neighbor.read_bytes(), foreign.encode())
                if kind == "grub":
                    self.assertIn(b"--id 'Arch Linux ID must not change'", actual)

    def test_encrypted_neighbor_with_same_mapper_name_is_not_owned(self):
        parameters = ["root=/dev/mapper/root", "cryptdevice=UUID=current:root"]
        own = '/Arch Linux (current)\n cmdline: root=/dev/mapper/root cryptdevice=UUID=current:root\n'
        foreign = '/Arch Linux (neighbor)\n cmdline: root=/dev/mapper/root cryptdevice=UUID=foreign:root\n'
        updated = self.module.retitle_owned_boot_text(own + foreign, parameters, "limine")
        self.assertIn("/02_OS (current)", updated)
        self.assertTrue(updated.endswith(foreign))

    def test_refind_output_matches_esp_and_separate_boot_layouts(self):
        for separate in (False, True):
            with self.subTest(separate=separate), tempfile.TemporaryDirectory(prefix="02os-refind-") as tmp:
                root = Path(tmp)
                target = self.boot_target(root)
                efi = target._get_efi_partition()
                boot = SimpleNamespace(mountpoint=Path("/kernels"), dev_path=Path("/dev/separate")) if separate else efi
                target._get_boot_partition = lambda: boot
                rel = "kernels/refind_linux.conf" if separate else "boot/refind_linux.conf"
                target.generated = {rel: '"Arch Linux (linux)" "root=UUID=current quiet splash"\n'}
                foreign_path = root / "efi/refind_linux.conf"
                foreign_path.parent.mkdir(parents=True)
                foreign = '"Arch Linux (neighbor)" "root=UUID=foreign quiet splash"\n'
                foreign_path.write_text(foreign)
                with patch("importlib.metadata.version", return_value="4.5"):
                    self.plugin._configure_boot_branding(target)
                target.add_bootloader("refind")
                self.assertIn('"02_OS (linux)"', (root / rel).read_text())
                self.assertEqual(foreign_path.read_bytes(), foreign.encode())

    def test_incompatible_archinstall_fails_before_adapting_instance(self):
        with tempfile.TemporaryDirectory(prefix="02os-title-") as tmp:
            target = self.boot_target(Path(tmp))
            original = target.add_bootloader
            with patch("importlib.metadata.version", return_value="4.6"), self.assertRaisesRegex(RuntimeError, "requires archinstall 4.5"):
                self.plugin._configure_boot_branding(target)
            self.assertIs(target.add_bootloader, original)
            with patch("importlib.metadata.version", return_value="4.5"):
                target._add_efistub_bootloader = lambda boot_partition: None
                with self.assertRaisesRegex(RuntimeError, "Unsupported archinstall 4.5"):
                    self.plugin._configure_boot_branding(target)


class FirmwareTitleTests(unittest.TestCase):
    setUp = InstalledBootTests.setUp
    installation = InstalledBootTests.installation

    def test_official_dispatch_changes_only_current_efistub_label(self):
        fixture = ROOT / "scripts/tests/fixtures/archinstall_4_5_bootloader.py"
        spec = importlib.util.spec_from_file_location("archinstall45_recording", fixture)
        official = importlib.util.module_from_spec(spec)
        previous = sys.dont_write_bytecode
        try:
            sys.dont_write_bytecode = True
            spec.loader.exec_module(official)
        finally:
            sys.dont_write_bytecode = previous

        class Bootloader(Enum):
            Systemd = "systemd"
            Grub = "grub"
            Efistub = "efistub"
            Limine = "limine"
            Refind = "refind"

            def has_uki_support(self):
                return True

            def has_removable_support(self):
                return False

        official.Bootloader = Bootloader
        official.SysInfo = SimpleNamespace(has_uefi=lambda: True)
        official.get_parent_device_path = lambda _partition: Path("/dev/example")
        official.plugins = {}
        official.debug = official.info = official.warn = lambda _message: None
        official.HardwareIncompatibilityError = RuntimeError

        for uki in (False, True):
            for parameters in (
                ["root=UUID=current", "quiet", "splash"],
                ["root=/dev/mapper/root", "cryptdevice=UUID=current:root", "resume=UUID=swap", "quiet", "splash"],
                ["root=PARTUUID=current", r"initrd=\intel-ucode.img", "quiet", "splash"],
            ):
                with self.subTest(uki=uki, parameters=parameters):
                    commands = []
                    official.SysCommand = lambda argv: commands.append(list(argv))
                    target = self.installation(["base", "udev", "kms", "filesystems"])
                    boot = SimpleNamespace(safe_dev_path=Path("/dev/example1"), dev_path=Path("/dev/example1"), partn=1)
                    target._get_boot_partition = lambda: boot
                    target._get_efi_partition = lambda: SimpleNamespace(mountpoint=Path("/efi"))
                    target._get_root = lambda: object()
                    target._get_kernel_params = lambda _root: parameters
                    target._helper_flags = {}
                    target.pacman = SimpleNamespace(strap=Mock())
                    target.kernels = ["linux", "linux-lts"]
                    target._disk_config = SimpleNamespace(has_default_btrfs_vols=lambda: False)
                    target._config_uki = Mock()
                    target._add_efistub_bootloader = MethodType(official.Installer45._add_efistub_bootloader, target)
                    target.add_bootloader = MethodType(official.Installer45.add_bootloader, target)
                    target.add_bootloader(Bootloader.Efistub, uki)
                    before = list(commands)
                    commands.clear()

                    modules = {}
                    for name in ("archinstall", "archinstall.lib", "archinstall.lib.disk",
                                 "archinstall.lib.command", "archinstall.lib.disk.utils",
                                 "archinstall.lib.exceptions", "archinstall.lib.hardware"):
                        modules[name] = ModuleType(name)
                    modules["archinstall.lib.command"].SysCommand = official.SysCommand
                    modules["archinstall.lib.disk.utils"].get_parent_device_path = official.get_parent_device_path
                    modules["archinstall.lib.exceptions"].HardwareIncompatibilityError = RuntimeError
                    modules["archinstall.lib.hardware"].SysInfo = official.SysInfo
                    self.plugin._provision = self.plugin._prepare_boot = self.plugin._activate_theme = Mock()
                    with patch("importlib.metadata.version", return_value="4.5"), patch.dict("sys.modules", modules):
                        # Exercise real plugin timing, then the unmodified
                        # Archinstall add_bootloader dispatch from its fixture.
                        self.plugin.on_install(target)
                        target.add_bootloader(Bootloader.Efistub, uki)
                    expected = []
                    for argv in before:
                        argv = list(argv)
                        position = argv.index("--label") + 1
                        argv[position] = argv[position].replace("Arch Linux", "02_OS", 1)
                        expected.append(argv)
                    self.assertEqual(commands, expected)
                    self.assertEqual(target._helper_flags["bootloader"], "efistub")
                    self.assertTrue(all("--create" in argv and "--bootnum" not in argv and "--delete-bootnum" not in argv for argv in commands))


if __name__ == "__main__":
    unittest.main()
