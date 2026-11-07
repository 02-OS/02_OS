#!/usr/bin/env python3
"""Generate the full 02-OS icon pack.

This entry point delegates to ``build_and_install_theme.py``, which writes the
scalable icon theme (and optional user install when ``--install`` or
``--install-user`` is passed).
"""

import os
import runpy
import sys

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
BUILDER = os.path.join(SCRIPT_DIR, "build_and_install_theme.py")

if __name__ == "__main__":
    sys.argv[0] = BUILDER
    runpy.run_path(BUILDER, run_name="__main__")
