"""Generate the `workshop-skill` skill from a pinned wiki snapshot (#414, SPEC-414).

The skill is community knowledge that works without Wright: a short SKILL.md, one index per category, and one file
per article. It is built deterministically from `SNAPSHOT.json`; nothing is written by hand except SKILL.md. Spellings
for OverPy come from the workshop-rs catalog and the opy-rs manifest and are kept only when the pinned upstream
compiler's own data contains them, so the skill never invents an alias.
"""

from __future__ import annotations

import hashlib
import json
import re
from datetime import datetime, timezone
from pathlib import Path

import bench_grade
import bench_wiki

KIND = {"actions": "action", "values": "value", "events": "event", "constants": "constant", "references": "reference"}
SKILL_NAME = "workshop-skill"
DESCRIPTION = (
    "Use when you are unsure of the exact name, parameters, or behavior of an Overwatch Workshop action, value, event, "
    "or constant, in raw Workshop script or OverPy, or hit a Workshop quirk such as timing, event semantics, or a HUD or "
    "effect limit. Look it up in the local community wiki notes instead of guessing names or semantics."
)
SKILL_MD = f"""---
name: {SKILL_NAME}
description: {DESCRIPTION}
---

# Workshop wiki notes

Local benchmark material only. The source content remains subject to the Workshop.codes Terms of Service
(https://workshop.codes/tos); this generated skill grants no redistribution or AI-training permission.

Community-written notes on Workshop actions, values, events, constants, and references, stored as one small file per
article. They are not exhaustive and may be out of date: check the `updated` date, and prefer a compiler or validator
result over a note when the two disagree.

## Find a note without reading everything

1. `references/categories.md` lists the categories (a few lines).
2. `references/<category>.md` indexes one category, one line per article: title, the OverPy spelling when known,
   last update, and a short summary. Search it instead of reading it: `grep -i "keyword" references/*.md`.
3. `references/articles/<slug>.md` is the note itself. Read only the ones you need.

## Use them well

- Notes use Workshop names (`Play Effect`). In OverPy the same thing is usually camelCase (`playEffect`); an index line
  shows the OverPy spelling only when it could be verified, so a missing one means unknown, not absent. Control flow
  (`If`, `Loop`), assignments (`Set Global Variable`, `Modify …`), and operators are OverPy syntax, not functions.
- A note describes behavior and pitfalls. It does not replace checking that your code is valid.
- Do not load a whole category index or many articles at once; look up what the task needs.
"""


def norm(text: str) -> str:
    return re.sub(r"[^a-z0-9]", "", text.lower())


def catalog_index(catalog: dict) -> dict[str, dict]:
    """Normalized English name -> item, over actions, values, events, and enum domains."""
    index: dict[str, dict] = {}
    for group in ("actions", "values", "events"):
        for item in catalog[group]:
            index.setdefault(norm(item["aliases"].get("en-US", item["id"])), {"id": item["id"], "group": group})
    for enum in catalog["enums"]:
        index.setdefault(norm(enum["domain"]), {"id": enum["domain"], "group": "enums"})
    return index


def upstream_spellings(manifest: dict) -> dict[str, str]:
    """Workshop catalog id -> the OverPy spelling of the pinned upstream compiler.

    The manifest maps a catalog id to its OverPy function id (they differ, for example `createHudText` and `hudText`),
    and an alias maps a canonical id to the spelling upstream actually uses."""
    upstream = {a["target"]: a["source"] for a in manifest["aliases"]}
    spellings: dict[str, str] = {}
    for function in manifest["functions"]:
        name = upstream.get(function["id"], function["id"])
        spellings.setdefault(function.get("catalogId", function["id"]), name)
    return spellings


def verified(name: str, upstream_source: str) -> bool:
    return re.search(rf"(?<![A-Za-z0-9_]){re.escape(name)}(?![A-Za-z0-9_])", upstream_source) is not None


def summary(body: str, title: str = "") -> str:
    """First sentence of the article text without markup; for a stub article, its first code snippet line."""
    for paragraph in re.split(r"\n\s*\n", re.sub(r"```.*?```", "", body, flags=re.S)):
        text = re.sub(r"\s+", " ", re.sub(r"<[^>]+>|[`*_>#\[\]]|\(https?://[^)]*\)", "", paragraph)).strip()
        if len(text) > 20 and text.lower() != title.lower() and not text.lower().startswith(("source:", "title:", "updated:")):
            sentence = re.split(r"(?<=[.!?])\s", text)[0]
            return sentence[:110].rstrip(" .,;:") + ("…" if len(sentence) > 110 else "")
    snippet = re.search(r"```[^\n]*\n(.+?)\n", body)
    return f"syntax `{snippet.group(1).strip()[:90]}`" if snippet else ""


