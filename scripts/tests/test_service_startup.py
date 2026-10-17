"""Check account eligibility and the live boot dependency graph."""

from pathlib import Path
import shlex
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[2]
OVERLAY = ROOT / "profile/airootfs"
SYSTEM = OVERLAY / "etc/systemd/system"


def directives(path, key):
    return [line.split("=", 1)[1] for line in path.read_text().splitlines()
            if line.startswith(key + "=")]


class ServiceStartupTests(unittest.TestCase):
    def test_daemon_execution_condition_accepts_users_and_skips_greeters(self):
        unit = OVERLAY / "usr/lib/systemd/user/02-agentd.service"
        # Execute the shipped condition, including systemd's literal-dollar
        # escape, with the manager username supplied by its %u specifier.
        command, = directives(unit, "ExecCondition")
        args = shlex.split(command.replace("$$", "$"))
        for user, expected in (("live", 0), ("02tester", 0), ("gdmdeveloper", 0), ("gdm-developer", 0),
                               ("gdm", 1), ("gdm-greeter", 1),
                               ("gdm-greeter-1", 1), ("gdm-greeter-999", 1)):
            with self.subTest(user=user):
                result = subprocess.run([user if arg == "%u" else arg for arg in args],
                                        capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, expected, result.stderr)
        self.assertIn("!@system", directives(unit, "ConditionUser"))

    def test_user_daemon_can_start_before_its_wanting_target(self):
        unit = OVERLAY / "usr/lib/systemd/user/02-agentd.service"
        # A target orders itself after units it wants by default; the daemon
        # must not add the reverse edge and form an ordering cycle.
        enabled = OVERLAY / "etc/systemd/user/default.target.wants/02-agentd.service"
        self.assertTrue(enabled.is_symlink())
        after = " ".join(directives(unit, "After")).split()
        self.assertNotIn("default.target", after)

    def test_live_home_is_prepared_before_sessions_without_early_linger(self):
        unit = SYSTEM / "02os-ensure-live-user.service"
        before = " ".join(directives(unit, "Before")).split()
        self.assertIn("systemd-user-sessions.service", before)
        self.assertIn("gdm.service", before)
        self.assertIn("systemd-sysusers.service", " ".join(directives(unit, "After")).split())
        self.assertTrue((SYSTEM / "multi-user.target.wants/02os-ensure-live-user.service").is_symlink())
        self.assertFalse((OVERLAY / "var/lib/systemd/linger/live").exists())

    def test_live_boot_does_not_enable_network_or_mirror_waits(self):
        enabled = {path.name for wants in SYSTEM.glob("*.wants") for path in wants.iterdir()}
        self.assertNotIn("NetworkManager-wait-online.service", enabled)
        self.assertNotIn("systemd-networkd-wait-online.service", enabled)
        self.assertNotIn("systemd-networkd.service", enabled)
        self.assertNotIn("reflector.service", enabled)
        self.assertIn("NetworkManager.service", enabled)
        self.assertIn("systemd-resolved.service", enabled)
        # The optional kernel-command-line mirror override requires no network
        # wait of its own and still remains available.
        mirror = SYSTEM / "choose-mirror.service"
        self.assertIn("mirror", directives(mirror, "ConditionKernelCommandLine"))
        self.assertNotIn("network-online.target", mirror.read_text())


if __name__ == "__main__":
    unittest.main()
