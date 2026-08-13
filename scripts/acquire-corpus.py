#!/usr/bin/env python3
"""M11 phase-1 real-world corpus acquisition (issue #81, SPEC-M11).

Downloads the committed corpus sources from immutable pinned GitHub commits,
resolves multi-file `#!include` closures exactly as OverPy 9.7.10 (the pinned
oracle) does, verifies every file against recorded SHA-256 values, and records
full provenance in ``scripts/corpus-manifest.json`` and in each fixture's
``fixture.json`` ``files`` map.

Acquisition method
------------------
Each source repository tree is downloaded once as the GitHub archive of its
pinned commit (``https://codeload.github.com/<owner>/<repo>/tar.gz/<sha>``,
content-addressed by the immutable commit). Files are copied from the
extracted archive; nothing is taken from a mutable branch tip. ``raw
.githubusercontent.com`` is not used (it redirects to a content host that is
unavailable in the authoring environment); the GitHub contents API is not used
for bulk downloads (it is rate limited to 60 requests/hour unauthenticated).
The archive is cached under ``target/corpus-cache/`` so re-runs and CI
verification do not re-download.

Include-closure resolution mirrors the pinned oracle: a ``#!include "path"``
directive resolves relative to the directory of the file that contains it; a
path naming a directory imports every ``.opy`` file in it (sorted); a file is
imported at most once; commented directives (``##!include``, ``# !include``)
are ignored.

Layout rule
-----------
The oracle runner (``compatibility/run_oracle.py``) is unmodified and resolves
the main file as ``<fixture-dir>/<basename>`` (``--main-file <name>``). Every
fixture therefore places the main file at the fixture root under its original
file name, and every other file keeps its repo path relative to the main
file's directory, so all relative includes resolve exactly as in the source
repository. ``fixture.json`` records the SHA-256 of every committed file.

Provenance requirements (from the PM spec)
------------------------------------------
- sources are committed only from immutable pinned commits;
- GPL-3.0-only and BSD-2-Clause sources are redistributable (precedent:
  ``real-world/overpy-cake``);
- the LICENSE file of each source repository is verified at the pinned commit
  before sources are committed;
- a source whose license cannot be verified, or which cannot be laid out under
  the oracle runner's constraints, is recorded metadata-only (hashes and
  acquisition instructions) and is not committed.

Usage: python3 scripts/acquire-corpus.py [--target <corpus-root>]
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import urllib.request
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_TARGET = ROOT / "compatibility" / "fixtures"
CACHE_DIR = ROOT / "target" / "corpus-cache"
MANIFEST_PATH = ROOT / "scripts" / "corpus-manifest.json"

#: The pinned OverPy oracle identity recorded in oracle snapshots.
OVERPY_COMMIT = "eea67adbcf6926c4004e35e25ab4be072624a44e"

#: Expected first lines of the verified license texts at the pinned commits.
LICENSE_EXPECTATIONS = {
    "GPL-3.0-only": ("GNU GENERAL PUBLIC LICENSE", "Version 3"),
    "BSD-2-Clause": ("Redistribution and use in source and binary forms",),
}


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def normalize_relative(path: str) -> str:
    return path.replace("\\", "/")


def parse_includes(text: str) -> list[str]:
    """The oracle's include directives: ``#!include "path"`` lines."""
    includes = []
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped.startswith("#!include"):
            continue
        rest = stripped[len("#!include"):].strip()
        if len(rest) >= 2 and rest[0] == rest[-1] == '"':
            includes.append(rest[1:-1])
        elif rest:
            includes.append(rest)
    return includes


