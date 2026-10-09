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
        "correctionRounds": mean_of([r.get("correctionRounds") for r in runs]),
    }


def fmt(value, digits: int = 0) -> str:
    return "n/a" if value is None else f"{value:,.{digits}f}"


def mean_of(values: list) -> float | None:
    values = [v for v in values if v is not None]
    return mean(values) if values else None


def search_reads(result: dict) -> int:
    """Information-gathering operations per run, the same definition at both levels: every wright invocation (CLI calls under
    `bin`, `tools/call` under `mcp` — the shim counts both) plus `bash` calls that ran a search/read shell command. A `bash`
    call that itself invokes the wright CLI is counted once, on the wright side."""
    import bench_trace
    shell = bench_trace.shell_search_reads(result["_dir"] / "transcript.jsonl")
    wright = ((result.get("toolUse") or {}).get("wright") or {}).get("invocations") or 0
    return shell + wright


def level_stats(runs: list[dict]) -> dict:
    """One level's metrics inside a paired group: rates, search/read and tool-call counts, turns, tokens."""
    usage = [(r.get("usage") or {}) for r in runs]
    return {
        "n": len(runs),
        "usable": sum(1 for r in runs if r.get("usable")),
        "passed": sum(1 for r in runs if r.get("passed")),
        "searchReads": mean_of([search_reads(r) for r in runs]),
        "wrightCalls": mean_of([(r.get("toolUse") or {}).get("wright", {}).get("invocations") for r in runs]),
        "bashCalls": mean_of([(r.get("toolCalls") or {}).get("bash") for r in runs]),
        "toolCalls": mean_of([sum(calls.values()) if calls else None for calls in (r.get("toolCalls") for r in runs)]),
        "turns": mean_of([u.get("turns") for u in usage]),
        "tokens": mean_of([total_tokens(r) for r in runs]),
    }


def levels(runs: list[dict]) -> tuple[list[str], list[dict]]:
    """`mcp` vs `bin` paired on (scenario, agent, trial) within one cell: rates with Wilson intervals and efficiency (#474)."""
    by_key: dict[tuple, dict[str, dict]] = defaultdict(dict)
    for r in runs:
        condition = r.get("condition") or {}
        if condition.get("tool") != "wright":
            continue
        key = (r["scenario"], r["agent"]["id"], r["_trial"], condition.get("knowledge"), condition.get("network"), tuple(condition.get("skills") or []))
        by_key[key][condition.get("level", "bin")] = r
    groups: dict[tuple, list[tuple]] = defaultdict(list)
    for (scenario, agent, _trial, knowledge, network, skills), sides in by_key.items():
        if "bin" in sides and "mcp" in sides:
            groups[(agent, skills, knowledge, network)].append((sides["bin"], sides["mcp"]))
    lines, records = [], []
    for (agent, skills, knowledge, network), pairs in sorted(groups.items()):
        cell = f"wright{'+'.join(['', *skills]) if skills else ''}/{knowledge}/{network}"
        bins, mcps = [b for b, _ in pairs], [m for _, m in pairs]
        stats = {level: level_stats(group) for level, group in (("bin", bins), ("mcp", mcps))}
        gain = sum(1 for b, m in pairs if m.get("usable") and not b.get("usable"))
        loss = sum(1 for b, m in pairs if b.get("usable") and not m.get("usable"))
        both = [(total_tokens(b), total_tokens(m)) for b, m in pairs if b.get("usable") and m.get("usable") and total_tokens(b) and total_tokens(m)]
        record = {"agent": agent, "cell": cell, "pairs": len(pairs), "stats": stats, "usableGained": gain, "usableLost": loss,
                  "tokenSaving": mean(1 - m / b for b, m in both) if both else None, "bothUsable": len(both)}
        records.append(record)
        for level in ("bin", "mcp"):
            s = stats[level]
            lines.append(f"| {agent} | {cell} | {level} | {s['n']} | {rate(s['usable'], s['n'])} | {rate(s['passed'], s['n'])} | "
                         f"{fmt(s['searchReads'], 1)} | {fmt(s['wrightCalls'], 1)} | {fmt(s['bashCalls'], 1)} | {fmt(s['toolCalls'], 1)} | {fmt(s['turns'], 1)} | {fmt(s['tokens'])} |")
        saving = f"{record['tokenSaving']:+.0%} tokens (n={len(both)})" if both else "no both-usable pairs"
        lines.append(f"| {agent} | {cell} | Δ paired | {len(pairs)} | +{gain} / -{loss} | — | — | — | — | — | — | {saving} |")
    return lines, records


