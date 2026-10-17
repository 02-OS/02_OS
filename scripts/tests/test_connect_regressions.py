"""Real connect CLI preserves TOML semantics as parsed independently by Python."""
import os
import json
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]
BINARY = Path(os.environ.get("AGENT_BINARY", ROOT / "components/02-agent/target/debug/02")).resolve()


class ConnectRegressionTests(unittest.TestCase):
    def test_xdg_clients_write_private_configs_and_launch_without_shell_path(self):
        for target in ("opencode", "zed"):
            with self.subTest(target=target), tempfile.TemporaryDirectory(prefix="02-connect-xdg-") as directory:
                base = Path(directory)
                home = base / "home"
                home.mkdir()
                config_home = base / "custom config"
                env = {**os.environ, "HOME": str(home), "XDG_CONFIG_HOME": str(config_home),
                       "XDG_DATA_HOME": str(base / "data"), "XDG_CACHE_HOME": str(base / "cache"),
                       "PATH": "/nonexistent"}
                subprocess.run(["git", "init", "-q", directory],
                               env={**env, "PATH": os.environ["PATH"]}, check=True, timeout=10)
                result = subprocess.run([str(BINARY), "connect", target, "--write"],
                                        cwd=directory, env=env, text=True, capture_output=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
                config = config_home / target / ("opencode.json" if target == "opencode" else "settings.json")
                document = json.loads(config.read_text())
                entry = document["mcp"]["02"] if target == "opencode" else document["context_servers"]["02"]
                command = entry["command"] if target == "opencode" else [entry["command"], *entry["args"]]
                self.assertEqual(command, [str(BINARY), "mcp"])
                self.assertEqual(config.stat().st_mode & 0o777, 0o600)
                self.assertFalse((home / ".config").exists())
                self.assertEqual(list(config.parent.iterdir()), [config])
                response = subprocess.run(command, input=json.dumps({"jsonrpc": "2.0", "id": 1, "method": "ping"}) + "\n",
                                          cwd=directory, env=env, text=True, capture_output=True, timeout=15)
                self.assertEqual(response.returncode, 0, response.stderr)
                self.assertEqual(json.loads(response.stdout), {"jsonrpc": "2.0", "id": 1, "result": {}})

    def test_codex_write_preserves_values_with_independent_toml_parser(self):
        with tempfile.TemporaryDirectory(prefix="02-connect-regression-") as directory:
            home = Path(directory) / "home"
            config = home / ".codex/config.toml"
            config.parent.mkdir(parents=True)
            original = r'''
model = "keep-me"
custom_instructions = """Follow \u0061ll checks. \U0001F642\b\f
Trim \
    this continuation."""
quoted = "quote:\" backslash:\\ tab:\t carriage:\r newline:\n"
integers = [0x10, 0o10, 0b10, -1_000]
date = 2026-10-08
inline = { "quoted.key" = { enabled = true, weights = [1.5, 2.25] } }

[[skills.config]]
path = "/tmp/fixture/one/SKILL.md"
enabled = false

[[skills.config]]
path = "/tmp/fixture/two/SKILL.md"
enabled = true

[projects."/tmp/fixture/a.b"]
trust_level = "trusted"

[mcp_servers.other]
command = "keep-other"
args = ["--keep"]

[mcp_servers.02]
command = "old-command"
args = ["old-argument"]
startup_timeout_sec = 30

[mcp_servers.02.env]
FIXTURE_VALUE = "keep-me"
'''
            # A multiline literal string has different escape rules from the
            # multiline basic string, so include both in the actual document.
            original = "literal = '''Keep literal \\u0061 and \"quotes\".'''\n" + original
            expected = tomllib.loads(original)
            self.assertTrue(expected["custom_instructions"].startswith("Follow all checks. 🙂"))
            config.write_text(original)
            os.chmod(config, 0o640)
            with config.open() as old_reader:
                result = subprocess.run([str(BINARY), "connect", "codex", "--write"],
                                        cwd=directory, env={**os.environ, "HOME": str(home)},
                                        text=True, capture_output=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("Successfully configured", result.stdout)
                actual = tomllib.loads(config.read_text())
                expected["mcp_servers"]["02"].update(command=str(BINARY), args=["mcp"])
                self.assertEqual(actual, expected)
                self.assertEqual(old_reader.read(), original)
                self.assertEqual(config.stat().st_mode & 0o777, 0o640)
                self.assertEqual(list(config.parent.iterdir()), [config])


if __name__ == "__main__":
    unittest.main()