class Repo:
    """An extracted archive of one pinned commit."""

    def __init__(self, repo: str, commit: str) -> None:
        self.repo = repo
        self.commit = commit
        self.root = self._extract()

    @property
    def _cache_key(self) -> str:
        return f"{self.repo.replace('/', '__')}__{self.commit}"

    def _extract(self) -> Path:
        archive = CACHE_DIR / f"{self._cache_key}.tar.gz"
        dest = CACHE_DIR / self._cache_key
        if not dest.is_dir():
            CACHE_DIR.mkdir(parents=True, exist_ok=True)
            if not archive.is_file():
                url = f"https://codeload.github.com/{self.repo}/tar.gz/{self.commit}"
                print(f"downloading {url}")
                with urllib.request.urlopen(url, timeout=600) as response:
                    archive.write_bytes(response.read())
            with tarfile.open(archive, "r:gz") as tar:
                tar.extractall(dest)
            members = list(dest.iterdir())
            if len(members) != 1 or not members[0].is_dir():
                raise RuntimeError(f"unexpected archive layout in {dest}")
            return members[0]
        members = list(dest.iterdir())
        if len(members) != 1 or not members[0].is_dir():
            raise RuntimeError(f"unexpected archive layout in {dest}")
        return members[0]

    def read(self, repo_path: str) -> bytes:
        return (self.root / repo_path).read_bytes()

    def read_text(self, repo_path: str) -> str:
        return self.read(repo_path).decode("utf-8")

    def list_opy(self, repo_dir: str) -> list[str]:
        """Sorted ``.opy`` files directly inside ``repo_dir`` (dir-include)."""
        directory = self.root / repo_dir
        if not directory.is_dir():
            raise RuntimeError(f"missing include directory {repo_dir!r}")
        return sorted(
            p.relative_to(self.root).as_posix()
            for p in directory.iterdir()
            if p.is_file() and p.name.lower().endswith(".opy")
        )

    def license_lines(self, repo_path: str) -> list[str]:
        return self.read_text(repo_path).splitlines()[:8]


def resolve_closure(repo: Repo, main_path: str) -> list[str]:
    """BFS the include closure from ``main_path``; oracle semantics."""
    main_dir = main_path.rsplit("/", 1)[0] if "/" in main_path else ""
    visited: set[str] = set()
    order: list[str] = []
    pending = [main_path]

    def add(path: str) -> None:
        if path in visited:
            return
        visited.add(path)
        order.append(path)
        pending.append(path)

    add(main_path)
    while pending:
        current = pending.pop(0)
        text = repo.read_text(current)
        base = current.rsplit("/", 1)[0] if "/" in current else ""
        for include in parse_includes(text):
            parts = include.split("/")
            if include.startswith("/") or (parts and ":" in parts[0]):
                raise RuntimeError(f"absolute include not supported: {include!r} in {current}")
            raw = normalize_relative(f"{base}/{include}" if base else include)
            is_dir_target = raw.endswith("/") or repo.root.joinpath(raw).is_dir()
            joined = os.path.normpath(raw).replace("\\", "/")
            if joined in visited:
                continue
            if is_dir_target:
                for file_path in repo.list_opy(joined):
                    add(file_path)
            else:
                target = repo.root / joined
                if target.is_file() and target.name.lower().endswith(".opy"):
                    add(joined)
                elif target.is_file():
                    raise RuntimeError(
                        f"include target is not .opy: {include!r} in {current}"
                    )
                else:
                    raise RuntimeError(
                        f"include target missing: {include!r} in {current}"
                    )
    if not order or order[0] != main_path:
        raise RuntimeError(f"closure resolution failed for {main_path}")
    return order


def relocate(main_path: str, repo_path: str) -> str:
    """Fixture-relative path: main at root, others relative to main's dir."""
    main_dir = main_path.rsplit("/", 1)[0] if "/" in main_path else ""
    if repo_path == main_path:
        return main_path.rsplit("/", 1)[-1]
    if main_dir:
        prefix = main_dir + "/"
        if not repo_path.startswith(prefix):
            raise RuntimeError(f"closure file escapes main dir: {repo_path}")
        relative = repo_path[len(prefix):]
    else:
        relative = repo_path
    if ".." in relative.split("/"):
        raise RuntimeError(f"closure file escapes fixture root: {repo_path}")
    return relative