def discrimination(runs: list[dict]) -> dict[str, dict]:
    """Per-scenario `discrimination` flag (#533), computed from results — never stored in a scenario file. A scenario is
    `smoke` when every condition that ran it got the same 0% or 100% usable rate; it cannot separate conditions. It is
    `indeterminate` with fewer than two conditions (a single condition cannot tell). Anything else is `discriminating`;
    `differingConditions` names the conditions whose usable rate departs from the rest."""
    groups: dict[str, dict[str, list[dict]]] = defaultdict(lambda: defaultdict(list))
    for r in runs:
        groups[r["scenario"]][label(r)].append(r)
    out = {}
    for scenario, conditions in groups.items():
        rates = {cell: sum(1 for r in g if r.get("usable")) / len(g) for cell, g in conditions.items()}
        if len(conditions) < 2:
            flag, differs = "indeterminate", []
        elif len(set(rates.values())) == 1 and next(iter(rates.values())) in (0.0, 1.0):
            flag, differs = "smoke", []
        else:
            flag = "discriminating"
            by_rate: dict[float, list[str]] = defaultdict(list)
            for cell, value in rates.items():
                by_rate[value].append(cell)
            majority = max(len(v) for v in by_rate.values())
            differs = sorted(cell for tied in by_rate.values() if len(tied) < majority for cell in tied) or sorted(rates)
        out[scenario] = {
            "discrimination": flag,
            "conditions": {cell: {"usable": sum(1 for r in g if r.get("usable")), "runs": len(g), "rate": round(rates[cell], 3)}
                           for cell, g in sorted(conditions.items())},
            "differingConditions": differs,
        }
    return out


def pair_identity(run: dict) -> dict:
    """What the two sides of a matched pair must share — binary, suite, agent, model, protocol, enforcement —
    minus `skills`, which belongs to the cell difference being measured."""
    from bench_score import identity_of
    return {k: v for k, v in identity_of(run).items() if k != "skills"}


def paired(runs: list[dict], reference: str = BASELINE, flags: dict[str, dict] | None = None) -> tuple[list[str], list[str]]:
    """Table lines plus notes on pairs dropped as non-comparable.

    Pairs form on (scenario, agent, trial) — possibly across run directories, so both sides must record the same
    environment (`wright_mismatch` only guards repeats inside one directory). A pair whose recorded identity
    differs is dropped and reported, never silently mixed into the lift."""
    from bench_score import cluster_interval
    by_key: dict[tuple, dict] = {(r["scenario"], r["agent"]["id"], r["_trial"], label(r)): r for r in runs}
    discriminating = {s for s, f in (flags or {}).items() if f["discrimination"] == "discriminating"}
    lines, skipped = [], []
    cells = sorted({label(r) for r in runs} - {reference})
    for agent in sorted({r["agent"]["id"] for r in runs}):
        for cell in cells:
            pairs, differed, dropped = [], set(), 0
            for (s, a, t, c), r in by_key.items():
                if a != agent or c != cell or (s, a, t, reference) not in by_key:
                    continue
                b = by_key[(s, a, t, reference)]
                differs = {k for k in pair_identity(b) if pair_identity(b)[k] != pair_identity(r)[k]}
                if differs:
                    differed.update(differs)
                    dropped += 1
                else:
                    pairs.append((b, r))
            if differed:
                skipped.append(f"{agent} · `{cell} vs {reference}`: {dropped} pair(s) dropped — differ in {', '.join(sorted(differed))}")
            if not pairs:
                continue
            gain = sum(1 for b, r in pairs if r.get("usable") and not b.get("usable"))
            loss = sum(1 for b, r in pairs if b.get("usable") and not r.get("usable"))
            narrow = [(b, r) for b, r in pairs if r["scenario"] in discriminating]
            narrow_gain = sum(1 for b, r in narrow if r.get("usable") and not b.get("usable"))
            narrow_loss = sum(1 for b, r in narrow if b.get("usable") and not r.get("usable"))
            deltas: dict[str, list[int]] = defaultdict(list)
            for b, r in pairs:
                deltas[r["scenario"]].append((1 if r.get("usable") else 0) - (1 if b.get("usable") else 0))
            lo, hi = cluster_interval(deltas)
            lift = mean(mean(v) for v in deltas.values()) * 100
            both = [(total_tokens(b), total_tokens(r)) for b, r in pairs if b.get("usable") and r.get("usable") and total_tokens(b) and total_tokens(r)]
            saving = f"{mean(1 - r / b for b, r in both):+.0%} tokens (n={len(both)})" if both else "no both-usable pairs"
            lines.append(f"| {agent} | {cell} vs {reference} | {len(pairs)} | +{gain} / -{loss} | +{narrow_gain} / -{narrow_loss} (n={len(narrow)}) | {lift:+.0f}pp [{lo:+.0f}–{hi:+.0f}] | {saving} |")
    return lines, skipped


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


