#!/usr/bin/env python3
"""Write the machine-readable identity for a CI build artifact."""

from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--runner-os", required=True)
    parser.add_argument("--target")
    parser.add_argument("--toolchain", required=True)
    parser.add_argument("--profile", required=True)
    args = parser.parse_args()

    identity = {
        "revision": args.revision,
        "runner_os": args.runner_os,
        "target": args.target
        or subprocess.check_output(["rustc", "-vV"], text=True).split("host: ", 1)[1].splitlines()[0],
        "toolchain": args.toolchain,
        "profile": args.profile,
        "packages": ["wright-cli", "wright-lsp"],
        "features": [],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(identity, separators=(",", ":")) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
