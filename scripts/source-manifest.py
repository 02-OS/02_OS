#!/usr/bin/env python3
"""Identify the actual tracked and nonignored source inputs of a build."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args])


def snapshot(root, require_clean=False):
    revision = git(root, "rev-parse", "HEAD").decode().strip()
    if git(root, "ls-files", "--unmerged", "-z"):
        raise ValueError("Resolve unmerged source files before building")
    dirty = bool(git(root, "status", "--porcelain=v1", "--untracked-files=normal"))
    if require_clean and dirty:
        raise ValueError("Release requires a clean source checkout, including untracked files")
    names = sorted(set(git(root, "ls-files", "--cached", "--others", "--exclude-standard", "-z").split(b"\0")) - {b""})
    records = []
    for name in names:
        relative = os.fsdecode(name)
        path = root / relative
        try:
            metadata = path.lstat()
        except FileNotFoundError:
            records.append({"path": relative, "type": "missing"})
            continue
        record = {"path": relative, "mode": stat.S_IMODE(metadata.st_mode)}
        if stat.S_ISLNK(metadata.st_mode):
            record.update(type="symlink", target=os.readlink(path))
        elif stat.S_ISREG(metadata.st_mode):
            with path.open("rb") as source:
                before = os.fstat(source.fileno())
                digest = hashlib.file_digest(source, "sha256").hexdigest()
                after = os.fstat(source.fileno())
            if any(getattr(before, key) != getattr(after, key) for key in
                   ("st_ino", "st_dev", "st_size", "st_mtime_ns", "st_ctime_ns")):
                raise ValueError(f"Source changed while hashing: {relative}")
            record.update(type="file", sha256=digest, size_bytes=after.st_size)
        else:
            raise ValueError(f"Unsupported source file: {relative}")
        records.append(record)
    identity = hashlib.sha256(json.dumps(records, sort_keys=True, ensure_ascii=True,
                                         separators=(",", ":")).encode()).hexdigest()
    return {"revision": revision, "dirty": dirty, "source_sha256": identity, "files": records}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--require-clean", action="store_true")
    args = parser.parse_args()
    try:
        text = json.dumps(snapshot(args.root.resolve(), args.require_clean), indent=2) + "\n"
        if args.output:
            args.output.write_text(text)
        else:
            print(text, end="")
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"source-manifest: {error}\n")


if __name__ == "__main__":
    main()
