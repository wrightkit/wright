#!/usr/bin/env python3
"""Wright v1 N-level gate: compiled output vs recorded reference snapshots.

Runs `wright compile --profile compat` over Wright's selected consumer
regression fixtures and compares the emitted Workshop text to immutable
recorded reference snapshots with a documented normalizer (debug/print HUD
lines collapse to a canonical marker; whitespace-only differences collapse).
The live OverPy oracle and authoritative OPY corpus are owned by `opy-rs`;
this script only consumes evidence committed to Wright. Produces a
machine-readable report at target/v1-gates-report.json and exits non-zero when
a selected Wright regression fixture diverges after normalization.

Usage: python3 scripts/v1-gates.py [--wright path/to/wright]
"""

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = [
    "synthetic/basic-rule",
    "synthetic/control-flow",
    "synthetic/declarations-rules",
    "synthetic/expressions-values",
    "synthetic/preprocessing",
    "real-world/overpy-cake",
]


def _collapse_hud(text: str) -> str:
    """Collapse each `Create HUD Text(...)` statement (balanced parens,
    possibly multi-line) to a canonical marker."""
    out = []
    i = 0
    while i < len(text):
        start = text.find("Create HUD Text(", i)
        if start == -1:
            out.append(text[i:])
            break
        out.append(text[i:start])
        depth = 0
        j = start + len("Create HUD Text(") - 1
        while j < len(text):
            if text[j] == "(":
                depth += 1
            elif text[j] == ")":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        out.append("Create HUD Text(<debug>);")
        i = j + 1
    return "".join(out)


def normalize(text: str) -> str:
    text = _collapse_hud(text)
    return re.sub(r"\s+", "", text)


def fixture_hash(fixture_id: str) -> str:
    source = (ROOT / "compatibility/fixtures" / fixture_id / "source.opy").read_bytes()
    return hashlib.sha256(source).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--wright", default=str(ROOT / "target/debug/wright"))
    parser.add_argument("--profile", default="compat")
    args = parser.parse_args()

    report = {
        "schemaVersion": 1,
        "gate": "N",
        "reference": {"frontend": "overpy@9.7.10", "recorded": True},
        "wright": {"commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip()},
        "normalizer": "debug-hud-collapse + whitespace-collapse",
        "fixtures": {},
    }
    failures = []

    for fixture_id in FIXTURES:
        fixture_dir = ROOT / "compatibility/fixtures" / fixture_id
        source = fixture_dir / "source.opy"
        snapshot = json.loads((fixture_dir / "oracle.json").read_text())
        expected = snapshot["compile"]["workshop"]

        result = subprocess.run(
            [args.wright, "compile", str(source), "--profile", args.profile, "-f", "json"],
            capture_output=True,
            text=True,
        )
        entry = {
            "inputSha256": fixture_hash(fixture_id),
            "compileExit": result.returncode,
        }
        if result.returncode != 0:
            entry["status"] = "compile-failed"
            entry["diagnostics"] = result.stderr.strip()
            failures.append(f"{fixture_id}: compile failed")
            report["fixtures"][fixture_id] = entry
            continue

        envelope = json.loads(result.stdout)
        got = envelope["result"]["output"]["text"]
        equal = normalize(got) == normalize(expected)
        entry["status"] = "pass" if equal else "fail"
        entry["byteEqual"] = got.strip() == expected.strip()
        entry["debugLineDiffers"] = "Create HUD Text(" in expected
        if not equal:
            failures.append(f"{fixture_id}: N-level divergence after normalization")
        report["fixtures"][fixture_id] = entry

    report["summary"] = {
        "passed": sum(1 for f in report["fixtures"].values() if f["status"] == "pass"),
        "total": len(FIXTURES),
    }
    out = ROOT / "target"
    out.mkdir(exist_ok=True)
    (out / "v1-gates-report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    if failures:
        print("\nFAILURES:\n" + "\n".join(failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
