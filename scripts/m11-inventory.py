#!/usr/bin/env python3
"""M11 phase-1 N-level evidence inventory (issue #81, SPEC-M11).

For every phase-1 real-world fixture, runs the native frontend
(``wright compile --profile compat -f json``) and compares the emitted
Workshop text to the pinned oracle snapshot with the v1 N-level normalizer
(debug/print HUD lines collapse to a canonical marker; whitespace-only
differences collapse; the normalizer logic is copied from ``scripts/v1-gates.py``
and must stay in sync with it — this script does not import it).

Also records the pinned adapter outcome (``wright-adapter``, code and message
verbatim) and the differential-suite status for the fixture. The report is
written to ``target/m11-nlevel.json``; the run is deterministic and safe to
re-run in CI (requires ``target/debug/wright``, the oracle snapshots, and the
adapter dependencies installed with ``pnpm --dir adapter install``).

Usage: python3 scripts/m11-inventory.py [--wright path/to/wright]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
WRIGHT = ROOT / "target" / "debug" / "wright"
ADAPTER = ROOT / "adapter" / "bin" / "wright-adapter.js"
REPORT = ROOT / "target" / "m11-nlevel.json"
GAP_REPORT = ROOT / "target" / "m11-gap-inventory.json"

FIXTURES = [
    "synthetic/settings",
    "real-world/overpy-pixelart",
    "real-world/overpy-santa",
    "real-world/overpy-meipocalypse",
    "real-world/overpy-zencopter",
    "real-world/overpy-cronch",
    "real-world/overpy-broken-weapons",
    "real-world/overpy-client-to-server",
    "real-world/overpy-parabola",
    "real-world/overpy-crosshair",
    "real-world/overpy-inputhud",
    "real-world/ow1-emulator",
    "real-world/6v6-adjustments",
]


def sha256_text(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def collapse_hud(text: str) -> str:
    """Collapse each `Create HUD Text(...)` statement (balanced parens,
    possibly multi-line) to a canonical marker (v1 normalizer)."""
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


def normalize_v1(text: str) -> str:
    """The v1 N-level normalizer: debug-HUD collapse + whitespace collapse."""
    return re.sub(r"\s+", "", collapse_hud(text))


def run_adapter(fixture_dir: Path, source: str) -> dict[str, Any]:
    """Run the pinned adapter; record code and message verbatim."""
    result = subprocess.run(
        ["node", str(ADAPTER), "--input", str(fixture_dir / source), "--root",
         str(fixture_dir), "--main-file", source, "--output", str(ROOT / "target" / "m11-adapter.tmp.json")],
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        return {"status": "success", "message": ""}
    record = {}
    for line in reversed(result.stderr.splitlines()):
        try:
            record = json.loads(line)
            break
        except json.JSONDecodeError:
            continue
    code = record.get("code", "unknown")
    return {
        "status": code,
        "code": code,
        "message": record.get("message", result.stderr.strip()),
    }


def fixture_source(fixture_dir: Path) -> str:
    metadata = json.loads((fixture_dir / "fixture.json").read_text())
    return metadata["source"]


def run_fixture(wright: Path, fixture_id: str) -> dict[str, Any]:
    fixture_dir = ROOT / "compatibility" / "fixtures" / fixture_id
    source_name = fixture_source(fixture_dir)
    oracle = json.loads((fixture_dir / "oracle.json").read_text())
    oracle_status = oracle["compile"]["status"]
    oracle_workshop = oracle["compile"]["workshop"]
    oracle_workshop_sha = oracle["compile"]["workshopSha256"]

    native = subprocess.run(
        [str(wright), "compile", str(fixture_dir / source_name),
         "--profile", "compat", "-f", "json", "--root", str(fixture_dir)],
        capture_output=True,
        text=True,
    )
    entry: dict[str, Any] = {
        "fixture": fixture_id,
        "oracleStatus": oracle_status,
        "oracleWorkshopSha256": oracle_workshop_sha,
        "nativeExit": native.returncode,
        "nativeErrorCode": None,
        "nativeOutputSha256": None,
        "byteEqual": None,
        "normalizedEqual": None,
        "adapterStatus": None,
        "differentialStatus": "not-applicable",
    }

    envelope = None
    try:
        envelope = json.loads(native.stdout)
    except json.JSONDecodeError:
        pass

    if envelope is not None and envelope.get("ok"):
        text = envelope["result"]["output"]["text"]
        entry["nativeOutputSha256"] = sha256_text(text)
        entry["byteEqual"] = text.strip() == oracle_workshop.strip()
        entry["normalizedEqual"] = normalize_v1(text) == normalize_v1(oracle_workshop)
    elif envelope is not None:
        diagnostics = envelope.get("diagnostics", [])
        entry["nativeErrorCode"] = diagnostics[0]["code"] if diagnostics else "unknown"

    adapter_fixture = ROOT / "adapter" / "fixtures" / f"{fixture_id}.json"
    if not adapter_fixture.is_file():
        entry["differentialStatus"] = "not-applicable (no adapter fixture)"
    else:
        entry["differentialStatus"] = "recorded in wright-differential-report.json"
    entry["adapterStatus"] = run_adapter(fixture_dir, source_name)
    return entry


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wright", type=Path, default=WRIGHT)
    args = parser.parse_args()
    wright = args.wright.resolve()
    if not wright.is_file():
        raise SystemExit(f"wright binary not found: {wright} (build with cargo build -p wright-cli)")

    records = [run_fixture(wright, fixture_id) for fixture_id in FIXTURES]

    target = ROOT / "target"
    target.mkdir(exist_ok=True)
    report = {
        "schemaVersion": 1,
        "evidence": "N-level: wright compile --profile compat vs oracle.json workshop with the v1 normalizer (debug-hud-collapse + whitespace-collapse)",
        "reference": {"frontend": "overpy@9.7.10", "recorded": True},
        "wright": {"commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip()},
        "fixtures": records,
    }
    (REPORT).write_text(json.dumps(report, indent=2) + "\n")

    gaps = [r for r in records if r["nativeErrorCode"] is not None]
    gap_report = {
        "schemaVersion": 1,
        "note": "native classification is QA's verdict; this inventory only records raw outcomes",
        "fixtures": [
            {
                "fixture": r["fixture"],
                "nativeErrorCode": r["nativeErrorCode"],
                "oracleStatus": r["oracleStatus"],
                "adapterStatus": r["adapterStatus"]["status"],
            }
            for r in gaps
        ],
    }
    GAP_REPORT.write_text(json.dumps(gap_report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
