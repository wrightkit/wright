"""Re-grade stored benchmark trials when a scenario's checks change (#614, SPEC-414 REQ-007).

A changed `scenario.json` or grader source changes the grader hash, so a run
directory recorded under an earlier hash no longer reflects the current
verdict. `regrade` copies each trial into an output directory — originals are
never written — and re-grades only the trials whose recorded hash differs.
"""

from __future__ import annotations

import shutil
from pathlib import Path

import bench_grade
import bench_report

GRADER_UNAVAILABLE = "a required grader was unavailable"


def regrade_trial(result: dict, dest: Path, scenario: dict, wright: str) -> bool:
    """Copy the stored trial into `dest` and re-grade it when the grader hash changed.

    The copy carries every field the agent produced untouched; only the fields
    `grade` owns are replaced. Returns True when the trial was re-graded.
    """
    source = result["_dir"]
    # `symlinks=True` keeps agent-written links as links; dereferencing them
    # would copy an outside target's contents recursively into `--out`.
    shutil.copytree(source, dest, symlinks=True)
    current = bench_grade.grader_hash(scenario)
    recorded = (result.get("grader") or {}).get("hash")
    if recorded == current:
        return False
    workspace = dest / "workspace"
    if not workspace.is_dir():
        return False
    usable_before = result.get("usable")
    verdict = bench_grade.grade(scenario, workspace, wright, dest / "grading")
    result.update(verdict)
    result["grader"]["regradedFrom"] = recorded
    # Grader availability is re-derived like a fresh run: a re-grade that
    # hits an unavailable check records the reason, and one that resolves it
    # drops it. Agent-side invalid reasons cannot be re-derived and stay.
    reasons = result["invalid"].split("; ") if result.get("invalid") else []
    if any(c.get("unavailable") for c in result["checks"]):
        if GRADER_UNAVAILABLE not in reasons:
            reasons.append(GRADER_UNAVAILABLE)
    else:
        reasons = [r for r in reasons if r != GRADER_UNAVAILABLE]
    if reasons:
        result["invalid"] = "; ".join(reasons)
    else:
        result.pop("invalid", None)
    result["status"] = bench_report.run_status(result)
    result["_verdictChanged"] = usable_before != result.get("usable")
    bench_report.write_json(dest / "result.json", {k: v for k, v in result.items() if not k.startswith("_")})
    return True


def regrade(dirs: list[Path], wright: str, load_scenario, out: Path) -> dict:
    """Copy every completed trial under `dirs` into `out`, re-grading hash-mismatched trials.

    `out` is laid out as `<out>/<run-dir-name>/<trial path>` so the result is
    consumable by `report`, `score`, and `publish` exactly like a run
    directory. A trial dir without a parseable contract `result.json` is not
    a finished trial and is skipped, as `load` does everywhere. No input
    directory is modified.
    """
    resolved = [base.resolve() for base in dirs]
    out = out.resolve()
    for base in resolved:
        if base == out or out in base.parents or base in out.parents:
            raise SystemExit("--out must be disjoint from every input directory")
    names = [base.name for base in resolved]
    if len(set(names)) != len(names):
        raise SystemExit("input directories must have distinct names")
    if out.exists() and any(out.iterdir()):
        raise SystemExit(f"--out {out} already exists and is not empty")
    scenarios: dict[str, dict] = {}
    graded = copied = changed = 0
    transitions: dict[str, set] = {}
    for base in resolved:
        for result in bench_report.load([base]):
            scenario_id = result["scenario"]
            if scenario_id not in scenarios:
                scenarios[scenario_id] = load_scenario(scenario_id)
            dest = out / base.name / result["_dir"].relative_to(base)
            dest.parent.mkdir(parents=True, exist_ok=True)
            if regrade_trial(result, dest, scenarios[scenario_id], wright):
                graded += 1
                changed += bool(result.pop("_verdictChanged", False))
                grader = result["grader"]
                # Pre-hash archives have no recorded hash — `regradedFrom`
                # stays `null` in the result and reads `<none>` here.
                transitions.setdefault(scenario_id, set()).add((grader["regradedFrom"] or "<none>", grader["hash"]))
            else:
                copied += 1
    lines = []
    for scenario_id in sorted(transitions):
        for old, new in sorted(transitions[scenario_id]):
            lines.append(f"{scenario_id}: grader {old[:12]} -> {new[:12]}")
    summary = {
        "trials": graded + copied,
        "copied": copied,
        "regraded": graded,
        "verdictChanged": changed,
        "out": str(out),
        "transitions": lines,
    }
    return summary


def main(dirs: list[Path], wright: str, load_scenario, out: Path) -> int:
    summary = regrade(dirs, wright, load_scenario, out)
    if summary["regraded"] == 0:
        print(f"every recorded grader hash is current; {summary['copied']} trial(s) copied to {summary['out']}")
    else:
        for line in summary["transitions"]:
            print(f"REGRADED {line}")
        print(
            f"{summary['regraded']} trial(s) re-graded, {summary['verdictChanged']} usable verdict(s) changed, "
            f"{summary['copied']} copied unchanged -> {summary['out']}"
        )
    return 0
