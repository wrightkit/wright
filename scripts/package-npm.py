#!/usr/bin/env python3
"""Package Wright for npm distribution (#121).

Assembles the @wrightkit/wright meta package and platform-specific native binary
packages (@wrightkit/wright-darwin-arm64, etc.) from canonical release archives
or pre-built binaries, verifies their contents and permissions, and creates npm
.tgz tarballs ready for publishing or installation.

Usage:
  # From canonical release archives (release workflow / CI):
  python3 scripts/package-npm.py --version 0.1.0 --artifacts-dir artifacts --out-dir dist/npm-packages

  # From local binaries (development / smoke testing):
  python3 scripts/package-npm.py --version 0.1.0 --binaries-dir target/release --out-dir dist/npm-packages
"""

import argparse
import hashlib
import importlib.util
import json
import os
import platform
import shutil
import subprocess
import tarfile
import tempfile
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# npm is a .cmd shim on Windows; subprocess needs the explicit extension
# because CreateProcess does not apply PATHEXT.
NPM = "npm.cmd" if os.name == "nt" else "npm"


def load_update_dist_manifests():
    spec = importlib.util.spec_from_file_location(
        "wright_dist", REPO_ROOT / "scripts" / "update-dist-manifests.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


dist_module = load_update_dist_manifests()
NPM_PLATFORM_PACKAGES = dist_module.NPM_PLATFORM_PACKAGES
TARGETS = dist_module.TARGETS
ARCHIVE_EXT = dist_module.ARCHIVE_EXT
version_from_tag = dist_module.version_from_tag


def detect_host_target() -> str:
    system = platform.system()
    machine = platform.machine().lower()
    if system == "Darwin":
        if machine in ("arm64", "aarch64"):
            return "darwin-arm64"
        elif machine in ("x86_64", "amd64"):
            return "darwin-x64"
    elif system == "Linux":
        if machine in ("x86_64", "amd64"):
            return "linux-x64"
    elif system == "Windows":
        if machine in ("x86_64", "amd64"):
            return "windows-x64"
    raise SystemExit(f"Unsupported host platform/architecture: {system} {machine}")


def calculate_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()


def find_archive(artifacts_dir: Path, version: str, triple: str, ext: str) -> Path | None:
    expected_name = f"wright-{version}-{triple}.{ext}"
    direct = artifacts_dir / expected_name
    if direct.is_file():
        return direct
    # Check subdirectories (e.g. artifacts/wright-<triple>/...)
    for candidate in artifacts_dir.rglob(expected_name):
        if candidate.is_file():
            return candidate
    return None


def extract_archive(archive_path: Path, ext: str, dest_dir: Path) -> None:
    if ext == "zip":
        with zipfile.ZipFile(archive_path, "r") as z:
            z.extractall(dest_dir)
    else:
        with tarfile.open(archive_path, "r:*") as t:
            t.extractall(dest_dir)


def stage_platform_package(
    platform_key: str,
    config: dict,
    version: str,
    bin_dir: Path,
    staging_root: Path,
) -> Path:
    pkg_staging = staging_root / config["dir_name"]
    pkg_staging.mkdir(parents=True, exist_ok=True)

    # Copy binary files
    is_windows = "win32" in config["os"]
    exe_suffix = ".exe" if is_windows else ""

    wright_bin = bin_dir / f"wright{exe_suffix}"
    wright_lsp_bin = bin_dir / f"wright-lsp{exe_suffix}"
    version_json = bin_dir / "version.json"

    if not wright_bin.is_file():
        raise SystemExit(f"Missing wright binary at {wright_bin}")
    if not wright_lsp_bin.is_file():
        raise SystemExit(f"Missing wright-lsp binary at {wright_lsp_bin}")

    dest_wright = pkg_staging / f"wright{exe_suffix}"
    dest_wright_lsp = pkg_staging / f"wright-lsp{exe_suffix}"
    dest_version_json = pkg_staging / "version.json"

    shutil.copy2(wright_bin, dest_wright)
    shutil.copy2(wright_lsp_bin, dest_wright_lsp)

    if not is_windows:
        os.chmod(dest_wright, 0o755)
        os.chmod(dest_wright_lsp, 0o755)

    if version_json.is_file():
        shutil.copy2(version_json, dest_version_json)
        # Verify version inside version.json
        try:
            vdata = json.loads(dest_version_json.read_text())
            if vdata.get("version") != version:
                raise SystemExit(f"version.json reports {vdata.get('version')}, expected {version}")
        except Exception as e:
            raise SystemExit(f"Failed to read/verify {dest_version_json}: {e}")
    else:
        # Create minimal version.json if not present
        dest_version_json.write_text(json.dumps({"version": version, "requires": {"node": False, "overpy": False}}, indent=2) + "\n")

    # Copy package.json, README.md, LICENSE
    dist_dir = REPO_ROOT / "dist" / "npm" / config["dir_name"]
    shutil.copy2(dist_dir / "package.json", pkg_staging / "package.json")
    shutil.copy2(dist_dir / "README.md", pkg_staging / "README.md")
    shutil.copy2(REPO_ROOT / "LICENSE", pkg_staging / "LICENSE")

    return pkg_staging


def stage_meta_package(version: str, staging_root: Path) -> Path:
    pkg_staging = staging_root / "wright"
    pkg_staging.mkdir(parents=True, exist_ok=True)

    dist_dir = REPO_ROOT / "dist" / "npm" / "wright"
    shutil.copytree(dist_dir / "bin", pkg_staging / "bin", dirs_exist_ok=True)
    shutil.copy2(dist_dir / "index.js", pkg_staging / "index.js")
    shutil.copy2(dist_dir / "index.d.ts", pkg_staging / "index.d.ts")
    shutil.copy2(dist_dir / "package.json", pkg_staging / "package.json")
    shutil.copy2(dist_dir / "README.md", pkg_staging / "README.md")
    shutil.copy2(REPO_ROOT / "LICENSE", pkg_staging / "LICENSE")

    # Set executable permissions on bin scripts
    for script in (pkg_staging / "bin").glob("*.js"):
        os.chmod(script, 0o755)

    return pkg_staging


def pack_directory(pkg_dir: Path, out_dir: Path) -> Path:
    result = subprocess.run(
        [NPM, "pack", "--pack-destination", str(out_dir)],
        cwd=pkg_dir,
        capture_output=True,
        text=True,
        check=True,
    )
    tarball_name = result.stdout.strip().splitlines()[-1]
    tarball_path = out_dir / tarball_name
    if not tarball_path.is_file():
        raise SystemExit(f"npm pack failed to produce {tarball_path}")
    return tarball_path


def verify_tarball(tarball_path: Path, is_meta: bool) -> None:
    forbidden_suffixes = {".rs", ".o", ".a", ".pdb", ".d", ".rlib"}
    found_files = set()

    with tarfile.open(tarball_path, "r:gz") as tar:
        for member in tar.getmembers():
            name = member.name
            found_files.add(name)
            for suffix in forbidden_suffixes:
                if name.endswith(suffix):
                    raise SystemExit(f"Forbidden file in tarball {tarball_path.name}: {name}")

            # Check permissions for binaries and scripts
            if name in ("package/wright", "package/wright-lsp", "package/bin/wright.js", "package/bin/wright-lsp.js"):
                if not (member.mode & 0o111):
                    raise SystemExit(f"File {name} in {tarball_path.name} is not executable (mode {oct(member.mode)})")

    if is_meta:
        expected = {
            "package/package.json",
            "package/index.js",
            "package/index.d.ts",
            "package/bin/wright.js",
            "package/bin/wright-lsp.js",
            "package/README.md",
            "package/LICENSE",
        }
        for exp in expected:
            if exp not in found_files:
                raise SystemExit(f"Meta package tarball {tarball_path.name} is missing {exp}")
    else:
        # Check required base files
        for exp in ("package/package.json", "package/version.json", "package/README.md", "package/LICENSE"):
            if exp not in found_files:
                raise SystemExit(f"Platform package tarball {tarball_path.name} is missing {exp}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True, help="Wright release version (e.g. 0.1.0)")
    parser.add_argument("--artifacts-dir", type=Path, help="Directory containing release archives and checksums")
    parser.add_argument("--binaries-dir", type=Path, help="Directory containing local wright/wright-lsp binaries (host platform only)")
    parser.add_argument("--host-target", help="Explicit platform key (e.g. darwin-arm64) when packaging local binaries")
    parser.add_argument("--out-dir", type=Path, default=REPO_ROOT / "dist" / "npm-packages", help="Output directory for .tgz tarballs")
    args = parser.parse_args()

    version = version_from_tag(args.version)
    out_dir = args.out_dir.resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    if not args.artifacts_dir and not args.binaries_dir:
        raise SystemExit("Must provide either --artifacts-dir or --binaries-dir")

    created_tarballs = []

    with tempfile.TemporaryDirectory(prefix="wright-npm-") as tmp:
        tmp_dir = Path(tmp)
        staging_dir = tmp_dir / "staging"
        extract_root = tmp_dir / "extracted"

        if args.artifacts_dir:
            artifacts_dir = args.artifacts_dir.resolve()
            print(f"Packaging npm packages for version {version} from artifacts in {artifacts_dir}")

            for platform_key, config in NPM_PLATFORM_PACKAGES.items():
                triple = TARGETS[platform_key]
                ext = ARCHIVE_EXT[triple]
                archive_path = find_archive(artifacts_dir, version, triple, ext)
                if not archive_path:
                    raise SystemExit(f"Could not find release archive for {platform_key} ({triple}) in {artifacts_dir}")

                # Check sha256 file if present
                sha256_file = archive_path.with_name(f"{archive_path.name}.sha256")
                if sha256_file.is_file():
                    expected_hash = sha256_file.read_text().split()[0].strip()
                    actual_hash = calculate_sha256(archive_path)
                    if expected_hash.lower() != actual_hash.lower():
                        raise SystemExit(f"Checksum mismatch for {archive_path.name}: expected {expected_hash}, got {actual_hash}")
                    print(f"verified checksum for {archive_path.name}")

                extract_dir = extract_root / platform_key
                extract_dir.mkdir(parents=True, exist_ok=True)
                extract_archive(archive_path, ext, extract_dir)

                payload_name = f"wright-{version}-{triple}"
                bin_dir = extract_dir / payload_name
                if not bin_dir.is_dir():
                    # Fallback to extract_dir if top-level payload directory is flat
                    bin_dir = extract_dir

                pkg_dir = stage_platform_package(platform_key, config, version, bin_dir, staging_dir)
                tarball = pack_directory(pkg_dir, out_dir)
                verify_tarball(tarball, is_meta=False)
                created_tarballs.append(tarball)
                print(f"packaged {config['name']} -> {tarball.name}")

        elif args.binaries_dir:
            binaries_dir = args.binaries_dir.resolve()
            platform_key = args.host_target or detect_host_target()
            config = NPM_PLATFORM_PACKAGES.get(platform_key)
            if not config:
                raise SystemExit(f"Unknown platform key {platform_key}")

            print(f"Packaging npm package for host platform {platform_key} from {binaries_dir}")
            pkg_dir = stage_platform_package(platform_key, config, version, binaries_dir, staging_dir)
            tarball = pack_directory(pkg_dir, out_dir)
            verify_tarball(tarball, is_meta=False)
            created_tarballs.append(tarball)
            print(f"packaged {config['name']} -> {tarball.name}")

        # Package meta package
        meta_dir = stage_meta_package(version, staging_dir)
        meta_tarball = pack_directory(meta_dir, out_dir)
        verify_tarball(meta_tarball, is_meta=True)
        created_tarballs.append(meta_tarball)
        print(f"packaged @wrightkit/wright -> {meta_tarball.name}")

    print("\nSummary of created npm packages:")
    for tb in created_tarballs:
        print(f"  {tb.name} ({tb.stat().st_size} bytes)")
    print("done")


if __name__ == "__main__":
    main()
