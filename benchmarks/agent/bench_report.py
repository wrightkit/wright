"""Aggregate agent benchmark results (#414, SPEC-414): intervals, paired comparison, efficiency, diagnostics."""

from __future__ import annotations

import json
import math
import os
import re
from collections import defaultdict
from pathlib import Path
from statistics import mean, pstdev

BASELINE = "none/none/off"
HEADROOM = 0.95


def write_json(path: Path, data: dict) -> None:
    """A kill mid-write leaves no partial JSON to crash the next resume: temp file in the same directory, then replace."""
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_text(json.dumps(data, indent=2) + "\n")
    os.replace(temporary, path)


def wilson(k: int, n: int, z: float = 1.96) -> tuple[float, float]:
    if n == 0:
        return (0.0, 0.0)
    p = k / n
    denom = 1 + z * z / n
    centre = (p + z * z / (2 * n)) / denom
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / denom
    return (max(0.0, centre - half), min(1.0, centre + half))


def label(result: dict) -> str:
    return result["condition"]["label"]


def load(dirs: list[Path]) -> list[dict]:
    results = []
    for base in dirs:
        for path in sorted(base.rglob("result.json")):
            try:
                result = json.loads(path.read_text())
            except json.JSONDecodeError:
                continue  # a partial file means the trial never finished; it will be retried
            if str(result.get("contract", "")).startswith("wright-agent-bench/") and all(k in result for k in ("status", "language", "condition", "scenario", "agent", "environment")):
                result["_dir"] = path.parent
                match = re.search(r"-(\d+)$", path.parent.name)
                result["_trial"] = int(match.group(1)) if match else 0
                results.append(result)
    return results


def rate(k: int, n: int) -> str:
    lo, hi = wilson(k, n)
    return f"{k}/{n} [{lo:.2f}-{hi:.2f}]"


def rate_runs(runs: list[dict]) -> str:
    """Usable rate of a group. With two or more scenarios the interval is a clustered bootstrap, because trials of one scenario
    are correlated; a single scenario falls back to Wilson."""
    k, n = sum(1 for r in runs if r.get("usable")), len(runs)
    per: dict[str, list[int]] = defaultdict(list)
    for r in runs:
        per[r["scenario"]].append(1 if r.get("usable") else 0)
    if len(per) < 2:
        return rate(k, n)
    from bench_score import cluster_interval
    lo, hi = cluster_interval(per)
    return f"{k}/{n} [{lo / 100:.2f}-{hi / 100:.2f}]"


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
        "usedTool": sum(1 for r in runs if any(u["invocations"] for u in r.get("toolUse", {}).values())),
    }


def fmt(value, digits: int = 0) -> str:
    return "n/a" if value is None else f"{value:,.{digits}f}"


def paired(runs: list[dict], reference: str = BASELINE) -> list[str]:
    by_key: dict[tuple, dict] = {(r["scenario"], r["agent"]["id"], r["_trial"], label(r)): r for r in runs}
    lines = []
    cells = sorted({label(r) for r in runs} - {reference})
    for agent in sorted({r["agent"]["id"] for r in runs}):
        for cell in cells:
            pairs = [(by_key[(s, a, t, reference)], r) for (s, a, t, c), r in by_key.items() if a == agent and c == cell and (s, a, t, reference) in by_key]
            if not pairs:
                continue
            gain = sum(1 for b, r in pairs if r.get("usable") and not b.get("usable"))
            loss = sum(1 for b, r in pairs if b.get("usable") and not r.get("usable"))
            both = [(total_tokens(b), total_tokens(r)) for b, r in pairs if b.get("usable") and r.get("usable") and total_tokens(b) and total_tokens(r)]
            saving = f"{mean(1 - r / b for b, r in both):+.0%} tokens (n={len(both)})" if both else "no both-usable pairs"
            lines.append(f"| {agent} | {cell} vs {reference} | {len(pairs)} | +{gain} / -{loss} | {saving} |")
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
    infra = [r for r in runs if r["status"] in ("agent-error", "timeout") or r.get("infraRetries")]
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


def setup_rows(runs: list[dict]) -> list[str]:
    """What each agent was actually given: CLI version, model, tools, and loaded skills, as the adapters observed them."""
    out = ["", "## Agent setup", "", "What the adapters observed, not what was requested. `not recorded` means the CLI does not expose it.", "",
           "| agent | condition | CLI | model | effort | tools | loaded skills |", "| --- | --- | --- | --- | --- | --- | --- |"]
    for agent in sorted({r["agent"]["id"] for r in runs}):
        for cell in sorted({label(r) for r in runs if r["agent"]["id"] == agent}):
            group = [r for r in runs if r["agent"]["id"] == agent and label(r) == cell]
            infos = [r.get("agentInfo") or {} for r in group]
            first = next((i for i in infos if i), {})
            tools = first.get("tools") or first.get("toolsObserved")
            tool_text = "not recorded" if tools is None else f"{len(tools)}: {', '.join(tools[:12])}{' ...' if len(tools) > 12 else ''}" if tools else "none"
            loaded = sorted({s for r in group for s in ((r.get("context") or {}).get("loaded") or [])})
            out.append(f"| {agent} | {cell} | {first.get('version') or 'not recorded'} | {first.get('model') or 'not recorded'} | {first.get('effort') or 'not recorded'} | {tool_text} | {', '.join(loaded) or 'none'} |")
    return out


