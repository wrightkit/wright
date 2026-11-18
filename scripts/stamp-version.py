#!/usr/bin/env python3
"""Stamp the release version into the checkout.

The checked-in version is a placeholder. A release job stamps the version
taken from the release tag into `Cargo.toml`, `Cargo.lock` (workspace packages
only), and `version.txt` before it builds. The stamped files are never committed.

    python3 scripts/stamp-version.py 1.2.3
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SEMVER = r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"


def replace_once(text: str, pattern: str, replacement: str, label: str) -> str:
    result, count = re.subn(pattern, replacement, text, count=1, flags=re.MULTILINE)
    if count != 1:
        sys.exit(f"{label}: version line not found")
    return result


def main() -> None:
    if len(sys.argv) != 2 or not re.fullmatch(SEMVER, sys.argv[1]):
        sys.exit("usage: stamp-version.py MAJOR.MINOR.PATCH")
    version = sys.argv[1]

    manifest = ROOT / "Cargo.toml"
    text = manifest.read_text()
    section = re.search(r"^\[workspace\.package\]\n(?:(?!\[).*\n)*", text, re.MULTILINE)
    if section is None:
        sys.exit("Cargo.toml: [workspace.package] not found")
    stamped = replace_once(
        section.group(0), r'^version = ".*"$', f'version = "{version}"', "Cargo.toml"
    )
    manifest.write_text(text.replace(section.group(0), stamped, 1))

    # A workspace package is a lockfile entry without `source`.
    lock = ROOT / "Cargo.lock"
    stamped_lock = re.sub(
        r'(\[\[package\]\]\nname = "[^"]+"\n)version = "[^"]+"\n(?!source = )',
        lambda m: f'{m.group(1)}version = "{version}"\n',
        lock.read_text(),
    )
    lock.write_text(stamped_lock)

    (ROOT / "version.txt").write_text(f"{version}\n")


if __name__ == "__main__":
    main()
