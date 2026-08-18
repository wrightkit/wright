"""npm tarball bin-metadata determinism/portability regressions (#123).

Drives the REAL shipped packaging functions (`scripts/package-npm.py` loaded
via the repo's importlib convention) on staged inputs whose bin files are
deliberately created WITHOUT the executable bit, simulating a Windows source
tree where `os.chmod` cannot set exec bits. Asserts:

* after the shipped post-pack normalization every bin entry in the produced
  `.tgz` carries mode `0o755` (and all entry metadata is fixed);
* the normalized bytes are identical to a pack of the same inputs staged WITH
  exec bits, and identical across repeated packs;
* the shipped `verify_tarball` still rejects a non-executable bin entry
  (the Unix executability validation is retained, not loosened);
* an end-to-end npm pack of an exec-less staged package (when npm is
  available) passes the real validation after normalization.
"""

import gzip
import hashlib
import importlib.util
import io
import json
import shutil
import tarfile
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
PACKAGE_NPM = REPO_ROOT / "scripts" / "package-npm.py"


def load_package_npm():
    spec = importlib.util.spec_from_file_location("wright_package_npm", PACKAGE_NPM)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


package_npm = load_package_npm()

META_LAYOUT = {
    "package/": b"",
    "package/bin/": b"",
    "package/bin/wright.js": b"#!/usr/bin/env node\nconst { getBinaryPath } = require('../index.js');\n",
    "package/bin/wright-lsp.js": b"#!/usr/bin/env node\nconst { getBinaryPath } = require('../index.js');\n",
    "package/index.js": b"module.exports = { getBinaryPath: () => null };\n",
    "package/index.d.ts": b"declare const x: number;\nexport = x;\n",
    "package/package.json": json.dumps(
        {"name": "@wrightkit/wright", "version": "0.1.0", "bin": {"wright": "bin/wright.js", "wright-lsp": "bin/wright-lsp.js"}}
    ).encode(),
    "package/README.md": b"# wright\n",
    "package/LICENSE": b"AGPL-3.0-or-later\n",
}

PLATFORM_LAYOUT = {
    "package/": b"",
    "package/wright": b"#!/bin/sh\nexit 0\n",
    "package/wright-lsp": b"#!/bin/sh\nexit 0\n",
    "package/wright.exe": b"MZ\x00\x00fake\n",
    "package/wright-lsp.exe": b"MZ\x00\x00fake\n",
    "package/version.json": b'{"version": "0.1.0"}\n',
    "package/package.json": json.dumps(
        {"name": "@wrightkit/wright-win32-x64", "version": "0.1.0"}
    ).encode(),
    "package/README.md": b"# wright\n",
    "package/LICENSE": b"AGPL-3.0-or-later\n",
}

BIN_PATHS = {
    "package/wright",
    "package/wright-lsp",
    "package/wright.exe",
    "package/wright-lsp.exe",
    "package/bin/wright.js",
    "package/bin/wright-lsp.js",
}


def stage_package(root: Path, layout: dict[str, bytes], exec_bits: bool) -> None:
    for rel, content in layout.items():
        path = root / rel
        if content == b"" and rel.endswith("/"):
            path.mkdir(parents=True, exist_ok=True)
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        if rel in BIN_PATHS:
            path.chmod(0o755 if exec_bits else 0o644)
        else:
            path.chmod(0o644)


def pack_staged(root: Path, mtime: int) -> bytes:
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w", format=tarfile.USTAR_FORMAT) as tar:
        for path in sorted(root.rglob("*")):
            rel = str(path.relative_to(root))
            info = tar.gettarinfo(str(path), arcname=rel)
            info.uid = 501
            info.gid = 20
            info.mtime = mtime
            if path.is_dir():
                info.mode = 0o755
                tar.addfile(info)
            else:
                with open(path, "rb") as handle:
                    tar.addfile(info, handle)
    raw = buffer.getvalue()
    out = io.BytesIO()
    with gzip.GzipFile(fileobj=out, mode="wb", mtime=987654) as gz:
        gz.write(raw)
    return out.getvalue()


def write_tgz(bytes_: bytes) -> Path:
    directory = tempfile.mkdtemp(prefix="wright-npm-test-")
    path = Path(directory) / "wrightkit-wright-0.1.0.tgz"
    path.write_bytes(bytes_)
    return path


def entry_modes(tgz: Path) -> dict[str, tuple[int, int, int, int]]:
    with tarfile.open(tgz, "r:gz") as tar:
        return {m.name: (m.mode, m.uid, m.gid, m.mtime) for m in tar.getmembers()}


