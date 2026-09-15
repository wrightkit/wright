#!/usr/bin/env python3
"""Exercise release packaging with synthetic binaries on the host runner."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def main() -> int:
    runner_os = (
        "Windows"
        if os.name == "nt"
        else "macOS" if sys.platform == "darwin" else "Linux"
    )
    extension = "zip" if runner_os == "Windows" else "tar.gz"
    suffix = ".exe" if runner_os == "Windows" else ""
    target = f"packaging-smoke-{runner_os.lower()}"
    version = "0.0.0-packaging-smoke"
    commit = "packaging-smoke-commit"
    build_dir = ROOT / "target" / target / "release"
    build_dir.mkdir(parents=True, exist_ok=True)
    for binary in ("wright", "wright-lsp"):
        (build_dir / f"{binary}{suffix}").write_bytes(
            f"synthetic {binary}\n".encode()
        )

    try:
        with tempfile.TemporaryDirectory(prefix="wright-package-test-") as directory:
            output_dir = Path(directory)
            subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "scripts/package-release.py"),
                    "--version",
                    version,
                    "--target",
                    target,
                    "--extension",
                    extension,
                    "--commit",
                    commit,
                    "--runner-os",
                    runner_os,
                    "--output-dir",
                    str(output_dir),
                ],
                cwd=ROOT,
                check=True,
            )

            payload_name = f"wright-{version}-{target}"
            archive = output_dir / f"{payload_name}.{extension}"
            checksum = archive.with_name(f"{archive.name}.sha256")
            identity_path = archive.with_name(f"{archive.name}.build.json")
            for path in (archive, checksum, identity_path):
                if not path.is_file():
                    raise SystemExit(f"missing package smoke output: {path}")

            identity = json.loads(identity_path.read_text())
            if identity["revision"] != commit or identity["target"] != target:
                raise SystemExit("release package identity does not match smoke input")

            version_member = f"{payload_name}/version.json"
            if extension == "zip":
                with zipfile.ZipFile(archive) as bundle:
                    stamp = json.loads(bundle.read(version_member))
            else:
                with tarfile.open(archive) as bundle:
                    member = bundle.extractfile(version_member)
                    if member is None:
                        raise SystemExit("release package is missing version.json")
                    stamp = json.load(member)

            expected = {
                "version": version,
                "contract": "wright-result/v1",
                "commit": commit,
                "requires": {"node": False, "overpy": False},
            }
            if any(stamp.get(key) != value for key, value in expected.items()):
                raise SystemExit("release version stamp does not match smoke input")
    finally:
        shutil.rmtree(ROOT / "target" / target, ignore_errors=True)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
