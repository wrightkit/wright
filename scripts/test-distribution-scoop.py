#!/usr/bin/env python3
"""Exercise the native Scoop channel against a local release artifact."""

from __future__ import annotations

import platform
import shutil
import subprocess
from pathlib import Path

from distribution_test_support import ROOT, ReleaseFixture, fail, native_smoke, run


CHANNEL = "Scoop"


def powershell_literal(value: object) -> str:
    return "'" + str(value).replace("'", "''") + "'"


def scoop_command(arguments: list[object]) -> list[str]:
    expression = "& (Get-Command scoop -ErrorAction Stop).Source " + " ".join(
        powershell_literal(argument) for argument in arguments
    )
    return ["pwsh", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command", expression]


def scoop_run(label: str, arguments: list[object]) -> str:
    return run(CHANNEL, label, scoop_command(arguments))


def target_info() -> str:
    if platform.system() != "Windows" or platform.machine().lower() not in {"x86_64", "amd64"}:
        fail(CHANNEL, f"unsupported validation host {platform.system()}/{platform.machine()}")
    return "x86_64-pc-windows-msvc"


def initialize_bucket(fixture: ReleaseFixture):
    bucket_root = fixture.work / "buckets" / "wright-local"
    bucket = bucket_root / "bucket"
    bucket.mkdir(parents=True)
    shutil.copy2(fixture.metadata / "dist" / "scoop" / "wright.json", bucket / "wright.json")
    run(CHANNEL, "initialize local bucket", ["git", "init", "--quiet", str(bucket_root)])
    run(CHANNEL, "configure local bucket", ["git", "-C", str(bucket_root), "config", "user.name", "wright-ci"])
    run(CHANNEL, "configure local bucket email", ["git", "-C", str(bucket_root), "config", "user.email", "wright-ci@example.invalid"])
    run(CHANNEL, "commit local bucket", ["git", "-C", str(bucket_root), "add", "bucket/wright.json"])
    run(CHANNEL, "commit local bucket contents", ["git", "-C", str(bucket_root), "commit", "--quiet", "-m", "test bucket"])
    return bucket_root


def main() -> None:
    version = (ROOT / "version.txt").read_text().strip()
    with ReleaseFixture(CHANNEL, version, target_info(), "zip", ".exe") as fixture:
        bucket_root = initialize_bucket(fixture)
        bucket_uri = bucket_root.as_uri()
        run(CHANNEL, "verify local bucket Git URI", ["git", "ls-remote", bucket_uri])
        added = False
        try:
            scoop_run("add generated local bucket", ["bucket", "add", "wright-local", bucket_uri])
            added = True
            scoop_run("install from generated local manifest", ["install", "wright-local/wright"])
            prefix = scoop_run("resolve installed prefix", ["prefix", "wright"]).strip()
            native_smoke(CHANNEL, Path(prefix) / "wright.exe", Path(prefix) / "wright-lsp.exe", version)
        finally:
            subprocess.run(scoop_command(["uninstall", "wright"]), cwd=ROOT, check=False, capture_output=True, text=True)
            if added:
                subprocess.run(scoop_command(["bucket", "rm", "wright-local"]), cwd=ROOT, check=False, capture_output=True, text=True)
    print(f"{CHANNEL} distribution validation passed")


if __name__ == "__main__":
    main()