def verify_license(repo: Repo, license_path: str, expected: str) -> list[str]:
    lines = repo.license_lines(license_path)
    markers = LICENSE_EXPECTATIONS[expected]
    for marker in markers:
        if not any(marker in line for line in lines):
            raise RuntimeError(
                f"license verification failed for {repo.repo} ({expected}): "
                f"first lines are {lines!r}"
            )
    return lines


#: Fixture acquisition table. ``main`` is the repo-relative entry point;
#: ``files`` (when present) is the complete file set, ``closure`` resolves the
#: include closure of ``main`` exactly as the pinned oracle does.
FIXTURES: list[dict[str, Any]] = [
    {
        "id": "real-world/overpy-pixelart",
        "repo": "Zezombye/overpy",
        "commit": OVERPY_COMMIT,
        "license": "GPL-3.0-only",
        "licensePath": "LICENSE",
        "origin": "Zezombye/overpy examples/pixelart.opy",
        "main": "examples/pixelart.opy",
        "files": ["examples/pixelart.opy"],
    },
    {
        "id": "real-world/overpy-santa",
        "repo": "Zezombye/overpy",
        "commit": OVERPY_COMMIT,
        "license": "GPL-3.0-only",
        "licensePath": "LICENSE",
        "origin": "Zezombye/overpy examples/santa.opy",
        "main": "examples/santa.opy",
        "files": ["examples/santa.opy"],
    },
    {
        "id": "real-world/overpy-meipocalypse",
        "repo": "Zezombye/overpy",
        "commit": OVERPY_COMMIT,
        "license": "GPL-3.0-only",
        "licensePath": "LICENSE",
        "origin": "Zezombye/overpy examples/meipocalypse/",
        "main": "examples/meipocalypse/meipocalypse.opy",
        "files": [
            "examples/meipocalypse/meipocalypse.opy",
            "examples/meipocalypse/settings.opy",
            "examples/meipocalypse/debug_settings.opy",
            "examples/meipocalypse/zones.opy",
            "examples/meipocalypse/waves.opy",
            "examples/meipocalypse/shop.opy",
            "examples/meipocalypse/heroUnlock.opy",
            "examples/meipocalypse/barricades.opy",
            "examples/meipocalypse/debug.opy",
            "examples/meipocalypse/fightforyourlife.opy",
            "examples/meipocalypse/mei_types.opy",
        ],
    },
    {
        "id": "real-world/overpy-zencopter",
        "repo": "Zezombye/overpy",
        "commit": OVERPY_COMMIT,
        "license": "GPL-3.0-only",
        "licensePath": "LICENSE",
        "origin": "Zezombye/overpy examples/Zencopter/heli.opy",
        "main": "examples/Zencopter/heli.opy",
        "files": ["examples/Zencopter/heli.opy"],
    },
    {
        "id": "real-world/overpy-cronch",
        "repo": "Zezombye/overpy",
        "commit": OVERPY_COMMIT,
        "license": "GPL-3.0-only",
        "licensePath": "LICENSE",
        "origin": "Zezombye/overpy examples/cronch.opy",
        "main": "examples/cronch.opy",
        "files": ["examples/cronch.opy"],
    },
    {
        "id": "real-world/overpy-broken-weapons",
        "repo": "Zezombye/overpy",
        "commit": OVERPY_COMMIT,
        "license": "GPL-3.0-only",
        "licensePath": "LICENSE",
        "origin": "Zezombye/overpy examples/broken_weapons.opy",
        "main": "examples/broken_weapons.opy",
        "files": ["examples/broken_weapons.opy"],
    },
    {
        "id": "real-world/overpy-client-to-server",
        "repo": "Zezombye/overpy",
        "commit": OVERPY_COMMIT,
        "license": "GPL-3.0-only",
        "licensePath": "LICENSE",
        "origin": "Zezombye/overpy examples/clientToServer.opy",
        "main": "examples/clientToServer.opy",
        "files": ["examples/clientToServer.opy"],
    },
    {
        "id": "real-world/overpy-parabola",
        "repo": "Zezombye/overpy",
        "commit": OVERPY_COMMIT,
        "license": "GPL-3.0-only",
        "licensePath": "LICENSE",
        "origin": "Zezombye/overpy examples/parabola.opy",
        "main": "examples/parabola.opy",
        "files": ["examples/parabola.opy"],
    },
    {
        "id": "real-world/overpy-crosshair",
        "repo": "Zezombye/overpy",
        "commit": OVERPY_COMMIT,
        "license": "GPL-3.0-only",
        "licensePath": "LICENSE",
        "origin": "Zezombye/overpy examples/crosshair.opy",
        "main": "examples/crosshair.opy",
        "files": ["examples/crosshair.opy"],
    },
    {
        "id": "real-world/overpy-inputhud",
        "repo": "Zezombye/overpy",
        "commit": OVERPY_COMMIT,
        "license": "GPL-3.0-only",
        "licensePath": "LICENSE",
        "origin": "Zezombye/overpy examples/inputhud.opy",
        "main": "examples/inputhud.opy",
        "files": ["examples/inputhud.opy"],
    },
    {
        "id": "real-world/ow1-emulator",
        "repo": "Overwatch-1-Emulator/ow1-emulator",
        "commit": "25cd6ce8d4acdd64b66c862a55c7ed66c8e50af1",
        "license": "BSD-2-Clause",
        "licensePath": "LICENSE",
        "origin": "Overwatch-1-Emulator/ow1-emulator src/1v1_main.opy (full include closure)",
        "main": "src/1v1_main.opy",
        "closure": True,
    },
    {
        "id": "real-world/6v6-adjustments",
        "repo": "6v6-Adjustments/6v6-adjustments",
        "commit": "624480db6b7494f8bd5f3ab68fbb7e96a7726702",
        "license": "BSD-2-Clause",
        "licensePath": "LICENSE",
        "origin": "6v6-Adjustments/6v6-adjustments src/main.opy (full include closure, dev branch pinned)",
        "main": "src/main.opy",
        "closure": True,
    },
]

