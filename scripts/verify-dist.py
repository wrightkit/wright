#!/usr/bin/env python3
"""Validate Wright distribution metadata and the install script (#108).

Runs in CI and locally. It detects version drift and hand-edited metadata by
regenerating every checked-in dist/ manifest with the workspace version and
placeholder hashes and comparing it byte-for-byte to what is committed. It
also validates manifest structure (hash format, artifact URLs), checks that
install.sh covers the declared release target matrix, and verifies the shell
syntax of install.sh.

Usage: python3 scripts/verify-dist.py
"""

import importlib.util
import json
import os
import re
import subprocess
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


def workspace_version() -> str:
    metadata = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    for package in json.loads(metadata)["packages"]:
        if package["name"] == "wright-cli":
            return package["version"]
    raise SystemExit("wright-cli not found in workspace metadata")


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
    version = workspace_version()
    print(f"workspace version: {version}")
    verify_workspace_packages_are_private()

    with tempfile.TemporaryDirectory() as tmp:
        generated = gen.generate(version, {}, Path(tmp))
        for rel in generated:
            path = REPO_ROOT / rel
            if not path.is_file():
                fail(f"{rel} is missing; run python3 scripts/update-dist-manifests.py --version {version}")
            expected = (Path(tmp) / rel).read_text()
            actual = path.read_text()
            if actual != expected:
                fail(
                    f"{rel} is out of sync with the workspace version {version}; "
                    f"regenerate it with python3 scripts/update-dist-manifests.py --version {version}"
                )
            print(f"ok: {rel} matches generated metadata")

    hashes = set()
    for text in [
        (REPO_ROOT / "dist" / "homebrew" / "wright.rb").read_text(),
        (REPO_ROOT / "dist" / "scoop" / "wright.json").read_text(),
    ] + [
        p.read_text()
        for p in (REPO_ROOT / "dist" / "winget" / "manifests").rglob("*.yaml")
    ]:
        hashes.update(re.findall(r"[0-9a-fA-F]{64}", text))
    for value in sorted(hashes):
        if value != "0" * 64 and len(set(value)) == 1:
            fail(f"suspicious placeholder-like hash {value}")

    for text, what in [
        ((REPO_ROOT / "dist" / "homebrew" / "wright.rb").read_text(), "homebrew formula"),
        ((REPO_ROOT / "dist" / "scoop" / "wright.json").read_text(), "scoop manifest"),
    ]:
        if f"v{version}" not in text or f"wright-{version}" not in text:
            fail(f"{what} does not reference version {version}")

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

    print("dist validation passed")


if __name__ == "__main__":
    main()