def render(results: list[dict], regrade: list[str] | None = None, reference: str = BASELINE) -> tuple[str, dict]:
    invalid = [r for r in results if r["status"] == "invalid"]
    infrastructure = [r for r in results if r["status"] == "provider-interrupted"]
    runs = [r for r in results if r["status"] not in ("invalid", "provider-interrupted")]
    out = ["# Agent benchmark report", "", f"{len(runs)} valid run(s), {len(invalid)} invalid, {len(infrastructure)} infrastructure failures excluded.", ""]
    summary: dict = {"cells": {}, "infrastructureFailures": len(infrastructure)}
    out += ["## Outcome by agent and condition", "", "| agent | condition | runs | usable | passed | used a tool | tokens/run | tokens per usable | peak context | s/run |", "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"]
    for agent in sorted({r["agent"]["id"] for r in runs}):
        for cell in sorted({label(r) for r in runs}):
            group = [r for r in runs if r["agent"]["id"] == agent and label(r) == cell]
            if not group:
                continue
            row = group_rows(group)
            summary["cells"][f"{agent}|{cell}"] = row
            out.append(f"| {agent} | {cell} | {row['n']} | {rate_runs(group)} | {row['passed']}/{row['n']} | {row['usedTool']}/{row['n']} | "
                       f"{fmt(row['tokens'])} | {fmt(row['tokensPerUsable'])} | {fmt(row['peakContext'])} | {fmt(row['seconds'], 1)} |")
    out += setup_rows(runs)
    out += ["", "## By scenario", "", "| scenario | agent | condition | usable |", "| --- | --- | --- | --- |"]
    groups: dict[tuple, list[dict]] = defaultdict(list)
    for r in runs:
        groups[(r["scenario"], r["agent"]["id"], label(r))].append(r)
    for (scenario, agent, cell), g in sorted(groups.items()):
        out.append(f"| {scenario} | {agent} | {cell} | {rate(sum(1 for r in g if r.get('usable')), len(g))} |")
    out += ["", "## By language", "", "| language | condition | usable |", "| --- | --- | --- |"]
    for language in sorted({r["language"] for r in runs}):
        for cell in sorted({label(r) for r in runs}):
            g = [r for r in runs if r["language"] == language and label(r) == cell]
            if g:
                out.append(f"| {language} | {cell} | {rate_runs(g)} |")
    splits = sorted({r["split"] for r in runs if r.get("split")})
    if splits:
        out += ["", "## By split", "", "| split | condition | usable |", "| --- | --- | --- |"]
        for split in splits:
            for cell in sorted({label(r) for r in runs}):
                g = [r for r in runs if r.get("split") == split and label(r) == cell]
                if g:
                    out.append(f"| {split} | {cell} | {rate_runs(g)} |")
    pairs = paired(runs, reference)
    if pairs:
        out += ["", f"## Paired against `{reference}` (same scenario, agent, trial)", "", "| agent | comparison | pairs | usable gained/lost | tokens where both usable |", "| --- | --- | --- | --- | --- |", *pairs]
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
        for tool, use in (r.get("toolUse") or {}).items():
            for cmd, t in (use.get("outputTokensEstimate") or {}).items():
                tok[f"{tool} {cmd}"]["tokens"].append(t)
    if tok:
        out += ["", "## Tool output size per command (estimated tokens per run that used it)", "", "| command | runs | mean | max |", "| --- | --- | --- | --- |"]
        out += [f"| {cmd} | {len(v['tokens'])} | {mean(v['tokens']):.0f} | {max(v['tokens'])} |" for cmd, v in sorted(tok.items())]
    notes = diagnostics(runs, invalid) + (regrade or [])
    if infrastructure:
        notes.append(f"INFRASTRUCTURE: {len(infrastructure)} provider/infrastructure failures (exit 75) excluded from outcome metrics.")
        out += ["", "## Provider/infrastructure failures", "", "| agent | scenario | condition | artifact |", "| --- | --- | --- | --- |"]
        out += [f"| {r['agent']['id']} | {r['scenario']} | {label(r)} | {r['_dir']} |" for r in infrastructure]
    missing_context = sum(1 for r in runs if not (r.get("context") or {}).get("reported"))
    if missing_context:
        notes.append(f"CONTEXT: {missing_context} run(s) lack observed loaded-context data; installed skills are not proof of loading.")
    out += ["", "## Diagnostics", ""] + ([f"- {n}" for n in notes] or ["- none"])
    summary["diagnostics"] = notes
    summary["stdev"] = {k: pstdev([r["agent"]["seconds"] for r in runs if f"{r['agent']['id']}|{label(r)}" == k]) for k in summary["cells"]} if runs else {}
    return "\n".join(out) + "\n", summary


def main(dirs: list[Path], wright: str, regrade: bool, load_scenario, reference: str = BASELINE) -> int:
    results = load(dirs)
    if not results:
        print("no results found")
        return 1
    notes = regrade_notes([r for r in results if r["status"] != "invalid"], wright, load_scenario) if regrade else None
    text, summary = render(results, notes, reference)
    (dirs[0] / "report.md").write_text(text)
    write_json(dirs[0] / "summary.json", summary)
    print(text)
    return 0
