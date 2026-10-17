"""Native browser provisioning, without installing packages or changing the OS."""

import hashlib
import importlib.machinery
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
HELPER = ROOT / "profile/airootfs/usr/local/bin/02os-install-browsers"


class BrowserProvisioningTests(unittest.TestCase):
    def setUp(self):
        loader = importlib.machinery.SourceFileLoader("browser_provisioning_test", str(HELPER))
        spec = importlib.util.spec_from_loader(loader.name, loader)
        self.helper = importlib.util.module_from_spec(spec)
        with patch.object(sys, "dont_write_bytecode", True):
            spec.loader.exec_module(self.helper)
        self.temporary = tempfile.TemporaryDirectory(prefix="02os-browser-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / "browser-repo"
        self.repo.mkdir()
        self.config = self.root / "pacman.conf"
        self.original_config = "[options]\nSigLevel = Required DatabaseOptional\n[core]\nServer = https://arch.example/$repo/os/$arch\n[extra]\nInclude = /etc/pacman.d/mirrorlist\n"
        self.resolved_config = ("[options]\nDBPath = /var/lib/pacman/\nSigLevel = PackageRequired\n"
                                "SigLevel = PackageTrustedOnly\nLocalFileSigLevel = PackageOptional\n"
                                "LocalFileSigLevel = PackageTrustedOnly\n[core]\n"
                                "SigLevel = PackageRequired\nServer = https://arch.example/core/os/x86_64\n")
        self.config.write_text(self.original_config)
        self.packages = []
        for name, version in (("google-chrome", "155.0.8059.79-1"), ("brave-bin", "1.86.146-1")):
            filename = f"{name}-{version}-x86_64.pkg.tar.zst"
            payload = f"unchanged verified vendor payload for {name}".encode()
            (self.repo / filename).write_bytes(payload)
            self.packages.append({"name": name, "version": version, "filename": filename,
                                  "sha256": hashlib.sha256(payload).hexdigest()})
        self.manifest = {"format_version": 1, "architecture": "x86_64", "packages": self.packages,
                         "repository": {"name": "02os-browsers", "filename": "02os-browsers.db.tar.gz",
                                        "database_filename": "02os-browsers.db"}}
        self.write_database()
        self.calls = []
        self.installed = {}
        self.missing = []

    def write_manifest(self):
        (self.repo / "manifest.json").write_text(json.dumps(self.manifest))

    def write_database(self, packages=None):
        archive = self.repo / "02os-browsers.db.tar.gz"
        with tarfile.open(archive, "w:gz") as output:
            for package in packages if packages is not None else self.packages:
                desc = "\n\n".join(f"%{field}%\n{value}" for field, value in (
                    ("NAME", package["name"]), ("VERSION", package["version"]),
                    ("FILENAME", package["filename"]), ("SHA256SUM", package["sha256"]),
                    ("ARCH", "x86_64"), ("DEPENDS", "\n".join(package.get("depends", ["glibc"]))))) + "\n\n"
                member = tarfile.TarInfo(f"{package['name']}-{package['version']}/desc")
                member.size = len(desc.encode())
                output.addfile(member, io.BytesIO(desc.encode()))
        payload = archive.read_bytes()
        (self.repo / "02os-browsers.db").write_bytes(payload)
        digest = hashlib.sha256(payload).hexdigest()
        self.manifest["repository"].update(sha256=digest, database_sha256=digest)
        self.write_manifest()

    def pacman(self, command, **kwargs):
        self.calls.append((command, kwargs))
        if command[0] == "/usr/bin/pacman-conf":
            return subprocess.CompletedProcess(command, 0, stdout=self.resolved_config)
        if "--deptest" in command:
            self.assertEqual(command[2:], ["glibc"])
            return subprocess.CompletedProcess(command, 127 if self.missing else 0,
                                               stdout="".join(dependency + "\n" for dependency in self.missing))
        if "--sync" in command:
            self.assertEqual(command[command.index("--config") + 1], str(self.config))
            self.official_config = self.config.read_text()
            self.missing = []
            return subprocess.CompletedProcess(command, 0)
        if "--upgrade" in command:
            path = Path(command[command.index("--config") + 1])
            self.transaction_config = path.read_text()
            self.transaction_config_path = path
            self.installed = {package["name"]: package["version"] for package in self.packages}
            return subprocess.CompletedProcess(command, 0)
        self.assertEqual(command, ["/usr/bin/pacman", "--query", "google-chrome", "brave-bin"])
        stdout = "".join(f"{name} {version}\n" for name, version in self.installed.items())
        return subprocess.CompletedProcess(command, 0 if len(self.installed) == 2 else 1, stdout=stdout)

    def install(self, pacman=None, uid=0, verify_only=False):
        with patch.object(self.helper.os, "geteuid", return_value=uid), \
                patch.object(self.helper, "PACMAN_CONF", self.config), \
                patch.object(self.helper, "TEMP_ROOT", str(self.root)), \
                patch.object(self.helper.subprocess, "run", side_effect=pacman or self.pacman):
            return self.helper.install_browsers(self.repo, verify_only=verify_only)

    def test_installs_exact_image_versions_with_official_dependencies_and_is_repeatable(self):
        self.missing = ["glibc"]
        self.install()
        self.install()
        transactions = [command for command, kwargs in self.calls if "--sync" in command]
        self.assertEqual(len(transactions), 1)
        for command in transactions:
            self.assertEqual(command[0], "/usr/bin/pacman")
            for flag in ("--sync", "--refresh", "--sysupgrade", "--needed", "--noconfirm"):
                self.assertIn(flag, command)
            self.assertEqual(command[-1:], ["glibc"])
            self.assertNotIn("--root", command)
            self.assertNotIn("--nodeps", command)
            self.assertNotIn("--disable-sandbox", command)
        self.assertEqual(self.official_config, self.original_config)
        self.assertNotIn("[02os-browsers]", self.transaction_config)
        self.assertNotIn("[core]", self.transaction_config)
        self.assertIn("SigLevel = PackageRequired", self.transaction_config)
        self.assertIn("SigLevel = PackageTrustedOnly", self.transaction_config)
        self.assertIn("DBPath = /var/lib/pacman/", self.transaction_config)
        self.assertEqual(self.transaction_config.count("LocalFileSigLevel"), 1)
        self.assertIn("LocalFileSigLevel = Optional TrustAll", self.transaction_config)
        upgrades = [command for command, _ in self.calls if "--upgrade" in command]
        self.assertEqual(len(upgrades), 1)
        self.assertEqual(upgrades[0][-2:], [str(self.repo / package["filename"]) for package in self.packages])
        self.assertEqual(self.config.read_text(), self.original_config)
        self.assertFalse(self.transaction_config_path.exists())
        queries = [command for command, kwargs in self.calls if "--query" in command]
        self.assertEqual(queries, [["/usr/bin/pacman", "--query", "google-chrome", "brave-bin"]] * 3)

    def test_satisfied_dependencies_install_offline_without_creating_or_removing_sync_databases(self):
        sync = self.root / "var/lib/pacman/sync"
        sync.mkdir(parents=True)
        for name in ("02os-browsers.db", "core.db", "unrelated.db"):
            (sync / name).write_bytes(b"pre-existing sync database")
        before = {path.name: path.read_bytes() for path in sync.iterdir()}
        self.install()
        self.assertFalse(any("--sync" in command or "--refresh" in command for command, _ in self.calls))
        self.assertTrue(any("--upgrade" in command for command, _ in self.calls))
        self.assertFalse(any("--nodeps" in command for command, _ in self.calls))
        self.assertNotIn("Server", self.transaction_config)
        self.assertNotIn("Include", self.transaction_config)
        self.assertEqual({path.name: path.read_bytes() for path in sync.iterdir()}, before)

    def test_dependency_check_errors_never_trigger_a_transaction(self):
        for code, output in ((1, ""), (127, "unexpected\n"), (127, ""), (0, "glibc\n")):
            with self.subTest(code=code, output=output):
                self.calls.clear()
                def failed_check(command, **kwargs):
                    result = self.pacman(command, **kwargs)
                    if "--deptest" in command:
                        return subprocess.CompletedProcess(command, code, stdout=output)
                    return result
                with self.assertRaisesRegex(ValueError, "dependencies"):
                    self.install(pacman=failed_check)
                self.assertFalse(any("--sync" in command or "--upgrade" in command for command, _ in self.calls))

    def test_failed_offline_install_never_falls_back_to_network(self):
        def failed_upgrade(command, **kwargs):
            result = self.pacman(command, **kwargs)
            if "--upgrade" in command:
                raise subprocess.CalledProcessError(1, command)
            return result
        with self.assertRaises(subprocess.CalledProcessError):
            self.install(pacman=failed_upgrade)
        self.assertFalse(any("--sync" in command for command, _ in self.calls))
        self.assertFalse(self.transaction_config_path.exists())

    def test_invalid_dependency_in_verified_database_fails_before_pacman(self):
        self.packages[0]["depends"] = ["--nodeps"]
        self.write_database()
        with self.assertRaisesRegex(ValueError, "invalid dependency"):
            self.install()
        self.assertEqual(self.calls, [])

    def test_exact_versions_skip_transaction_even_without_config_or_writable_temp_root(self):
        self.installed = {package["name"]: package["version"] for package in self.packages}
        self.config.unlink()
        with patch.object(self.helper.tempfile, "TemporaryDirectory", side_effect=AssertionError("transaction attempted")):
            self.install()
        self.assertEqual(len(self.calls), 1)
        self.assertEqual(self.calls[0][0], ["/usr/bin/pacman", "--query", "google-chrome", "brave-bin"])

    def test_verify_only_uses_retained_manifest_without_payload_mirrors_keyring_or_transaction(self):
        self.installed = {package["name"]: package["version"] for package in self.packages}
        for path in self.repo.iterdir():
            if path.name != "manifest.json":
                path.unlink()
        self.config.unlink()
        with patch.object(self.helper.tempfile, "TemporaryDirectory", side_effect=AssertionError("transaction attempted")):
            self.install(verify_only=True)
        self.assertEqual([command for command, _ in self.calls],
                         [["/usr/bin/pacman", "--query", "google-chrome", "brave-bin"]])
        self.assertEqual([path.name for path in self.repo.iterdir()], ["manifest.json"])

    def test_verify_only_rejects_missing_or_wrong_versions_without_installing(self):
        for installed in ({}, {"google-chrome": self.packages[0]["version"]},
                          {"google-chrome": "wrong-1", "brave-bin": self.packages[1]["version"]}):
            with self.subTest(installed=installed):
                self.installed = installed
                with self.assertRaisesRegex(ValueError, "version differs"):
                    self.install(verify_only=True)
        self.assertTrue(all(command[1] == "--query" for command, _ in self.calls))

    def test_verify_only_rejects_invalid_manifest_before_query(self):
        self.packages[0]["sha256"] = "invalid"
        self.write_manifest()
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            self.install(verify_only=True)
        self.assertEqual(self.calls, [])

    def test_live_image_customization_selects_verification_mode(self):
        customize = (ROOT / "profile/airootfs/root/customize_airootfs.sh").read_text()
        commands = [line.strip() for line in customize.splitlines()
                    if line.strip().startswith("/usr/local/bin/02os-install-browsers")]
        self.assertEqual(commands, ["/usr/local/bin/02os-install-browsers --verify-only"])
        with patch.object(self.helper.sys, "argv", [str(HELPER), "--repo", str(self.repo), "--verify-only"]), \
                patch.object(self.helper, "install_browsers") as install:
            self.assertEqual(self.helper.main(), 0)
        install.assert_called_once_with(self.repo, verify_only=True)

    def test_non_root_fails_before_reading_payload_or_calling_pacman(self):
        with self.assertRaisesRegex(PermissionError, "root.*chroot"):
            self.install(uid=1000)
        self.assertEqual(self.calls, [])

    def test_missing_or_tampered_files_abort_before_pacman(self):
        paths = ["manifest.json", self.packages[0]["filename"], "02os-browsers.db", "02os-browsers.db.tar.gz"]
        for filename in paths:
            path = self.repo / filename
            original = path.read_bytes()
            with self.subTest(filename=filename, mode="missing"):
                path.unlink()
                with self.assertRaises((OSError, ValueError)):
                    self.install()
                path.write_bytes(original)
            if filename != "manifest.json":
                with self.subTest(filename=filename, mode="tampered"):
                    path.write_bytes(original + b"tampered")
                    with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                        self.install()
                    path.write_bytes(original)
        self.assertEqual(self.calls, [])

    def test_rejects_symlink_and_path_escape(self):
        package = self.packages[0]
        path = self.repo / package["filename"]
        external = self.root / "external.pkg.tar.zst"
        path.rename(external)
        path.symlink_to(external)
        with self.assertRaisesRegex(ValueError, "regular local file"):
            self.install()
        path.unlink()
        external.rename(path)
        package["filename"] = "../" + package["filename"]
        self.write_manifest()
        with self.assertRaisesRegex(ValueError, "invalid package filename"):
            self.install()
        self.assertEqual(self.calls, [])

    def test_rejects_wrong_architecture_or_duplicate_browser(self):
        self.manifest["architecture"] = "aarch64"
        self.write_manifest()
        with self.assertRaisesRegex(ValueError, "architecture"):
            self.install()
        self.manifest["architecture"] = "x86_64"
        self.manifest["packages"] = [self.packages[0], self.packages[0]]
        self.write_manifest()
        with self.assertRaisesRegex(ValueError, "duplicate package"):
            self.install()
        self.assertEqual(self.calls, [])

    def test_database_must_match_manifest_even_when_its_hash_is_valid(self):
        changed = [dict(package) for package in self.packages]
        changed[0]["version"] = "unexpected-2"
        self.write_database(changed)
        with self.assertRaisesRegex(ValueError, "database disagrees"):
            self.install()
        self.write_database(self.packages[:1])
        with self.assertRaisesRegex(ValueError, "missing a required browser"):
            self.install()
        self.assertEqual(self.calls, [])

    def test_preserves_exact_versions_and_reports_failed_transaction(self):
        def wrong_version(command, **kwargs):
            result = self.pacman(command, **kwargs)
            if "--query" in command:
                result.stdout = result.stdout.replace("155.0.8059.79-1", "155.0.8059.80-1")
            return result

        with self.assertRaisesRegex(ValueError, "version differs.*google-chrome"):
            self.install(pacman=wrong_version)

        def fail(command, **kwargs):
            result = self.pacman(command, **kwargs)
            if "--sync" in command:
                raise subprocess.CalledProcessError(1, command)
            return result

        self.installed = {}
        self.missing = ["glibc"]
        self.calls.clear()
        with self.assertRaises(subprocess.CalledProcessError):
            self.install(pacman=fail)
        self.assertFalse(any("--upgrade" in command for command, _ in self.calls))
        self.assertFalse(self.transaction_config_path.exists())
        self.assertEqual(self.config.read_text(), self.original_config)

    def test_rejects_a_stale_permanent_browser_repository(self):
        self.config.write_text(self.original_config + "\n[02os-browsers]\nServer = file:///stale\n")
        with self.assertRaisesRegex(ValueError, "stale build-only repository"):
            self.install()
        self.assertEqual([command for command, _ in self.calls],
                         [["/usr/bin/pacman", "--query", "google-chrome", "brave-bin"]])


if __name__ == "__main__":
    unittest.main()
