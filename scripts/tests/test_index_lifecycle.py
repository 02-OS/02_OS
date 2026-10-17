"""Completed-index and automatic-watch contracts through the production CLI."""
import json
import os
from pathlib import Path
import subprocess
import time
import unittest

import test_runtime_smoke as smoke

BINARY = smoke.BINARY


class IndexLifecycleTests(unittest.TestCase):
    setUp = smoke.RuntimeSmokeTests.setUp
    run_command = smoke.RuntimeSmokeTests.run_command
    cli = smoke.RuntimeSmokeTests.cli
    commit = smoke.RuntimeSmokeTests.commit
    initialize = smoke.RuntimeSmokeTests.initialize

    def test_init_cannot_certify_unindexed_or_obsolete_symbols(self):
        self.cli("init")
        self.assertFalse(self.cli("status")["is_index_fresh"])
        self.cli("index")
        self.assertTrue(self.cli("status")["is_index_fresh"])
        (self.repo / "main.rs").write_text("fn replacement_entry() {}\n")
        self.commit("replace committed source")
        self.cli("init")
        self.assertFalse(self.cli("status")["is_index_fresh"])
        self.assertEqual(self.cli("symbol", "smoke_entry")["count"], 1)
        self.cli("index")
        self.assertTrue(self.cli("status")["is_index_fresh"])
        self.assertEqual(self.cli("symbol", "smoke_entry")["count"], 0)

    def test_untracked_content_config_and_deletion_change_freshness(self):
        self.initialize()
        source = self.repo / "untracked.rs"
        source.write_text("fn first_symbol() {}\n")
        self.assertFalse(self.cli("status")["is_index_fresh"])
        self.cli("index")
        self.assertTrue(self.cli("status")["is_index_fresh"])
        old = source.stat()
        source.write_text("fn other_symbol() {}\n")
        os.utime(source, ns=(old.st_atime_ns, old.st_mtime_ns))
        self.assertFalse(self.cli("status")["is_index_fresh"])
        self.cli("index")
        config = self.repo / ".02agent/config.toml"
        config.write_text(config.read_text() + '\n# A comment does not change selection\n')
        self.assertTrue(self.cli("status")["is_index_fresh"])
        config.write_text('index_docs = false\n')
        self.assertFalse(self.cli("status")["is_index_fresh"])
        self.cli("index")
        source.unlink()
        self.assertFalse(self.cli("status")["is_index_fresh"])
        self.cli("index")
        self.assertTrue(self.cli("status")["is_index_fresh"])
        self.assertEqual(self.cli("symbol", "other_symbol")["count"], 0)

    def test_nested_ignore_and_component_boundaries(self):
        self.cli("init")
        (self.repo / ".02agent/config.toml").write_text('custom_ignores = ["src/generated/", "build/"]\n')
        for relative, symbol in (("src/generated/out.rs", "excluded_nested"),
                                 ("src/generated-other/out.rs", "included_similar"),
                                 ("nested/build/out.rs", "excluded_component"),
                                 ("build.rs", "included_build_script")):
            path = self.repo / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f"fn {symbol}() {{}}\n")
        self.cli("index")
        for symbol, count in (("excluded_nested", 0), ("excluded_component", 0),
                              ("included_similar", 1), ("included_build_script", 1)):
            self.assertEqual(self.cli("symbol", symbol)["count"], count)

    def test_failed_index_invalidates_completion_and_retry_restores_it(self):
        self.initialize()
        ontology = self.repo / ".02agent/ontology.toml"
        ontology.write_text("invalid = [")
        result = self.run_command(str(BINARY), "--json", "index", check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.cli("status")["is_index_fresh"])
        ontology.unlink()
        self.cli("index")
        self.assertTrue(self.cli("status")["is_index_fresh"])

    def test_automatic_registration_respects_persistent_opt_out(self):
        self.initialize()
        self.assertTrue(self.cli("status")["watching"])
        self.assertFalse(self.cli("status")["daemon_available"])
        self.cli("unwatch")
        self.cli("init")
        self.cli("index")
        self.assertFalse(self.cli("status")["watching"])
        self.assertTrue(self.cli("watch")["watching"])
        state = Path(self.env["XDG_DATA_HOME"]) / "02-agent/watched_repos.json"
        self.assertIn(str(self.repo), json.loads(state.read_text())["repositories"])
        self.assertEqual(json.loads(state.read_text())["opted_out_repositories"], [])

    def test_daemon_indexes_automatically_registered_repository_after_startup(self):
        self.initialize()
        log = (self.base / "daemon.log").open("w")
        self.addCleanup(log.close)
        daemon = subprocess.Popen([str(BINARY.with_name("02-agentd")), "--interval", "1"],
                                  cwd=self.base, env=self.env, stdout=log, stderr=log)
        def stop():
            daemon.terminate()
            try:
                daemon.wait(timeout=5)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait(timeout=5)
        self.addCleanup(stop)
        (self.repo / "main.rs").write_text("fn automatically_watched_symbol() {}\n")
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            self.assertIsNone(daemon.poll())
            if self.cli("symbol", "automatically_watched_symbol")["count"] == 1:
                break
            time.sleep(.05)
        else:
            self.fail("daemon never indexed the automatically registered repository")
        self.assertTrue(self.cli("status")["daemon_available"])
        self.assertTrue(self.cli("status")["is_index_fresh"])


if __name__ == "__main__":
    unittest.main()
