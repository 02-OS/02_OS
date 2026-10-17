"""Destructive guest installation is guarded by the exact disposable disk identity."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("vm_guest_checks", ROOT / "scripts/vm/guest_checks.py")
guest = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guest)


class DisposableDiskTests(unittest.TestCase):
    def setUp(self):
        self.serial = "02TEST-0123456789ab"
        self.disk = {"name": "/dev/vda", "serial": self.serial, "type": "disk",
                     "size": 32 * 1024**3, "ro": False}

    def test_exact_empty_disposable_disk_is_accepted(self):
        guest.disk_guard({"blockdevices": [self.disk]}, self.serial)

    def test_foreign_partitioned_readonly_or_extra_disks_are_rejected(self):
        for change in ({"serial": "foreign"}, {"name": "/dev/sda"}, {"ro": True},
                       {"size": 64 * 1024**3}, {"children": [{"name": "/dev/vda1"}]}):
            with self.subTest(change=change):
                disk = {**self.disk, **change}
                with self.assertRaises(ValueError):
                    guest.disk_guard({"blockdevices": [disk]}, self.serial)
        with self.assertRaises(ValueError):
            guest.disk_guard({"blockdevices": [self.disk, {**self.disk, "name": "/dev/vdb"}]}, self.serial)

    def test_installer_fixture_targets_only_disposable_disk(self):
        config = json.loads((ROOT / "scripts/vm/archinstall-test-config.json").read_text())
        changes = config["disk_config"]["device_modifications"]
        self.assertEqual(len(changes), 1)
        self.assertEqual(changes[0]["device"], "/dev/vda")
        self.assertTrue(changes[0]["wipe"])
        self.assertEqual(config["custom_commands"], [])
        self.assertIn("qemu-guest-agent", config["services"])


if __name__ == "__main__":
    unittest.main()
