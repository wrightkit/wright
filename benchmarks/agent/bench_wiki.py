"""Snapshot of the Workshop wiki Markdown mirror for the benchmark's `wiki` knowledge level (#414, SPEC-414)."""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
import time
from concurrent.futures import ThreadPoolExecutor
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


def fetch(url: str, attempts: int = 4) -> bytes:
    """GET through curl: the mirror rejects Python's HTTP client fingerprint with 403. Retries slow or failed requests."""
    error = ""
    for attempt in range(attempts):
        proc = subprocess.run(["curl", "-fsSL", "-m", "60", "-A", USER_AGENT, url], capture_output=True)
        if proc.returncode == 0 and proc.stdout:
            return proc.stdout
        error = proc.stderr.decode(errors="replace").strip() or "empty response"
        time.sleep(2 * (attempt + 1))
    raise SystemExit(f"fetch failed for {url}: {error}")


CATEGORIES = ("actions", "values", "events", "constants", "references")
FRONT = re.compile(r"\A---\n(.*?)\n---\n", re.S)


def front_matter(markdown: bytes) -> dict:
    match = FRONT.match(markdown.decode(errors="replace"))
    fields = {}
    for line in (match.group(1).splitlines() if match else []):
        key, _, value = line.partition(": ")
        fields[key.strip()] = value.strip().strip('"')
    return fields


def category_slugs(base: str, category: str) -> list[str]:
    """Article slugs of one category page. The manifest lists only the first upstream page, so categories are the source."""
    page = fetch(f"{base}/wiki/categories/{category}").decode()
    return list(dict.fromkeys(re.findall(r"\]\([^)]*/wiki/articles/([^)]+)\)", page)))


def snapshot(base: str, out: Path, categories: tuple[str, ...] = CATEGORIES, delay: float = 0.1, workers: int = 4) -> dict:
    """Crawl the given categories once and write the files plus SNAPSHOT.json with content hashes."""
    if (out / "SNAPSHOT.json").exists():
        raise SystemExit(f"{out} already holds a snapshot; snapshots are pinned, so choose a new directory")
    articles = out / "articles"
    articles.mkdir(parents=True, exist_ok=True)
    slugs: dict[str, list[str]] = {}
    for category in categories:
        listing = category_slugs(base, category)
        if not listing:
            raise SystemExit(f"category {category!r} listed no articles")
        for slug in listing:
            if not SLUG.match(slug):
                raise SystemExit(f"refusing unsafe slug {slug!r}")
            slugs.setdefault(slug, []).append(category)

    def one(item: tuple[str, list[str]]) -> dict:
        slug, cats = item
        target = articles / f"{slug}.md"
        body = target.read_bytes() if target.is_file() and target.stat().st_size else fetch(f"{base}/wiki/articles/{slug}")  # resumes an interrupted crawl
        target.write_bytes(body)
        meta = front_matter(body)
        time.sleep(delay)
        return {
            "slug": slug, "categories": cats, "title": meta.get("title"), "updatedAt": meta.get("updated_at"),
            "contentHash": meta.get("content_hash"), "sha256": hashlib.sha256(body).hexdigest(),
        }

    with ThreadPoolExecutor(max_workers=workers) as pool:
        documents = list(pool.map(one, slugs.items()))
    (out / "NOTICE.txt").write_text(NOTICE)
    documents.sort(key=lambda d: d["slug"])
    identity_text = "\n".join(f"{d['slug']} {d['sha256']}" for d in documents)
    record = {
        "source": base, "fetchedAt": datetime.now(timezone.utc).isoformat(timespec="seconds"), "categories": list(categories),
        "documents": documents, "snapshotSha256": hashlib.sha256(identity_text.encode()).hexdigest(),
    }
    (out / "SNAPSHOT.json").write_text(json.dumps(record, indent=2) + "\n")
    return record


def load_snapshot(wiki_dir: Path) -> dict:
    record = json.loads((wiki_dir / "SNAPSHOT.json").read_text())
    for doc in record["documents"]:
        if not SLUG.fullmatch(doc["slug"]):
            raise SystemExit(f"refusing unsafe slug {doc['slug']!r}")
        path = wiki_dir / "articles" / f"{doc['slug']}.md"
        if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != doc["sha256"]:
            raise SystemExit(f"snapshot content mismatch: {path}")
    identity_text = "\n".join(f"{d['slug']} {d['sha256']}" for d in sorted(record["documents"], key=lambda d: d["slug"]))
    if hashlib.sha256(identity_text.encode()).hexdigest() != record["snapshotSha256"]:
        raise SystemExit(f"snapshot identity mismatch: {wiki_dir}")
    return record


def identity(wiki_dir: Path) -> dict:
    """The verified identity of a snapshot, recorded in every result that used it."""
    record = load_snapshot(wiki_dir)
    return {k: record[k] for k in ("source", "fetchedAt", "snapshotSha256")} | {"documents": len(record["documents"])}
