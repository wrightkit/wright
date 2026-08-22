#!/usr/bin/env python3
"""Wright compile-time scenario regression runner (#50).

Each scenario in scenarios/<id>.json declares a source program, target
metadata, and expected compile-time behaviors. The runner compiles the scenario
through the real `wright` executable (compat profile), records the emitted
Workshop text and analysis findings as repeatable compile-time evidence, and
verifies every declared expectation structurally. The report
(target/scenarios-report.json) is machine-readable and reproducible. It does
not establish E-level semantic compatibility: running the Overwatch client is
outside the current scope, so the recorded evidence is the compile-time
WIR/emission trace plus configurable lint findings.

Usage: python3 scripts/run-scenarios.py [--wright path/to/wright]
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCENARIOS = sorted(ROOT.glob("scenarios/*.json"))


def verify(check: str, text: str, findings: list) -> tuple:
    """Return (passed, detail) for one expected-behavior check."""
    if check == "compiles":
        return True, "compile succeeded"
    if check == "explicitWaitDefault":
        return "Wait(0.016, Ignore Condition)" in text, "wait defaults filled"
    if check == "explicitDuration":
        return bool(re.search(r"Wait\(0\.5, Ignore Condition\)", text)), "explicit duration kept"
    if check == "boundedFor":
        return "For Global Variable" in text, "bounded for"
    if check == "finding min-wait-loop":
        return any(f.get("code") == "min-wait-loop" for f in findings), "min-wait-loop finding"
    if check == "branchingIf":
        return bool(re.search(r"If\(", text)), "If action"
    if check == "comparisons":
        return "Compare(" in text, "comparison values"
    if check == "initializeRule":
        return "Initialize global variables" in text and "Set Global Variable" in text, "initialize rule"
    if check == "modifyAction":
        return "Modify Global Variable" in text, "modify action"
    if check == "arrayLiteral":
        return bool(re.search(r"Set Global Variable\(\w+, Array\(", text)), "array literal"
    if check == "appendAction":
        return "Append To Array" in text, "append action"
    if check == "firstOfFold":
        return "First Of(" in text, "x[0] folds to First Of"
    if check == "subroutineRule":
        return bool(re.search(r'rule \("Subroutine \w+"\)', text)), "subroutine rule"
    if check == "callSubroutine":
        return "Call Subroutine(" in text, "call subroutine"
    if check == "globalEvent":
        return "Ongoing - Global;" in text, "global event"
    if check == "eachPlayerEvent":
        return "Ongoing - Each Player;" in text and "All;" in text, "each player event"
    if check == "playerState":
        return "Set Player Variable(Event Player" in text, "player state"
    return False, f"unknown check {check}"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--wright", default=str(ROOT / "target/debug/wright"))
    args = parser.parse_args()

    report = {
        "schemaVersion": 1,
        "framework": "wright-scenarios/v1",
        "target": {"game": "overwatch", "runtime": "workshop", "reference": "overpy@9.7.10"},
        "scope": "compile-time WIR/emission trace + static findings; client execution is out of v1 scope",
        "scenarios": {},
    }
    failures = []

    for manifest_path in SCENARIOS:
        manifest = json.loads(manifest_path.read_text())
        scenario_id = manifest["id"]
        source = ROOT / "scenarios" / manifest["source"]
        compile_result = subprocess.run(
            [args.wright, "compile", str(source), "--profile", "compat", "-f", "json"],
            capture_output=True,
            text=True,
        )
        lint_result = subprocess.run(
            [args.wright, "lint", str(source), "--profile", "compat", "-f", "json"],
            capture_output=True,
            text=True,
        )

        entry = {"title": manifest["title"], "category": manifest["category"], "checks": []}
        if compile_result.returncode != 0:
            entry["compileOk"] = False
            entry["error"] = compile_result.stderr.strip()
            failures.append(f"{scenario_id}: compile failed")
            report["scenarios"][scenario_id] = entry
            continue

        envelope = json.loads(compile_result.stdout)
        text = envelope["result"]["output"]["text"]
        findings = json.loads(lint_result.stdout)["result"]["findings"]
        entry["compileOk"] = True
        entry["emittedLines"] = len(text.strip().splitlines())
        entry["findings"] = [
            {"code": f["code"], "severity": f["severity"]} for f in findings
        ]

        all_passed = True
        for expectation in manifest["expects"]:
            passed, detail = verify(expectation["check"], text, findings)
            entry["checks"].append(
                {
                    "description": expectation["description"],
                    "check": expectation["check"],
                    "passed": passed,
                    "detail": detail,
                }
            )
            if not passed:
                all_passed = False
                failures.append(f"{scenario_id}: {expectation['check']}")

        entry["passed"] = all_passed
        report["scenarios"][scenario_id] = entry

    report["summary"] = {
        "passed": sum(1 for s in report["scenarios"].values() if s.get("passed")),
        "total": len(SCENARIOS),
    }
    out = ROOT / "target"
    out.mkdir(exist_ok=True)
    (out / "scenarios-report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    if failures:
        print("\nFAILURES:\n" + "\n".join(failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