#: Sources that cannot be committed under the oracle runner's layout
#: constraints are recorded metadata-only (hashes and instructions) instead.
#: WallerTrevor/zombies main (antarctic peninsula) uses ``../../`` includes
#: relative to a two-level-deep entry point, which cannot be reproduced with
#: the unmodified runner's ``--main-file <basename>`` resolution.
DEFERRED: list[dict[str, Any]] = [
    {
        "id": "real-world/zombies",
        "repo": "WallerTrevor/zombies",
        "commit": "9394dd3026e4240f800dd82d477c743567aa7141",
        "license": "GPL-3.0-only",
        "licensePath": "LICENSE",
        "origin": (
            "WallerTrevor/zombies Zombies collection/PvE 2.0 all maps/"
            "antarctic peninsula/Mainfile/Main.opy (include closure)"
        ),
        "main": (
            "Zombies collection/PvE 2.0 all maps/antarctic peninsula/"
            "Mainfile/Main.opy"
        ),
        "reason": (
            "main file sits two levels deep and includes via ../../ paths; the "
            "oracle runner (run_oracle.py, unmodifiable for M11) resolves the "
            "main file as <fixture-dir>/<basename>, so the include closure "
            "cannot be relocated inside a fixture directory without editing "
            "the sources. Recorded metadata-only with content hashes."
        ),
    },
]


def fixture_dir(target: Path, fixture_id: str) -> Path:
    return target / fixture_id


