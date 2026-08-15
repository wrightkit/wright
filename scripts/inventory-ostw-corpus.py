#!/usr/bin/env python3
"""Regenerate the immutable OSTW corpus file inventory from committed sources."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / "compatibility" / "ostw" / "corpus"
OUTPUT = ROOT / "compatibility" / "ostw" / "corpus.json"

PROJECTS = (
    ("mobawatch", "pharingWell/MOBAwatch", "b9b1ac3b77a484256e89aca6be8c27470803f665", "BSD-2-Clause", "Source Files/main.ostw", ["rules", "imports", "macros", "classes", "arrays", "workshop-calls", "project-settings"]),
    ("protect-ban", "GrandeurHammers/protect-ban", "f8c2353ed8447f13038fbf6b9938031cced5796f", "MIT", "main.ostw", ["rules", "imports", "macros", "arrays", "workshop-calls", "project-settings"]),
)

def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main() -> None:
    projects = []
    for identifier, repository, revision, license_name, entry, categories in PROJECTS:
        root = CORPUS / identifier
        files = []
        for path in sorted(root.rglob("*")):
            if path.is_file():
                files.append({"path": path.relative_to(root).as_posix(), "sha256": digest(path)})
        projects.append({
            "id": identifier,
            "path": f"compatibility/ostw/corpus/{identifier}",
            "repository": repository,
            "revision": revision,
            "license": license_name,
            "licensePath": "LICENSE",
            "sourceKind": "licensed-project-source-and-import-closure",
            "entry": entry,
            "projectSettings": "ds.toml",
            "categories": categories,
            "files": files,
        })
    OUTPUT.write_text(json.dumps({"schemaVersion": 1, "projects": projects}, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if __name__ == "__main__":
    main()