def article_file(doc: dict, raw: str, opy: str | None, catalog_id: str | None) -> str:
    body = raw.split("\n---\n", 1)[1].lstrip("\n") if raw.startswith("---\n") else raw
    head = [f"title: {doc['title']}", f"category: {', '.join(doc['categories'])}", f"updated: {(doc['updatedAt'] or '')[:10]}", f"content_hash: {doc['contentHash']}"]
    if catalog_id:
        head.append(f"catalog_id: {catalog_id}")
    if opy:
        head.append(f"overpy: {opy}")
    return "---\n" + "\n".join(head) + "\n---\n\n" + body


def build(snapshot: Path, out: Path, catalog: dict, manifest: dict, upstream_source: str) -> dict:
    if out.name != SKILL_NAME:
        raise SystemExit(f"the output directory must be named {SKILL_NAME!r} so it installs under that skill name")
    if out.exists():
        raise SystemExit(f"{out} exists; generated skills are not overwritten")
    record = bench_wiki.load_snapshot(snapshot)
    by_name, spelling = catalog_index(catalog), upstream_spellings(manifest)
    (out / "references/articles").mkdir(parents=True)
    index: dict[str, list[str]] = {}
    stats = {"articles": 0, "catalogMatched": 0, "overpy": 0, "overpyUnverified": 0}
    for doc in record["documents"]:
        raw = (snapshot / "articles" / f"{doc['slug']}.md").read_text()
        match = by_name.get(norm(doc["title"] or doc["slug"]))
        opy = None
        if match:
            candidate = spelling.get(match["id"], match["id"] if match["group"] in ("enums", "events") else None)
            if candidate and verified(candidate, upstream_source):
                opy = candidate
            elif candidate:
                stats["overpyUnverified"] += 1
        (out / "references/articles" / f"{doc['slug']}.md").write_text(article_file(doc, raw, opy, match["id"] if match else None))
        body = raw.split("\n---\n", 1)[1] if raw.startswith("---\n") else raw
        line = f"- [{doc['title']}](articles/{doc['slug']}.md)" + (f" · overpy `{opy}`" if opy else "") + f" · {(doc['updatedAt'] or '')[:7]} · {summary(body, doc['title'] or '')}"
        for category in doc["categories"]:
            index.setdefault(category, []).append(line)
        stats["articles"] += 1
        stats["catalogMatched"] += bool(match)
        stats["overpy"] += bool(opy)
    for category, lines in index.items():
        (out / f"references/{category}.md").write_text(f"# {category} ({len(lines)})\n\n" + "\n".join(sorted(lines, key=str.lower)) + "\n")
    (out / "references/categories.md").write_text("# Categories\n\n" + "\n".join(f"- [{c}]({c}.md): {len(index[c])} {KIND.get(c, c)} notes" for c in sorted(index)) + "\n")
    (out / "SKILL.md").write_text(SKILL_MD)
    build_record = {"name": SKILL_NAME, "snapshotSha256": record["snapshotSha256"], "builtAt": datetime.now(timezone.utc).isoformat(timespec="seconds"), "skillSha256": content_hash(out), **stats}
    (out / "BUILD.json").write_text(json.dumps(build_record, indent=2) + "\n")
    return build_record


def content_hash(skill_dir: Path) -> str:
    return hashlib.sha256("".join(f"{p.relative_to(skill_dir)}{hashlib.sha256(p.read_bytes()).hexdigest()}" for p in sorted(skill_dir.rglob("*"))
                                  if p.is_file() and p.name != "BUILD.json").encode()).hexdigest()  # the build record is written after the hash is taken


def identity(skill_dir: Path) -> dict:
    record = json.loads((skill_dir / "BUILD.json").read_text())
    if record["name"] != SKILL_NAME or content_hash(skill_dir) != record["skillSha256"]:
        raise SystemExit(f"wiki skill content mismatch: {skill_dir}")
    return record


def upstream_source_text() -> str:
    path = bench_grade.ORACLE / "node_modules/overpy/overpy.js"
    if not path.is_file():
        raise SystemExit("upstream oracle not installed; run `agent_bench.py setup-oracle`")
    return path.read_text()
