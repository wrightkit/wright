#!/usr/bin/env python3
"""Package one cross-platform Wright release build and its identity."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
import tarfile
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--tag")
    parser.add_argument("--target", required=True)
    parser.add_argument("--extension", choices=("tar.gz", "zip"), required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--runner-os", required=True)
    parser.add_argument(
        "--features",
        default="",
        help="Cargo feature spec the binaries were built with, recorded verbatim",
    )
    parser.add_argument("--output-dir", type=Path, default=Path("dist"))
    args = parser.parse_args()
    version = args.version.removeprefix("v")
    if not re.fullmatch(
        r"[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?", version
    ):
        parser.error(
            f"--version must be MAJOR.MINOR.PATCH(-prerelease)(+build), got {args.version!r}"
        )
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", args.target) or ".." in args.target:
        parser.error(f"--target must be a platform triple name, got {args.target!r}")

    output_dir = args.output_dir if args.output_dir.is_absolute() else ROOT / args.output_dir
    payload_name = f"wright-{version}-{args.target}"
    payload = output_dir / payload_name
    if payload.exists():
        shutil.rmtree(payload)
    payload.mkdir(parents=True)

    subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts/make-version-stamp.py"),
            version,
            str(payload / "version.json"),
            "--commit",
            args.commit,
        ],
        cwd=ROOT,
        check=True,
    )
    suffix = ".exe" if args.runner_os == "Windows" else ""
    for binary in ("wright", "wright-lsp"):
        source = ROOT / "target" / args.target / "release" / f"{binary}{suffix}"
        if not source.is_file():
            raise SystemExit(f"release binary missing: {source}")
        shutil.copy2(source, payload / source.name)

    archive = output_dir / f"{payload_name}.{args.extension}"
    if archive.exists():
        archive.unlink()
    if args.extension == "zip":
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
            for path in payload.iterdir():
                output.write(path, f"{payload_name}/{path.name}")
    else:
        with tarfile.open(archive, "w:gz") as output:
            output.add(payload, arcname=payload_name)

    checksum = archive.with_name(f"{archive.name}.sha256")
    checksum.write_text(f"{digest(archive)}  {archive.name}\n")
    toolchain_toml = (ROOT / "rust-toolchain.toml").read_text()
    channel = re.search(r'(?m)^channel\s*=\s*"([^"]+)"', toolchain_toml)
    if channel is None:
        raise SystemExit("rust-toolchain.toml has no channel pin to record")
    identity = {
        "revision": args.commit,
        "tag": args.tag or f"v{version}",
        "target": args.target,
        "toolchain": channel.group(1),
        "profile": "release",
        "packages": ["wright-cli", "wright-lsp"],
        "features": args.features.split(),
    }
    archive.with_name(f"{archive.name}.build.json").write_text(
        json.dumps(identity, separators=(",", ":")) + "\n"
    )
    print(archive)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