def write_fixture_json(
    fixture_dir: Path, fixture: dict[str, Any], files_sha256: dict[str, str]
) -> None:
    main_name = fixture["main"].rsplit("/", 1)[-1]
    provenance = {
        "kind": "imported-example",
        "origin": fixture["origin"],
        "sourceUrl": (
            f"https://github.com/{fixture['repo']}/blob/{fixture['commit']}/{fixture['main']}"
        ),
        "sourceCommit": fixture["commit"],
        "license": fixture["license"],
        "licenseUrl": (
            f"https://github.com/{fixture['repo']}/blob/{fixture['commit']}/{fixture['licensePath']}"
        ),
        "redistributable": True,
        "modifications": "none",
    }
    metadata: dict[str, Any] = {
        "schemaVersion": 1,
        "id": fixture["id"],
        "category": "real-world",
        "features": [],
        "source": main_name,
        "expectedStatus": "success",
        "acquisitionMethod": (
            "scripts/acquire-corpus.py: GitHub archive of the pinned commit "
            "(codeload.tar.gz), include-closure resolution matching the pinned "
            "oracle, SHA-256 verification"
        ),
        "files": {path: files_sha256[path] for path in files_sha256},
        "provenance": provenance,
    }
    metadata_path = fixture_dir / "fixture.json"
    if metadata_path.is_file():
        existing = json.loads(metadata_path.read_text())
        for key in ("features", "expectedStatus", "runtimeSeconds", "provenanceNote"):
            if key in existing:
                metadata[key] = existing[key]
        for path in existing.get("files", {}):
            if path not in metadata["files"]:
                raise RuntimeError(
                    f"{fixture['id']}: committed file {path!r} not in acquisition set"
                )
    write_json(metadata_path, metadata)


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def acquire_fixture(target: Path, fixture: dict[str, Any], repo: Repo) -> dict[str, Any]:
    if "files" in fixture:
        repo_paths = list(fixture["files"])
    elif fixture.get("closure"):
        repo_paths = resolve_closure(repo, fixture["main"])
    else:
        repo_paths = [fixture["main"]]
    main_name = fixture["main"].rsplit("/", 1)[-1]
    if fixture["main"] not in repo_paths:
        raise RuntimeError(f"{fixture['id']}: main not in file set")
    if "files" in fixture and repo_paths != fixture["files"]:
        raise RuntimeError(f"{fixture['id']}: file set does not match fixture table")

    fdir = fixture_dir(target, fixture["id"])
    records = []
    for repo_path in repo_paths:
        relative = relocate(fixture["main"], repo_path)
        if relative != main_name and main_name in relative.split("/"):
            raise RuntimeError(f"{fixture['id']}: main name collision at {relative}")
        data = repo.read(repo_path)
        dest = (fdir / relative).resolve()
        if not str(dest).startswith(str(fdir.resolve())):
            raise RuntimeError(f"{fixture['id']}: destination escapes fixture dir: {relative}")
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
        records.append(
            {
                "path": relative,
                "repoPath": repo_path,
                "sha256": sha256_bytes(data),
                "size": len(data),
            }
        )

    license_lines = verify_license(repo, fixture["licensePath"], fixture["license"])
    files_map = {r["path"]: r["sha256"] for r in records}

    write_fixture_json(fdir, fixture, files_map)
    return {
        "id": fixture["id"],
        "repo": fixture["repo"],
        "commit": fixture["commit"],
        "license": fixture["license"],
        "licenseFirstLines": license_lines,
        "main": fixture["main"],
        "files": sorted(records, key=lambda r: r["path"]),
        "filesSha256": files_map,
    }


def verify_fixture(target: Path, fixture: dict[str, Any], repo: Repo) -> None:
    fdir = fixture_dir(target, fixture["id"])
    metadata = json.loads((fdir / "fixture.json").read_text())
    recorded = metadata["files"]
    for path in fdir.rglob("*"):
        if not path.is_file():
            continue
        rel = path.relative_to(fdir).as_posix()
        if rel in ("fixture.json", "oracle.json"):
            continue
        if rel not in recorded:
            raise RuntimeError(f"{fixture['id']}: unrecorded committed file {rel!r}")
        actual = sha256_file(path)
        if actual != recorded[rel]:
            raise RuntimeError(
                f"{fixture['id']}: {rel!r} sha256 mismatch (recorded {recorded[rel]}, "
                f"actual {actual})"
            )
    for rel in recorded:
        if not (fdir / rel).is_file():
            raise RuntimeError(f"{fixture['id']}: recorded file missing: {rel!r}")
    print(f"VERIFY PASS {fixture['id']} ({len(recorded)} files)")


