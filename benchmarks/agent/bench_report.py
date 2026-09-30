"""Aggregate agent benchmark results (#414, SPEC-414): intervals, paired comparison, efficiency, diagnostics."""

from __future__ import annotations

import json
import math
import re
from collections import defaultdict
from pathlib import Path
from statistics import mean, pstdev

BASELINE = "none/none/off"
HEADROOM = 0.95


def wilson(k: int, n: int, z: float = 1.96) -> tuple[float, float]:
    if n == 0:
        return (0.0, 0.0)
    p = k / n
    denom = 1 + z * z / n
    centre = (p + z * z / (2 * n)) / denom
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / denom
    return (max(0.0, centre - half), min(1.0, centre + half))


def label(result: dict) -> str:
    c = result["condition"]
    return f"{c['wright']}/{c['knowledge']}/{c['network']}"


def load(dirs: list[Path]) -> list[dict]:
    results = []
    for base in dirs:
        for path in sorted(base.rglob("result.json")):
            result = json.loads(path.read_text())
            if str(result.get("contract", "")).startswith("wright-agent-bench/"):
                result["_dir"] = path.parent
                match = re.search(r"-(\d+)$", path.parent.name)
                result["_trial"] = int(match.group(1)) if match else 0
                results.append(result)
    return results


def rate(k: int, n: int) -> str:
    lo, hi = wilson(k, n)
    return f"{k}/{n} [{lo:.2f}-{hi:.2f}]"


def total_tokens(result: dict) -> int | None:
    return (result.get("usage") or {}).get("totalTokens")


def group_rows(runs: list[dict]) -> dict:
    usable = [r for r in runs if r.get("usable")]
    tokens = [t for t in (total_tokens(r) for r in runs) if t is not None]
    peaks = [p for p in ((r.get("usage") or {}).get("peakContext") for r in runs) if p]
    return {
        "n": len(runs), "usable": len(usable), "passed": sum(1 for r in runs if r.get("passed")),
        "tokens": mean(tokens) if tokens else None,
        "tokensPerUsable": (sum(tokens) / len(usable)) if tokens and usable and len(tokens) == len(runs) else None,
        "peakContext": mean(peaks) if peaks else None,
        "seconds": mean(r["agent"]["seconds"] for r in runs),
        "usedWright": sum(1 for r in runs if r.get("wrightUse", {}).get("invocations")),
    }


def fmt(value, digits: int = 0) -> str:
    return "n/a" if value is None else f"{value:,.{digits}f}"


def paired(runs: list[dict]) -> list[str]:
    by_key: dict[tuple, dict] = {(r["scenario"], r["agent"]["id"], r["_trial"], label(r)): r for r in runs}
    lines = []
    cells = sorted({label(r) for r in runs} - {BASELINE})
    for agent in sorted({r["agent"]["id"] for r in runs}):
        for cell in cells:
            pairs = [(by_key[(s, a, t, BASELINE)], r) for (s, a, t, c), r in by_key.items() if a == agent and c == cell and (s, a, t, BASELINE) in by_key]
            if not pairs:
                continue
            gain = sum(1 for b, r in pairs if r.get("usable") and not b.get("usable"))
            loss = sum(1 for b, r in pairs if b.get("usable") and not r.get("usable"))
            both = [(total_tokens(b), total_tokens(r)) for b, r in pairs if b.get("usable") and r.get("usable") and total_tokens(b) and total_tokens(r)]
            saving = f"{mean(1 - r / b for b, r in both):+.0%} tokens (n={len(both)})" if both else "no both-usable pairs"
            lines.append(f"| {agent} | {cell} vs {BASELINE} | {len(pairs)} | +{gain} / -{loss} | {saving} |")
    return lines


