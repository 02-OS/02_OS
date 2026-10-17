"""Installed GNOME defaults ship their packages, and not a broken installer entry."""
import ast
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
OVERLAY = ROOT / "profile/airootfs"
PROVISION = OVERLAY / "usr/local/bin/02os-provision"
MANIFEST = OVERLAY / "usr/local/share/02os/installed-packages.txt"
PLUGIN = OVERLAY / "usr/local/share/02os/archinstall_plugin.py"
DEFAULTS = OVERLAY / "etc/dconf/db/local.d/00-02os"
ICON_THEME = OVERLAY / "usr/share/icons/02-OS/index.theme"
ISO_PACKAGES = ROOT / "profile/packages.x86_64"

EXTENSIONS = (
    "dash-to-dock@micxgx.gmail.com",
    "pop-shell@system76.com",
    "flourish@orsso.github.io",
    "compiz-windows-effect@hermes83.github.com",
)
SCHEMAS = (
    "org.gnome.shell.extensions.dash-to-dock",
    "org.gnome.shell.extensions.pop-shell",
    "org.gnome.shell.extensions.flourish",
    "org.gnome.shell.extensions.com.github.hermes83.compiz-windows-effect",
)


def package_names(path):
    names = []
    for raw in path.read_text().splitlines():
        line = raw.split("#", 1)[0].strip()
        if line:
            names.append(line)
    return names


def schema_xml(schema_id):
    path = "/" + schema_id.replace(".", "/") + "/"
    return (
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
        "<schemalist>\n"
        f"  <schema id=\"{schema_id}\" path=\"{path}\">\n"
        "    <key name=\"marker\" type=\"s\">\n"
        "      <default>'02os'</default>\n"
        "      <summary>Marker</summary>\n"
        "    </key>\n"
        "</schema>\n"
        "</schemalist>\n"
    )


class InstallerPackageTests(unittest.TestCase):
    def test_gnome_install_set_includes_desktop_default_packages(self):
        defaults = DEFAULTS.read_text()
        theme = ICON_THEME.read_text()
        self.assertIn("gtk-theme='adw-gtk3-dark'", defaults)
        self.assertIn("cursor-theme='capitaine-cursors'", defaults)
        self.assertIn("font-name='Inter 11'", defaults)
        self.assertIn("google-chrome.desktop", defaults)
        self.assertNotIn("firefox.desktop", defaults)
        self.assertIn("Papirus", theme)

        required = {
            "adw-gtk-theme",
            "capitaine-cursors",
            "git",
            "inter-font",
            "papirus-icon-theme",
            "power-profiles-daemon",
            "rtkit",
            "vulkan-swrast",
            "wl-clipboard",
            "xorg-xprop",
        }
        installed = package_names(MANIFEST)
        iso = package_names(ISO_PACKAGES)
        self.assertEqual(len(installed), len(set(installed)))
        self.assertTrue(required <= set(installed), installed)
        self.assertTrue(required <= set(iso), "live ISO manifest is missing a default package")
        self.assertNotIn("firefox", installed)
        self.assertTrue({"google-chrome", "brave-bin"} <= set(iso))
        self.assertTrue({"google-chrome", "brave-bin"}.isdisjoint(installed),
                        "Image-local browser packages are installed by their verified payload helper")

        installer = (OVERLAY / "usr/local/bin/02os-install").read_text()
        self.assertIn("installed-packages.txt", installer)
        self.assertIn("before the final provision", installer)
        self.assertIn(
            "exec archinstall --plugin /usr/local/share/02os/archinstall_plugin.py --skip-version-check",
            installer,
        )

        tree = ast.parse(PLUGIN.read_text(), filename=str(PLUGIN))
        assigned = {}
        functions = {}
        for node in tree.body:
            if isinstance(node, ast.Assign):
                for target in node.targets:
                    if isinstance(target, ast.Name) and isinstance(node.value, ast.Constant):
                        assigned[target.id] = node.value.value
            if isinstance(node, ast.ClassDef) and node.name == "Plugin":
                for item in node.body:
                    if isinstance(item, ast.FunctionDef):
                        functions[item.name] = item
        self.assertEqual(assigned["INSTALLED_PACKAGES"], "/usr/local/share/02os/installed-packages.txt")

        install_fn = functions["_install_desktop_packages"]
        constants = [node.value for node in ast.walk(install_fn) if isinstance(node, ast.Constant)]
        names = [node.id for node in ast.walk(install_fn) if isinstance(node, ast.Name)]
        self.assertIn("add_additional_packages", constants)
        self.assertIn("INSTALLED_PACKAGES", names)

        calls = []
        for stmt in functions["on_genfstab"].body:
            if isinstance(stmt, ast.Expr) and isinstance(stmt.value, ast.Call):
                func = stmt.value.func
                if isinstance(func, ast.Attribute):
                    calls.append(func.attr)
        self.assertEqual(calls, ["_install_desktop_packages", "_provision", "arch_chroot", "_install_browsers", "_activate_theme"])
        install_calls = []
        for stmt in functions["on_install"].body:
            if isinstance(stmt, ast.Expr) and isinstance(stmt.value, ast.Call):
                func = stmt.value.func
                if isinstance(func, ast.Attribute):
                    install_calls.append(func.attr)
        self.assertEqual(install_calls, ["add_additional_packages", "_provision", "_prepare_boot",
                                         "_configure_boot_branding", "_activate_theme"])


