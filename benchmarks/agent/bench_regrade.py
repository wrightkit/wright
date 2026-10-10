"""Re-grade stored benchmark trials when a scenario's checks change (#614, SPEC-414 REQ-007).

A changed `scenario.json` or grader source changes the grader hash, so a run
directory recorded under an earlier hash no longer reflects the current
verdict. `regrade` copies each trial into an output directory — originals are
never written — and re-grades only the trials whose recorded hash differs.
"""

from __future__ import annotations

import shutil
import tempfile
from pathlib import Path

import bench_grade
import bench_report

GRADER_UNAVAILABLE = "a required grader was unavailable"


def status_of(result: dict) -> str:
    """`agent_bench.run_status` without the import cycle: the agent outcome, not the verdict."""
    if "invalid" in result:
        return "invalid"
    code = result["agent"]["exit"]
    if code == 75:
        return "provider-interrupted"
    if code is None:
        return "timeout"
    return "agent-error" if code else "completed"


def regrade_trial(result: dict, dest: Path, scenario: dict, wright: str) -> bool:
    """Copy the stored trial into `dest` and re-grade it when the grader hash changed.

    The copy carries every field the agent produced untouched; only the fields
    `grade` owns are replaced. Returns True when the trial was re-graded.
    """
    source = result["_dir"]
    shutil.copytree(source, dest)
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
    # The agent-side invalid reasons cannot be re-derived, but a grader that
    # was unavailable then and resolves now stops being a blocking reason.
    if "invalid" in result and not any(c.get("unavailable") for c in result["checks"]):
        reasons = [r for r in result["invalid"].split("; ") if r != GRADER_UNAVAILABLE]
        if reasons != result["invalid"].split("; "):
            if reasons:
                result["invalid"] = "; ".join(reasons)
            else:
                del result["invalid"]
    result["status"] = status_of(result)
    result["_verdictChanged"] = usable_before != result.get("usable")
    bench_report.write_json(dest / "result.json", {k: v for k, v in result.items() if not k.startswith("_")})
    return True


def regrade(dirs: list[Path], wright: str, load_scenario, out: Path) -> dict:
    """Copy every trial under `dirs` into `out`, re-grading hash-mismatched trials.

    `out` is laid out as `<out>/<run-dir-name>/<trial path>` so the result is
    consumable by `report`, `score`, and `publish` exactly like a run
    directory. No input directory is modified.
    """
    resolved = [base.resolve() for base in dirs]
    out = out.resolve()
    for base in resolved:
        if base == out or out in base.parents or base in out.parents:
            raise SystemExit("--out must be disjoint from every input directory")
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
                transitions.setdefault(scenario_id, set()).add((grader["regradedFrom"], grader["hash"]))
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
