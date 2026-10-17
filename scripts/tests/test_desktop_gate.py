"""Run the production package/extension/API compatibility validator on fixtures."""
import importlib.util
import importlib.machinery
import json
from pathlib import Path
import shutil
import tempfile
from types import ModuleType, SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
OVERLAY = ROOT / "profile/airootfs"
HELPER = OVERLAY / "usr/local/bin/02os-check-desktop"
loader = importlib.machinery.SourceFileLoader("desktop_gate", str(HELPER))
spec = importlib.util.spec_from_loader(loader.name, loader)
gate = importlib.util.module_from_spec(spec)
loader.exec_module(gate)


class DesktopGateTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="02os-desktop-gate-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        contract = OVERLAY / "usr/local/share/02os/desktop-compatibility.json"
        dest = self.root / contract.relative_to(OVERLAY)
        dest.parent.mkdir(parents=True)
        shutil.copyfile(contract, dest)
        self.contract = json.loads(contract.read_text())
        for uuid in self.contract["extensions"]:
            relative = Path("usr/share/gnome-shell/extensions") / uuid / "metadata.json"
            (self.root / relative).parent.mkdir(parents=True)
            shutil.copyfile(OVERLAY / relative, self.root / relative)
        self.versions = {"gnome-shell": "51.1-1", "mutter": "51.1-1", "archinstall": "4.5.3-1"}
        def query(args, **kwargs):
            return SimpleNamespace(stdout=f"{args[-1]} {self.versions[args[-1]]}\n")
        self.query = patch.object(gate.subprocess, "run", side_effect=query).start()
        self.addCleanup(patch.stopall)

    def test_current_packages_and_all_five_extensions_are_checked(self):
        self.assertEqual(gate.check(self.root), {key: self.versions[key] for key in ("gnome-shell", "mutter")})
        vendor = json.loads((ROOT / "docs/desktop-motion-vendor.json").read_text())
        self.assertEqual(vendor["target_shell"], self.contract["gnome_major"])

    def test_future_major_and_mismatched_mutter_are_rejected(self):
        for package, version in (("gnome-shell", "52.0-1"), ("mutter", "50.5-1")):
            with self.subTest(package=package):
                original = self.versions[package]
                self.versions[package] = version
                with self.assertRaisesRegex(ValueError, "Unsupported"):
                    gate.check(self.root)
                self.versions[package] = original

    def test_extension_rejection_is_not_masked_by_package_match(self):
        uuid = "02-zero-portal@02os"
        path = self.root / "usr/share/gnome-shell/extensions" / uuid / "metadata.json"
        metadata = json.loads(path.read_text())
        metadata["shell-version"] = ["50"]
        path.write_text(json.dumps(metadata))
        with self.assertRaisesRegex(ValueError, uuid):
            gate.check(self.root)

    def test_installer_version_and_api_are_checked_before_disk_work(self):
        class Installer:
            def add_bootloader(self, bootloader, uki_enabled=False, bootloader_removable=False, plymouth=None):
                raise AssertionError("must only inspect the API")
            def _add_efistub_bootloader(self, boot_partition, root, uki_enabled):
                raise AssertionError("must only inspect the API")
        modules = {name: ModuleType(name) for name in ("archinstall", "archinstall.lib", "archinstall.lib.installer")}
        modules["archinstall.lib.installer"].Installer = Installer
        with patch.dict("sys.modules", modules):
            self.assertEqual(gate.check(self.root, True), {"archinstall": "4.5.3-1"})
            self.versions["archinstall"] = "4.5-1"
            self.assertEqual(gate.check(self.root, True), {"archinstall": "4.5-1"})
            self.versions["archinstall"] = "4.6.0-1"
            with self.assertRaisesRegex(ValueError, "requires archinstall"):
                gate.check(self.root, True)
            self.versions["archinstall"] = "4.5.3-1"
            Installer.add_bootloader = lambda self, changed: None
            with self.assertRaisesRegex(ValueError, "Unsupported Archinstall API"):
                gate.check(self.root, True)


if __name__ == "__main__":
    unittest.main()
