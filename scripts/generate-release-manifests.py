#!/usr/bin/env python3
"""Generate release package-manager manifests from native checksums."""

from __future__ import annotations

import argparse
import importlib.util
import shutil
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "linux-x64": ("x86_64-unknown-linux-gnu", "tar.gz"),
    "darwin-arm64": ("aarch64-apple-darwin", "tar.gz"),
    "darwin-x64": ("x86_64-apple-darwin", "tar.gz"),
    "windows-x64": ("x86_64-pc-windows-msvc", "zip"),
}


def load_generator():
    spec = importlib.util.spec_from_file_location(
        "wright_dist", ROOT / "scripts/update-dist-manifests.py"
    )
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load scripts/update-dist-manifests.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--artifacts-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--attach-dir", type=Path, required=True)
    args = parser.parse_args()
    version = args.version.removeprefix("v")

    hashes = {}
    for key, (target, extension) in TARGETS.items():
        checksum = args.artifacts_dir / f"wright-{version}-{target}.{extension}.sha256"
        if not checksum.is_file():
            raise SystemExit(f"missing {checksum}")
        value = checksum.read_text().split()[0]
        if len(value) != 64 or any(char not in "0123456789abcdef" for char in value):
            raise SystemExit(f"invalid hash in {checksum}")
        hashes[key] = value

    written = load_generator().generate(version, hashes, args.output_dir)
    args.attach_dir.mkdir(parents=True, exist_ok=True)
    shutil.copy2(
        args.output_dir / "dist/homebrew/wright.rb",
        args.attach_dir / f"wright-{version}.homebrew.rb",
    )
    shutil.copy2(
        args.output_dir / "dist/scoop/wright.json",
        args.attach_dir / f"wright-{version}.scoop.json",
    )
    winget_root = args.output_dir / "dist/winget/manifests"
    with zipfile.ZipFile(
        args.attach_dir / f"wright-{version}.winget.zip", "w", zipfile.ZIP_DEFLATED
    ) as output:
        for path in winget_root.rglob("*"):
            if path.is_file():
                output.write(path, path.relative_to(args.output_dir / "dist/winget"))
    print(f"generated {len(written)} manifests for {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
