"""Publishable results page (#467): one table of Wright Agent Scores for every evaluated agent, as Markdown, a self-contained HTML page, and JSON."""

from __future__ import annotations

import html
import json
import re
from collections import Counter
from datetime import date
from pathlib import Path

import bench_report

TRACKS = (("workshop", "Workshop"), ("opy", "OverPy"))


def suite_shape(data: dict) -> tuple[int | None, int | None]:
    """(scenarios per language, trials per scenario) when every run used the same suite shape, else (None, None)."""
    counts = {(t["scenarios"], t["trials"]) for e in data["entries"] for t in e["tracks"].values()}
    return next(iter(counts)) if len(counts) == 1 else (None, None)


def reading(data: dict) -> list[str]:
    scenarios, trials = suite_shape(data)
    tasks = f"{scenarios} realistic Workshop tasks" if scenarios else "the held-out tasks"
    times = f"{trials} times" if trials else "a fixed number of times"
    return [
        f"The score is the share of tasks an agent completed with a valid, safe result, averaged over {tasks} per language, each tried {times}.",
        f"The bar shows the score; the range in brackets is the 95% confidence interval. With {scenarios or 'few'} tasks the range is wide: agents marked **tied with top** cannot be told apart from the first row.",
        "It is a reference for how an agent behaves with Wright and its guide, not a measure of general ability, and a model's score depends on the agent program that runs it.",
    ]


def limits(data: dict) -> list[str]:
    scenarios, _ = suite_shape(data)
    networks = {n for e in data["entries"] for n in e["network"]}
    network = ("The harness verified that the network was unreachable on every run." if networks == {"canary-checked"}
               else "Network access was switched off by instruction only for at least some runs; the harness did not block it.")
    return [
        network,
        "A run that hit the time limit counts as a failure. Runs cut off by provider outages are retried, not counted.",
        f"The suite has {scenarios or 'few'} test tasks per language, so differences of a few points mean nothing.",
    ]


def load_entries(dirs: list[Path]) -> list[dict]:
    """One entry per evaluation run directory that has a score.json."""
    entries = []
    for directory in dirs:
        path = directory / "score.json"
        if not path.is_file():
            continue
        cards = {c["language"]: c for c in json.loads(path.read_text())["cards"] if "refused" not in c}
        if not cards:
            continue
        first = next(iter(cards.values()))
        ident = first["identity"]
        info = next((r.get("agentInfo") or {} for r in bench_report.load([directory]) if r.get("agentInfo")), {})
        entries.append({
            "run": directory.name,
            "harness": info.get("agent") or ident["agent"], "harnessVersion": info.get("version"),
            "model": info.get("model") or ident.get("model"), "effort": info.get("effort") or ident.get("effort"),
            "tracks": {lang: {"score": c["score"], "ci": c["ci95"], "trials": c["trialsPerScenario"], "scenarios": c["scenarios"], "passPowK": c["passPowK"], "provisional": c["provisional"], "exclusions": c["exclusions"]} for lang, c in cards.items()},
            "environment": {"wrightSha256": ident["wrightSha256"], "wright": ident["wright"], "skills": ident["skills"], "suite": first["suite"].get("hash")},
            "network": first["networkEnforcement"], "harnessCommit": first["harness"],
        })
    return entries


def mean_score(entry: dict) -> float:
    scores = [t["score"] for t in entry["tracks"].values()]
    return sum(scores) / len(scores)


def overlaps(a: list[float], b: list[float]) -> bool:
    return a[1] >= b[0] and a[0] <= b[1]


def standing(entry: dict, leader: dict) -> str:
    """`top`, `tied with top` when every track's interval overlaps the leader's, else `below top`."""
    if entry is leader:
        return "top"
    shared = [lang for lang in entry["tracks"] if lang in leader["tracks"]]
    return "tied with top" if shared and all(overlaps(entry["tracks"][l]["ci"], leader["tracks"][l]["ci"]) for l in shared) else "below top"