def expectations(runs: list[dict]) -> list[str]:
    lines = []
    for cell in sorted({label(r) for r in runs}):
        cell_runs = [r for r in runs if label(r) == cell and r.get("expectations")]
        if not cell_runs:
            continue
        cells = []
        for eid in sorted({e for r in cell_runs for e in r["expectations"]}):
            statuses = [r["expectations"][eid]["status"] for r in cell_runs if eid in r["expectations"]]
            ok, bad = statuses.count("pass"), statuses.count("fail")
            cells.append(f"{eid} {ok}/{ok + bad}" if ok + bad else f"{eid} -")
        lines.append(f"| {cell} | {' · '.join(cells)} |")
    return lines


def diagnostics(runs: list[dict], invalid: list[dict]) -> list[str]:
    notes = []
    base = [r for r in runs if label(r) == BASELINE]
    if base and sum(1 for r in base if r.get("usable")) / len(base) >= HEADROOM:
        notes.append(f"HEADROOM: baseline `{BASELINE}` usable rate is at least {HEADROOM:.0%}; the tasks cannot show a gain.")
    infra = [r for r in runs if r["agent"]["exit"] != 0 or r.get("infraRetries")]
    if infra:
        notes.append(f"INFRASTRUCTURE: {len(infra)} run(s) exited non-zero, timed out, or needed an infrastructure retry.")
    if invalid:
        notes.append(f"INVALID: {len(invalid)} run(s) excluded ({', '.join(sorted({r['invalid'] for r in invalid}))}).")
    groups: dict[tuple, list[dict]] = defaultdict(list)
    for r in runs:
        groups[(r["scenario"], r["agent"]["id"], label(r))].append(r)
    noisy = [k for k, g in groups.items() if len(g) >= 3 and 0 < sum(1 for r in g if r.get("usable")) < len(g)]
    if noisy:
        notes.append(f"VARIANCE: {len(noisy)} scenario/agent/cell group(s) mix usable and unusable trials; compare with the smallest effect that matters before hillclimbing.")
    return notes


def regrade_notes(runs: list[dict], wright: str, load_scenario) -> list[str]:
    """Grader consistency: grading the same stored workspace twice must give one verdict."""
    import bench_grade
    unstable = []
    for r in runs:
        workspace = r["_dir"] / "workspace"
        if not workspace.is_dir():
            continue
        scenario = load_scenario(r["scenario"])
        first, second = (bench_grade.grade(scenario, workspace, wright, r["_dir"] / f"regrade-{i}") for i in (1, 2))
        if [(c["id"], c["passed"]) for c in first["checks"]] != [(c["id"], c["passed"]) for c in second["checks"]]:
            unstable.append(str(r["_dir"]))
    return [f"GRADER: unstable verdict on {len(unstable)} workspace(s): {unstable}"] if unstable else ["GRADER: consistent on every regraded workspace."]