class NpmPackagingNormalizationTests(unittest.TestCase):
    def setUp(self):
        self._dirs = []

    def tearDown(self):
        for d in self._dirs:
            shutil.rmtree(d, ignore_errors=True)

    def _pack(self, layout, exec_bits, mtime):
        root = Path(tempfile.mkdtemp(prefix="wright-npm-stage-"))
        self._dirs.append(str(root))
        stage_package(root, layout, exec_bits)
        return pack_staged(root, mtime)

    def test_bin_entries_are_0755_after_normalization_from_exec_less_sources(self):
        _, _, expected_mtime = package_npm.fixed_entry_metadata("0.1.0")
        covered = set()
        for layout in (META_LAYOUT, PLATFORM_LAYOUT):
            raw = self._pack(layout, exec_bits=False, mtime=499162500)
            tgz = write_tgz(raw)
            package_npm.normalize_tarball(tgz, "0.1.0")
            modes = entry_modes(tgz)
            for path in BIN_PATHS & set(layout):
                covered.add(path)
                self.assertIn(path, modes, f"{path} missing after normalization")
                self.assertEqual(modes[path][0], 0o755, f"{path} must be 0o755")
            for path, (_, uid, gid, mtime) in modes.items():
                self.assertEqual(uid, 0, f"{path} uid must be fixed")
                self.assertEqual(gid, 0, f"{path} gid must be fixed")
                self.assertEqual(mtime, expected_mtime, f"{path} mtime must be fixed")
        self.assertEqual(covered, BIN_PATHS)

    def test_normalized_bytes_match_exec_bit_source_pack(self):
        windows = self._pack(META_LAYOUT, exec_bits=False, mtime=499162500)
        unix = self._pack(META_LAYOUT, exec_bits=True, mtime=1610613000)
        a = write_tgz(windows)
        b = write_tgz(unix)
        package_npm.normalize_tarball(a, "0.1.0")
        package_npm.normalize_tarball(b, "0.1.0")
        self.assertEqual(hashlib.sha256(a.read_bytes()).hexdigest(), hashlib.sha256(b.read_bytes()).hexdigest())

    def test_normalization_is_byte_deterministic(self):
        first = self._pack(META_LAYOUT, exec_bits=False, mtime=499162500)
        second = self._pack(META_LAYOUT, exec_bits=False, mtime=1700000000)
        a = write_tgz(first)
        b = write_tgz(second)
        package_npm.normalize_tarball(a, "0.1.0")
        package_npm.normalize_tarball(b, "0.1.0")
        self.assertEqual(hashlib.sha256(a.read_bytes()).hexdigest(), hashlib.sha256(b.read_bytes()).hexdigest())

    def test_verify_tarball_still_rejects_non_executable_bin(self):
        raw = self._pack(META_LAYOUT, exec_bits=False, mtime=499162500)
        tgz = write_tgz(raw)
        with self.assertRaises(SystemExit) as ctx:
            package_npm.verify_tarball(tgz, is_meta=True)
        message = str(ctx.exception)
        self.assertIn("not executable", message)
        self.assertIn("mode 0o644", message)

    def test_normalized_tarballs_pass_real_verify(self):
        for layout, is_meta in ((META_LAYOUT, True), (PLATFORM_LAYOUT, False)):
            raw = self._pack(layout, exec_bits=False, mtime=499162500)
            tgz = write_tgz(raw)
            package_npm.normalize_tarball(tgz, "0.1.0")
            package_npm.verify_tarball(tgz, is_meta=is_meta)

    def test_end_to_end_npm_pack_of_exec_less_staged_package(self):
        npm = shutil.which("npm.cmd") or shutil.which("npm")
        if npm is None:
            self.skipTest("npm is not available on this host")
        root = Path(tempfile.mkdtemp(prefix="wright-npm-e2e-"))
        self._dirs.append(str(root))
        out = root.parent / "wright-npm-e2e-out"
        out.mkdir(exist_ok=True)
        self._dirs.append(str(out))
        for rel, content in META_LAYOUT.items():
            if rel.endswith("/"):
                continue
            path = root / rel.removeprefix("package/")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
            path.chmod(0o755 if rel in BIN_PATHS else 0o644)
        tarball = package_npm.pack_directory(root, out)
        package_npm.normalize_tarball(tarball, "0.1.0")
        package_npm.verify_tarball(tarball, is_meta=True)
        modes = entry_modes(tarball)
        for path in ("package/bin/wright.js", "package/bin/wright-lsp.js"):
            self.assertEqual(modes[path][0], 0o755, f"{path} must be 0o755")

    def test_exec_bit_validation_source_is_preserved(self):
        source = PACKAGE_NPM.read_text()
        self.assertIn("mode & 0o111", source)
        self.assertIn("not executable", source)
        for path in sorted(BIN_PATHS):
            self.assertIn(path, source)


if __name__ == "__main__":
    unittest.main()
