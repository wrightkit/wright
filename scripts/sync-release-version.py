#!/usr/bin/env python3
"""Synchronize Cargo's workspace version and lockfile for a product release."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
VERSION_RE = re.compile(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$")
WORKSPACE_PACKAGE_RE = re.compile(
    r"(?ms)(^\[workspace\.package\]\s*.*?^version\s*=\s*)\"[^\"]+\""
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True, help="Wright product version")
    return parser.parse_args()


def update_workspace_manifest(version: str) -> None:
    manifest = REPO_ROOT / "Cargo.toml"
    contents = manifest.read_text()
    updated, count = WORKSPACE_PACKAGE_RE.subn(r'\1"' + version + '"', contents, count=1)
    if count != 1:
        raise SystemExit("Cargo.toml does not contain one [workspace.package] version")
    manifest.write_text(updated)


def update_lockfile() -> None:
    subprocess.run(["cargo", "update", "--workspace"], cwd=REPO_ROOT, check=True)


def verify_workspace_version(version: str) -> None:
    output = subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
        cwd=REPO_ROOT,
        text=True,
    )
    packages = json.loads(output)["packages"]
    mismatches = [
        f'{package["name"]}={package["version"]}'
        for package in packages
        if package["version"] != version
    ]
    if mismatches:
        raise SystemExit(
            f"workspace package versions do not match {version}: {', '.join(mismatches)}"
        )


def main() -> None:
    args = parse_args()
    if not VERSION_RE.fullmatch(args.version):
        raise SystemExit(f"invalid release version: {args.version}")
    update_workspace_manifest(args.version)
    update_lockfile()
    verify_workspace_version(args.version)
    print(f"synchronized Cargo workspace to {args.version}")


if __name__ == "__main__":
    main()
