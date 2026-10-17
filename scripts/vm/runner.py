#!/usr/bin/env python3
"""Prepare and inspect fresh, isolated 02_OS acceptance VMs using local Docker/QEMU."""
import argparse
import base64
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
IMAGE = "02os-boot-proof-kvm-ubuntu:20261007"


def save(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n")
    temporary.replace(path)


class JsonSocket:
    def __init__(self, path, qmp=False):
        self.connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.connection.settimeout(10)
        self.connection.connect(str(path))
        self.file = self.connection.makefile("rwb")
        if qmp:
            greeting = self.receive()
            if "QMP" not in greeting:
                raise RuntimeError("Missing QMP greeting")
            self.call("qmp_capabilities")
        else:
            token = int(time.time_ns() % (2**60))
            request = {"execute": "guest-sync-delimited", "arguments": {"id": token}}
            self.connection.sendall(b"\xff" + json.dumps(request).encode() + b"\n")
            deadline = time.monotonic() + 10
            # The flush byte can produce a parser error. Old replies can also
            # remain on this persistent channel; ignore both until our token.
            while self.receive().get("return") != token:
                if time.monotonic() >= deadline:
                    raise TimeoutError("Guest-agent synchronization timed out")

    def receive(self):
        while True:
            line = self.file.readline()
            if not line:
                raise ConnectionError("QEMU channel closed")
            try:
                return json.loads(line.rsplit(b"\xff", 1)[-1])
            except (ValueError, UnicodeError):
                continue

    def call(self, method, arguments=None):
        message = {"execute": method}
        if arguments is not None:
            message["arguments"] = arguments
        self.file.write(json.dumps(message).encode() + b"\n")
        self.file.flush()
        while True:
            response = self.receive()
            if "error" in response:
                raise RuntimeError(response["error"])
            if "return" in response:
                return response["return"]

    def close(self):
        self.file.close()
        self.connection.close()


def docker(*args):
    return subprocess.check_output(["docker", *map(str, args)], text=True).strip()


def prepare(args):
    for address in args.dns:
        ipaddress.ip_address(address)
    iso = args.iso.resolve(strict=True)
    manifest = json.loads(Path(str(iso) + ".manifest.json").read_text())
    subprocess.run(["python3", str(ROOT / "scripts/iso-manifest.py"), "verify", str(iso), manifest["revision"]], check=True)
    args.runs_root.mkdir(parents=True, exist_ok=True)
    if shutil.disk_usage(args.runs_root).free < 14 * 1024**3:
        raise RuntimeError("Need 14 GiB free before preparing a fresh installed-system VM")
    run = args.runs_root.resolve() / (time.strftime("%Y%m%d-%H%M%S-") + uuid.uuid4().hex[:6])
    run.mkdir(mode=0o700)
    identity = uuid.uuid4().hex[:12]
    metadata = {"image": args.runner_image, "iso": str(iso), "iso_revision": manifest["revision"],
                "iso_sha256": manifest["sha256"], "firmware": args.firmware, "graphics": args.graphics,
                "serial": "02TEST-" + identity, "name": "02os-acceptance-" + identity,
                "installed": False, "host_block_devices": False, "host_filesystem_shares": False,
                "dns": args.dns}
    docker("run", "--rm", "--user", f"{os.getuid()}:{os.getgid()}",
           "--mount", f"type=bind,src={run},dst=/run-test", "--entrypoint", "/usr/bin/qemu-img",
           args.runner_image, "create", "-f", "qcow2", "/run-test/disk.qcow2", "32G")
    if args.firmware == "uefi":
        if not args.firmware_code or not args.firmware_vars:
            raise ValueError("UEFI requires explicit matching firmware code and variable template paths")
        for path, destination in ((args.firmware_code, "OVMF_CODE.fd"), (args.firmware_vars, "OVMF_VARS.fd")):
            shutil.copyfile(path.resolve(strict=True), run / destination)
    save(run / "metadata.json", metadata)
    print(run, flush=True)


def load(run):
    run = run.resolve(strict=True)
    metadata = json.loads((run / "metadata.json").read_text())
    if not re.fullmatch("02TEST-[0-9a-f]{12}", metadata["serial"]):
        raise ValueError("Invalid disposable-run identity")
    if metadata["name"] != "02os-acceptance-" + metadata["serial"].removeprefix("02TEST-"):
        raise ValueError("Container name does not match the disposable-run identity")
    disk = run / "disk.qcow2"
    if disk.is_symlink() or not disk.is_file():
        raise ValueError("Disposable disk must be a run-local regular file")
    info = json.loads(docker("run", "--rm", "--user", f"{os.getuid()}:{os.getgid()}",
           "--mount", f"type=bind,src={run},dst=/run-test,readonly", "--entrypoint", "/usr/bin/qemu-img",
           metadata["image"], "info", "-U", "--output=json", "/run-test/disk.qcow2"))
    if info.get("backing-filename") or info.get("format") != "qcow2" or info.get("virtual-size") != 32 * 1024**3:
        raise ValueError("Expected standalone 32 GiB qcow2")
    return run, metadata


def start(run, phase):
    run, data = load(run)
    if docker("ps", "--filter", f"name=^{data['name']}$", "-q"):
        raise ValueError("This disposable VM is already running")
    if phase == "disk" and not data["installed"]:
        raise ValueError("Installed-disk boot requires a completed guarded installation")
    for name in ("qmp.sock", "qga.sock"):
        path = run / name
        if path.exists():
            path.unlink()
    qemu = ["/usr/bin/qemu-system-x86_64", "-machine", "q35", "-accel", "kvm", "-cpu", "host",
            "-m", "4096", "-smp", "4", "-device", "qemu-xhci", "-device", "usb-tablet",
            "-drive", f"if=none,id=testdisk,file={run}/disk.qcow2,format=qcow2",
            "-device", f"virtio-blk-pci,drive=testdisk,serial={data['serial']}",
            "-device", "virtio-serial-pci", "-chardev", f"socket,id=qga,path={run}/qga.sock,server=on,wait=off",
            "-device", "virtserialport,chardev=qga,name=org.qemu.guest_agent.0",
            "-qmp", f"unix:{run}/qmp.sock,server=on,wait=off", "-serial", f"file:{run}/{phase}-serial.log",
            "-netdev", "user,id=network", "-device", "virtio-net-pci,netdev=network,romfile=",
            "-audiodev", f"wav,id=audio0,path={run}/{phase}-audio.wav",
            "-device", "ich9-intel-hda", "-device", "hda-duplex,audiodev=audio0", "-no-reboot"]
    devices = ["/dev/kvm"]
    if data["graphics"] == "gl":
        devices.append("/dev/dri/renderD128")
        qemu += ["-device", "virtio-vga-gl", "-display", "egl-headless,rendernode=/dev/dri/renderD128"]
    else:
        qemu += ["-device", "virtio-vga", "-display", "none"]
    if data["firmware"] == "uefi":
        qemu += ["-drive", f"if=pflash,format=raw,readonly=on,file={run}/OVMF_CODE.fd",
                 "-drive", f"if=pflash,format=raw,file={run}/OVMF_VARS.fd"]
    if phase != "disk":
        iso = Path(data["iso"])
        with iso.open("rb") as image:
            if hashlib.file_digest(image, "sha256").hexdigest() != data["iso_sha256"]:
                raise ValueError("ISO bytes changed since preparation")
        qemu += ["-cdrom", str(iso), "-boot", "order=d,menu=on"]
    else:
        qemu += ["-boot", "order=c,menu=on"]
    args = ["run", "--rm", "-d", "--name", data["name"], "--user", f"{os.getuid()}:{os.getgid()}",
            "--mount", f"type=bind,src={run},dst={run}"]
    for address in data.get("dns", []):
        ipaddress.ip_address(address)
        args += ["--dns", address]
    for device in devices:
        args += ["--device", device, "--group-add", str(os.stat(device).st_gid)]
    if phase != "disk":
        args += ["--mount", f"type=bind,src={data['iso']},dst={data['iso']},readonly"]
    args += ["-e", "MESA_SHADER_CACHE_DISABLE=true", data["image"], *qemu]
    data.update(phase=phase, docker_command=args, started_at=time.time())
    data["container_id"] = docker(*args)
    save(run / "metadata.json", data)
    print(json.dumps({"started": data["name"], "phase": phase}), flush=True)


def guest(run, mode, timeout):
    run, data = load(run)
    deadline = time.monotonic() + timeout
    while True:
        try:
            agent = JsonSocket(run / "qga.sock")
            agent.call("guest-ping")
            break
        except (OSError, RuntimeError, ConnectionError):
            if time.monotonic() >= deadline:
                raise TimeoutError("Guest agent did not become available")
            time.sleep(1)
    payload = {"username": "02tester" if data["phase"] == "disk" else "live"}
    if mode == "install":
        if data["phase"] != "live" or data["firmware"] != "uefi":
            raise ValueError("Install requires a freshly booted UEFI live VM")
        credentials = json.loads((ROOT / "scripts/vm-test-credentials.json").read_text())
        password = subprocess.run(["openssl", "passwd", "-6", "-stdin"], input=credentials["password"] + "\n",
                                  text=True, capture_output=True, check=True).stdout.strip()
        payload.update(serial=data["serial"], host_epoch=time.time(),
                       config=json.loads(Path(__file__).with_name("archinstall-test-config.json").read_text()),
                       credentials={"users": [{"username": credentials["username"], "enc_password": password,
                                                 "sudo": credentials["sudo"], "groups": []}]})
    script = Path(__file__).with_name("guest_checks.py").read_bytes()
    pid = agent.call("guest-exec", {"path": "/usr/bin/python3", "arg": ["-", mode,
                      base64.b64encode(json.dumps(payload).encode()).decode()],
                      "input-data": base64.b64encode(script).decode(), "capture-output": True})["pid"]
    while time.monotonic() < deadline:
        if shutil.disk_usage(run).free < 2 * 1024**3:
            docker("stop", "--time", "5", data["name"])
            raise RuntimeError("Stopped disposable VM before exhausting host disk space")
        status = agent.call("guest-exec-status", {"pid": pid})
        if status.get("exited"):
            result = {key: value for key, value in status.items() if key not in ("out-data", "err-data")}
            for key, destination in (("out-data", "stdout"), ("err-data", "stderr")):
                result[destination] = base64.b64decode(status.get(key, "")).decode(errors="replace")
            save(run / f"{data['phase']}-{mode}.json", result)
            agent.close()
            if result.get("exitcode"):
                raise RuntimeError(result)
            if mode == "install":
                data["installed"] = True
                save(run / "metadata.json", data)
            print(json.dumps(result), flush=True)
            return
        time.sleep(1)
    raise TimeoutError("Guest operation exceeded its deadline")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_subparsers(dest="mode", required=True)
    prep = modes.add_parser("prepare")
    prep.add_argument("--iso", type=Path, required=True)
    prep.add_argument("--runs-root", type=Path, required=True)
    prep.add_argument("--runner-image", default=IMAGE)
    prep.add_argument("--dns", action="append", default=[], help="Upstream DNS address for the runner container; repeatable")
    prep.add_argument("--firmware", choices=("uefi", "bios"), default="uefi")
    prep.add_argument("--graphics", choices=("software", "gl"), default="software")
    prep.add_argument("--firmware-code", type=Path)
    prep.add_argument("--firmware-vars", type=Path)
    for mode in ("start", "stop", "snapshot", "install", "capture"):
        part = modes.add_parser(mode)
        part.add_argument("--run", type=Path, required=True)
        if mode == "start":
            part.add_argument("--phase", choices=("live", "disk"), default="live")
        if mode in ("snapshot", "install"):
            part.add_argument("--timeout", type=int, default=300 if mode == "snapshot" else 3000)
    args = parser.parse_args()
    if args.mode == "prepare":
        prepare(args)
    elif args.mode == "start":
        start(args.run, args.phase)
    elif args.mode in ("snapshot", "install"):
        guest(args.run, args.mode, args.timeout)
    elif args.mode == "stop":
        run, data = load(args.run)
        docker("stop", "--time", "10", data["name"])
    elif args.mode == "capture":
        run = args.run.resolve(strict=True)
        qmp = JsonSocket(run / "qmp.sock", qmp=True)
        path = run / f"capture-{time.time_ns()}.ppm"
        qmp.call("screendump", {"filename": str(path)})
        qmp.close()
        print(path)


if __name__ == "__main__":
    main()
