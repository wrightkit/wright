#!/usr/bin/env python3
"""Exercise native installation channels against a locally staged artifact (#255)."""

from __future__ import annotations

import hashlib
import http.server
import importlib.util
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import zipfile
from pathlib import Path
from typing import NoReturn


ROOT = Path(__file__).resolve().parent.parent
SMOKE = ROOT / "scripts" / "smoke-native.py"


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, _format: str, *_args: object) -> None:
        pass

    def copyfile(self, source, outputfile) -> None:
        try:
            super().copyfile(source, outputfile)
        except BrokenPipeError:
            pass


def fail(message: str) -> NoReturn:
    raise SystemExit(f"distribution channel validation failed: {message}")


def run(label: str, command: list[str], env: dict[str, str] | None = None) -> str:
    print(f"==> distribution channel: {label}")
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            env=env,
            check=True,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError as error:
        fail(f"{label}: missing executable {error.filename}")
    except subprocess.CalledProcessError as error:
        output = "\n".join(part for part in (error.stdout, error.stderr) if part)
        if output:
            print(output, file=sys.stderr, end="" if output.endswith("\n") else "\n")
        fail(f"{label}: command exited with status {error.returncode}")
    return result.stdout


def target_info() -> tuple[str, str, str]:
    system = platform.system()
    machine = platform.machine().lower()
    if system == "Linux" and machine in {"x86_64", "amd64"}:
        return "x86_64-unknown-linux-gnu", "tar.gz", ""
    if system == "Darwin" and machine in {"arm64", "aarch64"}:
        return "aarch64-apple-darwin", "tar.gz", ""
    if system == "Darwin" and machine in {"x86_64", "amd64"}:
        return "x86_64-apple-darwin", "tar.gz", ""
    if system == "Windows" and machine in {"x86_64", "amd64"}:
        return "x86_64-pc-windows-msvc", "zip", ".exe"
    fail(f"unsupported validation host {system}/{platform.machine()}")


