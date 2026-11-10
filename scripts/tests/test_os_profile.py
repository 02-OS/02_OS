"""Validate the ISO overlay without running root customization or installing an OS."""
import ast
import configparser
import fnmatch
import json
import os
from pathlib import Path
import re
import select
import shutil
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[2]
OVERLAY = ROOT / "profile/airootfs"
EXTENSIONS = {
    "dash-to-dock@micxgx.gmail.com": "org.gnome.shell.extensions.dash-to-dock",
    "pop-shell@system76.com": "org.gnome.shell.extensions.pop-shell",
    "flourish@orsso.github.io": "org.gnome.shell.extensions.flourish",
    "compiz-windows-effect@hermes83.github.com": "org.gnome.shell.extensions.com.github.hermes83.compiz-windows-effect",
}


class OSProfileTests(unittest.TestCase):
    def checked(self, *command):
        result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, f"{command}: {result.stderr}{result.stdout}")

    def test_package_manifest_has_unique_entries(self):
        packages = [line.strip() for line in (ROOT / "profile/packages.x86_64").read_text().splitlines()
                    if line.strip() and not line.lstrip().startswith("#")]
        self.assertEqual(len(packages), len(set(packages)), "Duplicate packages in ISO manifest")
        self.assertTrue({"linux", "gnome-shell", "gnome-session", "networkmanager", "archinstall"} <= set(packages))

    def test_bash_scripts_parse_without_execution(self):
        scripts = [ROOT / "build.sh", ROOT / "profile/profiledef.sh", *sorted((ROOT / "scripts").glob("*.sh")),
                   OVERLAY / "root/customize_airootfs.sh", *sorted((OVERLAY / "usr/local/bin").iterdir())]
        checked = 0
        required = [ROOT / "build.sh", ROOT / "profile/profiledef.sh", ROOT / "scripts/build-agent-runtime.sh",
                    ROOT / "scripts/release.sh", OVERLAY / "root/customize_airootfs.sh",
                    OVERLAY / "usr/local/bin/02os-gnome-session", OVERLAY / "usr/local/bin/02os-ensure-live-user"]
        for path in required:
            with self.subTest(required=str(path.relative_to(ROOT))):
                self.assertTrue(path.is_file())
                self.checked("bash", "-n", str(path))
        for path in scripts:
            if path.is_file() and path.read_bytes().split(b"\n", 1)[0] in (b"#!/bin/bash", b"#!/usr/bin/bash", b"#!/usr/bin/env bash"):
                with self.subTest(path=str(path.relative_to(ROOT))):
                    self.checked("bash", "-n", str(path))
                    checked += 1
        self.assertGreater(checked, 5)

    def test_python_generators_parse_without_execution(self):
        scripts = sorted((ROOT / "scripts/icon-generator").glob("*.py"))
        self.assertGreater(len(scripts), 0)
        for path in scripts:
            with self.subTest(path=path.name):
                ast.parse(path.read_text(), filename=str(path))

    def test_svg_and_schema_xml_is_well_formed(self):
        assets = sorted(OVERLAY.rglob("*.svg")) + sorted(OVERLAY.rglob("*.gschema.xml"))
        self.assertGreater(len(assets), 0)
        for path in assets:
            with self.subTest(path=str(path.relative_to(OVERLAY))):
                ET.parse(path)

    def test_desktop_entries_and_glib_schemas(self):
        for command in ("desktop-file-validate", "glib-compile-schemas"):
            self.assertIsNotNone(shutil.which(command), f"Install required CI dependency: {command}")
        for path in sorted((OVERLAY / "usr/share/applications").glob("*.desktop")):
            with self.subTest(path=path.name):
                self.checked("desktop-file-validate", str(path))
        self.checked("glib-compile-schemas", "--strict", "--dry-run", str(OVERLAY / "usr/share/glib-2.0/schemas"))
        for path in sorted((OVERLAY / "usr/share/gnome-shell/extensions").glob("*/schemas")):
            with self.subTest(schemas=path.parent.name):
                self.checked("glib-compile-schemas", "--strict", "--dry-run", str(path))

    def test_motion_packages_and_defaults(self):
        manifest = json.loads((ROOT / "docs/desktop-motion-vendor.json").read_text())
        for package in manifest["packages"]:
            ext = OVERLAY / "usr/share/gnome-shell/extensions" / package["uuid"]
            metadata = json.loads((ext / "metadata.json").read_text())
            with self.subTest(extension=package["uuid"]):
                self.assertEqual(metadata["uuid"], package["uuid"])
                self.assertEqual(metadata.get("version-name", metadata.get("version")), package["version"])
                self.assertIn(manifest["target_shell"], metadata["shell-version"])
                self.assertTrue((ext / package["license_file"]).is_file())
                self.assertRegex(package["sha256"], r"^[0-9a-f]{64}$")
                for schema in (ext / "schemas").glob("*.gschema.xml"):
                    self.assertEqual(schema.read_bytes(), (OVERLAY / "usr/share/glib-2.0/schemas" / schema.name).read_bytes())

        defaults = configparser.ConfigParser(interpolation=None)
        defaults.read(OVERLAY / "etc/dconf/db/local.d/00-02os")
        for uuid in EXTENSIONS:
            self.assertIn(uuid, defaults["org/gnome/shell"]["enabled-extensions"])
        self.assertEqual(defaults["org/gnome/shell/extensions/flourish"]["motion-profile"], "'custom'")
        self.assertEqual(defaults["org/gnome/shell/extensions/com/github/hermes83/compiz-windows-effect"]["maximize-effect"], "false")
        self.assertEqual(defaults["org/gnome/shell/extensions/com/github/hermes83/compiz-windows-effect"]["resize-effect"], "false")
        # Validate types, ranges and enum values with GLib, using an ephemeral
        # backend so no desktop settings are changed on the test machine.
        with tempfile.TemporaryDirectory(prefix="02os-schemas-") as tmp:
            schemas = Path(tmp)
            for path in (OVERLAY / "usr/share/glib-2.0/schemas").glob("*.gschema.xml"):
                shutil.copy(path, schemas)
            self.checked("glib-compile-schemas", "--strict", str(schemas))
            env = dict(os.environ, GSETTINGS_BACKEND="memory", GSETTINGS_SCHEMA_DIR=str(schemas))
            for uuid, schema_id in EXTENSIONS.items():
                section = schema_id.replace(".", "/")
                for key, value in defaults[section].items():
                    with self.subTest(extension=uuid, key=key):
                        result = subprocess.run(["gsettings", "set", schema_id, key, value],
                                                env=env, capture_output=True, text=True, timeout=10)
                        self.assertEqual(result.returncode, 0, result.stderr)

    def test_graphical_pam_configuration_is_idempotent(self):
        helper = OVERLAY / "usr/local/bin/02os-configure-greetd"
        with tempfile.TemporaryDirectory(prefix="02os-pam-") as tmp:
            pam = Path(tmp)
            login = "auth required pam_unix.so\nsession required pam_limits.so\n-session optional pam_systemd.so type=tty\n"
            greetd = "auth include system-local-login\naccount include system-local-login\nsession include system-local-login\n"
            (pam / "system-login").write_text(login)
            (pam / "greetd").write_text(greetd)
            self.checked(str(helper), str(pam))
            configured = (pam / "greetd").read_text()
            graphical = (pam / "02os-graphical-login").read_text()
            self.assertEqual((pam / "system-login").read_text(), login)
            self.assertIn("session    include      02os-graphical-login", configured)
            self.assertIn("auth include system-local-login", configured)
            self.assertIn("account include system-local-login", configured)
            self.assertIn("pam_systemd.so type=wayland", graphical)
            self.assertNotIn("type=tty", graphical)
            self.checked(str(helper), str(pam))
            self.assertEqual((pam / "greetd").read_text(), configured)
            self.assertEqual((pam / "02os-graphical-login").read_text(), graphical)
            # An unfamiliar PAM stack must fail before changing either file.
            (pam / "greetd").write_text("session required pam_unix.so\n")
            result = subprocess.run([str(helper), str(pam)], capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual((pam / "greetd").read_text(), "session required pam_unix.so\n")
            self.assertEqual((pam / "02os-graphical-login").read_text(), graphical)

    def test_live_optical_media_skips_gpt_root_loop_discovery(self):
        rule_path = OVERLAY / "etc/udev/rules.d/98-02os-live-media.rules"
        self.assertTrue(rule_path.is_file())
        self.assertLess(rule_path.name, "99-systemd.rules")
        self.checked("udevadm", "verify", str(rule_path))
        lines = [line for line in rule_path.read_text().splitlines()
                 if line.strip() and not line.lstrip().startswith("#")]
        self.assertEqual(len(lines), 1)
        fields = []
        for field in lines[0].split(","):
            match = re.fullmatch(r'\s*(SUBSYSTEM|ENV\{[^}]+\})(==|=)"([^"]*)"\s*', field)
            self.assertIsNotNone(match, field)
            fields.append(match.groups())
        predicates = [(key, value) for key, operator, value in fields if operator == "=="]
        assignments = [(key, value) for key, operator, value in fields if operator == "="]
        self.assertEqual(assignments, [("ENV{ID_PART_GPT_AUTO_ROOT_DISK_NEEDS_LOOP}", "0")])
        # Evaluate the real profile label rather than assuming its case or prefix.
        profile = subprocess.run(
            ["bash", "-c", 'declare -A file_permissions; source profile/profiledef.sh; printf "%s" "$iso_label"'],
            cwd=ROOT, env=dict(os.environ, TZ="UTC", SOURCE_DATE_EPOCH="1791282600"),
            capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(profile.returncode, 0, profile.stderr)
        self.assertEqual(profile.stdout, "02_OS_202610")
        optical = {"SUBSYSTEM": "block", "ENV{ID_CDROM}": "1",
                   "ENV{ID_FS_TYPE}": "iso9660", "ENV{ID_FS_LABEL}": profile.stdout}
        cases = [({}, True), ({"SUBSYSTEM": "net"}, False),
                 ({"ENV{ID_CDROM}": "0"}, False), ({"ENV{ID_CDROM}": ""}, False),
                 ({"ENV{ID_FS_TYPE}": "ext4"}, False),
                 ({"ENV{ID_FS_LABEL}": "ARCH_202610"}, False),
                 ({"ENV{ID_FS_LABEL}": "02_os_202610"}, False)]
        for changed, expected in cases:
            with self.subTest(properties=changed):
                properties = dict(optical, **changed)
                matches = all(fnmatch.fnmatchcase(properties.get(key, ""), value)
                              for key, value in predicates)
                self.assertEqual(matches, expected)

    def test_profile_permissions_and_runtime_alias(self):
        text = (ROOT / "profile/profiledef.sh").read_text()
        permissions = dict(re.findall(r'\["([^"]+)"\]="([^"]+)"', text))
        optional_runtime = {"/usr/bin/02", "/usr/bin/02agent", "/usr/bin/02-agentd"}
        for relative, encoded in permissions.items():
            with self.subTest(path=relative):
                path = OVERLAY / relative.lstrip("/")
                if relative not in optional_runtime:
                    self.assertTrue(path.exists() or path.is_symlink(), f"Permission mapping has no overlay path: {relative}")
                uid, gid, mode = encoded.split(":")
                self.assertEqual((uid, gid), ("0", "0"))
                self.assertEqual(int(mode, 8) & 0o022, 0, f"Group/other writable privileged overlay file: {relative}")
        self.assertEqual(permissions["/etc/sudoers.d/01-live"], "0:0:440")
        for binary in optional_runtime:
            self.assertEqual(permissions[binary], "0:0:755")
        alias = OVERLAY / "usr/bin/02agent"
        if alias.exists() or alias.is_symlink():
            self.assertTrue(alias.is_symlink(), "usr/bin/02agent must be a symlink to 02 when it is staged")
            self.assertEqual(alias.readlink(), Path("02"))
        for script in ("02os-gnome-session", "02os-ensure-live-user", "pop-shell-shortcuts",
                       "02os-install", "02os-provision", "pop-launcher", "02os-check-runtime", "02os-configure-greetd"):
            self.assertEqual(permissions[f"/usr/local/bin/{script}"], "0:0:755")

    def test_install_desktop_launches_provisioner_entry(self):
        desktop_path = OVERLAY / "usr/share/applications/02os-install.desktop"
        desktop = desktop_path.read_text()
        self.assertIn("Name=Install 02_OS", desktop)
        exec_lines = [line for line in desktop.splitlines() if line.startswith("Exec=")]
        self.assertTrue(exec_lines)
        for line in exec_lines:
            self.assertIn("02os-install", line)
            self.assertNotIn("sudo archinstall", line)
        plugin = (OVERLAY / "usr/local/share/02os/archinstall_plugin.py").read_text()
        self.assertIn("__archinstall__version__ = 4.5", plugin)
        self.assertIn("def on_install", plugin)
        self.assertIn("def on_genfstab", plugin)
        installer = (OVERLAY / "usr/local/bin/02os-install").read_text()
        self.assertIn("GNOME desktop profile", installer)
        self.assertIn("exec archinstall --plugin /usr/local/share/02os/archinstall_plugin.py --skip-version-check", installer)

    def test_launcher_does_not_call_plain_archinstall(self):
        desktop = (OVERLAY / "usr/share/applications/02os-install.desktop").read_text()
        self.assertNotIn("archinstall", desktop)
        installer = (OVERLAY / "usr/local/bin/02os-install").read_text()
        invocations = []
        for line in installer.splitlines():
            stripped = line.strip()
            if stripped.startswith("#") or "archinstall" not in stripped:
                continue
            invocations.append(stripped)
            self.assertIn("--plugin", stripped, stripped)
            self.assertIn("/usr/local/share/02os/archinstall_plugin.py", stripped, stripped)
        self.assertGreaterEqual(len(invocations), 1)

    def test_plugin_runs_on_archinstall_4_5(self):
        # packages.x86_64 installs Extra archinstall 4.5-1. Its load_plugin compares
        # __archinstall__version__ with float(version.rsplit(".", 1)[0]) and still
        # instantiates Plugin(). Installer.minimal_installation calls on_install,
        # and genfstab calls on_genfstab, both with the installer object.
        plugin_path = OVERLAY / "usr/local/share/02os/archinstall_plugin.py"
        namespace = {}
        exec(compile(plugin_path.read_text(), str(plugin_path), "exec"), namespace)
        namespace["INSTALLED_PACKAGES"] = OVERLAY / "usr/local/share/02os/installed-packages.txt"
        packaged_version = "4.5"
        gate = float(packaged_version.rsplit(".", 1)[0])
        self.assertGreaterEqual(namespace["__archinstall__version__"], gate)
        plugin = namespace["Plugin"]()
        calls = []
        queries = []

        def check_call(argv, **_kwargs):
            calls.append(list(argv))

        def run(argv, **_kwargs):
            queries.append(list(argv))
            return subprocess.CompletedProcess(argv, 0)

        import subprocess
        original = subprocess.check_call
        original_run = subprocess.run
        subprocess.check_call = check_call
        subprocess.run = run
        plugin._cleanup_browser_payload = lambda _installation: None
        try:
            class Installation:
                def __init__(self):
                    self.target = "/mnt/02os-target"
                    self._hooks = ["base", "udev", "kms", "keyboard", "filesystems"]
                    self._kernel_params = ["root=UUID=installed-root"]
                    self.packages = []
                    self.chroot_calls = []
                    self.initramfs_calls = []

                def add_additional_packages(self, packages):
                    self.packages.extend(packages)

                def arch_chroot(self, command):
                    self.chroot_calls.append(command)

                def mkinitcpio(self, flags):
                    self.initramfs_calls.append(flags)
                    return True

            installation = Installation()
            plugin.on_install(installation)
            plugin.on_genfstab(installation)
        finally:
            subprocess.check_call = original
            subprocess.run = original_run
        self.assertEqual(
            calls,
            [
                ["/usr/local/bin/02os-provision", "/mnt/02os-target"],
                ["/usr/local/bin/02os-provision", "/mnt/02os-target"],
            ],
        )
        desktop_packages = namespace["_read_installed_packages"](namespace["INSTALLED_PACKAGES"])
        self.assertEqual(installation.packages, ["plymouth"] + desktop_packages)
        self.assertEqual(installation._kernel_params, ["root=UUID=installed-root", "quiet", "splash"])
        self.assertEqual(queries, [["arch-chroot", "-S", "/mnt/02os-target",
                                    "/usr/local/bin/02os-install-browsers", "--verify-only"]])
        self.assertEqual(installation.chroot_calls, ["plymouth-set-default-theme 02-zero-portal",
                                                     "/usr/local/bin/02os-check-desktop",
                                                     "plymouth-set-default-theme 02-zero-portal"])
        self.assertEqual(installation.initramfs_calls, [["-P"], ["-P"]])

    def test_product_name_is_02os(self):
        identity = (OVERLAY / "etc/os-release").read_text()
        self.assertIn('NAME="02_OS"\n', identity)
        self.assertIn('PRETTY_NAME="02_OS"\n', identity)
        self.assertNotIn("Arch Linux", identity)
        self.assertNotIn("Arch Linux", (OVERLAY / "etc/motd").read_text())
        readme_lead = (ROOT / "README.md").read_text().splitlines()[2]
        self.assertIn("operating system", readme_lead)
        self.assertNotIn("Arch Linux", readme_lead)
        self.assertNotIn("Arch Linux", (ROOT / "scripts/icon-generator/generate_html_gallery.py").read_text())

    def test_customize_installs_identity_on_both_os_release_paths(self):
        script = (OVERLAY / "root/customize_airootfs.sh").read_text().splitlines()
        start = script.index("install_02os_identity() {")
        function = []
        for line in script[start:]:
            function.append(line)
            if line == "}":
                break
        self.assertEqual(function[-1], "}")
        identity = (OVERLAY / "etc/os-release").read_text()

        def run(root):
            command = "set -euo pipefail\n" + "\n".join(function) + f"\ninstall_02os_identity {root}\n"
            return subprocess.run(["bash", "-c", command], capture_output=True, text=True, timeout=30)

        with tempfile.TemporaryDirectory(prefix="02os-id-") as tmp:
            root = Path(tmp) / "replaced"
            (root / "etc").mkdir(parents=True)
            (root / "usr/lib").mkdir(parents=True)
            (root / "etc/os-release").write_text(identity)
            (root / "usr/lib/os-release").write_text('NAME="Arch Linux"\nID=arch\n')
            result = run(root)
            self.assertEqual(result.returncode, 0, result.stderr)
            for rel in ("etc/os-release", "usr/lib/os-release"):
                path = root / rel
                self.assertFalse(path.is_symlink(), rel)
                self.assertEqual(path.read_text(), identity, rel)

            linked = Path(tmp) / "followed"
            (linked / "usr/lib").mkdir(parents=True)
            (linked / "etc").mkdir()
            (linked / "usr/lib/os-release").write_text(identity)
            (linked / "etc/os-release").symlink_to("../usr/lib/os-release")
            result = run(linked)
            self.assertEqual(result.returncode, 0, result.stderr)
            for rel in ("etc/os-release", "usr/lib/os-release"):
                path = linked / rel
                self.assertFalse(path.is_symlink(), rel)
                self.assertEqual(path.read_text(), identity, rel)

            untouched = Path(tmp) / "vendor"
            (untouched / "usr/lib").mkdir(parents=True)
            (untouched / "etc").mkdir()
            vendor = untouched / "usr/lib/os-release"
            vendor.write_text('NAME="Arch Linux"\nID=arch\n')
            (untouched / "etc/os-release").symlink_to("../usr/lib/os-release")
            result = run(untouched)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(vendor.read_text(), 'NAME="Arch Linux"\nID=arch\n')
            self.assertTrue((untouched / "etc/os-release").is_symlink())

    def test_plugin_retitle_preserves_boot_paths_ids_and_foreign_generator(self):
        plugin_path = OVERLAY / "usr/local/share/02os/archinstall_plugin.py"
        namespace = {}
        exec(compile(plugin_path.read_text(), str(plugin_path), "exec"), namespace)
        foreign = (
            "### BEGIN /etc/grub.d/30_os-prober ###\n"
            "menuentry 'Arch Linux (neighbor)' {\n"
            " linux /vmlinuz-linux root=UUID=foreign\n}\n"
            "### END /etc/grub.d/30_os-prober ###\n"
        )
        own = (
            "### BEGIN /etc/grub.d/10_linux ###\n"
            "menuentry 'Arch Linux' --id 'Arch Linux menu ID' {\n"
            " linux /vmlinuz-linux root=UUID=current quiet splash\n}\n"
            "### END /etc/grub.d/10_linux ###\n"
        )
        actual = namespace["retitle_grub_text"](own + foreign)
        self.assertIn("menuentry '02_OS' --id 'Arch Linux menu ID'", actual)
        self.assertIn("linux /vmlinuz-linux root=UUID=current quiet splash\n", actual)
        self.assertTrue(actual.endswith(foreign))

    def test_agentd_user_unit_is_not_ignored(self):
        unit = OVERLAY / "usr/lib/systemd/user/02-agentd.service"
        text = unit.read_text()
        self.assertIn("ProtectHome=no", text)
        self.assertNotIn("ProtectHome=read-write", text)
        self.assertIn("ProtectSystem=strict", text)
        self.assertIn("ExecStartPre=+/usr/lib/02-agent/prepare-storage.sh", text)
        self.assertIn(
            "ReadWritePaths=%h/.local/share/02-agent %h/.cache/02-agent %h/.config/02-agent %t %h",
            text,
        )
        self.assertNotIn("ReadWritePaths=-", text)
        binary = "/usr/bin/systemd-analyze"
        self.assertTrue(Path(binary).is_file(), binary)
        result = subprocess.run([binary, "--user", "verify", str(unit)], capture_output=True, text=True, timeout=30)
        combined = result.stdout + result.stderr
        self.assertNotIn("ignoring", combined.lower(), combined)

    def test_lock_shortcut_does_not_overlap_pop_shell_focus_bindings(self):
        defaults = configparser.ConfigParser(interpolation=None)
        defaults.read(OVERLAY / "etc/dconf/db/local.d/00-02os")
        lock = set(ast.literal_eval(defaults["org/gnome/settings-daemon/plugins/media-keys"]["screensaver"]))
        focus_right = set(ast.literal_eval(defaults["org/gnome/shell/extensions/pop-shell"]["focus-right"]))
        self.assertIn("<Super>l", lock)
        self.assertIn("<Super>Right", focus_right)
        self.assertFalse(lock & focus_right, "Window focus must not intercept screen locking")
        floating = set(ast.literal_eval(defaults["org/gnome/shell/extensions/pop-shell"]["toggle-floating"]))
        settings = defaults["org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom2"]
        self.assertEqual(floating, {"<Super>g"})
        self.assertEqual(settings["command"], "'gnome-control-center'")
        self.assertEqual(settings["binding"], "'<Super>i'")
        for binding in (floating, {settings["binding"].strip("'")}):
            self.assertFalse(binding & lock, "Developer keys must not take the screen lock")

    def test_provisioner_copies_allowlist_and_skips_live_paths(self):
        provision = OVERLAY / "usr/local/bin/02os-provision"
        with tempfile.TemporaryDirectory(prefix="02os-prov-") as tmp:
            source = Path(tmp) / "source"
            target = Path(tmp) / "target"
            self._write_provision_fixture(source, include_runtime=True)
            forbidden = self._write_forbidden_live_paths(source)
            vendor_identity = target / "usr/lib/os-release"
            vendor_identity.parent.mkdir(parents=True)
            vendor_identity.write_text('NAME="Arch Linux"\nID=arch\n')
            (target / "etc").mkdir()
            (target / "etc/os-release").symlink_to("../usr/lib/os-release")
            result = subprocess.run(
                [str(provision), "--source", str(source), "--target", str(target)],
                capture_output=True, text=True, timeout=30,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((target / "usr/bin/02").read_text(), "runtime")
            self.assertEqual((target / "usr/bin/02-agentd").read_text(), "daemon")
            self.assertTrue((target / "usr/bin/02agent").is_symlink())
            self.assertEqual((target / "usr/bin/02agent").readlink(), Path("02"))
            self.assertEqual((target / "usr/lib/systemd/user/02-agentd.service").read_text(), "[Service]\n")
            self.assertTrue((target / "etc/systemd/user/default.target.wants/02-agentd.service").is_symlink())
            for rel in ("usr/lib/02-agent/prepare-storage.sh", "usr/lib/systemd/user-generators/02-agent-storage"):
                self.assertEqual((target / rel).read_bytes(), (source / rel).read_bytes())
            self.assertEqual((target / "usr/local/bin/pop-launcher").read_text(), "launcher")
            for uuid in EXTENSIONS:
                ext = target / "usr/share/gnome-shell/extensions" / uuid
                self.assertEqual((ext / "extension.js").read_text(), "ext")
                self.assertTrue((ext / "schemas/gschemas.compiled").is_file())
            self.assertEqual((target / "usr/share/gnome-shell/extensions/02-zero-portal@02os/extension.js").read_text(), "portal")
            self.assertEqual((target / "usr/lib/tmpfiles.d/02os-zero-portal.conf").read_bytes(), (OVERLAY / "usr/lib/tmpfiles.d/02os-zero-portal.conf").read_bytes())
            self.assertFalse((target / "usr/share/applications/02os-install.desktop").exists())
            self.assertFalse((target / "usr/local/bin/02os-install").exists())
            self.assertEqual((target / "usr/lib/02-agent/SOURCE_REVISION").read_text(), "rev\n")
            installed_defaults = configparser.ConfigParser(interpolation=None)
            installed_defaults.read(target / "etc/dconf/db/local.d/00-02os")
            favorites = ast.literal_eval(installed_defaults["org/gnome/shell"]["favorite-apps"])
            self.assertNotIn("02os-install.desktop", favorites)
            self.assertIn("org.gnome.Nautilus.desktop", favorites)
            self.assertIn("org.gnome.Console.desktop", favorites)
            self.assertIn("google-chrome.desktop", favorites)
            source_defaults = configparser.ConfigParser(interpolation=None)
            source_defaults.read(source / "etc/dconf/db/local.d/00-02os")
            source_favorites = ast.literal_eval(source_defaults["org/gnome/shell"]["favorite-apps"])
            self.assertEqual(favorites, [entry for entry in source_favorites if entry != "02os-install.desktop"])
            self.assertIn("02os-install.desktop", (source / "etc/dconf/db/local.d/00-02os").read_text())
            self.assertNotEqual((target / "etc/dconf/db/local").read_bytes(), b"db")
            self.assertNotIn(b"02os-install.desktop", (target / "etc/dconf/db/local").read_bytes())
            self.assertEqual((target / "etc/dconf/profile/user").read_text(), "user-db:user\nsystem-db:local\n")
            identity = (OVERLAY / "etc/os-release").read_text()
            self.assertNotIn("Arch Linux", identity)
            self.assertFalse((target / "etc/os-release").is_symlink())
            self.assertFalse(vendor_identity.is_symlink())
            self.assertEqual((target / "etc/os-release").read_text(), identity)
            self.assertEqual(vendor_identity.read_text(), identity)
            for rel in (
                "etc/dconf/profile/gdm",
                "etc/dconf/db/gdm.d/00-02os-branding",
                "usr/share/pixmaps/02os-logo.svg",
                "usr/share/icons/hicolor/scalable/apps/02os-logo.svg",
            ):
                self.assertEqual((target / rel).read_bytes(), (OVERLAY / rel).read_bytes(), rel)
            self.assertEqual((target / "etc/dconf/db/gdm").read_bytes(), b"gdm-db")
            self.assertEqual((target / "usr/share/backgrounds/02os/desktop.jpg").read_text(), "jpg")
            self.assertEqual((target / "usr/share/plymouth/themes/02-zero-portal/02-zero-portal.plymouth").read_text(), "theme")
            self.assertEqual((target / "etc/plymouth/plymouthd.conf").read_text(), "[Daemon]\nTheme=02-zero-portal\n")
            self.assertFalse((target / "etc/mkinitcpio.conf.d/archiso.conf").exists())
            schema_dir = target / "usr/share/glib-2.0/schemas"
            for schema_id in EXTENSIONS.values():
                self.assertEqual((schema_dir / f"{schema_id}.gschema.xml").read_text(), self._schema(schema_id))
            self.assertTrue((schema_dir / "gschemas.compiled").is_file())
            self.assertFalse((schema_dir / "org.example.gschema.xml").exists())
            icon = target / "usr/share/icons/02-OS/scalable/apps/a.svg"
            self.assertTrue(icon.is_symlink())
            self.assertEqual(icon.readlink(), Path("b.svg"))
            for rel in forbidden:
                self.assertFalse((target / rel).exists(), rel)
            self.assertFalse((target / "etc/hostname").exists())

            bare = Path(tmp) / "bare-source"
            bare_target = Path(tmp) / "bare-target"
            self._write_provision_fixture(bare, include_runtime=False)
            env = os.environ.copy()
            env["02OS_OVERLAY"] = str(bare)
            result = subprocess.run(
                [str(provision), str(bare_target)],
                capture_output=True, text=True, timeout=30, env=env,
            )
            self.assertNotEqual(result.returncode, 0, result.stdout)
            self.assertIn("required 02_OS file is missing", result.stderr)
            self.assertFalse((bare_target / "usr/bin/02").exists())
            self.assertFalse((bare_target / "usr/share/applications/02os-install.desktop").exists())

    def test_repeated_provisioning_replaces_service_link_without_following_its_leaf(self):
        provision = OVERLAY / "usr/local/bin/02os-provision"
        with tempfile.TemporaryDirectory(prefix="02os-repeat-service-") as tmp:
            source, target = Path(tmp) / "source", Path(tmp) / "target"
            self._write_provision_fixture(source, include_runtime=True)
            relative = "etc/systemd/user/default.target.wants/02-agentd.service"
            destination = target / relative
            destination.parent.mkdir(parents=True)
            destination.symlink_to("/usr/lib/systemd/user/02-agentd.service")
            outside = Path(tmp) / "outside-service"
            outside.write_text("preserve outside data\n")
            for existing in (None, outside, None):
                if existing is not None:
                    destination.unlink()
                    destination.symlink_to(existing)
                result = subprocess.run([str(provision), "--source", str(source), "--target", str(target)],
                                        capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(destination.is_symlink())
                self.assertEqual(destination.readlink(), (source / relative).readlink())
                self.assertEqual(outside.read_text(), "preserve outside data\n")

    def test_service_link_copy_rejects_redirected_parent_or_directory_leaf(self):
        provision = OVERLAY / "usr/local/bin/02os-provision"
        for kind in ("parent", "directory"):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory(prefix="02os-service-guard-") as tmp:
                source, target = Path(tmp) / "source", Path(tmp) / "target"
                self._write_provision_fixture(source, include_runtime=True)
                parent = target / "etc/systemd/user/default.target.wants"
                if kind == "parent":
                    outside = Path(tmp) / "outside"
                    outside.mkdir()
                    sentinel = outside / "02-agentd.service"
                    parent.parent.mkdir(parents=True)
                    parent.symlink_to(outside)
                else:
                    sentinel = parent / "02-agentd.service/preserve"
                    sentinel.parent.mkdir(parents=True)
                sentinel.write_text("preserve existing data\n")
                result = subprocess.run([str(provision), "--source", str(source), "--target", str(target)],
                                        capture_output=True, text=True, timeout=30)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(sentinel.read_text(), "preserve existing data\n")

    def test_provisioner_stages_browser_archives_only_in_explicit_final_phase(self):
        provision = OVERLAY / "usr/local/bin/02os-provision"
        with tempfile.TemporaryDirectory(prefix="02os-browser-copy-") as tmp:
            source, target = Path(tmp) / "source", Path(tmp) / "target"
            self._write_provision_fixture(source, include_runtime=True)
            helper = source / "usr/local/bin/02os-install-browsers"
            shutil.copyfile(OVERLAY / "usr/local/bin/02os-install-browsers", helper)
            helper.chmod(0o755)
            repo = source / "usr/local/share/02os/browser-repo"
            repo.mkdir(parents=True, exist_ok=True)
            for name, payload in (("manifest.json", b"manifest"), ("02os-browsers.db", b"database"),
                                  ("browser.pkg.tar.zst", b"verified package"),
                                  (".02os-generated-browser-repo", b"source ownership marker")):
                (repo / name).write_bytes(payload)
            result = subprocess.run([str(provision), "--source", str(source), "--target", str(target)],
                                    capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((target / "usr/local/bin/02os-install-browsers").read_bytes(), helper.read_bytes())
            self.assertTrue(os.access(target / "usr/local/bin/02os-install-browsers", os.X_OK))
            installed_repo = target / "usr/local/share/02os/browser-repo"
            self.assertEqual([path.name for path in installed_repo.iterdir()], ["manifest.json"])
            result = subprocess.run([str(provision), "--browser-payload", "--source", str(source),
                                     "--target", str(target)], capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            for payload in repo.iterdir():
                if payload.name == ".02os-generated-browser-repo":
                    self.assertFalse((installed_repo / payload.name).exists())
                    self.assertEqual(payload.read_bytes(), b"source ownership marker")
                    continue
                self.assertEqual((installed_repo / payload.name).read_bytes(), payload.read_bytes())
                if payload.name != "manifest.json":
                    (installed_repo / payload.name).unlink()
            result = subprocess.run([str(provision), "--source", str(source), "--target", str(target)],
                                    capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual([path.name for path in installed_repo.iterdir()], ["manifest.json"])
            self.assertTrue((repo / "browser.pkg.tar.zst").exists())

    def test_provisioner_requires_browser_helper_and_manifest_before_copying(self):
        provision = OVERLAY / "usr/local/bin/02os-provision"
        for missing in ("usr/local/bin/02os-install-browsers", "usr/local/share/02os/browser-repo/manifest.json"):
            with self.subTest(missing=missing), tempfile.TemporaryDirectory(prefix="02os-browser-required-") as tmp:
                source, target = Path(tmp) / "source", Path(tmp) / "target"
                self._write_provision_fixture(source, include_runtime=True)
                (source / missing).unlink()
                result = subprocess.run([str(provision), "--source", str(source), "--target", str(target)],
                                        capture_output=True, text=True, timeout=30)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("required 02_OS file is missing", result.stderr)
                self.assertFalse((target / "usr/bin/02").exists())

    def test_provisioner_removes_only_known_live_installer_leftovers(self):
        provision = OVERLAY / "usr/local/bin/02os-provision"
        with tempfile.TemporaryDirectory(prefix="02os-installer-cleanup-") as tmp:
            source, target = Path(tmp) / "source", Path(tmp) / "target"
            self._write_provision_fixture(source, include_runtime=True)
            leftovers = ("usr/share/applications/02os-install.desktop", "usr/local/bin/02os-install")
            for rel in leftovers:
                path = target / rel
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("old image installer")
            unrelated = target / "usr/share/applications/my-app.desktop"
            unrelated.write_text("user application")
            result = subprocess.run([str(provision), "--source", str(source), "--target", str(target)],
                                    capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            for rel in leftovers:
                self.assertFalse((target / rel).exists())
                self.assertTrue((source / rel).exists())
            self.assertEqual(unrelated.read_text(), "user application")

    def test_browser_payload_stage_rejects_live_root_and_target_symlink_escape(self):
        provision = OVERLAY / "usr/local/bin/02os-provision"
        with tempfile.TemporaryDirectory(prefix="02os-browser-escape-") as tmp:
            source, target, outside = (Path(tmp) / name for name in ("source", "target", "outside"))
            self._write_provision_fixture(source, include_runtime=True)
            outside.mkdir()
            (outside / "keep.txt").write_text("untouched")
            repo = target / "usr/local/share/02os/browser-repo"
            repo.parent.mkdir(parents=True)
            repo.symlink_to(outside)
            result = subprocess.run([str(provision), "--browser-payload", "--source", str(source),
                                     "--target", str(target)], capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("resolves outside the installed target", result.stderr)
            self.assertEqual([path.name for path in outside.iterdir()], ["keep.txt"])
            repo.unlink()
            repo.mkdir()
            (repo / "02os-browsers.db").symlink_to(outside / "keep.txt")
            (source / "usr/local/share/02os/browser-repo/02os-browsers.db").write_text("new database")
            result = subprocess.run([str(provision), "--browser-payload", "--source", str(source),
                                     "--target", str(target)], capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("resolves outside the installed target", result.stderr)
            self.assertEqual((outside / "keep.txt").read_text(), "untouched")
            for root in (source, Path("/")):
                result = subprocess.run([str(provision), "--browser-payload", "--source", str(source),
                                         "--target", str(root)], capture_output=True, text=True, timeout=30)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("separate from", result.stderr)

    def test_browser_copy_rejects_in_target_symlinks_before_overwriting_referents(self):
        provision = OVERLAY / "usr/local/bin/02os-provision"
        for mode in ([], ["--browser-payload"]):
            redirects = ["repository", "repository-parent", "manifest", "helper"]
            if mode:
                redirects.append("archive")
            for redirect in redirects:
                with self.subTest(mode=mode, redirect=redirect), tempfile.TemporaryDirectory(prefix="02os-browser-local-link-") as tmp:
                    source, target = Path(tmp) / "source", Path(tmp) / "target"
                    self._write_provision_fixture(source, include_runtime=True)
                    data = target / "home/user/data"
                    data.mkdir(parents=True)
                    repo = target / "usr/local/share/02os/browser-repo"
                    if redirect == "repository":
                        destination, referent = repo, data
                        (data / "manifest.json").write_text("valuable user manifest")
                    elif redirect == "repository-parent":
                        destination, referent = repo.parent, data
                        (data / "browser-repo").mkdir()
                        (data / "browser-repo/manifest.json").write_text("valuable user manifest")
                    elif redirect == "manifest":
                        destination, referent = repo / "manifest.json", data / "notes.json"
                        referent.write_text("valuable user manifest")
                    elif redirect == "helper":
                        destination = target / "usr/local/bin/02os-install-browsers"
                        referent = data / "helper-notes"
                        referent.write_text("valuable user helper")
                    else:
                        destination, referent = repo / "browser.pkg.tar.zst", data / "package-notes"
                        referent.write_text("valuable user archive")
                        (source / "usr/local/share/02os/browser-repo/browser.pkg.tar.zst").write_text("image package")
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    destination.symlink_to(os.path.relpath(referent, destination.parent))
                    before = {path.relative_to(data): path.read_bytes() for path in data.rglob("*") if path.is_file()}
                    result = subprocess.run([str(provision), *mode, "--source", str(source), "--target", str(target)],
                                            capture_output=True, text=True, timeout=30)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("refusing browser destination symlink", result.stderr)
                    self.assertTrue(destination.is_symlink())
                    after = {path.relative_to(data): path.read_bytes() for path in data.rglob("*") if path.is_file()}
                    self.assertEqual(after, before)

    def test_provisioner_requires_motion_extensions_before_copying(self):
        provision = OVERLAY / "usr/local/bin/02os-provision"
        for uuid in ("flourish@orsso.github.io", "compiz-windows-effect@hermes83.github.com"):
            with self.subTest(extension=uuid), tempfile.TemporaryDirectory(prefix="02os-missing-ext-") as tmp:
                source, target = Path(tmp) / "source", Path(tmp) / "target"
                self._write_provision_fixture(source, include_runtime=True)
                shutil.rmtree(source / "usr/share/gnome-shell/extensions" / uuid)
                result = subprocess.run([str(provision), "--source", str(source), "--target", str(target)],
                                        capture_output=True, text=True, timeout=30)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(uuid, result.stderr)
                self.assertFalse((target / "usr/bin/02").exists())

    def test_provisioner_preserves_installed_boot_and_login_configuration(self):
        provision = OVERLAY / "usr/local/bin/02os-provision"
        with tempfile.TemporaryDirectory(prefix="02os-target-config-") as tmp:
            source, target = Path(tmp) / "source", Path(tmp) / "target"
            self._write_provision_fixture(source, include_runtime=True)
            self._write_forbidden_live_paths(source)
            preserved = {
                "etc/gdm/custom.conf": "[daemon]\nAutomaticLoginEnable=False\n# installed account configuration\n",
                "etc/mkinitcpio.conf": "HOOKS=(base systemd kms keyboard sd-encrypt filesystems)\n",
                "boot/loader/entries/arch.conf": "options root=UUID=target rd.luks.name=encrypted=root\n",
            }
            for rel, content in preserved.items():
                path = target / rel
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content)
            result = subprocess.run([str(provision), "--source", str(source), "--target", str(target)],
                                    capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            for rel, content in preserved.items():
                self.assertEqual((target / rel).read_text(), content, rel)

    def test_provisioner_refuses_symlink_outside_source(self):
        provision = OVERLAY / "usr/local/bin/02os-provision"
        with tempfile.TemporaryDirectory(prefix="02os-escape-") as tmp:
            source = Path(tmp) / "source"
            target = Path(tmp) / "target"
            escaped = source / "usr/bin/02"
            escaped.parent.mkdir(parents=True)
            escaped.symlink_to("/etc/passwd")
            desktop = source / "usr/share/applications/02os-install.desktop"
            desktop.parent.mkdir(parents=True)
            desktop.write_text("desktop")
            result = subprocess.run(
                [str(provision), "--source", str(source), "--target", str(target)],
                capture_output=True, text=True, timeout=30,
            )
            self.assertNotEqual(result.returncode, 0, result.stderr)
            self.assertFalse((target / "usr/bin/02").exists())
            self.assertFalse((target / "usr/share/applications/02os-install.desktop").exists())

            nested = Path(tmp) / "nested-source"
            nested_target = Path(tmp) / "nested-target"
            icon = nested / "usr/share/icons/02-OS"
            icon.mkdir(parents=True)
            (icon / "ok.txt").write_text("ok")
            (icon / "bad").symlink_to("/etc/passwd")
            result = subprocess.run(
                [str(provision), "--source", str(nested), "--target", str(nested_target)],
                capture_output=True, text=True, timeout=30,
            )
            self.assertNotEqual(result.returncode, 0, result.stderr)
            self.assertFalse((nested_target / "usr/share/icons/02-OS/ok.txt").exists())
            self.assertFalse((nested_target / "usr/share/icons/02-OS/bad").exists())

    def _schema(self, schema_id):
        path = "/" + schema_id.replace(".", "/") + "/"
        return (
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
            "<schemalist>\n"
            f"  <schema id=\"{schema_id}\" path=\"{path}\">\n"
            "    <key name=\"marker\" type=\"s\">\n"
            "      <default>'02os'</default>\n"
            "      <summary>Marker</summary>\n"
            "    </key>\n"
            "  </schema>\n"
            "</schemalist>\n"
        )

    def _write_provision_fixture(self, source, include_runtime):
        desktop = source / "usr/share/applications/02os-install.desktop"
        desktop.parent.mkdir(parents=True)
        desktop.write_text("desktop")
        if not include_runtime:
            return
        bindir = source / "usr/bin"
        bindir.mkdir(parents=True)
        (bindir / "02").write_text("runtime")
        (bindir / "02-agentd").write_text("daemon")
        (bindir / "02agent").symlink_to("02")
        launcher = source / "usr/local/bin/pop-launcher"
        launcher.parent.mkdir(parents=True)
        launcher.write_text("launcher")
        (launcher.parent / "02os-install").write_text("live installer")
        browser_helper = launcher.parent / "02os-install-browsers"
        shutil.copyfile(OVERLAY / "usr/local/bin/02os-install-browsers", browser_helper)
        browser_helper.chmod(0o755)
        browser_repo = source / "usr/local/share/02os/browser-repo"
        browser_repo.mkdir(parents=True)
        (browser_repo / "manifest.json").write_text("fixture browser manifest")
        for ext, schema_id in EXTENSIONS.items():
            ext_dir = source / "usr/share/gnome-shell/extensions" / ext
            (ext_dir / "schemas").mkdir(parents=True)
            (ext_dir / "extension.js").write_text("ext")
            (ext_dir / "schemas" / f"{schema_id}.gschema.xml").write_text(self._schema(schema_id))
        portal = source / "usr/share/gnome-shell/extensions/02-zero-portal@02os"
        portal.mkdir(parents=True)
        (portal / "extension.js").write_text("portal")
        phase = source / "usr/share/gnome-shell/extensions/02-index-phase@02os"
        phase.mkdir(parents=True)
        (phase / "extension.js").write_text("phase")
        status = source / "usr/share/gnome-shell/extensions/02-agent-status@02os"
        status.mkdir(parents=True)
        (status / "extension.js").write_text("status")
        tmpfiles = source / "usr/lib/tmpfiles.d/02os-zero-portal.conf"
        tmpfiles.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(OVERLAY / "usr/lib/tmpfiles.d/02os-zero-portal.conf", tmpfiles)
        apps = source / "usr/share/icons/02-OS/scalable/apps"
        apps.mkdir(parents=True)
        (source / "usr/share/icons/02-OS/index.theme").write_text("[Icon Theme]\nName=02-OS\nDirectories=scalable/apps\n")
        (apps / "b.svg").write_text("<svg xmlns='http://www.w3.org/2000/svg'/>")
        (apps / "a.svg").symlink_to("b.svg")
        wallpaper = source / "usr/share/backgrounds/02os/desktop.jpg"
        wallpaper.parent.mkdir(parents=True)
        wallpaper.write_text("jpg")
        theme = source / "usr/share/plymouth/themes/02-zero-portal/02-zero-portal.plymouth"
        theme.parent.mkdir(parents=True)
        theme.write_text("theme")
        plymouth_config = source / "etc/plymouth/plymouthd.conf"
        plymouth_config.parent.mkdir(parents=True)
        plymouth_config.write_text("[Daemon]\nTheme=02-zero-portal\n")
        schema_dir = source / "usr/share/glib-2.0/schemas"
        schema_dir.mkdir(parents=True)
        (schema_dir / "org.example.gschema.xml").write_text("<schemalist/>")
        for schema_id in EXTENSIONS.values():
            (schema_dir / f"{schema_id}.gschema.xml").write_text(self._schema(schema_id))
        dconf = source / "etc/dconf/db/local.d/00-02os"
        dconf.parent.mkdir(parents=True)
        dconf.write_bytes((OVERLAY / "etc/dconf/db/local.d/00-02os").read_bytes())
        (source / "etc/dconf/profile").mkdir(parents=True)
        (source / "etc/dconf/profile/user").write_text("user-db:user\nsystem-db:local\n")
        (source / "etc/dconf/db/local").write_bytes(b"db")
        for rel in (
            "usr/local/bin/02os-check-desktop",
            "usr/local/share/02os/desktop-compatibility.json",
            "etc/sysctl.d/99-02os-performance.conf",
            "etc/systemd/zram-generator.conf",
            "etc/os-release",
            "etc/dconf/profile/gdm",
            "etc/dconf/db/gdm.d/00-02os-branding",
            "usr/share/pixmaps/02os-logo.svg",
            "usr/share/icons/hicolor/scalable/apps/02os-logo.svg",
        ):
            destination = source / rel
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(OVERLAY / rel, destination)
        (source / "etc/dconf/db/gdm").write_bytes(b"gdm-db")
        provenance = source / "usr/lib/02-agent/SOURCE_REVISION"
        provenance.parent.mkdir(parents=True)
        provenance.write_text("rev\n")
        service = source / "usr/lib/systemd/user/02-agentd.service"
        service.parent.mkdir(parents=True)
        service.write_text("[Service]\n")
        wants = source / "etc/systemd/user/default.target.wants/02-agentd.service"
        wants.parent.mkdir(parents=True)
        wants.symlink_to("../../../../usr/lib/systemd/user/02-agentd.service")
        (source / "usr/lib/02-agent/prepare-storage.sh").write_text("#!/bin/sh\nexit 0\n")
        generator = source / "usr/lib/systemd/user-generators/02-agent-storage"
        generator.parent.mkdir(parents=True)
        generator.write_text("#!/bin/sh\nexit 0\n")

    def _write_forbidden_live_paths(self, source):
        forbidden = [
            "etc/sudoers.d/01-live",
            "etc/greetd/config.toml",
            "etc/gdm/custom.conf",
            "etc/pam.d/greetd",
            "etc/pam.d/02os-graphical-login",
            "etc/sysusers.d/02os.conf",
            "etc/systemd/system/02os-ensure-live-user.service",
            "etc/systemd/system/graphical.target.wants/02os-ensure-live-user.service",
            "etc/systemd/system/graphical.target.wants/greetd.service",
            "etc/systemd/system/display-manager.service",
            "etc/systemd/system/graphical.target.wants/gdm.service",
            "etc/systemd/system/systemd-time-wait-sync.service.d/live-offline.conf",
            "etc/udev/rules.d/98-02os-live-media.rules",
            "etc/ssh/sshd_config.d/10-archiso.conf",
            "etc/polkit-1/rules.d/10-live-power.rules",
            "usr/local/bin/02os-ensure-live-user",
            "usr/local/bin/02os-configure-greetd",
            "etc/hostname",
            "etc/mkinitcpio.conf.d/archiso.conf",
        ]
        for rel in forbidden:
            path = source / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("live-only\n")
        return forbidden

    def test_pop_launcher_search_activate_and_exit(self):
        launcher = OVERLAY / "usr/local/bin/pop-launcher"
        self.assertTrue(launcher.is_file())
        dconf = (OVERLAY / "etc/dconf/db/local.d/00-02os").read_text()
        self.assertIn("pop-shell@system76.com", dconf)
        self.assertIn("dash-to-dock@micxgx.gmail.com", dconf)
        with tempfile.TemporaryDirectory(prefix="02os-launch-") as tmp:
            apps = Path(tmp) / "applications"
            apps.mkdir()
            desktop = apps / "org.gnome.Console.desktop"
            desktop.write_text(
                "[Desktop Entry]\n"
                "Type=Application\n"
                "Name=GNOME Console\n"
                "Exec=kgx\n"
                "Icon=org.gnome.Console\n"
            )
            env = os.environ.copy()
            env["XDG_DATA_DIRS"] = str(Path(tmp))
            proc = subprocess.Popen(
                [str(launcher)],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                bufsize=1,
                env=env,
            )
            try:
                proc.stdin.write(json.dumps({"Search": "GNOME Console"}) + "\n")
                proc.stdin.flush()
                update = self._read_launcher_line(proc)
                self.assertIn("Update", update)
                matches = [item for item in update["Update"] if item.get("name") == "GNOME Console"]
                self.assertTrue(matches, update)
                found = None
                matched_item = None
                for item in matches:
                    proc.stdin.write(json.dumps({"Activate": item["id"]}) + "\n")
                    proc.stdin.flush()
                    activated = self._read_launcher_line(proc)
                    entry = activated.get("DesktopEntry")
                    if entry and entry.get("path") == str(desktop):
                        found = entry
                        matched_item = item
                        break
                self.assertIsNotNone(found, "DesktopEntry did not return the fixture desktop path")
                self.assertEqual(found["gpu_preference"], "Default")
                self.assertEqual(found["path"], str(desktop))
                self.assertEqual(matched_item.get("icon"), {"Name": "org.gnome.Console"})
                self.assertEqual(matched_item.get("exec"), "kgx")
                proc.stdin.write(json.dumps("Exit") + "\n")
                proc.stdin.flush()
                proc.stdin.close()
                code = proc.wait(timeout=5)
                err = ""
                if proc.stderr is not None:
                    err = proc.stderr.read()
                self.assertEqual(code, 0, err)
            finally:
                if proc.poll() is None:
                    proc.kill()
                    proc.wait(timeout=5)
                for stream in (proc.stdin, proc.stdout, proc.stderr):
                    if stream is not None and not stream.closed:
                        stream.close()

    def _read_launcher_line(self, proc):
        ready, _, _ = select.select([proc.stdout.fileno()], [], [], 10)
        if not ready:
            proc.kill()
            self.fail("pop-launcher produced no output")
        line = proc.stdout.readline()
        self.assertTrue(line, "pop-launcher closed stdout")
        return json.loads(line)


if __name__ == "__main__":
    unittest.main()
