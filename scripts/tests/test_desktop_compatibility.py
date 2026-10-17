"""Exercise the bundled desktop fixes without a running GNOME session."""

import ast
import configparser
import json
from pathlib import Path
import re
import shutil
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[2]
OVERLAY = ROOT / "profile/airootfs"
POP = OVERLAY / "usr/share/gnome-shell/extensions/pop-shell@system76.com"


def method_source(name):
    """Extract a class method, keeping its real body and nested blocks intact."""
    source = (POP / "extension.js").read_text()
    start = source.index(f"    {name}() {{")
    end = source.index("\n    }", start) + len("\n    }")
    return source[start:end]


class PopShellCompatibilityTests(unittest.TestCase):
    def run_node(self, script):
        self.assertIsNotNone(shutil.which("node"), "Node.js is required for desktop compatibility tests")
        result = subprocess.run(
            ["node", "--input-type=module"], input=script, text=True,
            capture_output=True, cwd=ROOT, timeout=15,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_full_maximization_matches_supported_mutter_signatures(self):
        # The actual adapter runs with strict API stubs. GNOME 45-48 require
        # directions; 49+ take none, even when MaximizeFlags is exposed again.
        source = (POP / "maximization.js").read_text()
        versions = json.loads((POP / "metadata.json").read_text())["shell-version"]
        self.run_node(rf"""
            import assert from 'node:assert/strict';
            const original = {json.dumps(source)};
            const versions = {json.dumps(versions)};
            const expectedArgs = {{45: [3], 46: [3], 47: [3], 48: [3], 49: [], 50: [], 51: []}};
            for (const version of versions) {{
                assert.ok(Object.hasOwn(expectedArgs, version), `Add API fixture for ${{version}}`);
                // 49 must work without the enum, while 50/51 must not mistake
                // its presence for the old method signature.
                const meta = version === '49' ? {{}} : {{MaximizeFlags: {{BOTH: 3}}}};
                let source = original.replace(/^import Meta from 'gi:\/\/Meta';$/m,
                    `const Meta = ${{JSON.stringify(meta)}};`);
                source = source.replace(/^import \{{ PACKAGE_VERSION \}} from .*;$/m,
                    `const PACKAGE_VERSION = '${{version}}.1';`);
                const compat = await import('data:text/javascript;base64,' + Buffer.from(source).toString('base64'));
                for (const initial of [[false, false], [true, false], [false, true], [true, true]]) {{
                    const state = {{horizontal: initial[0], vertical: initial[1]}};
                    const calls = [];
                    const window = {{
                        maximize(...args) {{
                            assert.deepEqual(args, expectedArgs[version], `maximize on ${{version}}`);
                            assert.equal(this, window);
                            calls.push('maximize');
                            state.horizontal = state.vertical = true;
                        }},
                        unmaximize(...args) {{
                            assert.deepEqual(args, expectedArgs[version], `unmaximize on ${{version}}`);
                            assert.equal(this, window);
                            calls.push('unmaximize');
                            state.horizontal = state.vertical = false;
                        }},
                    }};
                    compat.unmaximize(window);
                    assert.deepEqual(state, {{horizontal: false, vertical: false}});
                    compat.maximize(window);
                    assert.deepEqual(state, {{horizontal: true, vertical: true}});
                    compat.unmaximize(window);
                    assert.deepEqual(state, {{horizontal: false, vertical: false}});
                    assert.deepEqual(calls, ['unmaximize', 'maximize', 'unmaximize']);
                }}
            }}
        """)

    def test_title_change_keeps_decoration_updates_without_wayland_menu(self):
        method = method_source("on_show_window_titles")
        self.run_node(f"""
            import assert from 'node:assert/strict';
            import vm from 'node:vm';
            for (const showTitle of [true, false]) {{
                for (const menu of ['wayland', 'x11', 'not-created']) {{
                    const events = [];
                    const indicator = menu === 'not-created' ? null : {{}};
                    if (menu === 'x11')
                        indicator.toggle_titles = {{setToggleState(value) {{events.push(['menu', value]);}}}};
                    const callback = vm.runInNewContext('({{' + {json.dumps(method)} + '}})', {{indicator}})
                        .on_show_window_titles;
                    const window = (name, clientDecorated) => ({{
                        is_client_decorated() {{return clientDecorated;}},
                        decoration_show(ext) {{assert.equal(ext, context); events.push([name, 'show']);}},
                        decoration_hide(ext) {{assert.equal(ext, context); events.push([name, 'hide']);}},
                    }});
                    const context = {{
                        settings: {{show_title() {{return showTitle;}}}},
                        windows: new Map([
                            [1, window('server-decorated', false)],
                            [2, window('client-decorated', true)],
                        ]),
                    }};
                    callback.call(context);
                    const expected = [['server-decorated', showTitle ? 'show' : 'hide']];
                    if (menu === 'x11') expected.unshift(['menu', showTitle]);
                    assert.deepEqual(events, expected, `${{menu}} / show-title=${{showTitle}}`);
                }}
            }}
        """)

    def test_all_pop_maximization_calls_use_the_version_adapter(self):
        # Catch any caller bypassing the adapter, including the former repeated
        # HORIZONTAL/VERTICAL/BOTH unmaximize sequence during monitor moves.
        for name in ("extension.js", "window.js", "tiling.js"):
            with self.subTest(module=name):
                source = (POP / name).read_text()
                self.assertNotRegex(source, r"MaximizeFlags|\bmeta\.(?:un)?maximize\(")
                self.assertIn("from './maximization.js'", source)

    def test_launcher_activates_existing_window_and_launches_stopped_app(self):
        source = (POP / "launcher.js").read_text()
        start = source.index("    launch_desktop_entry(entry) {")
        end = source.index("\n    }", start) + len("\n    }")
        method = source[start:end]
        self.run_node(f"""
            import assert from 'node:assert/strict';
            import vm from 'node:vm';
            for (const running of [false, true]) {{
                const calls = [];
                const app = {{get_windows: () => running ? [{{}}] : [],
                    activate: () => calls.push('activate'), get_app_info: () => ({{get_executable: () => 'fixture'}}),
                    launch: (...args) => calls.push(['launch', ...args])}};
                const callback = vm.runInNewContext('({{' + {json.dumps(method)} + '}})', {{
                    app_sys: {{lookup_desktop_wmclass: () => app}},
                    Shell: {{AppLaunchGpu: {{DEFAULT: 0, DISCRETE: 1}}}}, log: {{debug() {{}}}}
                }}).launch_desktop_entry;
                callback.call({{}}, {{path: '/usr/share/applications/example.desktop', gpu_preference: 'Default'}});
                assert.equal(calls.length, 1);
                assert.equal(JSON.stringify(calls), JSON.stringify(running ? ['activate'] : [['launch', 0, -1, 0]]));
            }}
        """)

    def test_flourish_disposal_disconnects_before_reentrant_label_hide(self):
        path = OVERLAY / "usr/share/gnome-shell/extensions/flourish@orsso.github.io/lib/runtime/iconDecorations.js"
        source = re.sub(r"^import .*?;\n", "", path.read_text(), flags=re.M)
        self.run_node(f"""
            import assert from 'node:assert/strict';
            const source = 'const St = {{Bin: class {{}}, Label: class {{}}}};' + {json.dumps(source)};
            const {{IconDecorations}} = await import('data:text/javascript;base64,' + Buffer.from(source).toString('base64'));
            let disposed = false;
            const actor = new Proxy({{connectObject() {{}}, disconnectObject() {{}},
                get_children: () => []}}, {{get(target, key) {{
                    assert.equal(disposed, false, 'actor accessed after label hide');
                    return target[key];
                }}}});
            let decorations;
            const label = {{connectObject() {{}}, disconnectObject() {{}}, hide() {{
                disposed = true;
                decorations.dispose();
                decorations.update();
            }}}};
            decorations = new IconDecorations({{container: {{label}}, bin: actor,
                icon: {{icon: {{get_parent: () => actor}}}}, onChanged() {{}}}});
            decorations.dispose();
            decorations.dispose();
        """)

    def test_reduced_motion_suppresses_flourish(self):
        path = OVERLAY / "usr/share/gnome-shell/extensions/flourish@orsso.github.io/lib/runtime/motionPreference.js"
        source = path.read_text().replace("import St from 'gi://St';", "const St = globalThis.fixtureSt;")
        self.run_node(f"""
            import assert from 'node:assert/strict';
            const settings = {{enable_animations: true, reduced_motion: 0}};
            globalThis.fixtureSt = {{Settings: {{get: () => settings}}, ReducedMotion: {{REDUCE: 1}}}};
            const {{motionEnabled}} = await import('data:text/javascript;base64,' + Buffer.from({json.dumps(source)}).toString('base64'));
            assert.equal(motionEnabled(), true);
            settings.reduced_motion = 1;
            assert.equal(motionEnabled(), false);
            settings.reduced_motion = 0;
            settings.enable_animations = false;
            assert.equal(motionEnabled(), false);
        """)


class WorkspaceShortcutTests(unittest.TestCase):
    def test_number_shortcuts_resolve_only_to_the_matching_workspace(self):
        defaults = configparser.ConfigParser(interpolation=None)
        defaults.read(OVERLAY / "etc/dconf/db/local.d/00-02os")
        wm = "org/gnome/desktop/wm/keybindings"
        shell = "org/gnome/shell/keybindings"
        # GNOME's normal application shortcuts are part of the effective state
        # unless explicitly overridden, so absence is not equivalent to [].
        bindings = {(shell, f"switch-to-application-{n}"): [f"<Super>{n}"] for n in range(1, 5)}
        for section in (wm, shell):
            for key, value in defaults[section].items():
                bindings[(section, key)] = ast.literal_eval(re.sub(r"^@as\s+", "", value))
        for number in range(1, 5):
            accelerator = f"<Super>{number}"
            owners = [key for key, accelerators in bindings.items() if accelerator in accelerators]
            self.assertEqual(owners, [(wm, f"switch-to-workspace-{number}")])
            self.assertEqual(bindings[(shell, f"switch-to-application-{number}")], [])
        self.assertEqual(defaults["org/gnome/desktop/wm/preferences"]["num-workspaces"], "4")
        self.assertEqual(defaults["org/gnome/mutter"]["dynamic-workspaces"], "false")
        self.assertEqual(defaults["org/gnome/shell/extensions/dash-to-dock"]["hot-keys"], "false")


if __name__ == "__main__":
    unittest.main()
