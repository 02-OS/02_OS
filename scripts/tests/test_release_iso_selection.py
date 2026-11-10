"""Fresh releases pick the ISO this run wrote; --reuse-iso stays strict."""
import hashlib
import os
import shlex
import shutil
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
RELEASE = ROOT / "scripts/release.sh"
MANIFEST = ROOT / "scripts/iso-manifest.py"


def git_head():
    return subprocess.check_output(
        ["git", "-C", str(ROOT), "rev-parse", "HEAD"],
        text=True,
        timeout=10,
    ).strip()


class ReleaseIsoSelectionTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="02os-release-")
        self.root = Path(self._tmp.name)
        self.out = self.root / "out"
        self.out.mkdir()
        scripts = self.root / "scripts"
        scripts.mkdir()
        for name in ("release.sh", "iso-manifest.py", "source-manifest.py"):
            shutil.copy2(ROOT / "scripts" / name, scripts / name)
        self.release = scripts / "release.sh"
        self.manifest = scripts / "iso-manifest.py"
        self.source = self.out / "source.json"
        (self.root / ".gitignore").write_text("out/\nbuild.sh\n__pycache__/\n")
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run(["git", "-C", str(self.root), "add", "."], check=True)
        subprocess.run(["git", "-C", str(self.root), "-c", "user.name=Fixture",
                        "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture"], check=True)
        self.head = subprocess.check_output(["git", "-C", str(self.root), "rev-parse", "HEAD"], text=True).strip()
        subprocess.run(["python3", str(scripts / "source-manifest.py"), str(self.root),
                        "--output", str(self.source)], check=True)

    def tearDown(self):
        self._tmp.cleanup()

    def complete(self, iso):
        subprocess.run(["python3", str(self.manifest), "create", str(iso), self.head,
                        "--source-manifest", str(self.source)],
                       check=True, capture_output=True, timeout=10)

    def write_stub(self, body, complete=None):
        if complete:
            body += (f'python3 {shlex.quote(str(self.manifest))} create '
                     f'"${{OUT_DIR}}/{complete}" {shlex.quote(self.head)} '
                     f'--source-manifest {shlex.quote(str(self.source))}\n')
        stub = self.root / "build.sh"
        stub.write_text("#!/bin/bash\nset -euo pipefail\n" + body)
        stub.chmod(0o755)
        return stub

    def run_release(self, stub, *args):
        env = os.environ.copy()
        env["RELEASE_DRY_RUN"] = "1"
        env["RELEASE_OUT_DIR"] = str(self.out)
        env["RELEASE_BUILD_SCRIPT"] = str(stub)
        return subprocess.run(
            ["bash", str(self.release), *args],
            cwd=self.root,
            env=env,
            capture_output=True,
            text=True,
            timeout=30,
        )

    def test_published_sha256_must_match_the_iso(self):
        iso = self.out / "02_OS-2099.10.06-x86_64.iso"
        payload = b"image-bytes"
        iso.write_bytes(payload)
        self.complete(iso)
        digest = hashlib.sha256(payload).hexdigest()
        sidecar = self.out / "02_OS.sha256"
        sidecar.write_text(f"{digest}  {iso.name}\n")
        good = subprocess.run(
            ["python3", str(self.manifest), "verify", str(iso), self.head],
            capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(good.returncode, 0, good.stderr)
        sidecar.write_text(f"{'0' * 64}  {iso.name}\n")
        bad = subprocess.run(
            ["python3", str(self.manifest), "verify", str(iso), self.head],
            capture_output=True, text=True, timeout=10,
        )
        self.assertNotEqual(bad.returncode, 0)
        self.assertIn("02_OS.sha256", bad.stderr)

    def test_fresh_build_selects_new_iso_when_an_older_one_exists(self):
        old = self.out / "02_OS-2099.10.05-x86_64.iso"
        old.write_bytes(b"previous-image")
        (self.out / (old.name + ".commit")).write_text("not-this-commit\n")
        stub = self.write_stub(
            'printf "new-image\\n" > "${OUT_DIR}/02_OS-2099.10.06-x86_64.iso"\n',
            complete="02_OS-2099.10.06-x86_64.iso",
        )
        result = self.run_release(stub)
        combined = result.stdout + result.stderr
        self.assertEqual(result.returncode, 0, combined)
        self.assertNotIn("found 2", combined)
        self.assertIn("Selected ISO:", result.stdout)
        selected = next(line for line in result.stdout.splitlines() if line.startswith("Selected ISO:"))
        self.assertIn("02_OS-2099.10.06-x86_64.iso", selected)
        self.assertNotIn("02_OS-2099.10.05-x86_64.iso", selected)
        self.assertIn("skipping split, tag, and GitHub publish", result.stdout)
        self.assertNotIn("Publishing GitHub", result.stdout)
        new = self.out / "02_OS-2099.10.06-x86_64.iso"
        self.assertEqual(new.read_text(), "new-image\n")
        self.assertEqual((self.out / (new.name + ".commit")).read_text().strip(), self.head)
        self.assertTrue(old.is_file(), "older ISO must be left in place")

    def test_release_rejects_dirty_source_before_calling_builder(self):
        (self.root / "new-source.rs").write_text("fn changed() {}\n")
        stub = self.write_stub('echo BUILDER_WAS_CALLED\n')
        result = self.run_release(stub)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("clean source checkout", result.stderr)
        self.assertNotIn("BUILDER_WAS_CALLED", result.stdout)

    def test_reuse_rejects_dirty_build_provenance_for_identical_bytes(self):
        import json
        iso = self.out / "02_OS-2099.10.06-x86_64.iso"
        iso.write_bytes(b"image")
        self.complete(iso)
        path = Path(str(iso) + ".manifest.json")
        manifest = json.loads(path.read_text())
        manifest["source"]["dirty"] = True
        path.write_text(json.dumps(manifest))
        result = self.run_release(self.write_stub('exit 99\n'), "--reuse-iso")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("source snapshot", result.stderr)

    def test_fresh_build_selects_new_iso_among_two_older_ones(self):
        for name in ("02_OS-2099.10.04-x86_64.iso", "02_OS-2099.10.05-x86_64.iso"):
            (self.out / name).write_bytes(b"old")
        stub = self.write_stub(
            'printf "newest\\n" > "${OUT_DIR}/02_OS-2099.10.06-x86_64.iso"\n',
            complete="02_OS-2099.10.06-x86_64.iso",
        )
        result = self.run_release(stub)
        combined = result.stdout + result.stderr
        self.assertEqual(result.returncode, 0, combined)
        self.assertNotIn("found 2", combined)
        self.assertNotIn("found 3", combined)
        self.assertIn("02_OS-2099.10.06-x86_64.iso", result.stdout)
        selected = next(line for line in result.stdout.splitlines() if line.startswith("Selected ISO:"))
        self.assertNotIn("2099.10.04", selected)
        self.assertNotIn("2099.10.05", selected)

    def test_same_name_rewrite_requires_head_stamp(self):
        iso = self.out / "02_OS-2099.10.06-x86_64.iso"
        iso.write_bytes(b"morning-build")
        os.utime(iso, (1_000_000_000, 1_000_000_000))
        (self.out / (iso.name + ".commit")).write_text("stale-stamp\n")
        stub = self.write_stub(
            'printf "evening-build\\n" > "${OUT_DIR}/02_OS-2099.10.06-x86_64.iso"\n'
            f'printf "%s\\n" "{self.head}" > "${{OUT_DIR}}/02_OS-2099.10.06-x86_64.iso.commit"\n',
            complete="02_OS-2099.10.06-x86_64.iso",
        )
        result = self.run_release(stub)
        combined = result.stdout + result.stderr
        self.assertEqual(result.returncode, 0, combined)
        selected = next(line for line in result.stdout.splitlines() if line.startswith("Selected ISO:"))
        self.assertIn("02_OS-2099.10.06-x86_64.iso", selected)
        self.assertEqual(iso.read_bytes(), b"evening-build\n")

    def test_same_second_same_size_rewrite_is_identified(self):
        iso = self.out / "02_OS-2099.10.06-x86_64.iso"
        iso.write_bytes(b"old-bytes")
        os.utime(iso, ns=(1_000_000_000_100_000_000, 1_000_000_000_100_000_000))
        self.complete(iso)
        # Keep whole-second mtime, byte count and inode unchanged. Only the
        # subsecond timestamp and content distinguish this completed build.
        stub = self.write_stub(
            'printf "new-bytes" > "${OUT_DIR}/' + iso.name + '"\n'
            'python3 -c \'import os,sys; os.utime(sys.argv[1], ns=(1000000000200000000, 1000000000200000000))\' '
            '"${OUT_DIR}/' + iso.name + '"\n', complete=iso.name)
        result = self.run_release(stub)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Selected ISO:", result.stdout)
        self.assertEqual(iso.read_bytes(), b"new-bytes")

    def test_same_name_rewrite_with_wrong_stamp_is_rejected(self):
        iso = self.out / "02_OS-2099.10.06-x86_64.iso"
        iso.write_bytes(b"morning-build")
        os.utime(iso, (1_000_000_000, 1_000_000_000))
        (self.out / (iso.name + ".commit")).write_text(self.head + "\n")
        stub = self.write_stub(
            'printf "evening-build\\n" > "${OUT_DIR}/02_OS-2099.10.06-x86_64.iso"\n'
            'printf "not-head\\n" > "${OUT_DIR}/02_OS-2099.10.06-x86_64.iso.commit"\n'
        )
        result = self.run_release(stub)
        combined = result.stdout + result.stderr
        self.assertNotEqual(result.returncode, 0, combined)
        self.assertIn("does not match HEAD", result.stderr)
        self.assertNotIn("Selected ISO:", result.stdout)
        self.assertNotIn("skipping split", result.stdout)

    def test_untouched_iso_is_not_selected_even_if_stamp_matches_head(self):
        iso = self.out / "02_OS-2099.10.05-x86_64.iso"
        iso.write_bytes(b"untouched")
        os.utime(iso, (1_000_000_000, 1_000_000_000))
        (self.out / (iso.name + ".commit")).write_text(self.head + "\n")
        stub = self.write_stub("exit 0\n")
        result = self.run_release(stub)
        combined = result.stdout + result.stderr
        self.assertNotEqual(result.returncode, 0, combined)
        self.assertIn("could not identify", result.stderr)
        self.assertNotIn("Selected ISO:", result.stdout)
        self.assertEqual(iso.read_bytes(), b"untouched")

    def test_reuse_iso_fails_when_two_isos_exist(self):
        first = self.out / "02_OS-2099.10.05-x86_64.iso"
        second = self.out / "02_OS-2099.10.06-x86_64.iso"
        first.write_bytes(b"one")
        second.write_bytes(b"two")
        (self.out / (second.name + ".commit")).write_text(self.head + "\n")
        stub = self.write_stub('echo "build should not run" >&2\nexit 99\n')
        result = self.run_release(stub, "--reuse-iso")
        combined = result.stdout + result.stderr
        self.assertNotEqual(result.returncode, 0, combined)
        self.assertIn("exactly one", result.stderr)
        self.assertIn("found 2", result.stderr)
        self.assertNotIn("Selected ISO:", result.stdout)
        self.assertNotIn("build should not run", combined)

    def test_reuse_iso_still_requires_head_stamp(self):
        iso = self.out / "02_OS-2099.10.06-x86_64.iso"
        iso.write_bytes(b"only")
        (self.out / (iso.name + ".commit")).write_text("other-commit\n")
        stub = self.write_stub("exit 99\n")
        bad = self.run_release(stub, "--reuse-iso")
        self.assertNotEqual(bad.returncode, 0, bad.stderr)
        self.assertIn("does not match HEAD", bad.stderr)
        self.assertNotIn("Selected ISO:", bad.stdout)

        (self.out / (iso.name + ".commit")).write_text(self.head + "\n")
        self.complete(iso)
        good = self.run_release(stub, "--reuse-iso")
        combined = good.stdout + good.stderr
        self.assertEqual(good.returncode, 0, combined)
        self.assertIn("Selected ISO:", good.stdout)
        self.assertIn(iso.name, good.stdout)
        self.assertIn("Reusing ISO", good.stdout)
        self.assertNotIn("build should not run", combined)

    def test_commit_only_iso_requires_a_rebuild(self):
        iso = self.out / "02_OS-2099.10.06-x86_64.iso"
        iso.write_bytes(b"legacy-image")
        Path(str(iso) + ".commit").write_text(self.head + "\n")
        result = self.run_release(self.write_stub("exit 99\n"), "--reuse-iso")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no completion manifest", result.stderr)
        self.assertNotIn("Selected ISO:", result.stdout)

    def test_reuse_rejects_changed_bytes_even_when_size_and_revision_match(self):
        iso = self.out / "02_OS-2099.10.06-x86_64.iso"
        iso.write_bytes(b"complete-image")
        self.complete(iso)
        iso.write_bytes(b"tampered-image")
        result = self.run_release(self.write_stub("exit 99\n"), "--reuse-iso")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("size/SHA-256", result.stderr)
        self.assertNotIn("Selected ISO:", result.stdout)

    def test_failed_in_place_rebuild_cannot_be_reused(self):
        iso = self.out / "02_OS-2099.10.06-x86_64.iso"
        iso.write_bytes(b"complete-image")
        self.complete(iso)
        stub = self.write_stub('printf "partial" > "${OUT_DIR}/' + iso.name + '"\nexit 1\n')
        failed = self.run_release(stub)
        self.assertNotEqual(failed.returncode, 0)
        reused = self.run_release(stub, "--reuse-iso")
        self.assertNotEqual(reused.returncode, 0)
        self.assertIn("size/SHA-256", reused.stderr)
        self.assertNotIn("Selected ISO:", reused.stdout)

    def test_empty_iso_does_not_publish_completion_metadata(self):
        iso = self.out / "02_OS-2099.10.06-x86_64.iso"
        iso.touch()
        result = subprocess.run(["python3", str(MANIFEST), "create", str(iso), self.head],
                                text=True, capture_output=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(Path(str(iso) + ".manifest.json").exists())


if __name__ == "__main__":
    unittest.main()
