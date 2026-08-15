#!/usr/bin/env python3
"""Smoke test Wright platform-native npm packages (#121).

Tests clean npm installation, npx CLI execution, compilation/check commands,
programmatic Node.js module resolution, and error handling in an isolated
sandbox environment without requiring source checkout or Rust runtime.
Also validates multi-platform archive packaging and checksum validation.

Usage: python3 scripts/test-npm.py [--binaries-dir target/release]
"""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


def get_workspace_version() -> str:
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


def find_binaries_dir() -> Path:
    candidates = [
        REPO_ROOT / "target" / "release",
        REPO_ROOT / "target" / "debug",
    ]
    for c in candidates:
        if (c / "wright").is_file() or (c / "wright.exe").is_file():
            return c
    raise SystemExit("Could not find wright binary in target/release or target/debug")


def test_multiplatform_packaging(version: str) -> None:
    print("\n--- Testing multi-platform archive packaging & verification ---")
    with tempfile.TemporaryDirectory(prefix="wright-npm-multi-") as tmp:
        tmp_path = Path(tmp)
        art_dir = tmp_path / "artifacts"
        art_dir.mkdir()
        out_dir = tmp_path / "out"
        out_dir.mkdir()

        triples = [
            ("x86_64-unknown-linux-gnu", "tar.gz", False),
            ("aarch64-apple-darwin", "tar.gz", False),
            ("x86_64-apple-darwin", "tar.gz", False),
            ("x86_64-pc-windows-msvc", "zip", True),
        ]

        for triple, ext, is_win in triples:
            stage = tmp_path / f"stage-{triple}"
            payload = stage / f"wright-{version}-{triple}"
            payload.mkdir(parents=True)
            exe = ".exe" if is_win else ""
            (payload / f"wright{exe}").write_text("#!/bin/sh\nexit 0\n")
            (payload / f"wright-lsp{exe}").write_text("#!/bin/sh\nexit 0\n")
            if not is_win:
                (payload / f"wright{exe}").chmod(0o755)
                (payload / f"wright-lsp{exe}").chmod(0o755)
            (payload / "version.json").write_text(
                json.dumps({"version": version, "requires": {"node": False, "overpy": False}}) + "\n"
            )

            arch_file = art_dir / f"wright-{version}-{triple}.{ext}"
            if ext == "zip":
                with zipfile.ZipFile(arch_file, "w") as z:
                    for f in payload.rglob("*"):
                        if f.is_file():
                            z.write(f, f.relative_to(stage))
            else:
                with tarfile.open(arch_file, "w:gz") as t:
                    t.add(payload, arcname=payload.name)

            h = hashlib.sha256(arch_file.read_bytes()).hexdigest()
            (art_dir / f"wright-{version}-{triple}.{ext}.sha256").write_text(f"{h}  {arch_file.name}\n")

        subprocess.run(
            [
                sys.executable,
                str(REPO_ROOT / "scripts" / "package-npm.py"),
                "--version",
                version,
                "--artifacts-dir",
                str(art_dir),
                "--out-dir",
                str(out_dir),
            ],
            check=True,
            capture_output=True,
        )

        pkgs = list(out_dir.glob("*.tgz"))
        if len(pkgs) != 5:
            raise SystemExit(f"Expected 5 npm packages, got {len(pkgs)}")
        print(f"ok: packaged all 5 npm packages from multi-platform release archives")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binaries-dir", type=Path, help="Directory with built wright and wright-lsp")
    parser.add_argument("--version", help="Wright version (default: workspace version)")
    args = parser.parse_args()

    version = args.version or get_workspace_version()
    bin_dir = args.binaries_dir or find_binaries_dir()

    print(f"Running npm distribution smoke tests for Wright v{version}")
    print(f"Using binaries from {bin_dir}")

    with tempfile.TemporaryDirectory(prefix="wright-npm-test-") as tmp:
        tmp_dir = Path(tmp)
        packages_dir = tmp_dir / "packages"
        sandbox_dir = tmp_dir / "sandbox"
        packages_dir.mkdir(parents=True, exist_ok=True)
        sandbox_dir.mkdir(parents=True, exist_ok=True)

        # 1. Package npm tarballs
        print("\n--- Step 1: Packaging npm tarballs ---")
        subprocess.run(
            [
                sys.executable,
                str(REPO_ROOT / "scripts" / "package-npm.py"),
                "--version",
                version,
                "--binaries-dir",
                str(bin_dir),
                "--out-dir",
                str(packages_dir),
            ],
            check=True,
        )

        meta_tarballs = list(packages_dir.glob("wrightkit-wright-*.tgz"))
        if not meta_tarballs:
            meta_tarballs = list(packages_dir.glob("*wright*.tgz"))
        meta_tarball = [tb for tb in meta_tarballs if "darwin" not in tb.name and "linux" not in tb.name and "win32" not in tb.name][0]
        platform_tarball = [tb for tb in packages_dir.glob("*.tgz") if tb != meta_tarball][0]

        print(f"Meta tarball: {meta_tarball.name}")
        print(f"Platform tarball: {platform_tarball.name}")

        # 2. Setup clean sandbox
        print("\n--- Step 2: Clean install in sandbox ---")
        package_json = {
            "name": "test-consumer",
            "version": "1.0.0",
            "private": True,
        }
        (sandbox_dir / "package.json").write_text(json.dumps(package_json, indent=2))

        # Install platform tarball and meta tarball
        subprocess.run(
            ["npm", "install", str(platform_tarball), str(meta_tarball)],
            cwd=sandbox_dir,
            check=True,
            capture_output=True,
        )
        print("npm install completed successfully in isolated sandbox")

        # 3. Test npx / node_modules/.bin execution
        print("\n--- Step 3: Testing CLI entry points ---")

        # Test wright --version
        res = subprocess.run(
            ["npx", "wright", "--version"],
            cwd=sandbox_dir,
            capture_output=True,
            text=True,
            check=True,
        )
        if version not in res.stdout:
            raise SystemExit(f"npx wright --version did not report {version}: {res.stdout}")
        print(f"ok: npx wright --version -> {res.stdout.strip()}")

        # Test wright-lsp --version
        res = subprocess.run(
            ["npx", "wright-lsp", "--version"],
            cwd=sandbox_dir,
            capture_output=True,
            text=True,
            check=True,
        )
        if version not in res.stdout:
            raise SystemExit(f"npx wright-lsp --version did not report {version}: {res.stdout}")
        print(f"ok: npx wright-lsp --version -> {res.stdout.strip()}")

        # 4. Test compiler functionality via npx
        print("\n--- Step 4: Testing compilation and check commands via npx ---")
        fixture_file = REPO_ROOT / "compatibility" / "fixtures" / "synthetic" / "basic-rule" / "source.opy"
        scenario_file = REPO_ROOT / "scenarios" / "loops.opy"

        res = subprocess.run(
            ["npx", "wright", "compile", str(fixture_file), "--profile", "compat"],
            cwd=sandbox_dir,
            capture_output=True,
            text=True,
            check=True,
        )
        if "rule" not in res.stdout:
            raise SystemExit(f"npx wright compile output unexpected: {res.stdout}")
        print("ok: npx wright compile produced Workshop output")

        res = subprocess.run(
            ["npx", "wright", "check", str(scenario_file), "--profile", "compat"],
            cwd=sandbox_dir,
            capture_output=True,
            text=True,
            check=True,
        )
        print("ok: npx wright check succeeded on scenario fixture")

        # 5. Test programmatic Node.js API
        print("\n--- Step 5: Testing programmatic Node.js API ---")
        test_script = """
const assert = require('assert');
const path = require('path');
const fs = require('fs');
const { getBinaryPath, getPlatformPackageName, getPlatformKey, PLATFORMS } = require('@wrightkit/wright');

const key = getPlatformKey();
const pkg = getPlatformPackageName();
console.log('Platform key:', key);
console.log('Platform pkg:', pkg);
assert.ok(pkg, 'Expected valid platform package');

const wrightPath = getBinaryPath('wright');
console.log('Resolved wright path:', wrightPath);
assert.ok(fs.existsSync(wrightPath), 'wright binary must exist');

const lspPath = getBinaryPath('wright-lsp');
console.log('Resolved wright-lsp path:', lspPath);
assert.ok(fs.existsSync(lspPath), 'wright-lsp binary must exist');

console.log('Programmatic API assertions passed');
"""
        (sandbox_dir / "test-api.cjs").write_text(test_script)
        subprocess.run(
            ["node", "test-api.cjs"],
            cwd=sandbox_dir,
            check=True,
        )
        print("ok: programmatic Node.js API works as expected")

        # 6. Test error handling
        print("\n--- Step 6: Testing error handling ---")
        error_test_script = """
const assert = require('assert');
const { getBinaryPath, PLATFORMS } = require('@wrightkit/wright');

// Test asking for non-existent binary
try {
  getBinaryPath('non-existent-binary-12345');
  assert.fail('Expected error for non-existent binary');
} catch (err) {
  assert.ok(err.message.includes('not found') || err.message.includes('incomplete'), 'Expected not found error message');
  console.log('ok: handled missing binary error properly');
}
"""
        (sandbox_dir / "test-error.cjs").write_text(error_test_script)
        subprocess.run(
            ["node", "test-error.cjs"],
            cwd=sandbox_dir,
            check=True,
        )
        print("ok: error handling verified")

    # 7. Test multi-platform archive packaging
    test_multiplatform_packaging(version)

    print("\nAll npm distribution smoke tests passed successfully!")


if __name__ == "__main__":
    main()
