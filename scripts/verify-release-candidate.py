#!/usr/bin/env python3
"""Identify a release-please commit after its exact CI run succeeds."""

from __future__ import annotations

import argparse
import re
import subprocess
from pathlib import Path


VERSION_RE = r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?"
RELEASE_SUBJECT = re.compile(
    rf"^chore\(main\): release (?P<version>{VERSION_RE})(?: \(#[0-9]+\))?$"
)


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], text=True).strip()


def write_output(path: Path, values: dict[str, str]) -> None:
    with path.open("a") as output:
        for key, value in values.items():
            output.write(f"{key}={value}\n")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    head = git("rev-parse", "HEAD")
    if head != args.commit:
        raise SystemExit(f"release candidate checkout is {head}, expected {args.commit}")

    subject = git("log", "-1", "--format=%s")
    match = RELEASE_SUBJECT.fullmatch(subject)
    if match is None:
        write_output(args.output, {"release": "false"})
        print(f"commit {args.commit} is not a release-please commit")
        return 0

    version = match.group("version")
    version_file = Path("version.txt").read_text().strip()
    if version_file != version:
        raise SystemExit(
            f"release commit subject announces {version}, but version.txt contains {version_file}"
        )

    tag = f"v{version}"
    write_output(
        args.output,
        {"release": "true", "commit": args.commit, "tag": tag, "version": version},
    )
    print(f"release candidate {tag} is ready from {args.commit}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