def record_deferred(repo: Repo, entry: dict[str, Any]) -> dict[str, Any]:
    """Metadata-only record: hashes and instructions, no committed sources."""
    closure = resolve_closure(repo, entry["main"])
    license_lines = verify_license(repo, entry["licensePath"], entry["license"])
    files = [
        {
            "path": repo_path,
            "repoPath": repo_path,
            "sha256": sha256_bytes(repo.read(repo_path)),
            "size": len(repo.read(repo_path)),
        }
        for repo_path in closure
    ]
    return {
        "id": entry["id"],
        "repo": entry["repo"],
        "commit": entry["commit"],
        "license": entry["license"],
        "licenseFirstLines": license_lines,
        "main": entry["main"],
        "committed": False,
        "reason": entry["reason"],
        "acquisition": (
            f"python3 scripts/acquire-corpus.py downloads "
            f"https://codeload.github.com/{entry['repo']}/tar.gz/{entry['commit']}; "
            "no fixture dir is created and no source is committed"
        ),
        "files": sorted(files, key=lambda r: r["path"]),
        "filesSha256": {r["path"]: r["sha256"] for r in files},
    }


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", type=Path, default=DEFAULT_TARGET)
    parser.add_argument(
        "--verify-only",
        action="store_true",
        help="verify committed fixtures against recorded hashes without downloading",
    )
    args = parser.parse_args()

    target = args.target.resolve()
    if not target.is_dir():
        raise SystemExit(f"target is not a directory: {target}")

    if args.verify_only:
        for fixture in FIXTURES:
            if not fixture_dir(target, fixture["id"]).is_dir():
                print(f"SKIP {fixture['id']} (not acquired here)")
                continue
            verify_fixture(target, fixture, Repo(fixture["repo"], fixture["commit"]))
        return 0

    old_notes: dict[str, str] = {}
    if MANIFEST_PATH.is_file():
        for record in json.loads(MANIFEST_PATH.read_text()).get("fixtures", []):
            if "note" in record:
                old_notes[record["id"]] = record["note"]

    manifest: dict[str, Any] = {
        "schemaVersion": 1,
        "procedure": (
            "download the GitHub archive (tar.gz) of each pinned commit from "
            "codeload.github.com, resolve include closures with oracle "
            "semantics, copy files into the fixture directory, and record "
            "SHA-256 per file; see scripts/acquire-corpus.py"
        ),
        "fixtures": [],
    }
    for fixture in FIXTURES:
        repo = Repo(fixture["repo"], fixture["commit"])
        print(f"acquiring {fixture['id']} from {fixture['repo']}@{fixture['commit'][:12]}")
        record = acquire_fixture(target, fixture, repo)
        if fixture["id"] in old_notes:
            record["note"] = old_notes[fixture["id"]]
        manifest["fixtures"].append(record)
        print(f"  {len(record['files'])} files, license {record['license']} verified")
        for file_record in record["files"]:
            print(f"    {file_record['size']:>8} {file_record['sha256'][:16]} {file_record['path']}")

    zombies = Repo("WallerTrevor/zombies", "9394dd3026e4240f800dd82d477c743567aa7141")
    print("recording deferred fixture real-world/zombies (metadata only)")
    deferred = record_deferred(zombies, DEFERRED[0])
    if deferred["id"] in old_notes:
        deferred["note"] = old_notes[deferred["id"]]
    manifest["fixtures"].append(deferred)

    write_json(MANIFEST_PATH, manifest)
    print(f"manifest written to {MANIFEST_PATH}")

    for fixture in FIXTURES:
        verify_fixture(target, fixture, Repo(fixture["repo"], fixture["commit"]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
