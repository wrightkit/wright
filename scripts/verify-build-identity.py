#!/usr/bin/env python3
"""Verify a downloaded CI build identity and prepare native executables."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--identity", type=Path, required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--runner-os", required=True)
    parser.add_argument("--wright", type=Path, required=True)
    parser.add_argument("--wright-lsp", type=Path, required=True)
    args = parser.parse_args()

    identity = json.loads(args.identity.read_text())
    expected = {
        "revision": args.revision,
        "runner_os": args.runner_os,
        "toolchain": "stable",
        "profile": "dev",
        "packages": ["wright-cli", "wright-lsp"],
    }
    for key, value in expected.items():
        if identity.get(key) != value:
            raise SystemExit(f"build identity mismatch for {key}: {identity.get(key)!r}")

    if args.runner_os != "Windows":
        os.chmod(args.wright, 0o755)
        os.chmod(args.wright_lsp, 0o755)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
