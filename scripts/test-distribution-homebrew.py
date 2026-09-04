#!/usr/bin/env python3
"""Exercise the native Homebrew channel against a local release artifact."""

from __future__ import annotations

import os
import platform
import shutil
import subprocess
from pathlib import Path

from distribution_test_support import ROOT, ReleaseFixture, fail, native_smoke, run


CHANNEL = "Homebrew"


def target_info() -> str:
    machine = platform.machine().lower()
    if platform.system() != "Darwin":
        fail(CHANNEL, f"unsupported validation host {platform.system()}")
    if machine in {"arm64", "aarch64"}:
        return "aarch64-apple-darwin"
    if machine in {"x86_64", "amd64"}:
        return "x86_64-apple-darwin"
    fail(CHANNEL, f"unsupported validation host Darwin/{platform.machine()}")


def initialize_tap(fixture: ReleaseFixture):
    tap_root = fixture.work / "homebrew-tap"
    formula = fixture.metadata / "dist" / "homebrew" / "wright.rb"
    tap_formula = tap_root / "Formula" / "wright.rb"
    tap_formula.parent.mkdir(parents=True)
    shutil.copy2(formula, tap_formula)
    run(CHANNEL, "initialize local Homebrew tap", ["git", "init", "--quiet", str(tap_root)])
    run(CHANNEL, "configure local Homebrew tap", ["git", "-C", str(tap_root), "config", "user.name", "wright-ci"])
    run(
        CHANNEL,
        "configure local Homebrew tap email",
        ["git", "-C", str(tap_root), "config", "user.email", "wright-ci@example.invalid"],
    )
    run(CHANNEL, "commit local Homebrew tap", ["git", "-C", str(tap_root), "add", "Formula/wright.rb"])
    run(CHANNEL, "commit local Homebrew tap contents", ["git", "-C", str(tap_root), "commit", "--quiet", "-m", "test tap"])
    return tap_root


def main() -> None:
    version = (ROOT / "version.txt").read_text().strip()
    with ReleaseFixture(CHANNEL, version, target_info(), "tar.gz", "") as fixture:
        tap_root = initialize_tap(fixture)
        tap_name = "wright-ci/local-tap"
        brew_env = {**os.environ, "HOMEBREW_NO_AUTO_UPDATE": "1", "HOMEBREW_NO_ENV_HINTS": "1"}
        tapped = False
        try:
            run(CHANNEL, "add generated local tap", ["brew", "tap", tap_name, str(tap_root)], brew_env)
            tapped = True
            run(CHANNEL, "install from generated local formula", ["brew", "install", f"{tap_name}/wright"], brew_env)
            run(CHANNEL, "formula test", ["brew", "test", f"{tap_name}/wright"], brew_env)
            prefix = run(CHANNEL, "resolve installed prefix", ["brew", "--prefix", "wright"], brew_env).strip()
            native_smoke(
                CHANNEL,
                Path(prefix) / "bin" / "wright",
                Path(prefix) / "bin" / "wright-lsp",
                version,
            )
        finally:
            subprocess.run(["brew", "uninstall", "--force", "wright"], cwd=ROOT, env=brew_env, check=False, capture_output=True, text=True)
            if tapped:
                subprocess.run(["brew", "untap", tap_name], cwd=ROOT, env=brew_env, check=False, capture_output=True, text=True)
    print(f"{CHANNEL} distribution validation passed")


if __name__ == "__main__":
    main()
