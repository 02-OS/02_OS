#!/usr/bin/env python3
"""Fixed checks and guarded installation inside a disposable 02_OS VM only."""
import base64
import json
import os
from pathlib import Path
import pwd
import re
import subprocess
import sys
import time

DISK_BYTES = 32 * 1024**3


def command(args, timeout=30):
    result = subprocess.run(args, text=True, capture_output=True, timeout=timeout)
    return {"exitcode": result.returncode, "stdout": result.stdout, "stderr": result.stderr}


def disk_guard(inventory, serial):
    disks = []
    for item in inventory["blockdevices"]:
        if item["type"] != "disk":
            continue
        path = Path(item["name"])
        # Only real kernel RAM devices are exempt from the sole-disk check.
        if re.fullmatch(r"zram[0-9]+", path.name):
            sysfs = Path("/sys/class/block") / path.name
            if sysfs.resolve() == Path("/sys/devices/virtual/block") / path.name:
                backing = sysfs / "backing_dev"
                if not backing.exists() or backing.read_text().strip() == "none":
                    continue
        disks.append(item)
    if len(disks) != 1:
        raise ValueError("Expected exactly one persistent disposable guest disk")
    disk = disks[0]
    if (disk["name"] != "/dev/vda" or disk.get("serial") != serial
            or disk["size"] != DISK_BYTES or disk.get("ro") or disk.get("children")):
        raise ValueError("Guest disk differs from the fresh, serial-identified disposable disk")


def install(payload):
    if not Path("/run/archiso/bootmnt").exists() or not Path("/sys/firmware/efi").exists():
        raise ValueError("Guarded installation requires the live ISO booted in UEFI mode")
    if abs(time.time() - payload["host_epoch"]) > 120:
        raise ValueError("Guest clock differs from the current host clock")
    config = payload["config"]
    changes = config.get("disk_config", {}).get("device_modifications", [])
    if (len(changes) != 1 or changes[0].get("device") != "/dev/vda"
            or changes[0].get("wipe") is not True or config.get("custom_commands")
            or config.get("disk_encryption") or config.get("disk_config", {}).get("lvm_config")):
        raise ValueError("Unsupported disposable installation configuration")
    users = payload["credentials"].get("users", [])
    if (len(users) != 1 or users[0].get("username") != "02tester"
            or not users[0].get("sudo") or not users[0].get("enc_password", "").startswith("$6$")):
        raise ValueError("Expected the canonical disposable test account")
    inventory = json.loads(subprocess.check_output(["lsblk", "--json", "--bytes", "--paths",
        "--output", "NAME,SERIAL,TYPE,SIZE,RO"], text=True))
    disk_guard(inventory, payload["serial"])
    subprocess.run(["/usr/local/bin/02os-check-desktop", "--installer"], check=True)
    work = Path("/run/02os-acceptance-install")
    work.mkdir(mode=0o700)  # A failed/partially installed disk is never retried.
    for name, value in (("config.json", config), ("creds.json", payload["credentials"])):
        path = work / name
        path.write_text(json.dumps(value))
        path.chmod(0o600)
    args = ["/usr/local/bin/02os-install", "--silent", "--config", str(work / "config.json"),
            "--creds", str(work / "creds.json"), "--skip-wifi-check", "--skip-ntp", "--skip-wkd"]
    with (work / "dry-run.log").open("w") as log:
        dry = subprocess.run(args + ["--dry-run"], stdout=log, stderr=subprocess.STDOUT,
                             stdin=subprocess.DEVNULL, timeout=120)
    if dry.returncode:
        raise RuntimeError("Archinstall dry run failed: " + (work / "dry-run.log").read_text()[-6000:])
    disk_guard(json.loads(subprocess.check_output(["lsblk", "--json", "--bytes", "--paths",
        "--output", "NAME,SERIAL,TYPE,SIZE,RO"], text=True)), payload["serial"])
    with (work / "installer.log").open("w") as log:
        result = subprocess.run(args, stdout=log, stderr=subprocess.STDOUT,
                                stdin=subprocess.DEVNULL, timeout=2700)
    log = Path("/var/log/archinstall/install.log").read_text(errors="replace")
    if (result.returncode or "Installation completed without any errors." not in log
            or "Some required steps were not successfully installed/configured" in log):
        raise RuntimeError("Archinstall did not complete: " + (work / "installer.log").read_text(errors="replace")[-12000:])
    subprocess.run(["arch-chroot", "/mnt", "/usr/local/bin/02os-check-desktop"], check=True)
    if (Path("/mnt/usr/local/bin/02os-install").exists()
            or Path("/mnt/usr/share/applications/02os-install.desktop").exists()):
        raise RuntimeError("Live-only installer remains in installed system")
    return {"installed": True, "serial": payload["serial"], "packages": command(["pacman", "--root", "/mnt", "-Q"])}


def snapshot(username):
    checks = {
        "desktop_contract": ["/usr/local/bin/02os-check-desktop"],
        "system_failed": ["systemctl", "--failed", "--no-legend", "--no-pager"],
        "gdm": ["systemctl", "is-active", "gdm"],
        "packages": ["pacman", "-Q"],
        "sessions": ["loginctl", "list-sessions", "--no-pager"],
        "journal": ["journalctl", "-b", "--no-pager", "-n", "200"],
        "runtime": ["02", "--version"],
        "network": ["getent", "hosts", "archlinux.org"],
        "zram": ["zramctl", "--output-all", "--raw"],
    }
    result = {name: command(args) for name, args in checks.items()}
    try:
        uid = pwd.getpwnam(username).pw_uid
        prefix = ["runuser", "-u", username, "--", "env", f"XDG_RUNTIME_DIR=/run/user/{uid}",
                  f"DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/{uid}/bus"]
        for name, args in {
            "extensions": ["gnome-extensions", "list", "--details"],
            "user_failed": ["systemctl", "--user", "--failed", "--no-legend", "--no-pager"],
            "daemon": ["systemctl", "--user", "is-active", "02-agentd"],
            "audio": ["wpctl", "status"],
            "favorites": ["gsettings", "get", "org.gnome.shell", "favorite-apps"],
        }.items():
            result[name] = command(prefix + args)
    except KeyError:
        result["session_error"] = "Expected test account is absent"
    portal = Path("/run/02os-zero-portal/result.json")
    if portal.is_file():
        result["portal"] = json.loads(portal.read_text())
    return result


def main():
    payload = json.loads(base64.b64decode(sys.argv[2]))
    result = install(payload) if sys.argv[1] == "install" else snapshot(payload["username"])
    print(json.dumps(result), flush=True)


if __name__ == "__main__":
    main()
