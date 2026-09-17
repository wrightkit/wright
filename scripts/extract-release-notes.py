#!/usr/bin/env python3
"""Extract one Release Please changelog section for a GitHub Release body."""

from __future__ import annotations

import argparse
import re
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True, help="release tag, such as v0.2.32")
    parser.add_argument("--changelog", type=Path, default=Path("CHANGELOG.md"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    version = args.tag.removeprefix("v")
    heading = re.compile(rf"^## \[{re.escape(version)}\](?:\(|$)")
    lines = args.changelog.read_text(encoding="utf-8").splitlines(keepends=True)

    start = next((index for index, line in enumerate(lines) if heading.match(line)), None)
    if start is None:
        raise SystemExit(f"CHANGELOG.md has no section for release {args.tag}")

    end = next(
        (
            index
            for index in range(start + 1, len(lines))
            if lines[index].startswith("## ")
        ),
        len(lines),
    )
    section = "".join(lines[start:end]).rstrip() + "\n"
    args.output.write_text(section, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
