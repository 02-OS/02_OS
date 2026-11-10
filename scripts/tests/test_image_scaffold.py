"""The bootc scaffold and the image workflow stay out of the ISO build."""
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]


class ImageScaffoldTests(unittest.TestCase):
    def test_build_script_does_not_call_the_containerfile(self):
        build = (ROOT / "build.sh").read_text()
        self.assertNotIn("Containerfile", build)
        self.assertNotIn("bootc", build)
        scaffold = (ROOT / "bootc/Containerfile").read_text()
        self.assertIn("FROM scratch", scaffold)
        self.assertIn("build.sh does not call this file", scaffold)
        doc = (ROOT / "docs/bootc.md").read_text()
        self.assertIn("does not call it", doc)
        self.assertIn("GitHub-hosted runners cannot produce this image", doc)
        self.assertNotIn("seconds", doc.lower())

    def test_image_workflow_is_dispatch_only(self):
        text = (ROOT / ".github/workflows/image.yml").read_text()
        self.assertIn("workflow_dispatch:", text)
        self.assertNotIn("pull_request:", text)
        self.assertNotIn("push:", text)
        self.assertIn("cannot produce the 02_OS image", text)
        self.assertNotIn("build.sh", text.split("run:")[0])
        self.assertNotIn("mkarchiso", text)
