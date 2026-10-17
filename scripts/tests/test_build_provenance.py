"""Source identity and cross-checkout work-directory ownership regressions."""
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("source_manifest", ROOT / "scripts/source-manifest.py")
source = importlib.util.module_from_spec(spec)
spec.loader.exec_module(source)


class BuildProvenanceTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="02os-source-provenance-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.git("init", "-q")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        (self.repo / ".gitignore").write_text(".cache/\n")
        self.source = self.repo / "source.txt"
        self.source.write_text("original")
        self.git("add", ".")
        self.git("commit", "-qm", "initial")

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repo), *args])

    def test_ignored_outputs_do_not_change_identity_but_untracked_source_does(self):
        initial = source.snapshot(self.repo, True)
        cache = self.repo / ".cache"
        cache.mkdir()
        (cache / "build-output").write_text("irrelevant")
        self.assertEqual(source.snapshot(self.repo), initial)
        (self.repo / "new_source.rs").write_text("fn added() {}\n")
        changed = source.snapshot(self.repo)
        self.assertTrue(changed["dirty"])
        self.assertNotEqual(changed["source_sha256"], initial["source_sha256"])
        with self.assertRaisesRegex(ValueError, "clean source checkout"):
            source.snapshot(self.repo, True)

    def test_content_modes_and_link_targets_change_identity(self):
        def digest():
            return source.snapshot(self.repo)["source_sha256"]
        initial = digest()
        self.source.write_text("modified")
        self.assertNotEqual(digest(), initial)
        self.source.write_text("original")
        self.assertEqual(digest(), initial)
        self.source.chmod(0o755)
        self.assertNotEqual(digest(), initial)
        link = self.repo / "source-link"
        link.symlink_to("source.txt")
        before = digest()
        link.unlink()
        link.symlink_to("other.txt")
        self.assertNotEqual(digest(), before)

    def test_shared_work_directory_cannot_be_reclaimed_by_another_checkout(self):
        work = self.root / "work"
        env = {**os.environ, "WORK_DIR": str(work), "SCRIPT_DIR": str(self.repo),
               "MIN_FREE_GB": "0", "PACMAN_CACHE_DIR": str(self.root / "cache")}
        script = ROOT / "scripts/prepare-work-dir.sh"
        command = 'source "$1"; prepare_work_dir; printf "READY\\n"; read -r release'
        owner = subprocess.Popen(["bash", "-euc", command, "fixture", str(script)],
                                 env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, text=True)
        def stop():
            if owner.poll() is None:
                owner.communicate("release\n", timeout=5)
        self.addCleanup(stop)
        self.assertEqual(owner.stdout.readline(), "READY\n")
        sentinel = work / "sentinel"
        sentinel.write_text("active build")
        other = self.root / "other-checkout"
        other.mkdir()
        result = subprocess.run(["bash", str(script)], env={**env, "SCRIPT_DIR": str(other)},
                                text=True, capture_output=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("another build owns work directory", result.stderr)
        self.assertEqual(sentinel.read_text(), "active build")


if __name__ == "__main__":
    unittest.main()