def public_result(result: dict) -> dict:
    """Private results expose outcomes, never grader text or host artifact paths."""
    from bench_score import is_private, private_id
    if not is_private(result):
        return result
    public = {k: result[k] for k in (
        "status", "language", "condition", "agent", "agentInfo", "environment", "split", "usable", "passed",
        "usage", "toolCalls", "correctionRounds", "infraRetries", "_trial", "_dir", "isPrivate"
    ) if k in result}
    public["isPrivate"] = True
    public["scenario"] = private_id(result)
    public["invalid"] = "private run rejected"
    public["agentInfo"] = {k: v for k, v in (result.get("agentInfo") or {}).items() if k in ("agent", "version", "model", "effort")}
    public["context"] = {"reported": (result.get("context") or {}).get("reported")}
    public["toolCalls"] = {k: v for k, v in (result.get("toolCalls") or {}).items() if k in ("bash", "wright", "mcp")}
    public["toolUse"] = {k: {f: v.get(f, 0) for f in ("invocations", "boundedUses")} for k, v in (result.get("toolUse") or {}).items() if k in ("wright", "overpy")}
    return public


def render(results: list[dict], regrade: list[str] | None = None, references: list[str] | None = None) -> tuple[str, dict]:
    from bench_score import is_private, suite_identity
    has_private = any(is_private(r) for r in results)
    suites = [suite_identity(r.get("environment", {}).get("suite") or {}) for r in results]
    mode = "official" if suites and all(s["mode"] == "official" for s in suites) else "public sample"
    results = [public_result(r) for r in results]
    invalid = [r for r in results if r["status"] == "invalid"]
    infrastructure = [r for r in results if r["status"] == "provider-interrupted"]
    runs = [r for r in results if r["status"] not in ("invalid", "provider-interrupted")]
    out = [f"# Agent benchmark report ({mode})", "", f"{len(runs)} valid run(s), {len(invalid)} invalid, {len(infrastructure)} infrastructure failures excluded.", ""]
    summary: dict = {"mode": mode, "suites": list({json.dumps(s, sort_keys=True): s for s in suites}.values()), "cells": {}, "infrastructureFailures": len(infrastructure)}
    out += ["## Outcome by agent and condition", "", "| agent | condition | runs | usable | passed | used a tool | tokens/run | tokens per usable | peak context | s/run | corr |", "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"]
    for agent in sorted({r["agent"]["id"] for r in runs}):
        for cell in sorted({label(r) for r in runs}):
            group = [r for r in runs if r["agent"]["id"] == agent and label(r) == cell]
            if not group:
                continue
            row = group_rows(group)
            summary["cells"][f"{agent}|{cell}"] = row
            out.append(f"| {agent} | {cell} | {row['n']} | {rate_runs(group)} | {row['passed']}/{row['n']} | {row['usedTool']}/{row['n']} | "
                       f"{fmt(row['tokens'])} | {fmt(row['tokensPerUsable'])} | {fmt(row['peakContext'])} | {fmt(row['seconds'], 1)} | {fmt(row['correctionRounds'], 1)} |")
    out += setup_rows(runs)
    out += ["", "## By scenario", "", "| scenario | agent | condition | usable |", "| --- | --- | --- | --- |"]
    groups: dict[tuple, list[dict]] = defaultdict(list)
    for r in runs:
        groups[(r["scenario"], r["agent"]["id"], label(r))].append(r)
    for (scenario, agent, cell), g in sorted(groups.items()):
        out.append(f"| {scenario} | {agent} | {cell} | {rate(sum(1 for r in g if r.get('usable')), len(g))} |")
    flags = discrimination(runs)
    if flags:
        out += ["", "## Scenario discrimination", "",
                "`smoke` means every condition that ran the scenario got the same 0% or 100% usable rate — it cannot "
                "separate conditions; `indeterminate` means fewer than two conditions ran it. Smoke scenarios still "
                "count in the canonical score.", "",
                "| scenario | discrimination | usable rate by condition | differing conditions |",
                "| --- | --- | --- | --- |"]
        for scenario, entry in sorted(flags.items()):
            cells = " · ".join(f"{cell} {c['usable']}/{c['runs']}" for cell, c in entry["conditions"].items())
            differs = ", ".join(entry["differingConditions"]) or "—"
            out.append(f"| {scenario} | {entry['discrimination']} | {cells} | {differs} |")
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
    level_lines, level_records = levels(runs)
    if level_lines:
        summary["levels"] = level_records
        out += ["", "## Level comparison: `mcp` vs `bin` (paired on scenario, agent, trial)", "",
                "`search/read` counts information-gathering operations: every wright invocation plus `bash` calls that ran a "
                "search/read shell command (a `bash` call invoking the wright CLI counts once, as a wright call).", "",
                "| agent | cell | level | runs | usable | passed | search/read | wright calls | bash calls | tool calls | turns | tokens/run |",
                "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |", *level_lines]
    smoke = sorted(s for s, f in flags.items() if f["discrimination"] == "smoke")
    key_set = {(r["scenario"], r["agent"]["id"], r["_trial"], label(r)) for r in runs}
    for reference in references or [BASELINE]:
        pairs, skipped = paired(runs, reference, flags)
        if pairs or skipped:
            out += ["", f"## Paired against `{reference}` (same scenario, agent, trial)", "",
                    "Lift is the paired usable-rate difference in percentage points with a clustered-bootstrap "
                    "interval over scenarios, reported over all paired scenarios and over discriminating scenarios "
                    "only; smoke scenarios cannot show a difference but stay in the canonical score. A matched pair "
                    "whose recorded environment differs (binary, suite, model, protocol, or enforcement) is dropped "
                    "and reported — the cell difference is the only variable.", "",
                    "| agent | comparison | pairs | usable gained/lost (all) | usable gained/lost (discriminating) | paired lift [95% CI] | tokens where both usable |",
                    "| --- | --- | --- | --- | --- | --- | --- |", *pairs]
            if skipped:
                out += ["", "Non-comparable pairs dropped:", *[f"- {s}" for s in skipped]]
            paired_scenarios = {s for (s, a, t, c) in key_set if c != reference and (s, a, t, reference) in key_set}
            named = [s for s in smoke if s in paired_scenarios]
            if named:
                out += ["", f"Smoke scenarios kept out of the discriminating column: {', '.join(named)}."]
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
    # #532: how often runs adopt a bounded form — `--brief` or the
    # selection flags/fields — as a share of wright uses per condition.
    bounded: dict[str, list[int]] = defaultdict(lambda: [0, 0])
    for r in runs:
        use = (r.get("toolUse") or {}).get("wright") or {}
        bounded[label(r)][0] += use.get("boundedUses", 0)
        bounded[label(r)][1] += use.get("invocations", 0)
    if any(n for _b, n in bounded.values()):
        summary["boundedAdoption"] = {
            cell: {"boundedUses": b, "invocations": n, "share": round(b / n, 4) if n else None}
            for cell, (b, n) in sorted(bounded.items())
        }
        out += ["", "## Bounded output adoption (`--brief` or selection fields; #532)", "",
                "| condition | bounded uses | wright uses | share |", "| --- | --- | --- | --- |"]
        out += [f"| {cell} | {b} | {n} | {f'{b / n:.1%}' if n else 'n/a'} |" for cell, (b, n) in sorted(bounded.items())]
    notes = diagnostics(runs, invalid) + ([] if has_private else (regrade or []))
    if infrastructure:
        notes.append(f"INFRASTRUCTURE: {len(infrastructure)} provider/infrastructure failures (exit 75) excluded from outcome metrics.")
        out += ["", "## Provider/infrastructure failures", "", "| agent | scenario | condition | artifact |", "| --- | --- | --- | --- |"]
        out += [f"| {r['agent']['id']} | {r['scenario']} | {label(r)} | {('[private artifact]' if r.get('isPrivate') else r['_dir'])} |" for r in infrastructure]
    missing_context = sum(1 for r in runs if not (r.get("context") or {}).get("reported"))
    if missing_context:
        notes.append(f"CONTEXT: {missing_context} run(s) lack observed loaded-context data; installed skills are not proof of loading.")
    out += ["", "## Diagnostics", ""] + ([f"- {n}" for n in notes] or ["- none"])
    summary["scenarios"] = [
        {"id": scenario, "runs": sum(len(g) for (s, _a, _c), g in groups.items() if s == scenario), **flags[scenario]}
        for scenario in sorted({s for s, _a, _c in groups})
    ]
    summary["diagnostics"] = notes
    summary["stdev"] = {k: pstdev([r["agent"]["seconds"] for r in runs if f"{r['agent']['id']}|{label(r)}" == k]) for k in summary["cells"]} if runs else {}
    return "\n".join(out) + "\n", summary


def main(dirs: list[Path], wright: str, regrade: bool, load_scenario, references: list[str] | None = None, out: Path | None = None) -> int:
    """Print the report; write report.md and summary.json into `out`, or into the run directory when there is exactly one.
    Several directories without `out` are only printed, so a combined report never overwrites one run's own."""
    results = load(dirs)
    if not results:
        print("no results found")
        return 1
    notes = regrade_notes([r for r in results if r["status"] != "invalid"], wright, load_scenario) if regrade else None
    text, summary = render(results, notes, references)
    target = out or (dirs[0] if len(dirs) == 1 else None)
    if target:
        target.mkdir(parents=True, exist_ok=True)
        (target / "report.md").write_text(text)
        write_json(target / "summary.json", summary)
    print(text)
    return 0
