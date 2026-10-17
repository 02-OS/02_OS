"""The tracked default database must describe the current source defaults."""

from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
DATABASE = ROOT / "profile/airootfs/etc/dconf/db/local"


class DconfSnapshotTests(unittest.TestCase):
    def test_tracked_database_matches_compiled_source_defaults(self):
        with tempfile.TemporaryDirectory(prefix="02os-dconf-snapshot-") as directory:
            generated = Path(directory) / "local"
            subprocess.run(["dconf", "compile", str(generated), str(DATABASE.with_name("local.d"))],
                           check=True, capture_output=True, timeout=10)
            self.assertEqual(DATABASE.read_bytes(), generated.read_bytes(),
                             "Regenerate the tracked local database after changing local.d defaults")


if __name__ == "__main__":
    unittest.main()