def load_generator():
    spec = importlib.util.spec_from_file_location(
        "wright_dist", ROOT / "scripts" / "update-dist-manifests.py"
    )
    if spec is None or spec.loader is None:
        fail("cannot load scripts/update-dist-manifests.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def stage_artifact(work: Path, version: str, target: str, extension: str, exe: str) -> Path:
    source_dir = ROOT / "target" / "debug"
    source_wright = source_dir / f"wright{exe}"
    source_lsp = source_dir / f"wright-lsp{exe}"
    if not source_wright.is_file() or not source_lsp.is_file():
        fail(
            "native debug binaries are missing; run "
            "cargo build --locked -p wright-cli -p wright-lsp first"
        )

    release_dir = work / "releases" / "download" / f"v{version}"
    payload_name = f"wright-{version}-{target}"
    payload = release_dir / payload_name
    payload.mkdir(parents=True)
    shutil.copy2(source_wright, payload / f"wright{exe}")
    shutil.copy2(source_lsp, payload / f"wright-lsp{exe}")
    (payload / "version.json").write_text(f'{{"version":"{version}"}}\n')

    archive = release_dir / f"{payload_name}.{extension}"
    if extension == "zip":
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
            for path in payload.iterdir():
                output.write(path, f"{payload_name}/{path.name}")
    else:
        with tarfile.open(archive, "w:gz") as output:
            output.add(payload, arcname=payload_name)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    (Path(f"{archive}.sha256")).write_text(f"{digest}  {archive.name}\n")
    return archive


def generate_metadata(work: Path, version: str, target: str, digest: str, base: str) -> Path:
    generator = load_generator()
    hashes = {key: "" for key in generator.TARGETS}
    target_key = next(key for key, value in generator.TARGETS.items() if value == target)
    hashes[target_key] = digest
    metadata = work / "metadata"
    generator.generate(version, hashes, metadata, base)
    return metadata


def native_smoke(wright: Path, lsp: Path, provider_bootstrap: bool = False) -> None:
    command = [
        sys.executable,
        str(SMOKE),
        "--wright",
        str(wright),
        "--wright-lsp",
        str(lsp),
        "--version",
        VERSION,
    ]
    if provider_bootstrap:
        command.append("--provider-bootstrap")
    run(
        "native post-install smoke",
        command,
    )


def test_unix_installer(work: Path, base: str) -> None:
    install_dir = work / "install-sh"
    home = work / "home"
    home.mkdir()
    run(
        "install.sh installation",
        [
            "bash",
            str(ROOT / "install.sh"),
            "--version",
            VERSION,
            "--dir",
            str(install_dir),
        ],
        {
            **os.environ,
            "HOME": str(home),
            "XDG_CONFIG_HOME": str(home / ".config"),
            "WRIGHT_INSTALL_BASE_URL": base,
        },
    )
    native_smoke(install_dir / "wright", install_dir / "wright-lsp")


def test_windows_installer(work: Path, base: str) -> None:
    install_dir = work / "install-ps1"
    run(
        "install.ps1 installation",
        [
            "pwsh",
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-File",
            str(ROOT / "install.ps1"),
            "-Version",
            VERSION,
            "-InstallDir",
            str(install_dir),
            "-BaseUrl",
            base,
            "-ApiUrl",
            f"{base}/unused/latest",
        ],
    )
    native_smoke(install_dir / "wright.exe", install_dir / "wright-lsp.exe")


def initialize_homebrew_tap(tap_root: Path, formula: Path) -> Path:
    tap_formula = tap_root / "Formula" / "wright.rb"
    tap_formula.parent.mkdir(parents=True)
    shutil.copy2(formula, tap_formula)
    run("initialize local Homebrew tap", ["git", "init", "--quiet", str(tap_root)])
    run("configure local Homebrew tap", ["git", "-C", str(tap_root), "config", "user.name", "wright-ci"])
    run(
        "configure local Homebrew tap email",
        ["git", "-C", str(tap_root), "config", "user.email", "wright-ci@example.invalid"],
    )
    run("commit local Homebrew tap", ["git", "-C", str(tap_root), "add", "Formula/wright.rb"])
    run(
        "commit local Homebrew tap contents",
        ["git", "-C", str(tap_root), "commit", "--quiet", "-m", "test tap"],
    )
    return tap_root


def test_homebrew(metadata: Path, work: Path) -> None:
    formula = metadata / "dist" / "homebrew" / "wright.rb"
    tap_root = initialize_homebrew_tap(work / "homebrew-tap", formula)
    brew_env = {**os.environ, "HOMEBREW_NO_AUTO_UPDATE": "1", "HOMEBREW_NO_ENV_HINTS": "1"}
    tap_name = "wright-ci/local-tap"
    tapped = False
    try:
        run("Homebrew add generated local tap", ["brew", "tap", tap_name, str(tap_root)], brew_env)
        tapped = True
        run("Homebrew install from generated local formula", ["brew", "install", f"{tap_name}/wright"], brew_env)
        run("Homebrew formula test", ["brew", "test", f"{tap_name}/wright"], brew_env)
        prefix = run("Homebrew resolve installed prefix", ["brew", "--prefix", "wright"], brew_env).strip()
        native_smoke(Path(prefix) / "bin" / "wright", Path(prefix) / "bin" / "wright-lsp")
    finally:
        subprocess.run(
            ["brew", "uninstall", "--force", "wright"],
            cwd=ROOT,
            env=brew_env,
            check=False,
            capture_output=True,
            text=True,
        )
        if tapped:
            subprocess.run(
                ["brew", "untap", tap_name],
                cwd=ROOT,
                env=brew_env,
                check=False,
                capture_output=True,
                text=True,
            )


def initialize_bucket(bucket_root: Path, manifest: Path) -> Path:
    bucket = bucket_root / "bucket"
    bucket.mkdir(parents=True)
    shutil.copy2(manifest, bucket / "wright.json")
    run("initialize local Scoop bucket", ["git", "init", "--quiet", str(bucket_root)])
    run("configure local Scoop bucket", ["git", "-C", str(bucket_root), "config", "user.name", "wright-ci"])
    run(
        "configure local Scoop bucket email",
        ["git", "-C", str(bucket_root), "config", "user.email", "wright-ci@example.invalid"],
    )
    run("commit local Scoop bucket", ["git", "-C", str(bucket_root), "add", "bucket/wright.json"])
    run(
        "commit local Scoop bucket contents",
        ["git", "-C", str(bucket_root), "commit", "--quiet", "-m", "test bucket"],
    )
    return bucket_root


def test_scoop(metadata: Path, work: Path) -> None:
    bucket_root = initialize_bucket(work / "scoop-bucket", metadata / "dist" / "scoop" / "wright.json")
    added = False
    try:
        run("Scoop add generated local bucket", ["scoop", "bucket", "add", "wright-local", str(bucket_root)])
        added = True
        run("Scoop install from generated local manifest", ["scoop", "install", "wright-local/wright"])
        prefix = run("Scoop resolve installed prefix", ["scoop", "prefix", "wright"]).strip()
        native_smoke(Path(prefix) / "wright.exe", Path(prefix) / "wright-lsp.exe")
    finally:
        subprocess.run(["scoop", "uninstall", "wright"], cwd=ROOT, check=False, capture_output=True, text=True)
        if added:
            subprocess.run(
                ["scoop", "bucket", "rm", "wright-local"],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
            )


def winget_binary(name: str) -> Path:
    local_app_data = os.environ.get("LOCALAPPDATA")
    if not local_app_data:
        fail("LOCALAPPDATA is not set")
    roots = [
        Path(local_app_data) / "Microsoft" / "WinGet" / "Packages",
        Path(local_app_data) / "Microsoft" / "WinGet" / "Links",
    ]
    candidates = [path for root in roots if root.exists() for path in root.rglob(name)]
    if not candidates:
        fail(f"WinGet installed package does not expose {name}")
    return candidates[0]


def test_winget(metadata: Path) -> None:
    manifest_dir = metadata / "dist" / "winget" / "manifests" / "w" / "WrightKit" / "Wright" / VERSION
    try:
        run("WinGet validate generated local manifests", ["winget", "validate", "--manifest", str(manifest_dir)])
        run(
            "WinGet install from generated local manifests",
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
        native_smoke(winget_binary("wright.exe"), winget_binary("wright-lsp.exe"))
    finally:
        subprocess.run(
            ["winget", "uninstall", "--id", "WrightKit.Wright", "--silent", "--disable-interactivity"],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
        )


def main() -> None:
    global VERSION
    VERSION = (ROOT / "version.txt").read_text().strip()
    target, extension, exe = target_info()
    with tempfile.TemporaryDirectory(prefix="wright-distribution-") as directory:
        work = Path(directory)
        stage = stage_artifact(work, VERSION, target, extension, exe)
        server = http.server.ThreadingHTTPServer(
            ("127.0.0.1", 0),
            lambda *args, **kwargs: QuietHandler(*args, directory=str(work), **kwargs),
        )
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        base = f"http://127.0.0.1:{server.server_port}/releases/download"
        metadata = generate_metadata(work, VERSION, target, hashlib.sha256(stage.read_bytes()).hexdigest(), base)
        try:
            if platform.system() == "Windows":
                test_windows_installer(work, base)
                test_scoop(metadata, work)
                test_winget(metadata)
            else:
                test_unix_installer(work, base)
                if platform.system() == "Darwin":
                    test_homebrew(metadata, work)
        finally:
            server.shutdown()
            thread.join(timeout=5)
    print("distribution channel validation passed")


if __name__ == "__main__":
    main()