def build(dirs: list[Path]) -> dict:
    entries = load_entries(dirs)
    if not entries:
        return {"entries": [], "excluded": [], "environment": None, "date": date.today().isoformat()}
    coverage = max(len(e["tracks"]) for e in entries)
    covered, partial = [e for e in entries if len(e["tracks"]) == coverage], [e for e in entries if len(e["tracks"]) < coverage]
    key = lambda e: json.dumps(e["environment"], sort_keys=True)
    main_key = Counter(key(e) for e in covered).most_common(1)[0][0]
    ranked = sorted([e for e in covered if key(e) == main_key], key=mean_score, reverse=True)
    for entry in ranked:
        entry["standing"] = standing(entry, ranked[0])
    other = [{"run": e["run"], "reason": "made against a different Wright binary, skills, or task suite"} for e in covered if key(e) != main_key]
    other += [{"run": e["run"], "reason": f"covers {len(e['tracks'])} of {coverage} language tracks"} for e in partial]
    return {"entries": ranked, "excluded": other, "environment": ranked[0]["environment"], "date": date.today().isoformat()}


def label(entry: dict) -> str:
    version = re.sub(r" \([0-9a-f]{7,}\)$", "", entry["harnessVersion"] or "")  # the build hash adds nothing for a reader
    return version if version.startswith(entry["harness"]) else " ".join(filter(None, (entry["harness"], version)))


def bar(score: float, width: int = 10) -> str:
    filled = round(score / 100 * width)
    return "█" * filled + "░" * (width - filled)


def markdown(data: dict) -> str:
    env = data["environment"]
    lines = ["# Wright Agent Score", "", f"How well coding agents work on real Overwatch Workshop projects with Wright. Results of {data['date']}.", ""]
    if not data["entries"]:
        return "\n".join(lines + ["No results yet.", ""])
    lines += ["| # | Agent | Model | Effort | " + " | ".join(f"{n} score" for _, n in TRACKS) + " | Against the top |", "| --- | --- | --- | --- | " + " | ".join("---" for _ in TRACKS) + " | --- |"]
    cell = lambda s: str(s if s is not None else "not recorded").replace("|", "\\|")  # a raw | would break the table row
    for rank, e in enumerate(data["entries"], 1):
        cells = []
        for lang, _ in TRACKS:
            t = e["tracks"].get(lang)
            cells.append(f"`{bar(t['score'])}` **{t['score']:.0f}** ({t['ci'][0]:.0f}–{t['ci'][1]:.0f})" + (" ⚠️" if t["provisional"] else "") if t else "n/a")
        lines.append(f"| {rank} | {cell(label(e))} | {cell(e['model'])} | {cell(e['effort'] or 'default')} | " + " | ".join(cells) + f" | {e['standing']} |")
    lines += ["", "## How to read this", "", *(f"- {s}" for s in reading(data)), "", "## Limits", "", *(f"- {s}" for s in limits(data))]
    if any(t["provisional"] for e in data["entries"] for t in e["tracks"].values()):
        lines.append("- ⚠️ marks a score that is provisional because the run did not cover every task or had unequal trials.")
    skills = ", ".join(f"{k} `{v[:8]}`" for k, v in (env["skills"] or {}).items()) or "none"
    commits = ", ".join(f"`{c[:8]}`" for c in sorted({c for e in data["entries"] for c in e["harnessCommit"]})) or "not recorded"
    lines += ["", "## What was run", "", f"- Wright: {env['wright'] or 'not recorded'} (sha256 `{(env['wrightSha256'] or 'not recorded')[:12]}`)", f"- Skills: {skills}", f"- Task suite: `{(env['suite'] or 'not recorded')[:12]}`",
              f"- Harness commit: {commits}", "- Scores come from the benchmark in `benchmarks/agent`; the run directories hold every result.json."]
    if data["excluded"]:
        lines += ["", "## Not comparable", "", *(f"- `{o['run']}`: {o['reason']}." for o in data["excluded"])]
    return "\n".join(lines) + "\n"


