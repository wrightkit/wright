#!/usr/bin/env python3
"""Append the debug CLI artifact's revision and digest to the CI summary."""

from __future__ import annotations

import hashlib
import os
from pathlib import Path


def main() -> int:
    binary = Path("target/debug/wright")
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    summary = Path(os.environ["GITHUB_STEP_SUMMARY"])
    with summary.open("a") as output:
        output.write(
            "## Wright CLI artifact\n\n"
            f"- Revision: `{os.environ['GITHUB_SHA']}`\n"
            "- Build identity: stable / debug / ubuntu-latest\n"
            f"- SHA-256: `{digest}`\n"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