def render(results: list[dict], regrade: list[str] | None = None) -> tuple[str, dict]:
    invalid = [r for r in results if "invalid" in r]
    runs = [r for r in results if "invalid" not in r]
    out = ["# Agent benchmark report", "", f"{len(runs)} valid run(s), {len(invalid)} invalid.", ""]
    summary: dict = {"cells": {}}
    out += ["## Outcome by agent and condition", "", "| agent | condition | runs | usable | passed | used wright | tokens/run | tokens per usable | peak context | s/run |", "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"]
    for agent in sorted({r["agent"]["id"] for r in runs}):
        for cell in sorted({label(r) for r in runs}):
            group = [r for r in runs if r["agent"]["id"] == agent and label(r) == cell]
            if not group:
                continue
            row = group_rows(group)
            summary["cells"][f"{agent}|{cell}"] = row
            out.append(f"| {agent} | {cell} | {row['n']} | {rate(row['usable'], row['n'])} | {row['passed']}/{row['n']} | {row['usedWright']}/{row['n']} | "
                       f"{fmt(row['tokens'])} | {fmt(row['tokensPerUsable'])} | {fmt(row['peakContext'])} | {fmt(row['seconds'], 1)} |")
    out += ["", "## By scenario", "", "| scenario | agent | condition | usable |", "| --- | --- | --- | --- |"]
    groups: dict[tuple, list[dict]] = defaultdict(list)
    for r in runs:
        groups[(r["scenario"], r["agent"]["id"], label(r))].append(r)
    for (scenario, agent, cell), g in sorted(groups.items()):
        out.append(f"| {scenario} | {agent} | {cell} | {rate(sum(1 for r in g if r.get('usable')), len(g))} |")
    splits = sorted({r["split"] for r in runs if r.get("split")})
    if splits:
        out += ["", "## By split", "", "| split | condition | usable |", "| --- | --- | --- |"]
        for split in splits:
            for cell in sorted({label(r) for r in runs}):
                g = [r for r in runs if r.get("split") == split and label(r) == cell]
                if g:
                    out.append(f"| {split} | {cell} | {rate(sum(1 for r in g if r.get('usable')), len(g))} |")
    pairs = paired(runs)
    if pairs:
        out += ["", f"## Paired against `{BASELINE}` (same scenario, agent, trial)", "", "| agent | comparison | pairs | usable gained/lost | tokens where both usable |", "| --- | --- | --- | --- | --- |", *pairs]
    exp = expectations(runs)
    if exp:
        out += ["", "## Expectation rates (pass/(pass+fail); n/a and unavailable excluded)", "", "| condition | expectations |", "| --- | --- |", *exp]
    disagreements = [(r["scenario"], r["disagreement"], str(r["_dir"])) for r in runs if r.get("disagreement")]
    if disagreements:
        out += ["", "## Authority disagreements (candidate owner issues)", ""]
        out += [f"- `{s}` {d['kind']}: wright `{d['wright'].get('error') or d['wright']['status']}` / oracle `{d['oracle'].get('error') or d['oracle']['status']}` ({path})" for s, d, path in disagreements]
    fr = defaultdict(lambda: defaultdict(int))
    for r in runs:
        for k, v in (r.get("friction") or {}).items():
            if isinstance(v, int) and k != "callsToFirstSuccess":
                fr[label(r)][k] += v
    if fr:
        keys = sorted({k for v in fr.values() for k in v})
        out += ["", "## Friction (sum over runs)", "", f"| condition | {' | '.join(keys)} |", f"| --- | {' | '.join('---' for _ in keys)} |"]
        out += [f"| {cell} | {' | '.join(str(v.get(k, 0)) for k in keys)} |" for cell, v in sorted(fr.items())]
    tok: dict[str, dict[str, list[int]]] = defaultdict(lambda: defaultdict(list))
    for r in runs:
        for cmd, t in (r.get("wrightUse", {}).get("outputTokensEstimate") or {}).items():
            tok[cmd]["tokens"].append(t)
    if tok:
        out += ["", "## Wright output size per command (estimated tokens per run that used it)", "", "| command | runs | mean | max |", "| --- | --- | --- | --- |"]
        out += [f"| {cmd} | {len(v['tokens'])} | {mean(v['tokens']):.0f} | {max(v['tokens'])} |" for cmd, v in sorted(tok.items())]
    notes = diagnostics(runs, invalid) + (regrade or [])
    out += ["", "## Diagnostics", ""] + ([f"- {n}" for n in notes] or ["- none"])
    summary["diagnostics"] = notes
    summary["stdev"] = {k: pstdev([r["agent"]["seconds"] for r in runs if f"{r['agent']['id']}|{label(r)}" == k]) for k in summary["cells"]} if runs else {}
    return "\n".join(out) + "\n", summary


def main(dirs: list[Path], wright: str, regrade: bool, load_scenario) -> int:
    results = load(dirs)
    if not results:
        print("no results found")
        return 1
    notes = regrade_notes([r for r in results if "invalid" not in r], wright, load_scenario) if regrade else None
    text, summary = render(results, notes)
    (dirs[0] / "report.md").write_text(text)
    (dirs[0] / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(text)
    return 0
