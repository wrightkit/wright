#!/usr/bin/env python3
"""Extract and run the native smoke contract against a release archive."""

from __future__ import annotations

import argparse
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--extension", choices=("tar.gz", "zip"), required=True)
    args = parser.parse_args()
    version = args.version.removeprefix("v")

    with tempfile.TemporaryDirectory(prefix="wright-release-smoke-") as directory:
        extract = Path(directory)
        if args.extension == "zip":
            with zipfile.ZipFile(args.archive) as archive:
                archive.extractall(extract)
        else:
            with tarfile.open(args.archive) as archive:
                archive.extractall(extract)
        suffix = ".exe" if args.target == "x86_64-pc-windows-msvc" else ""
        payload = extract / f"wright-{version}-{args.target}"
        wright = payload / f"wright{suffix}"
        lsp = payload / f"wright-lsp{suffix}"
        for path in (wright, lsp):
            if not path.is_file():
                raise SystemExit(f"missing packaged binary: {path}")
        subprocess.run(
            [
                sys.executable,
                str(ROOT / "scripts/smoke-native.py"),
                "--wright",
                str(wright),
                "--wright-lsp",
                str(lsp),
                "--version",
                version,
                "--provider-bootstrap",
            ],
            cwd=ROOT,
            check=True,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
