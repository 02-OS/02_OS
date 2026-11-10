"""Public page source stays in the repository and does not deploy."""
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]


class SiteSourceTests(unittest.TestCase):
    def test_site_pages_are_markdown_and_do_not_deploy(self):
        site = ROOT / "docs" / "site"
        pages = sorted(site.glob("*.md"))
        names = {path.name for path in pages}
        self.assertTrue({"README.md", "index.md", "download.md", "agent.md", "privacy.md"} <= names)
        blob = "\n".join(path.read_text() for path in pages)
        self.assertIn("02_OS", blob)
        self.assertIn("no telemetry", blob.lower())
        self.assertIn("does not deploy", blob.lower())
        self.assertIn("does not change DNS or MX", blob)
        self.assertNotIn("vercel deploy", blob.lower())

    def test_github_templates_exist(self):
        bug = (ROOT / ".github" / "ISSUE_TEMPLATE" / "bug.yml").read_text()
        feature = (ROOT / ".github" / "ISSUE_TEMPLATE" / "feature.yml").read_text()
        pull = (ROOT / ".github" / "PULL_REQUEST_TEMPLATE.md").read_text()
        self.assertIn("name: Bug", bug)
        self.assertIn("name: Feature", feature)
        self.assertIn("## Tests", pull)
        self.assertIn("DNS or MX", pull)
