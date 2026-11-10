"""Timing samples stay ignored, and the shipped appearance bytes stay put."""

import hashlib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]

MOTION_SHA256 = "1fb90061ff01dbb74ba67bbb1335f6dd838672cf63280b993b21e78c25198f0d"

IGNORED = (
    ("components/02-agent/crates/agent-context/src/benchkit.rs", "indexing_and_ranking_skeleton"),
    ("components/02-agent/crates/agent-context/src/compiler.rs", "benchmark_budget_pruning"),
)

THREATS = (
    "remote_and_wildcard_endpoints_are_rejected",
    "default_router_does_not_name_a_model",
    "test_secret_detection",
    "secret_named_dirty_file_is_omitted_from_context",
    "context_redacts_secrets_and_obeys_a_tight_budget",
    "test_config_refuses_symlink_and_does_not_follow_it",
    "test_socket_bind_and_permissions",
    "test_private_dir_is_mode_0700",
    "symlinks_are_not_indexed",
    "status_file_is_private_and_reports_the_phase_line",
    "gnome_decision_requires_the_displayed_hash_and_forces_gnome_source",
    "mcp_cannot_approve_itself",
    "mcp_approver_is_rejected_even_when_the_hash_matches",
    "calls_are_denied_without_consent_or_when_the_kill_file_exists",
    "tool_call_without_consent_is_an_error_result_and_list_is_local",
    "a_consented_fake_tree_lists_clicks_and_does_not_echo_typed_text",
    "session_probe_errors_without_a_bus_and_refuses_a_reachable_bus",
    "ingest_requires_a_matching_digest_and_rejects_an_applied_ask",
    "current_catalog_opens_while_a_writer_holds_the_reserved_lock",
    "rejects_path_escape_unknown_tools_and_symlinks",
    "token_is_stored_as_a_hash_and_compared_without_the_secret",
    "dashboard_binary_listener_is_loopback",
    "dashboard_page_reads_the_private_status_file",
)


class ProductGateTests(unittest.TestCase):
    def test_timing_samples_stay_ignored(self):
        for relative, name in IGNORED:
            text = (ROOT / relative).read_text()
            marker = f"fn {name}"
            index = text.index(marker)
            window = text[max(0, index - 240) : index]
            self.assertIn("#[ignore", window)
            self.assertIn("not a release gate", (ROOT / "docs/release-checklist.md").read_text())

    def test_capitaine_and_zero_portal_stay(self):
        dconf = (ROOT / "profile/airootfs/etc/dconf/db/local.d/00-02os").read_text()
        self.assertIn("cursor-theme='capitaine-cursors'", dconf)
        self.assertIn("screensaver=['<Super>l']", dconf)
        for relative in (
            "profile/airootfs/etc/skel/.config/gtk-3.0/settings.ini",
            "profile/airootfs/etc/skel/.config/gtk-4.0/settings.ini",
        ):
            self.assertIn(
                "gtk-cursor-theme-name=capitaine-cursors",
                (ROOT / relative).read_text(),
            )
        motion = (
            ROOT
            / "profile/airootfs/usr/share/gnome-shell/extensions/02-zero-portal@02os/motion.js"
        )
        digest = hashlib.sha256(motion.read_bytes()).hexdigest()
        self.assertEqual(digest, MOTION_SHA256)
        metadata = (
            ROOT
            / "profile/airootfs/usr/share/gnome-shell/extensions/02-zero-portal@02os/metadata.json"
        ).read_text()
        self.assertIn("02-zero-portal@02os", metadata)

    def test_security_audit_names_real_tests(self):
        audit = (ROOT / "docs/security-audit.md").read_text()
        rust = []
        for path in (ROOT / "components/02-agent").rglob("*.rs"):
            rust.append(path.read_text())
        blob = "\n".join(rust)
        for name in THREATS:
            self.assertIn(f"`{name}`", audit)
            self.assertIn(f"fn {name}", blob)

    def test_notes_record_the_skipped_image_build(self):
        notes = "\n".join(
            (ROOT / name).read_text()
            for name in (
                "CHANGELOG.md",
                "docs/launch-outline.md",
                "docs/release-checklist.md",
                "docs/migration.md",
                "docs/onboarding.md",
            )
        )
        self.assertIn("4 GiB", notes)
        self.assertIn("40 GiB", notes)
        self.assertIn("2026.09.26", notes)
        self.assertIn("2026.10.01", notes)
        self.assertIn("2026.10.06", notes)
        self.assertNotIn("seconds", (ROOT / "docs/launch-outline.md").read_text())
        self.assertNotIn("seconds", (ROOT / "docs/bootc.md").read_text())
