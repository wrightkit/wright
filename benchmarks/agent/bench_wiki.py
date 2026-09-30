"""Snapshot of the Workshop wiki Markdown mirror for the benchmark's `wiki` knowledge level (#414, SPEC-414)."""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
import time
from datetime import datetime, timezone
from pathlib import Path

BASE = "https://md.wrightkit.dev"
USER_AGENT = "wright-agent-bench/1 (+https://github.com/wrightkit/wright)"
SLUG = re.compile(r"^[A-Za-z0-9_-]+$")
NOTICE = (
    "Workshop.codes wiki content, rendered to Markdown by the mirror recorded in SNAPSHOT.json.\n"
    "Use and redistribution follow the Workshop.codes Terms of Service (https://workshop.codes/tos).\n"
    "This snapshot is for local benchmark runs; do not commit or redistribute it.\n"
)


def fetch(url: str) -> bytes:
    """GET through curl: the mirror rejects Python's HTTP client fingerprint with 403."""
    proc = subprocess.run(["curl", "-fsSL", "-m", "30", "-A", USER_AGENT, url], capture_output=True)
    if proc.returncode != 0:
        raise SystemExit(f"fetch failed for {url}: {proc.stderr.decode(errors='replace').strip()}")
    return proc.stdout


def snapshot(base: str, out: Path, delay: float = 0.2) -> dict:
    """Fetch the manifest and every article once, and write the files plus SNAPSHOT.json with content hashes."""
    if (out / "SNAPSHOT.json").exists():
        raise SystemExit(f"{out} already holds a snapshot; snapshots are pinned, so choose a new directory")
    manifest_bytes = fetch(f"{base}/manifest.json")
    manifest = json.loads(manifest_bytes)
    if manifest.get("schemaVersion") != 1:
        raise SystemExit(f"unsupported manifest schemaVersion {manifest.get('schemaVersion')!r}")
    articles = out / "articles"
    articles.mkdir(parents=True, exist_ok=True)
    documents = []
    for doc in manifest["documents"]:
        slug = doc["slug"]
        if not SLUG.match(slug):
            raise SystemExit(f"refusing unsafe slug {slug!r}")
        body = fetch(f"{base}/wiki/articles/{slug}")
        (articles / f"{slug}.md").write_bytes(body)
        documents.append({"slug": slug, "title": doc.get("title"), "updatedAt": doc.get("updatedAt"), "sourceUrl": doc.get("sourceUrl"), "sha256": hashlib.sha256(body).hexdigest()})
        time.sleep(delay)
    (out / "index.md").write_bytes(fetch(f"{base}/wiki/articles"))
    (out / "NOTICE.txt").write_text(NOTICE)
    identity = "\n".join(f"{d['slug']} {d['sha256']}" for d in sorted(documents, key=lambda d: d["slug"]))
    record = {
        "source": base, "fetchedAt": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "manifestSha256": hashlib.sha256(manifest_bytes).hexdigest(), "documents": documents,
        "snapshotSha256": hashlib.sha256(identity.encode()).hexdigest(),
    }
    (out / "SNAPSHOT.json").write_text(json.dumps(record, indent=2) + "\n")
    return record


def identity(wiki_dir: Path) -> dict:
    """The pinned identity of a snapshot, recorded in every result that used it."""
    record = json.loads((wiki_dir / "SNAPSHOT.json").read_text())
    return {k: record[k] for k in ("source", "fetchedAt", "snapshotSha256")} | {"documents": len(record["documents"])}
