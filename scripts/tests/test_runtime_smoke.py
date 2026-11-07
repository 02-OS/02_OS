"""Exercise the shipped CLI/MCP process, using only disposable repositories/XDG data."""
import json
import os
from pathlib import Path
import sqlite3
import socket
import subprocess
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[2]
def _default_agent_binary():
    for candidate in (
        ROOT / "vendor/agent-runtime/target/debug/02",
        ROOT.parent / "agent-runtime/target/debug/02",
        ROOT / "components/02-agent/target/debug/02",
    ):
        if candidate.is_file():
            return candidate
    return ROOT.parent / "agent-runtime/target/debug/02"


BINARY = Path(os.environ.get("AGENT_BINARY", _default_agent_binary())).resolve()


class RuntimeSmokeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="02-runtime-test-")
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.repo = self.base / "repo"
        self.repo.mkdir()
        self.env = os.environ.copy()
        for name, directory in (("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"),
                                ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "run")):
            self.env[name] = str(self.base / directory)
            (self.base / directory).mkdir(mode=0o700)
        self.run_command("git", "init", "-q")
        self.run_command("git", "config", "user.name", "CI fixture")
        self.run_command("git", "config", "user.email", "ci@example.invalid")
        (self.repo / ".gitignore").write_text(".02agent/\n.env\n")
        (self.repo / "main.rs").write_text("fn smoke_entry() { smoke_helper(); }\nfn smoke_helper() {}\n")
        self.commit("initial fixture")

    def run_command(self, *args, check=True, input=None):
        result = subprocess.run(args, cwd=self.repo, env=self.env, input=input,
                                text=True, capture_output=True, timeout=30)
        if check:
            self.assertEqual(result.returncode, 0, f"{args}: {result.stderr}\n{result.stdout}")
        return result

    def cli(self, *args):
        return json.loads(self.run_command(str(BINARY), "--json", *args).stdout)

    def commit(self, message):
        self.run_command("git", "add", ".")
        self.run_command("git", "commit", "-qm", message)

    def initialize(self):
        self.cli("init")
        self.cli("index", "--full")

    def test_initialize_index_and_idempotent_reindex(self):
        self.assertFalse(self.cli("status")["initialized"])
        initialized = self.cli("init")
        self.assertEqual(initialized["status"], "success")
        first = self.cli("index", "--full")
        self.assertGreaterEqual(first["total_symbols"], 2)
        second = self.cli("index")
        self.assertEqual(second["total_symbols"], first["total_symbols"])
        self.assertEqual(second["files_updated"], 0)
        self.assertEqual(self.cli("symbol", "smoke_entry")["count"], 1)

    def test_modification_and_deletion_remove_stale_symbols(self):
        self.initialize()
        (self.repo / "main.rs").write_text("fn replacement_entry() {}\n")
        self.cli("index")
        self.assertEqual(self.cli("symbol", "smoke_entry")["count"], 0)
        self.assertEqual(self.cli("symbol", "replacement_entry")["count"], 1)
        (self.repo / "main.rs").unlink()
        self.commit("delete source")
        self.cli("index")
        self.assertEqual(self.cli("symbol", "replacement_entry")["count"], 0)
        database = next((self.base / "data").rglob("index.sqlite"))
        with sqlite3.connect(database) as conn:
            self.assertEqual(conn.execute("SELECT count(*) FROM files WHERE relative_path='main.rs'").fetchone()[0], 0)

    def test_private_storage_and_sensitive_filename_exclusion(self):
        (self.repo / ".env").write_text("SYNTHETIC_ONLY=private_fixture_marker\n")
        self.run_command("git", "add", "-f", ".env")
        (self.repo / ".gitignore").write_text(".02agent/\n")
        self.initialize()
        database = next((self.base / "data").rglob("index.sqlite"))
        self.assertEqual(database.stat().st_mode & 0o777, 0o600)
        self.assertEqual(database.parent.stat().st_mode & 0o777, 0o700)
        with sqlite3.connect(database) as conn:
            self.assertEqual(conn.execute("SELECT count(*) FROM files WHERE relative_path='.env'").fetchone()[0], 0)
            self.assertEqual(conn.execute("SELECT count(*) FROM files_fts WHERE content LIKE '%private_fixture_marker%'").fetchone()[0], 0)

    def test_mcp_stdio_handshake_errors_and_tool_execution(self):
        self.initialize()
        requests = [
            {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "CI", "version": "1"}}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
            {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "find_symbol", "arguments": {"name": "smoke_entry"}}},
            {"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "unknown_tool", "arguments": {}}},
            {"jsonrpc": "2.0", "id": 5, "method": "ping"},
        ]
        wire = "\n".join(map(json.dumps, requests)) + "\n{invalid JSON}\n"
        result = self.run_command(str(BINARY), "mcp", input=wire)
        responses = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(len(responses), 6)
        by_id = {response["id"]: response for response in responses}
        self.assertEqual(by_id[1]["result"]["protocolVersion"], "2024-11-05")
        names = {tool["name"] for tool in by_id[2]["result"]["tools"]}
        self.assertTrue({"find_symbol", "repository_context", "memory_write", "graph_export"} <= names)
        self.assertFalse(by_id[3]["result"]["isError"])
        self.assertIn("smoke_entry", by_id[3]["result"]["content"][0]["text"])
        self.assertTrue(by_id[4]["result"]["isError"])
        self.assertEqual(by_id[5]["result"], {})
        self.assertEqual(by_id[None]["error"]["code"], -32700)

    def test_named_dataset_and_graph_export(self):
        self.initialize()
        first = self.cli("add", "--source", "text", "--text", "fixture architecture note", "--dataset", "ci-notes")
        second = self.cli("add", "--source", "text", "--text", "fixture architecture note", "--dataset", "ci-notes")
        self.assertEqual(first["dataset_id"], second["dataset_id"])
        self.assertEqual(second["datapoints_inserted"], 0)
        self.assertGreaterEqual(second["datapoints_unchanged"], 1)
        self.cli("cognify", "--dataset", "ci-notes")
        graph = self.cli("graph", "export", "--format", "json", "--dataset", "ci-notes")
        self.assertGreater(len(graph["nodes"]), 0)
        labels = {node["id"]: node["label"] for node in graph["nodes"]}
        self.assertTrue(any(edge["kind"] == "calls" and labels.get(edge["src"]) == "smoke_entry"
                            and labels.get(edge["dst"]) == "smoke_helper" for edge in graph["edges"]))
        self.assertTrue(all(edge["src"] in labels and edge["dst"] in labels for edge in graph["edges"]))
        output = self.base / "graph.dot"
        self.cli("graph", "export", "--format", "dot", "--dataset", "ci-notes", "--output", str(output))
        self.assertIn("digraph", output.read_text())

    def test_daemon_socket_routes_mcp_to_watched_repository(self):
        self.initialize()
        daemon_binary = Path(os.environ.get("AGENT_DAEMON_BINARY", BINARY.with_name("02-agentd"))).resolve()
        socket_path = self.base / "run/daemon.sock"
        daemon = subprocess.Popen([str(daemon_binary), "--socket", str(socket_path), "--interval", "1"],
                                  cwd=self.base, env=self.env, stdout=subprocess.DEVNULL,
                                  stderr=subprocess.DEVNULL)
        def stop():
            if daemon.poll() is None:
                daemon.terminate()
            try:
                daemon.wait(timeout=5)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait(timeout=5)
        self.addCleanup(stop)
        deadline = time.monotonic() + 5
        while not socket_path.exists():
            self.assertIsNone(daemon.poll(), "Daemon exited before socket became ready")
            self.assertLess(time.monotonic(), deadline, "Daemon socket startup timed out")
            time.sleep(0.05)
        self.assertEqual(socket_path.stat().st_mode & 0o777, 0o600)
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(5)
            client.connect(str(socket_path))
            with client.makefile("r") as reader:
                def request(value):
                    client.sendall((json.dumps(value) + "\n").encode())
                    return json.loads(reader.readline())
                watched = request({"jsonrpc": "2.0", "id": 1, "method": "daemon/watch", "params": {"path": str(self.repo)}})
                self.assertTrue(watched["result"]["added"])
                response = request({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "find_symbol", "arguments": {"name": "smoke_entry"}}})
                self.assertFalse(response["result"]["isError"])
                self.assertIn("smoke_entry", response["result"]["content"][0]["text"])

    def test_failure_outside_repository_has_nonzero_exit(self):
        result = subprocess.run([str(BINARY), "status"], cwd=self.base, env=self.env,
                                capture_output=True, text=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Error:", result.stderr)


if __name__ == "__main__":
    unittest.main()
