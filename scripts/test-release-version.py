#!/usr/bin/env python3
"""Test the stable release version selection and synchronization contract."""

from __future__ import annotations

import importlib.util
import shutil
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts" / "prepare-release-version.py"
spec = importlib.util.spec_from_file_location("prepare_release_version", SCRIPT)
if spec is None or spec.loader is None:
    raise SystemExit(f"cannot load {SCRIPT}")
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


def expect_error(callable_, *args) -> None:
    try:
        callable_(*args)
    except (SystemExit, ValueError):
        return
    raise SystemExit(f"expected failure from {callable_.__name__}{args!r}")


def main() -> int:
    assert module.select_version("0.2.32", None) == "0.2.33"
    assert module.select_version("0.2.32", "0.3.0") == "0.3.0"
    assert module.select_version("1.0.0-rc.1", "1.0.0") == "1.0.0"
    expect_error(module.select_version, "0.2.32", "0.2.32")
    expect_error(module.select_version, "0.2.32", "0.2.31")
    expect_error(module.select_version, "0.2.32", "v0.2.33")
    expect_error(module.select_version, "0.2.32", "0.2")
    expect_error(module.select_version, "0.2.32", "0.2.33-01")

    fixture = ROOT / "target" / "release-version-test"
    if fixture.exists():
        shutil.rmtree(fixture)
    fixture.mkdir(parents=True)
    (fixture / "scripts").mkdir()
    (fixture / "Cargo.toml").write_text(
        "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.2.32\"\n"
    )
    (fixture / "version.txt").write_text("0.2.32\n")
    shutil.copy2(ROOT / "scripts" / "update-dist-manifests.py", fixture / "scripts")
    try:
        module.synchronize_version(fixture, "0.2.33")
        assert module.workspace_version(fixture) == "0.2.33"
        assert (fixture / "version.txt").read_text() == "0.2.33\n"
        assert "Wright 0.2.33" in (
            fixture / "dist" / "homebrew" / "wright.rb"
        ).read_text()
        assert (
            fixture
            / "dist"
            / "winget"
            / "manifests"
            / "w"
            / "WrightKit"
            / "Wright"
            / "0.2.33"
        ).is_dir()
        expect_error(module.synchronize_version, fixture, "0.2.33")
    finally:
        shutil.rmtree(fixture)
    print("release version contract passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
