#!/usr/bin/env python3

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("stamp-version.py")

MANIFEST = """[workspace]
members = ["a"]

[workspace.package]
version = "0.0.0"
edition = "2024"

[workspace.dependencies]
serde = "1"
"""

LOCK = """version = 4

[[package]]
name = "a"
version = "0.0.0"
dependencies = [
 "serde",
]

[[package]]
name = "serde"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "00"
"""


def stamp(root: Path, version: str) -> int:
    (root / "scripts").mkdir()
    shutil.copy(SCRIPT, root / "scripts" / SCRIPT.name)
    (root / "Cargo.toml").write_text(MANIFEST)
    (root / "Cargo.lock").write_text(LOCK)
    (root / "version.txt").write_text("0.0.0\n")
    result = subprocess.run(
        [sys.executable, str(root / "scripts" / SCRIPT.name), version], capture_output=True
    )
    return result.returncode


class StampVersion(unittest.TestCase):
    def test_stamps_workspace_version_and_workspace_lock_entries_only(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.assertEqual(stamp(root, "1.2.3"), 0)
            self.check_stamped(root)

    def check_stamped(self, root: Path) -> None:
        self.assertIn('[workspace.package]\nversion = "1.2.3"', (root / "Cargo.toml").read_text())
        lock = (root / "Cargo.lock").read_text()
        self.assertIn('name = "a"\nversion = "1.2.3"', lock)
        self.assertIn('name = "serde"\nversion = "1.0.0"', lock)
        self.assertEqual((root / "version.txt").read_text(), "1.2.3\n")

    def test_rejects_non_semver(self) -> None:
        for bad in ("1.2", "v1.2.3", "1.2.3-rc.1", "01.2.3"):
            with tempfile.TemporaryDirectory() as tmp:
                self.assertNotEqual(stamp(Path(tmp), bad), 0, bad)


if __name__ == "__main__":
    unittest.main()
