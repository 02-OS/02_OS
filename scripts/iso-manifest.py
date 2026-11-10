#!/usr/bin/env python3
"""Publish/verify a completed ISO's revision and content, using only stdlib."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import sys
import tempfile
import importlib.util


def source_snapshot(root):
    spec = importlib.util.spec_from_file_location("build_source_manifest", Path(__file__).with_name("source-manifest.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.snapshot(root, require_clean=True)


def image_identity(path):
    with path.open("rb") as image:
        before = os.fstat(image.fileno())
        if not stat.S_ISREG(before.st_mode) or before.st_size == 0:
            raise ValueError("ISO must be a nonempty regular file")
        digest = hashlib.file_digest(image, "sha256").hexdigest()
        after = os.fstat(image.fileno())
    fields = ("st_ino", "st_dev", "st_size", "st_mtime_ns", "st_ctime_ns")
    if any(getattr(before, field) != getattr(after, field) for field in fields):
        raise ValueError("ISO changed while calculating its digest")
    return {"size_bytes": after.st_size, "sha256": digest}


def check_published_sha256(iso, digest):
    """Match a sha256sum sidecar when release wrote one. Absence is not a failure."""
    sidecar = iso.parent / "02_OS.sha256"
    if not sidecar.exists():
        return
    info = sidecar.lstat()
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISREG(info.st_mode):
        raise ValueError("02_OS.sha256 must be a regular file")
    lines = [line for line in sidecar.read_text().splitlines() if line.strip()]
    if len(lines) != 1:
        raise ValueError("02_OS.sha256 must contain one sha256sum line")
    parts = lines[0].split()
    if len(parts) != 2 or parts[0] != digest or parts[1] not in (iso.name, "./" + iso.name):
        raise ValueError("02_OS.sha256 does not match the ISO")


def atomic_write(path, text):
    fd, temporary = tempfile.mkstemp(prefix=path.name + ".", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as output:
            output.write(text)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("create", "verify"))
    parser.add_argument("iso", type=Path)
    parser.add_argument("revision")
    parser.add_argument("--source-manifest", type=Path)
    parser.add_argument("--source-root", type=Path)
    parser.add_argument("--packages", type=Path)
    args = parser.parse_args()
    manifest_path = Path(str(args.iso) + ".manifest.json")
    commit_path = Path(str(args.iso) + ".commit")
    try:
        if args.action == "create":
            manifest = {"schema_version": 2, "revision": args.revision,
                        **image_identity(args.iso)}
            if args.source_manifest:
                source = json.loads(args.source_manifest.read_text())
                if source["revision"] != args.revision:
                    raise ValueError("source snapshot revision differs from ISO revision")
                manifest["source"] = {key: source[key] for key in ("revision", "dirty", "source_sha256")}
            if args.packages:
                manifest["packages"] = args.packages.read_text().splitlines()
                if not manifest["packages"]:
                    raise ValueError("package inventory is empty")
            # The manifest is the completion boundary; a failed build never
            # publishes it. Keep the old revision sidecar for existing tools.
            atomic_write(commit_path, args.revision + "\n")
            atomic_write(manifest_path, json.dumps(manifest, indent=2) + "\n")
        else:
            if not commit_path.is_file() or commit_path.read_text().strip() != args.revision:
                raise ValueError(f"{commit_path} does not match HEAD {args.revision}")
            if not manifest_path.is_file():
                raise ValueError("ISO has no completion manifest; rebuild before release")
            manifest = json.loads(manifest_path.read_text())
            if not isinstance(manifest, dict) or manifest.get("schema_version") != 2:
                raise ValueError("invalid ISO completion manifest")
            if manifest.get("revision") != args.revision:
                raise ValueError("ISO manifest revision does not match HEAD")
            if args.source_root:
                current = source_snapshot(args.source_root)
                source = manifest.get("source", {})
                if source.get("dirty") is not False or source.get("source_sha256") != current["source_sha256"]:
                    raise ValueError("ISO source snapshot does not match the clean release checkout; rebuild")
            actual = image_identity(args.iso)
            if any(manifest.get(key) != value for key, value in actual.items()):
                raise ValueError("ISO content does not match its completion manifest (size/SHA-256)")
            check_published_sha256(args.iso, actual["sha256"])
    except (OSError, ValueError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
