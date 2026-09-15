#!/usr/bin/env python3
"""Append a compact machine-report summary to GitHub Actions."""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path


def summary(report: dict, kind: str, outcome: str | None) -> str:
    if kind == "semantic":
        stats = report.get("summary", {})
        passed = stats.get("passed", 0)
        total = stats.get("total", 0)
        state = "PASS" if passed == total else "FAIL"
        icon = "✅" if passed == total else "❌"
        lines = [f"### Semantic OPY gate: {icon} {state} ({passed}/{total} fixtures)", ""]
        failures = [
            (fixture_id, fixture)
            for fixture_id, fixture in report.get("fixtures", {}).items()
            if fixture.get("status") != "pass"
        ]
        if failures:
            lines.append("**Failures:**")
            lines.extend(
                f"- `{fixture_id}`: {fixture.get('status', 'unknown')}"
                for fixture_id, fixture in failures
            )
        else:
            lines.append("All fixtures passed.")
        return "\n".join(lines)

    if kind == "scenarios":
        stats = report.get("summary", {})
        passed = stats.get("passed", 0)
        total = stats.get("total", 0)
        state = "PASS" if passed == total else "FAIL"
        icon = "✅" if passed == total else "❌"
        lines = [f"### Scenarios: {icon} {state} ({passed}/{total})", ""]
        failures = [
            (scenario_id, scenario)
            for scenario_id, scenario in report.get("scenarios", {}).items()
            if not scenario.get("passed")
        ]
        if failures:
            lines.append("**Failed scenarios:**")
            for scenario_id, scenario in failures:
                checks = [
                    check["check"]
                    for check in scenario.get("checks", [])
                    if not check["passed"]
                ]
                lines.append(f"- `{scenario_id}`: {', '.join(checks) or 'compile failed'}")
        else:
            lines.append("All scenarios passed.")
        lines.extend(["", "_Full machine-readable report available as workflow artifact._"])
        return "\n".join(lines)

    if kind == "benchmarks":
        lines = [f"### Benchmarks: {'✅ completed' if outcome == 'success' else '❌ FAIL'}"]
        benchmarks = report.get("benchmarks") or report.get("results") or []
        if benchmarks:
            lines.extend(["", "| Benchmark | Duration |", "| --- | --- |"])
            for benchmark in benchmarks[:20]:
                name = benchmark.get("name") or benchmark.get("id", "?")
                duration = (
                    benchmark.get("duration_ms")
                    or benchmark.get("elapsed_ms")
                    or benchmark.get("ns")
                )
                unit = (
                    "ms"
                    if "duration_ms" in benchmark or "elapsed_ms" in benchmark
                    else "ns"
                    if "ns" in benchmark
                    else ""
                )
                lines.append(f"| {name} | {duration}{unit} |")
            if len(benchmarks) > 20:
                lines.append(f"_... and {len(benchmarks) - 20} more (see artifact)_")
            lines.extend(["", "_Full machine-readable report available as workflow artifact._"])
        return "\n".join(lines)

    raise ValueError(f"unsupported report kind: {kind}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--title", required=True)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--kind", choices=("semantic", "scenarios", "benchmarks"), required=True)
    parser.add_argument("--out", type=Path, help="summary file; defaults to GITHUB_STEP_SUMMARY")
    parser.add_argument("--outcome", help="workflow step outcome for benchmark summaries")
    args = parser.parse_args()

    lines = [f"## {args.title}", ""]
    if args.report is None or not args.report.is_file():
        lines.append("### Report: ⚠️ not generated")
    else:
        lines.append(summary(json.loads(args.report.read_text()), args.kind, args.outcome))
    content = "\n".join(lines) + "\n"

    destination = args.out or (
        Path(os.environ["GITHUB_STEP_SUMMARY"])
        if os.environ.get("GITHUB_STEP_SUMMARY")
        else None
    )
    if destination is None:
        sys.stdout.write(content)
    else:
        destination.parent.mkdir(parents=True, exist_ok=True)
        with destination.open("a") as output:
            output.write(content)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
