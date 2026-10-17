"""Build inputs are copied privately; failed admission cannot clean shared staging."""

import fcntl
import hashlib
import importlib.util
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
PREPARE = ROOT / "scripts/prepare-build-profile.py"


def inventory(root):
    result = {}
    for path in root.rglob("*"):
        mode = path.lstat().st_mode
        value = os.readlink(path) if path.is_symlink() else (
            hashlib.sha256(path.read_bytes()).hexdigest() if stat.S_ISREG(mode) else None)
        result[str(path.relative_to(root))] = (mode, path.lstat().st_mtime_ns, value)
    return result


class BuildProfileTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("prepare_build_profile_test", PREPARE)
        self.helper = importlib.util.module_from_spec(spec)
        with patch.object(sys, "dont_write_bytecode", True):
            spec.loader.exec_module(self.helper)
        self.temporary = tempfile.TemporaryDirectory(prefix="02os-build-profile-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        (self.source / "airootfs/usr/lib/02-agent").mkdir(parents=True)
        (self.source / "profiledef.sh").write_text("# profile\n")
        self.work = self.root / "work"
        self.work.mkdir()
        self.marker = Path(str(self.work) + ".02os-marker")
        self.marker.write_text(f"02os-work-dir\n{self.work}\n")

    def prepare(self, source=None, work=None):
        return self.helper.prepare_profile(source or self.source, work or self.work)

    def test_copy_preserves_symlinks_payload_and_modes_without_host_bytecode_or_source_changes(self):
        overlay = self.source / "airootfs"
        binary = overlay / "usr/bin/02"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"staged-runtime")
        binary.chmod(0o755)
        binary.with_name("02agent").symlink_to("02")
        (overlay / "absolute-link").symlink_to("/usr/bin/02")
        (overlay / "dangling-link").symlink_to("missing")
        outside = self.root / "outside"
        outside.mkdir()
        (outside / "host-secret").write_text("not an input")
        (overlay / "host-link").symlink_to(outside, target_is_directory=True)
        cache = overlay / "usr/lib/__pycache__"
        cache.mkdir()
        (cache / "plugin.cpython-312.pyc").write_bytes(b"host-cache")
        for name in ("unused.pyc", "unused.pyo"):
            (self.source / name).write_bytes(b"host-bytecode")
        (self.source / "aliased.pyc").symlink_to(binary)
        payload = overlay / "usr/local/share/02os/browser-repo"
        payload.mkdir(parents=True)
        (payload / "browser.pkg.tar.zst").write_bytes(b"offline-package")
        (payload / "manifest.json").write_text('{"versions":"retained"}')
        before = inventory(self.source)

        destination = self.prepare()

        self.assertEqual(destination, self.work / "profile")
        self.assertEqual(inventory(self.source), before)
        self.assertEqual((destination / "airootfs/usr/bin/02").read_bytes(), binary.read_bytes())
        self.assertEqual((destination / "airootfs/usr/bin/02").stat().st_mode & 0o777, 0o755)
        for relative in ("usr/bin/02agent", "absolute-link", "dangling-link", "host-link"):
            self.assertEqual(os.readlink(destination / "airootfs" / relative), os.readlink(overlay / relative))
        self.assertEqual((destination / payload.relative_to(self.source) / "browser.pkg.tar.zst").read_bytes(),
                         b"offline-package")
        self.assertFalse((destination / "airootfs/usr/lib/__pycache__").exists())
        self.assertFalse(any(path.name.endswith((".pyc", ".pyo")) for path in destination.rglob("*")))
        self.assertFalse((self.work / "archiso").exists())
        self.assertEqual((outside / "host-secret").read_text(), "not an input")

    def test_refuses_existing_destination_without_changing_it(self):
        destination = self.work / "profile"
        for kind in ("directory", "file", "symlink"):
            with self.subTest(kind=kind):
                if kind == "directory":
                    destination.mkdir()
                    (destination / "sentinel").write_text("keep")
                elif kind == "file":
                    destination.write_text("keep")
                else:
                    destination.symlink_to(self.source, target_is_directory=True)
                before = inventory(self.source)
                with self.assertRaisesRegex(ValueError, "existing private profile"):
                    self.prepare()
                self.assertEqual(inventory(self.source), before)
                if kind == "directory":
                    self.assertEqual((destination / "sentinel").read_text(), "keep")
                    shutil.rmtree(destination)
                else:
                    self.assertTrue(destination.exists())
                    destination.unlink()

    def test_requires_exact_regular_work_marker(self):
        original = self.marker.read_text()
        for contents in (None, "02os-work-dir\n/wrong-path\n", ""):
            with self.subTest(contents=contents):
                self.marker.unlink(missing_ok=True)
                if contents is not None:
                    self.marker.write_text(contents)
                with self.assertRaisesRegex(ValueError, "matching regular 02os-marker"):
                    self.prepare()
                self.assertEqual(list(self.work.iterdir()), [])
        self.marker.unlink()
        target = self.root / "marker-target"
        target.write_text(original)
        self.marker.symlink_to(target)
        with self.assertRaisesRegex(ValueError, "matching regular 02os-marker"):
            self.prepare()
        self.assertEqual(target.read_text(), original)

    def test_rejects_work_source_overlap_and_symlink_parents(self):
        for source, work in ((self.source, self.source), (self.source, self.source / "nested"),
                             (self.work / "nested", self.work)):
            with self.subTest(source=source, work=work):
                source.mkdir(exist_ok=True)
                work.mkdir(exist_ok=True)
                with self.assertRaisesRegex(ValueError, "must not overlap"):
                    self.prepare(source, work)
        alias = self.root / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        for source, work in ((alias / "source", self.work), (self.source, alias / "work")):
            with self.subTest(source=source, work=work):
                with self.assertRaisesRegex(ValueError, "without symlink parents"):
                    self.prepare(source, work)
        self.assertFalse((self.work / "profile").exists())

    def test_rejects_symlink_provenance_or_stamp_before_any_copy(self):
        provenance = self.source / "airootfs/usr/lib/02-agent"
        stamp = provenance / "BUILD_STAMP"
        outside = self.root / "outside"
        outside.mkdir()
        (outside / "sentinel").write_text("keep")
        stamp.symlink_to(outside / "sentinel")
        with self.assertRaisesRegex(ValueError, "BUILD_STAMP"):
            self.prepare()
        stamp.unlink()
        provenance.rmdir()
        provenance.symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "regular directories"):
            self.prepare()
        self.assertEqual(list(self.work.iterdir()), [])
        self.assertEqual((outside / "sentinel").read_text(), "keep")

    def test_failed_copy_cleans_only_its_own_partial_tree(self):
        os.mkfifo(self.source / "unsupported-fifo")
        sentinel = self.work / "sentinel"
        sentinel.write_text("keep")
        with self.assertRaises(shutil.Error):
            self.prepare()
        self.assertEqual(list(self.work.iterdir()), [sentinel])
        self.assertTrue((self.source / "unsupported-fifo").exists())
        self.assertEqual(sentinel.read_text(), "keep")


class BuildOrchestrationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="02os-build-admission-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / "repo"
        self.scripts = self.repo / "scripts"
        self.scripts.mkdir(parents=True)
        self.profile = self.repo / "profile"
        self.provenance = self.profile / "airootfs/usr/lib/02-agent"
        self.provenance.mkdir(parents=True)
        self.staged = self.profile / "airootfs/usr/bin/02"
        self.staged.parent.mkdir(parents=True)
        self.staged.write_bytes(b"keep staging until admitted")
        shutil.copy2(ROOT / "build.sh", self.repo / "build.sh")
        for name in ("prepare-work-dir.sh", "prepare-build-profile.py", "iso-manifest.py", "source-manifest.py"):
            shutil.copy2(ROOT / "scripts" / name, self.scripts / name)
        self.cache = self.repo / ".cache"
        self.cache.mkdir()
        self.work = self.root / "work"
        self.env = dict(os.environ, WORK_DIR=str(self.work), MIN_FREE_GB="0", PACMAN_CACHE_DIR=str(self.cache))

    def run_build(self):
        return subprocess.run(["bash", str(self.repo / "build.sh")], env=self.env,
                              capture_output=True, text=True, timeout=30)

    def test_failed_lock_cannot_clean_staging_or_prepare_work(self):
        with (self.cache / "iso-build.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            result = self.run_build()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("another 02_OS build is active", result.stderr)
        self.assertEqual(self.staged.read_bytes(), b"keep staging until admitted")
        self.assertFalse(self.work.exists())
        self.assertFalse((self.repo / "out").exists())

    def test_failed_work_validation_cannot_clean_staging(self):
        self.work.mkdir()
        (self.work / "sentinel").write_text("keep unmarked work")
        result = self.run_build()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("exists without marker", result.stderr)
        self.assertEqual(self.staged.read_bytes(), b"keep staging until admitted")
        self.assertEqual((self.work / "sentinel").read_text(), "keep unmarked work")
        self.assertFalse((self.repo / "out").exists())

    def test_orchestrator_stamps_only_private_input_and_mounts_it_read_only(self):
        for name in ("build-agent-runtime.sh", "verify-staged-runtime.sh", "build-browser-packages.sh"):
            script = self.scripts / name
            script.write_text("#!/bin/sh\nexit 0\n")
            script.chmod(0o755)
        (self.scripts / "stage-browser-repo.py").write_text("# Fixture staging and cleanup do nothing.\n")
        (self.profile / "test.pyc").write_bytes(b"host bytecode")
        fake_bin = self.root / "bin"
        fake_bin.mkdir()
        subprocess.run(["git", "init", "-q", str(self.repo)], check=True)
        (self.repo / ".gitignore").write_text(".cache/\nout/\nprofile/airootfs/usr/bin/\n")
        subprocess.run(["git", "-C", str(self.repo), "-c", "user.name=Fixture",
                        "-c", "user.email=fixture@example.invalid", "commit", "--allow-empty", "-qm", "fixture"], check=True)
        revision = subprocess.check_output(["git", "-C", str(self.repo), "rev-parse", "HEAD"], text=True).strip()
        docker = fake_bin / "docker"
        arguments = self.root / "docker-arguments"
        docker.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$DOCKER_ARGUMENTS\"\n"
                          "mkdir -p \"$FIXTURE_OUT\"\n"
                          "printf 'synthetic ISO bytes' > \"$FIXTURE_OUT/02_OS-2099.01.01-x86_64.iso\"\n"
                          "printf 'gnome-shell 51.1-1\\nmutter 51.1-1\\n' > \"$WORK_DIR/archiso/packages.txt\"\n")
        docker.chmod(0o755)
        self.env.update(PATH=str(fake_bin) + ":" + os.environ["PATH"], DOCKER_ARGUMENTS=str(arguments),
                        FIXTURE_OUT=str(self.repo / "out"))

        result = self.run_build()

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        private = self.work / "profile"
        self.assertEqual((private / "airootfs/usr/lib/02-agent/BUILD_STAMP").read_text(),
                         f"built_by=build.sh\nrevision={revision}\n")
        self.assertFalse((self.provenance / "BUILD_STAMP").exists())
        self.assertTrue((self.profile / "test.pyc").is_file())
        self.assertFalse((private / "test.pyc").exists())
        args = arguments.read_text().splitlines()
        self.assertIn(str(private) + ":/02_OS:ro", args)
        self.assertNotIn(str(self.profile) + ":/02_OS", args)
        self.assertIn(str(self.work / "archiso") + ":" + str(self.work / "archiso"), args)
        self.assertNotIn(str(self.work) + ":" + str(self.work), args)
        self.assertIn('    mkarchiso -v -w "${MKARCHISO_WORK_DIR}" -o /out /02_OS', args)


if __name__ == "__main__":
    unittest.main()
