"""Daemon watch-state errors and startup reconciliation, through real processes."""
import contextlib
import hashlib
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
BINARY = Path(os.environ.get("AGENT_BINARY", ROOT / "components/02-agent/target/debug/02")).resolve()
DAEMON_BINARY = Path(os.environ.get("AGENT_DAEMON_BINARY", BINARY.with_name("02-agentd"))).resolve()


class DaemonRegressionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="02-daemon-regression-")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.repo = self.base / "repo"
        self.repo.mkdir()
        self.env = os.environ.copy()
        for name, directory in (("HOME", "home"), ("XDG_DATA_HOME", "data"),
                                ("XDG_CONFIG_HOME", "config"), ("XDG_CACHE_HOME", "cache"),
                                ("XDG_RUNTIME_DIR", "run")):
            path = self.base / directory
            path.mkdir(mode=0o700)
            self.env[name] = str(path)
        self.state = self.base / "data/02-agent/watched_repos.json"
        self.state.parent.mkdir(mode=0o700)
        self.git("init", "-q")
        self.git("config", "user.name", "Daemon regression fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        (self.repo / ".gitignore").write_text(".02agent/\n")
        (self.repo / "main.rs").write_text("pub fn committed_definition() {}\n")
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.repo, env=self.env,
                              check=True, text=True, capture_output=True, timeout=30).stdout

    def cli(self, *args):
        result = subprocess.run([str(BINARY), "--json", *args], cwd=self.repo,
                                env=self.env, text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def start_daemon(self):
        socket_path = self.base / "run/daemon.sock"
        log_path = self.base / "daemon.stderr"
        log = log_path.open("w")
        self.addCleanup(log.close)
        process = subprocess.Popen([str(DAEMON_BINARY), "--socket", str(socket_path), "--interval", "1"],
                                   cwd=self.base / "home", env=self.env,
                                   stdout=subprocess.DEVNULL, stderr=log)

        def stop():
            if process.poll() is None:
                process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        self.addCleanup(stop)
        self.wait_until(lambda: socket_path.exists() or process.poll() is not None)
        self.assertIsNone(process.poll(), log_path.read_text())
        client = socket.socket(socket.AF_UNIX)
        client.settimeout(5)
        client.connect(str(socket_path))
        self.addCleanup(client.close)
        reader = client.makefile("r")
        self.addCleanup(reader.close)

        def request(method, params=None, notification=False):
            value = {"jsonrpc": "2.0", "method": method, "params": params or {}}
            if not notification:
                value["id"] = "fixture-request"
            client.sendall((json.dumps(value) + "\n").encode())
            if notification:
                return None
            response = json.loads(reader.readline())
            self.assertEqual(response["id"], "fixture-request")
            return response
        # Read a complete reply to establish that the socket handler is ready.
        self.assertEqual(request("ping")["result"], {})
        return process, request, log_path, stop

    def wait_until(self, predicate, timeout=6):
        deadline = time.monotonic() + timeout
        while not predicate():
            self.assertLess(time.monotonic(), deadline, "daemon did not reach expected state")
            time.sleep(0.05)

    def assert_server_error(self, response):
        self.assertNotIn("result", response)
        self.assertEqual(response["error"]["code"], -32000)
        self.assertTrue(response["error"]["message"])

    def test_watch_errors_are_returned_and_notifications_are_silent(self):
        corrupt = '{"repositories":['
        self.state.write_text(corrupt)
        _, request, log_path, _ = self.start_daemon()
        for method in ("daemon/watch", "daemon/unwatch"):
            self.assert_server_error(request(method, {"path": str(self.repo)}))
            request(method, {"path": str(self.repo)}, notification=True)
        # The next response must belong to ping, with no notification response
        # lines preceding it. Failures must still be recorded locally.
        self.assertEqual(request("ping")["result"], {})
        self.assertEqual(self.state.read_text(), corrupt)
        log = log_path.read_text()
        self.assertIn("daemon/watch", log)
        self.assertIn("daemon/unwatch", log)
        self.assertIn("malformed", log)

        self.state.unlink()
        self.state.mkdir()
        for method in ("daemon/watch", "daemon/unwatch"):
            self.assert_server_error(request(method, {"path": str(self.repo)}))
        self.assertTrue(self.state.is_dir())

    def test_watch_write_failures_are_not_reported_as_no_ops(self):
        self.state.write_text('{"repositories": []}')
        process, request, _, _ = self.start_daemon()
        for sequence, (method, repositories) in enumerate((
                ("daemon/watch", []), ("daemon/unwatch", [str(self.repo)]))):
            original = json.dumps({"repositories": repositories})
            self.state.write_text(original)
            # Force File::create to fail deterministically without depending on
            # UID/permission behavior: the daemon temporary path is a directory.
            blocked_temp = self.state.parent / f".watched_repos.json.{process.pid}.{sequence}.tmp"
            blocked_temp.mkdir()
            self.assert_server_error(request(method, {"path": str(self.repo)}))
            self.assertEqual(self.state.read_text(), original)

    def test_duplicate_watch_and_absent_unwatch_remain_successful(self):
        _, request, _, _ = self.start_daemon()
        path = {"path": str(self.repo)}
        self.assertEqual(request("daemon/watch", path)["result"], {"added": True})
        self.assertEqual(request("daemon/watch", path)["result"], {"added": False})
        self.assertEqual(request("daemon/unwatch", path)["result"], {"removed": True})
        self.assertEqual(request("daemon/unwatch", path)["result"], {"removed": False})

    def prepare_deleted_untracked_file(self):
        self.cli("index")
        (self.repo / "untracked.rs").write_text("pub fn removed_untracked_definition() {}\n")
        self.cli("index")
        (self.repo / "untracked.rs").unlink()
        self.assertEqual(self.git("status", "--porcelain"), "")
        self.state.write_text(json.dumps({"repositories": [str(self.repo)]}))
        return next((self.base / "data").rglob("index.sqlite"))

    def test_nested_repo_override_keeps_full_catalog_and_context_snippets(self):
        nested = self.repo / "src"
        nested.mkdir()
        (nested / "module.rs").write_text("pub fn nested_definition() {}\n")
        self.git("add", "src")
        self.git("commit", "-qm", "nested fixture")
        self.cli("index")
        database = next((self.base / "data").rglob("index.sqlite"))

        def catalog():
            with sqlite3.connect(database) as conn:
                return conn.execute("SELECT relative_path FROM files ORDER BY relative_path").fetchall()

        before = catalog()
        _, request, _, _ = self.start_daemon()

        def tool(name, arguments):
            response = request("tools/call", {"name": name, "arguments": arguments})
            self.assertNotIn("error", response)
            self.assertFalse(response["result"]["isError"], response)
            return json.loads(response["result"]["content"][0]["text"])

        package = tool("repository_context", {
            "repo_path": str(nested), "task": "committed_definition nested_definition",
        })
        snippets = {item["relative_path"]: item["snippet"] for item in package["relevant_files"]}
        self.assertIn("committed_definition", snippets["main.rs"])
        self.assertIn("nested_definition", snippets["src/module.rs"])
        tool("add", {"repo_path": str(nested)})
        # The session now also names the nested directory; an omitted override
        # must resolve to the same parent worktree before scanning.
        tool("cognify", {})
        self.assertEqual(catalog(), before)
        self.assertEqual(self.cli("symbol", "committed_definition")["count"], 1)
        self.assertEqual(self.cli("symbol", "nested_definition")["count"], 1)

    def test_restart_removes_deleted_untracked_catalog_and_search_results(self):
        self.prepare_deleted_untracked_file()
        self.start_daemon()
        self.wait_until(lambda: self.cli("symbol", "removed_untracked_definition")["count"] == 0)
        self.assertEqual(self.cli("search", "removed_untracked_definition")["count"], 0)
        database = next((self.base / "data").rglob("index.sqlite"))
        with sqlite3.connect(database) as conn:
            self.assertEqual(conn.execute("SELECT count(*) FROM files WHERE relative_path='untracked.rs'").fetchone()[0], 0)
        # Once reconciliation succeeds, unchanged ticks must not keep indexing.
        indexed_at = self.cli("status")["last_indexed_at"]
        time.sleep(1.2)
        self.assertEqual(self.cli("status")["last_indexed_at"], indexed_at)

    def test_restart_reconciles_tracked_content_reverted_to_clean_head(self):
        self.cli("index")
        (self.repo / "main.rs").write_text("pub fn transient_dirty_definition() {}\n")
        self.cli("index")
        self.git("checkout", "--", "main.rs")
        self.assertEqual(self.git("status", "--porcelain"), "")
        self.state.write_text(json.dumps({"repositories": [str(self.repo)]}))
        self.start_daemon()
        self.wait_until(lambda: self.cli("symbol", "transient_dirty_definition")["count"] == 0)
        self.assertEqual(self.cli("symbol", "committed_definition")["count"], 1)

    def test_failed_startup_reconciliation_retries_without_a_git_change(self):
        database = self.prepare_deleted_untracked_file()
        with sqlite3.connect(database) as conn:
            conn.execute("""CREATE TRIGGER fixture_sync_failure BEFORE DELETE ON files
                            BEGIN SELECT RAISE(ABORT, 'fixture sync failure'); END""")
        _, _, log_path, _ = self.start_daemon()
        self.wait_until(lambda: "fixture sync failure" in log_path.read_text())
        self.assertEqual(self.cli("symbol", "removed_untracked_definition")["count"], 1)
        with sqlite3.connect(database) as conn:
            conn.execute("DROP TRIGGER fixture_sync_failure")
        self.wait_until(lambda: self.cli("symbol", "removed_untracked_definition")["count"] == 0)
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_02_and_02agent_mcp_enforce_identical_policy_and_approval_binding(self):
        # 1. Stdio MCP parity between `02 mcp` and `02agent mcp` symlink + raw-arg-free telemetry.
        (self.repo / ".env").write_text("SECRET=HARMLESS_DOTENV_MARKER_ABC123456\n")
        self.cli("index")
        alias_dir = self.base / "bin"
        alias_dir.mkdir()
        alias_bin = alias_dir / "02agent"
        alias_bin.symlink_to(BINARY)

        outside_repo = self.base / "outside"
        outside_repo.mkdir()
        subprocess.run(["git", "init", "-q"], cwd=outside_repo, env=self.env, check=True)
        subprocess.run(["git", "config", "user.name", "Outside"], cwd=outside_repo, env=self.env, check=True)
        subprocess.run(["git", "config", "user.email", "out@example.invalid"], cwd=outside_repo, env=self.env, check=True)
        (outside_repo / "secret.rs").write_text("pub fn outside_only() {}\n")
        subprocess.run(["git", "add", "."], cwd=outside_repo, env=self.env, check=True)
        subprocess.run(["git", "commit", "-qm", "outside"], cwd=outside_repo, env=self.env, check=True)

        marker = "HARMLESS_DOTENV_MARKER_ABC123456"
        stripe_like = "sk_test_MCP_ARG_LEAK_PROOF_1234567890"
        requests_template = [
            {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
             "params": {"name": "approve", "arguments": {"decision": "allow"}}},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
             "params": {"name": "record_approval", "arguments": {"decision": "allow"}}},
            {"jsonrpc": "2.0", "id": 3, "method": "resources/read",
             "params": {"uri": "02://repo/.env"}},
            {"jsonrpc": "2.0", "id": 4, "method": "tools/call",
             "params": {"name": "repository_status", "arguments": {"repo_path": str(outside_repo)}}},
            {"jsonrpc": "2.0", "id": 5, "method": "tools/call",
             "params": {"name": "repository_search",
                        "arguments": {"query": marker, "password": "fixture-password-987"}}},
            {"jsonrpc": "2.0", "id": 6, "method": "tools/call",
             "params": {"name": "repository_search",
                        "arguments": {"query": stripe_like}}},
        ]
        wire = "\n".join(json.dumps(item) for item in requests_template) + "\n"
        observed = {}
        for label, executable in (("02", BINARY), ("02agent", alias_bin)):
            proc = subprocess.run([str(executable), "mcp"], cwd=self.repo, env=self.env,
                                  input=wire, text=True, capture_output=True, timeout=30)
            self.assertEqual(proc.returncode, 0, proc.stderr)
            replies = {msg["id"]: msg for msg in map(json.loads, proc.stdout.splitlines())}
            self.assertTrue(replies[1]["result"]["isError"])
            self.assertIn("cannot approve", replies[1]["result"]["content"][0]["text"].lower())
            self.assertTrue(replies[2]["result"]["isError"])
            self.assertIn("cannot approve", replies[2]["result"]["content"][0]["text"].lower())
            self.assertIn("error", replies[3])
            self.assertIn("secret filename", replies[3]["error"]["message"].lower())
            self.assertTrue(replies[4]["result"]["isError"])
            self.assertIn("outside allowed workspace", replies[4]["result"]["content"][0]["text"].lower())
            self.assertFalse(replies[5]["result"]["isError"])
            self.assertFalse(replies[6]["result"]["isError"])
            observed[label] = replies
        self.assertEqual(observed["02"], observed["02agent"])

        database = next((self.base / "data").rglob("index.sqlite"))
        with contextlib.closing(sqlite3.connect(database)) as conn:
            rows = conn.execute(
                "SELECT tool, args_hash, args_canonical, finished_at FROM tool_invocations"
            ).fetchall()
            self.assertEqual(len(rows), 4)
            for _, args_hash, args_canonical, finished_at in rows:
                self.assertEqual(len(args_hash), 64)
                self.assertEqual(args_canonical, "")
                self.assertIsNotNone(finished_at)
        raw_db = database.read_bytes()
        for forbidden in (marker.encode(), stripe_like.encode(), b"fixture-password-987", b'"query"'):
            self.assertNotIn(forbidden, raw_db)

        # 2. Daemon socket policy parity for `02 mcp` and `02agent mcp` peers + exact approval binding.
        payload_a = json.dumps({"cmd": "deploy-prod", "token": "ghp_" + "a" * 36}, separators=(",", ":"))
        payload_b = json.dumps({"cmd": "deploy-prod", "token": "ghp_" + "b" * 36}, separators=(",", ":"))
        payload_redacted = json.dumps({"cmd": "deploy-prod", "token": "[REDACTED]"}, separators=(",", ":"))
        hash_a = hashlib.sha256(payload_a.encode()).hexdigest()
        hash_b = hashlib.sha256(payload_b.encode()).hexdigest()
        hash_redacted = hashlib.sha256(payload_redacted.encode()).hexdigest()
        nonce = "nonce-daemon-parity-01"

        with contextlib.closing(sqlite3.connect(database)) as conn:
            with conn:
                repo_id = conn.execute("SELECT repo_id FROM repositories LIMIT 1").fetchone()[0]
                actor_id = conn.execute("SELECT id FROM actors LIMIT 1").fetchone()[0]
                conn.execute(
                    """INSERT INTO tool_invocations (
                           id, repo_id, actor_id, tool, args_hash, args_canonical,
                           started_at, finished_at, is_error, error_code, warning_json
                       ) VALUES (?, ?, ?, 'deploy', ?, '', '2026-11-21T00:00:00Z', NULL, 0, NULL, ?)""",
                    ("pending-parity-1", repo_id, actor_id, hash_a,
                     json.dumps({"confirmation_nonce": nonce, "status": "pending"})),
                )

        self.state.write_text(json.dumps({"repositories": [str(self.repo)]}))
        _, request, _, _ = self.start_daemon()
        socket_path = self.base / "run/daemon.sock"

        valid_decide = {
            "jsonrpc": "2.0",
            "id": "decide-1",
            "method": "daemon/decide",
            "params": {
                "repo_path": str(self.repo),
                "id": "gnome-parity-1",
                "pending_id": "pending-parity-1",
                "confirmation_nonce": nonce,
                "args_hash": hash_a,
                "confirmed_hash": hash_a,
                "decision": "allow",
                "approver": "desktop-user",
                "invoker": "agent",
            },
        }

        # Spawn real peers with exact Linux /proc/<pid>/cmdline for both `02 mcp` and `02agent mcp`.
        peer_script = self.base / "mcp"
        peer_script.write_text(
            "import os, socket, sys\n"
            "sock = socket.socket(socket.AF_UNIX)\n"
            "sock.settimeout(5)\n"
            "sock.connect(os.environ['DAEMON_SOCK'])\n"
            "sock.sendall((os.environ['DECIDE_PAYLOAD'] + '\\n').encode())\n"
            "print(sock.makefile('r').readline(), end='')\n"
            "sock.close()\n"
        )
        peer_env = dict(self.env, DAEMON_SOCK=str(socket_path), DECIDE_PAYLOAD=json.dumps(valid_decide))
        for argv0 in ("02", "/usr/bin/02", "02agent", "/usr/bin/02agent"):
            with self.subTest(peer_argv0=argv0):
                proc = subprocess.run(
                    [argv0, "mcp"],
                    executable=sys.executable,
                    cwd=self.base,
                    env=peer_env,
                    text=True,
                    capture_output=True,
                    timeout=15,
                )
                self.assertEqual(proc.returncode, 0, proc.stderr)
                reply = json.loads(proc.stdout)
                self.assertIn("error", reply)
                self.assertIn("MCP session cannot invoke daemon/decide", reply["error"]["message"])

        # Pending approval must still be open and unapproved after all MCP peer attempts.
        with contextlib.closing(sqlite3.connect(database)) as conn:
            self.assertEqual(conn.execute("SELECT COUNT(*) FROM approvals").fetchone()[0], 0)
            self.assertEqual(
                conn.execute("SELECT COUNT(*) FROM tool_invocations WHERE id='pending-parity-1' AND finished_at IS NULL").fetchone()[0],
                1,
            )

        # Non-MCP peer must still bind to the exact intended args_hash and nonce.
        for bad_label, overrides in (
            ("mutated_args_hash", {"args_hash": hash_b, "confirmed_hash": hash_b}),
            ("redacted_args_hash", {"args_hash": hash_redacted, "confirmed_hash": hash_redacted}),
            ("mismatched_confirmed_hash", {"confirmed_hash": hash_b}),
            ("wrong_nonce", {"confirmation_nonce": "wrong-nonce"}),
        ):
            with self.subTest(bad_case=bad_label):
                params = dict(valid_decide["params"], **overrides)
                self.assert_server_error(request("daemon/decide", params))

        # Exact intended args_hash + confirmed_hash + nonce succeeds once and rejects replay.
        ok = request("daemon/decide", valid_decide["params"])
        self.assertEqual(ok["result"], {"recorded": "gnome-parity-1", "source": "gnome"})
        self.assert_server_error(request("daemon/decide", valid_decide["params"]))

        with contextlib.closing(sqlite3.connect(database)) as conn:
            approvals = conn.execute("SELECT id, args_hash, decision, source FROM approvals").fetchall()
            self.assertEqual(approvals, [("gnome-parity-1", hash_a, "allow", "gnome")])
            consumed = conn.execute(
                "SELECT args_canonical, finished_at FROM tool_invocations WHERE id='pending-parity-1'"
            ).fetchone()
            self.assertEqual(consumed[0], "")
            self.assertIsNotNone(consumed[1])

        status_path = self.base / "data/02-agent/status.json"
        status_text = status_path.read_text()
        self.assertIn(hash_a, status_text)
        self.assertNotIn("deploy-prod", status_text)
        self.assertNotIn("ghp_", status_text)


if __name__ == "__main__":
    unittest.main()
