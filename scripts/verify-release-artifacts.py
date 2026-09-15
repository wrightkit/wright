#!/usr/bin/env python3
"""Verify and consolidate the complete native release artifact set."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
from pathlib import Path


TARGETS = {
    "x86_64-unknown-linux-gnu": "tar.gz",
    "x86_64-apple-darwin": "tar.gz",
    "aarch64-apple-darwin": "tar.gz",
    "x86_64-pc-windows-msvc": "zip",
}


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--artifacts-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    version = args.version.removeprefix("v")

    args.output_dir.mkdir(parents=True, exist_ok=True)
    archives = []
    for target, extension in TARGETS.items():
        archive = args.artifacts_dir / f"wright-{version}-{target}.{extension}"
        checksum = archive.with_name(f"{archive.name}.sha256")
        identity_path = archive.with_name(f"{archive.name}.build.json")
        for path in (archive, checksum, identity_path):
            if not path.is_file():
                raise SystemExit(f"missing {path}")
        expected_hash = checksum.read_text().split()[0]
        if expected_hash != digest(archive):
            raise SystemExit(f"checksum mismatch for {archive.name}")
        identity = json.loads(identity_path.read_text())
        expected = {
            "revision": args.commit,
            "tag": args.tag,
            "target": target,
            "toolchain": "stable",
            "profile": "release",
            "packages": ["wright-cli", "wright-lsp"],
            "features": [],
        }
        if any(identity.get(key) != value for key, value in expected.items()):
            raise SystemExit(f"release build identity mismatch for {target}")
        shutil.copy2(archive, args.output_dir / archive.name)
        shutil.copy2(checksum, args.output_dir / checksum.name)
        archives.append(args.output_dir / archive.name)

    with (args.output_dir / "SHA256SUMS").open("w") as output:
        for archive in sorted(archives):
            output.write(f"{digest(archive)}  {archive.name}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
