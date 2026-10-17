#!/usr/bin/env python3
"""Copy staged ISO inputs into an owned work directory, excluding host bytecode.

prepare-work-dir.sh must have prepared WORK_DIR first. The fixed profile child
is never replaced, and mkarchiso must use a separate child for its work tree.
Symlinks are preserved without following their targets.
"""

import argparse
import os
from pathlib import Path
import shutil
import sys
import tempfile


def prepare_profile(source, work_dir):
    source = Path(os.path.abspath(source))
    work_dir = Path(os.path.abspath(work_dir))
    for path, label in ((source, "Source profile"), (work_dir, "Work directory")):
        if path.is_symlink() or not path.is_dir() or path.resolve() != path:
            raise ValueError(f"{label} must be an existing directory without symlink parents: {path}")
    if source == work_dir or source in work_dir.parents or work_dir in source.parents:
        raise ValueError("Source profile and work directory must not overlap")
    marker = Path(str(work_dir) + ".02os-marker")
    if (marker.is_symlink() or not marker.is_file()
            or marker.read_text() != f"02os-work-dir\n{work_dir}\n"):
        raise ValueError("Work directory needs its matching regular 02os-marker")
    destination = work_dir / "profile"
    if destination.exists() or destination.is_symlink():
        raise ValueError(f"Refusing to replace an existing private profile: {destination}")

    # build.sh writes BUILD_STAMP here after copying. Reject symlink ancestors
    # and a symlink stamp so that write cannot escape the private profile.
    provenance = source
    for component in ("airootfs", "usr", "lib", "02-agent"):
        provenance /= component
        if provenance.is_symlink() or not provenance.is_dir():
            raise ValueError(f"Staged provenance path must contain only regular directories: {provenance}")
    stamp = provenance / "BUILD_STAMP"
    if stamp.is_symlink() or (stamp.exists() and not stamp.is_file()):
        raise ValueError("BUILD_STAMP must not be a symlink or special file")

    temporary = Path(tempfile.mkdtemp(prefix=".02os-profile-", dir=work_dir))
    try:
        shutil.copytree(source, temporary, symlinks=True, dirs_exist_ok=True,
                        ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "*.pyo"))
        # Reserve the final path atomically; rename must never replace an
        # existing directory created by another preparation attempt.
        destination.mkdir()
        try:
            temporary.rename(destination)
        except OSError:
            destination.rmdir()
            raise
    finally:
        if temporary.exists():
            # Copied profile directories may be read-only. Only this freshly
            # allocated tree is reclaimed, and os.walk never follows symlinks.
            temporary.chmod(temporary.stat().st_mode | 0o700)
            for root, directories, _ in os.walk(temporary, followlinks=False):
                for name in directories:
                    child = Path(root) / name
                    if not child.is_symlink():
                        child.chmod(child.stat().st_mode | 0o700)
            shutil.rmtree(temporary)
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="Verified, staged source profile")
    parser.add_argument("work_dir", type=Path, help="Work directory prepared by prepare-work-dir.sh")
    args = parser.parse_args()
    try:
        print(prepare_profile(args.source, args.work_dir))
    except (OSError, ValueError, shutil.Error) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