def page(data: dict) -> str:
    esc = html.escape
    rows = []
    for rank, e in enumerate(data["entries"], 1):
        cells = []
        for lang, _ in TRACKS:
            t = e["tracks"].get(lang)
            if not t:
                cells.append("<td>n/a</td>")
                continue
            lo, hi = t["ci"]
            cells.append(f'<td><div class="bar"><span class="fill" style="width:{t["score"]:.1f}%"></span><span class="ci" style="left:{lo:.1f}%;width:{max(hi - lo, 0.5):.1f}%"></span></div>'
                         f'<b>{t["score"]:.0f}</b> <small>{lo:.0f}–{hi:.0f}</small>{" ⚠️" if t["provisional"] else ""}</td>')
        rows.append(f'<tr class="{esc(e["standing"].split()[0])}"><td>{rank}</td><td><b>{esc(label(e))}</b></td><td>{esc(str(e["model"] or "not recorded"))}</td><td>{esc(e["effort"] or "default")}</td>{"".join(cells)}<td>{esc(e["standing"])}</td></tr>')
    env = data["environment"] or {}
    skills = ", ".join(f"{k} {v[:8]}" for k, v in (env.get("skills") or {}).items()) or "none"
    head = "".join(f"<th>{n} score</th>" for _, n in TRACKS)
    body = (f"<table><thead><tr><th>#</th><th>Agent</th><th>Model</th><th>Effort</th>{head}<th>Against the top</th></tr></thead><tbody>{''.join(rows)}</tbody></table>"
            if rows else "<p>No results yet.</p>")
    li = lambda items: "".join(f"<li>{esc(s.replace('**', ''))}</li>" for s in items)
    excluded = ("<section><h2>Not comparable</h2><ul>" + "".join(f"<li><code>{esc(o['run'])}</code>: {esc(o['reason'])}</li>" for o in data["excluded"]) + "</ul></section>" if data["excluded"] else "")
    prose = (f"<section><h2>How to read this</h2><ul>{li(reading(data))}</ul></section><section><h2>Limits</h2><ul>{li(limits(data))}</ul></section>" if data["entries"] else "")
    sha12 = lambda v: esc(str(v or "not recorded")[:12])
    return f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Wright Agent Score</title>
<style>
:root{{--bg:#fff;--fg:#1a1a1a;--muted:#667;--line:#e3e3e8;--bar:#2f6fed;--ci:#1a1a1a;--soft:#f5f6f8}}
@media (prefers-color-scheme:dark){{:root{{--bg:#101114;--fg:#ececf0;--muted:#9a9aa6;--line:#2a2b31;--bar:#6f9bff;--ci:#ececf0;--soft:#17181c}}}}
body{{margin:0;background:var(--bg);color:var(--fg);font:16px/1.5 system-ui,sans-serif}}main{{max-width:980px;margin:0 auto;padding:32px 16px}}
h1{{margin:0 0 4px}}p.sub{{color:var(--muted);margin:0 0 24px}}table{{width:100%;border-collapse:collapse}}th,td{{padding:10px 8px;border-bottom:1px solid var(--line);text-align:left;vertical-align:middle}}
th{{font-size:13px;color:var(--muted);font-weight:600}}.bar{{position:relative;height:10px;background:var(--soft);border-radius:5px;min-width:120px;margin-bottom:4px}}
.fill{{position:absolute;inset:0 auto 0 0;background:var(--bar);border-radius:5px}}.ci{{position:absolute;top:-3px;height:16px;border:2px solid var(--ci);border-top:0;border-bottom:0;opacity:.55;box-sizing:border-box}}
tr.top td:first-child{{font-weight:700}}small,.muted{{color:var(--muted)}}section{{margin-top:28px;background:var(--soft);border-radius:8px;padding:4px 16px 12px}}li{{margin:6px 0}}code{{font-size:13px}}
@media (max-width:640px){{th:nth-child(4),td:nth-child(4){{display:none}}}}
</style></head><body><main>
<h1>Wright Agent Score</h1><p class="sub">How well coding agents work on real Overwatch Workshop projects with Wright. Results of {esc(data.get("date", ""))}.</p>
{body}
{prose}{excluded}
<section><h2>What was run</h2><p class="muted">Wright {esc(str(env.get("wright") or "not recorded"))} · sha256 <code>{sha12(env.get("wrightSha256"))}</code> · skills {esc(skills)} · task suite <code>{sha12(env.get("suite"))}</code></p></section>
</main></body></html>
"""


def main(dirs: list[Path], out: Path) -> int:
    data = build(dirs)
    out.mkdir(parents=True, exist_ok=True)
    (out / "LEADERBOARD.md").write_text(markdown(data))
    (out / "leaderboard.html").write_text(page(data))
    (out / "leaderboard.json").write_text(json.dumps(data, indent=2) + "\n")
    print(markdown(data))
    print(f"wrote {out / 'LEADERBOARD.md'}, {out / 'leaderboard.html'}, {out / 'leaderboard.json'}")
    return 0 if data["entries"] else 1
