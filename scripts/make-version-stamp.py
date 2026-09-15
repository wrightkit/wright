#!/usr/bin/env python3
"""Write the authoritative Wright release version stamp."""

from __future__ import annotations

import argparse
import json
import subprocess
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def git_head() -> str:
    return subprocess.check_output(
        ["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True
    ).strip()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    built = (
        datetime.now(timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
    )
    stamp = {
        "version": args.version,
        "contract": "wright-result/v1",
        "commit": git_head(),
        "built": built,
        "requires": {"node": False, "overpy": False},
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(stamp, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