class InstalledLauncherTests(unittest.TestCase):
    def test_live_image_keeps_the_installer(self):
        desktop = (OVERLAY / "usr/share/applications/02os-install.desktop").read_text()
        self.assertIn("Name=Install 02_OS", desktop)
        self.assertIn("02os-install", desktop)
        self.assertIn("02os-install.desktop", DEFAULTS.read_text())
        self.assertTrue(PLUGIN.is_file())
        self.assertTrue((OVERLAY / "usr/local/bin/02os-install").is_file())

    def test_installed_system_has_no_launcher_for_a_missing_plugin(self):
        with tempfile.TemporaryDirectory(prefix="02os-install-copy-") as tmp:
            source = Path(tmp) / "source"
            target = Path(tmp) / "target"
            self._write_fixture(source)
            source_defaults = (source / "etc/dconf/db/local.d/00-02os").read_text()
            self.assertIn("02os-install.desktop", source_defaults)
            result = subprocess.run(
                [str(PROVISION), "--source", str(source), "--target", str(target)],
                capture_output=True,
                text=True,
                timeout=30,
            )
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            apps = target / "usr/share/applications"
            desktops = list(apps.glob("*.desktop")) if apps.is_dir() else []
            plugin = target / "usr/local/share/02os/archinstall_plugin.py"
            self.assertFalse(plugin.exists())
            for desktop in desktops:
                for line in desktop.read_text().splitlines():
                    if not line.startswith("Exec="):
                        continue
                    references_plugin = (
                        "archinstall_plugin.py" in line or "02os-install" in line
                    )
                    if references_plugin:
                        self.fail(f"{desktop} Exec references a missing plugin: {line}")
            self.assertFalse((target / "usr/share/applications/02os-install.desktop").exists())
            installed_defaults = (target / "etc/dconf/db/local.d/00-02os").read_text()
            self.assertNotIn("02os-install.desktop", installed_defaults)
            self.assertIn("google-chrome.desktop", installed_defaults)
            self.assertIn("gtk-theme='adw-gtk3-dark'", installed_defaults)
            compiled = (target / "etc/dconf/db/local").read_bytes()
            self.assertNotIn(b"02os-install.desktop", compiled)
            self.assertEqual((source / "etc/dconf/db/local.d/00-02os").read_text(), source_defaults)
            self.assertIn("02os-install.desktop", DEFAULTS.read_text())

    def _write_fixture(self, source):
        desktop = source / "usr/share/applications/02os-install.desktop"
        desktop.parent.mkdir(parents=True)
        desktop.write_text((OVERLAY / "usr/share/applications/02os-install.desktop").read_text())
        bindir = source / "usr/bin"
        bindir.mkdir(parents=True)
        (bindir / "02").write_text("runtime")
        (bindir / "02-agentd").write_text("daemon")
        (bindir / "02agent").symlink_to("02")
        launcher = source / "usr/local/bin/pop-launcher"
        launcher.parent.mkdir(parents=True)
        launcher.write_text("launcher")
        (launcher.parent / "02os-install-browsers").write_text("#!/bin/sh\nexit 0\n")
        browser_manifest = source / "usr/local/share/02os/browser-repo/manifest.json"
        browser_manifest.parent.mkdir(parents=True)
        browser_manifest.write_text("fixture browser manifest")
        for ext in EXTENSIONS:
            ext_dir = source / "usr/share/gnome-shell/extensions" / ext
            ext_dir.mkdir(parents=True)
            (ext_dir / "extension.js").write_text("ext")
        portal = source / "usr/share/gnome-shell/extensions/02-zero-portal@02os"
        portal.mkdir(parents=True)
        (portal / "extension.js").write_text("portal")
        for rel, contents in {
            "usr/local/bin/02os-check-desktop": "#!/bin/sh\nexit 0\n",
            "usr/local/share/02os/desktop-compatibility.json": "{}\n",
            "etc/sysctl.d/99-02os-performance.conf": "vm.swappiness = 180\n",
            "etc/systemd/zram-generator.conf": "[zram0]\nzram-size = ram / 2\n",
            "usr/lib/tmpfiles.d/02os-zero-portal.conf": "d /run/02os-zero-portal 0770 root gdm -\n",
            "usr/share/icons/hicolor/scalable/apps/02os-logo.svg": "<svg/>\n",
            "usr/share/pixmaps/02os-logo.svg": "<svg/>\n",
            "etc/os-release": 'NAME="02_OS"\nPRETTY_NAME="02_OS"\nID=02os\n',
            "usr/share/plymouth/themes/02-zero-portal/02-zero-portal.plymouth": "[Plymouth Theme]\n",
            "etc/plymouth/plymouthd.conf": "[Daemon]\nTheme=02-zero-portal\n",
            "etc/dconf/profile/gdm": "user-db:user\nsystem-db:gdm\n",
            "etc/dconf/db/gdm.d/00-02os-branding": "[org/gnome/login-screen]\nlogo='02os'\n",
            "etc/dconf/db/gdm": "gdm-db",
        }.items():
            path = source / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(contents)
        icon = source / "usr/share/icons/02-OS"
        icon.mkdir(parents=True)
        (icon / "index.theme").write_text("[Icon Theme]\nName=02-OS\n")
        wallpaper = source / "usr/share/backgrounds/02os/desktop.jpg"
        wallpaper.parent.mkdir(parents=True)
        wallpaper.write_text("jpg")
        schema_dir = source / "usr/share/glib-2.0/schemas"
        schema_dir.mkdir(parents=True)
        for schema_id in SCHEMAS:
            (schema_dir / f"{schema_id}.gschema.xml").write_text(schema_xml(schema_id))
        dconf = source / "etc/dconf/db/local.d/00-02os"
        dconf.parent.mkdir(parents=True)
        dconf.write_text(DEFAULTS.read_text())
        (source / "etc/dconf/profile").mkdir(parents=True, exist_ok=True)
        (source / "etc/dconf/profile/user").write_text("user-db:user\nsystem-db:local\n")
        (source / "etc/dconf/db/local").write_bytes(b"stale-compiled-db-with-02os-install.desktop")
        service = source / "usr/lib/systemd/user/02-agentd.service"
        service.parent.mkdir(parents=True)
        service.write_text("[Service]\n")
        wants = source / "etc/systemd/user/default.target.wants/02-agentd.service"
        wants.parent.mkdir(parents=True)
        wants.symlink_to("../../../../usr/lib/systemd/user/02-agentd.service")
        for rel in ("usr/lib/02-agent/prepare-storage.sh", "usr/lib/systemd/user-generators/02-agent-storage"):
            helper = source / rel
            helper.parent.mkdir(parents=True, exist_ok=True)
            helper.write_text("#!/bin/sh\nexit 0\n")


if __name__ == "__main__":
    unittest.main()
