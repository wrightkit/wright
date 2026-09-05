#!/usr/bin/env python3
"""Exercise the native install.sh channel against a local release artifact."""

from __future__ import annotations

import os
import platform
from pathlib import Path

from distribution_test_support import ROOT, ReleaseFixture, fail, native_smoke, run


CHANNEL = "install.sh"


def target_info() -> str:
    system = platform.system()
    machine = platform.machine().lower()
    if system == "Linux" and machine in {"x86_64", "amd64"}:
        return "x86_64-unknown-linux-gnu"
    if system == "Darwin" and machine in {"arm64", "aarch64"}:
        return "aarch64-apple-darwin"
    if system == "Darwin" and machine in {"x86_64", "amd64"}:
        return "x86_64-apple-darwin"
    fail(CHANNEL, f"unsupported validation host {system}/{platform.machine()}")


def main() -> None:
    version = (ROOT / "version.txt").read_text().strip()
    with ReleaseFixture(CHANNEL, version, target_info(), "tar.gz", "") as fixture:
        install_dir = fixture.work / "install"
        home = fixture.work / "home"
        home.mkdir()
        base_env = {
            **os.environ,
            "HOME": str(home),
            "XDG_CONFIG_HOME": str(home / ".config"),
            "WRIGHT_INSTALL_BASE_URL": fixture.base,
            "WRIGHT_API_URL": f"{fixture.base}/repos/wrightkit/wright/releases/latest",
        }
        run(
            CHANNEL,
            "install.sh installation",
            [
                "bash",
                str(ROOT / "install.sh"),
                "--version",
                version,
                "--dir",
                str(install_dir),
            ],
            base_env,
        )
        native_smoke(CHANNEL, install_dir / "wright", install_dir / "wright-lsp", version)
    print(f"{CHANNEL} distribution validation passed")


if __name__ == "__main__":
    main()
