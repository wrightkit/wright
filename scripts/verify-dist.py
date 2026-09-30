#!/usr/bin/env python3
"""Validate Wright distribution metadata and release packaging (#108, #338).

Runs in CI and locally. It checks that install.sh covers the declared release
target matrix, verifies the shell syntax of install.sh where Bash is native,
and exercises release packaging on the current host with synthetic binaries.

Usage: python3 scripts/verify-dist.py
"""

import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


def verify_workspace_packages_are_private() -> None:
    metadata = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    public = [
        package["name"]
        for package in json.loads(metadata)["packages"]
        if package.get("publish") != []
    ]
    if public:
        fail(
            "workspace packages must set publish = false; "
            f"public packages: {', '.join(sorted(public))}"
        )
    print("ok: workspace packages explicitly non-publishable")


def load_generator():
    spec = importlib.util.spec_from_file_location(
        "wright_dist", REPO_ROOT / "scripts" / "update-dist-manifests.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def fail(message: str) -> None:
    raise SystemExit(f"dist validation failed: {message}")


def main() -> None:
    gen = load_generator()
    verify_workspace_packages_are_private()

    install_sh = (REPO_ROOT / "install.sh").read_text()
    unix_triples = {t for k, t in gen.TARGETS.items() if k != "windows-x64"}
    for triple in sorted(unix_triples):
        if triple not in install_sh:
            fail(f"install.sh does not cover target triple {triple}")
    if gen.TARGETS["windows-x64"] in install_sh:
        fail("install.sh is a Unix-only installer and must not claim the Windows triple")
    print("ok: install.sh covers the declared Unix target matrix")

    if os.name != "nt":
        subprocess.run(["bash", "-n", str(REPO_ROOT / "install.sh")], check=True)
        print("ok: install.sh shell syntax valid")
    else:
        # install.sh is a Unix-only installer (checked above); on Windows the
        # `bash` on PATH is the WSL launcher, which fails without a distro.
        print("skip: install.sh shell syntax check (Unix-only, no real bash on Windows)")

    subprocess.run(
        [sys.executable, str(REPO_ROOT / "scripts" / "test-package-release.py")],
        cwd=REPO_ROOT,
        check=True,
    )
    print("ok: release packaging smoke passed on this host")
    print("dist validation passed")


if __name__ == "__main__":
    main()
