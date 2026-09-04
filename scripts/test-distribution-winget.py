#!/usr/bin/env python3
"""Exercise the native WinGet channel against a local release artifact."""

from __future__ import annotations

import os
import platform
import subprocess
from pathlib import Path

from distribution_test_support import ROOT, ReleaseFixture, fail, native_smoke, run


CHANNEL = "WinGet"


def target_info() -> str:
    if platform.system() != "Windows" or platform.machine().lower() not in {"x86_64", "amd64"}:
        fail(CHANNEL, f"unsupported validation host {platform.system()}/{platform.machine()}")
    return "x86_64-pc-windows-msvc"


def installed_binary(name: str) -> Path:
    roots = []
    for variable in ("LOCALAPPDATA", "ProgramFiles", "ProgramW6432"):
        value = os.environ.get(variable)
        if value:
            roots.extend(
                [
                    Path(value) / "Microsoft" / "WinGet" / "Packages",
                    Path(value) / "Microsoft" / "WinGet" / "Links",
                    Path(value) / "WinGet" / "Packages",
                    Path(value) / "WinGet" / "Links",
                ]
            )
    if not roots:
        fail(CHANNEL, "WinGet package roots are not available")
    candidates = [path for root in roots if root.exists() for path in root.rglob(name)]
    if not candidates:
        fail(CHANNEL, f"installed package does not expose {name}")
    return candidates[0]


def main() -> None:
    version = (ROOT / "version.txt").read_text().strip()
    with ReleaseFixture(CHANNEL, version, target_info(), "zip", ".exe") as fixture:
        manifest_dir = fixture.metadata / "dist" / "winget" / "manifests" / "w" / "WrightKit" / "Wright" / version
        try:
            run(CHANNEL, "validate generated local manifests", ["winget", "validate", "--manifest", str(manifest_dir)])
            run(
                CHANNEL,
                "install from generated local manifests",
                [
                    "winget",
                    "install",
                    "--manifest",
                    str(manifest_dir),
                    "--accept-source-agreements",
                    "--accept-package-agreements",
                    "--disable-interactivity",
                ],
            )
            native_smoke(CHANNEL, installed_binary("wright.exe"), installed_binary("wright-lsp.exe"), version)
        finally:
            subprocess.run(
                ["winget", "uninstall", "--id", "WrightKit.Wright", "--silent", "--disable-interactivity"],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
            )
    print(f"{CHANNEL} distribution validation passed")


if __name__ == "__main__":
    main()
