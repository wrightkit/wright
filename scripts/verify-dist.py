#!/usr/bin/env python3
"""Validate Wright distribution metadata and the install script (#108, #121).

Runs in CI and locally. It detects version drift and hand-edited metadata by
regenerating every checked-in dist/ manifest with the workspace version and
placeholder hashes and comparing it byte-for-byte to what is committed. It
also validates manifest structure (hash format, artifact URLs), checks that
install.sh covers the declared release target matrix, verifies the shell
syntax of install.sh, and verifies the integrity of npm packages and scripts.

Usage: python3 scripts/verify-dist.py
"""

import importlib.util
import json
import os
import re
import subprocess
import sys
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

    # npm distribution validation (#121)
    meta_json_path = REPO_ROOT / "dist" / "npm" / "wright" / "package.json"
    if not meta_json_path.is_file():
        fail("dist/npm/wright/package.json is missing")
    meta_json = json.loads(meta_json_path.read_text())

    if meta_json.get("name") != "@wrightkit/wright":
        fail("dist/npm/wright/package.json name must be @wrightkit/wright")
    if meta_json.get("version") != version:
        fail(f"dist/npm/wright/package.json version {meta_json.get('version')} does not match {version}")

    meta_opt_deps = meta_json.get("optionalDependencies", {})
    for platform_key, config in gen.NPM_PLATFORM_PACKAGES.items():
        pkg_name = config["name"]
        if pkg_name not in meta_opt_deps:
            fail(f"missing optional dependency {pkg_name} in @wrightkit/wright package.json")
        if meta_opt_deps[pkg_name] != version:
            fail(f"optional dependency {pkg_name} version {meta_opt_deps[pkg_name]} does not match {version}")

        platform_dir = REPO_ROOT / "dist" / "npm" / config["dir_name"]
        pkg_json_file = platform_dir / "package.json"
        if not pkg_json_file.is_file():
            fail(f"{pkg_json_file} is missing")
        pkg_json = json.loads(pkg_json_file.read_text())
        if pkg_json.get("name") != pkg_name:
            fail(f"{pkg_json_file} name mismatch")
        if pkg_json.get("version") != version:
            fail(f"{pkg_json_file} version mismatch")
        if pkg_json.get("os") != config["os"]:
            fail(f"{pkg_json_file} os constraint mismatch")
        if pkg_json.get("cpu") != config["cpu"]:
            fail(f"{pkg_json_file} cpu constraint mismatch")

        readme_file = platform_dir / "README.md"
        if not readme_file.is_file():
            fail(f"{readme_file} is missing")

    for script_file in [
        REPO_ROOT / "dist" / "npm" / "wright" / "bin" / "wright.js",
        REPO_ROOT / "dist" / "npm" / "wright" / "bin" / "wright-lsp.js",
        REPO_ROOT / "dist" / "npm" / "wright" / "index.js",
    ]:
        if not script_file.is_file():
            fail(f"{script_file} is missing")
        subprocess.run(["node", "-c", str(script_file)], check=True)

    dts_file = REPO_ROOT / "dist" / "npm" / "wright" / "index.d.ts"
    if not dts_file.is_file() or "getBinaryPath" not in dts_file.read_text():
        fail("dist/npm/wright/index.d.ts is missing or does not export getBinaryPath")

    print("ok: npm packages and wrapper scripts valid")

    # Detailed tarball-mode/determinism tests are distribution regressions,
    # not language compatibility tests. Run them once in the Linux matrix;
    # the normal dist smoke below still runs on every supported CI platform.
    if sys.platform.startswith("linux"):
        subprocess.run(
            [
                sys.executable,
                "-m",
                "unittest",
                "discover",
                "-s",
                str(REPO_ROOT / "scripts" / "tests"),
            ],
            cwd=REPO_ROOT,
            check=True,
        )
        print("ok: distribution helper regression tests passed")

    print("dist validation passed")


if __name__ == "__main__":
    main()
