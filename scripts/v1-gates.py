#!/usr/bin/env python3
"""Wright v1 semantic OPY integration gate.

Runs `wright compile --profile compat` over Wright's selected consumer
regression fixtures through the first-party OPY provider resolver, then
compares the emitted Workshop text with the recorded reference through the
canonical `workshop-rs` parser/WIR equivalence contract. The live OverPy oracle
and authoritative OPY corpus are owned by `opy-rs`; this script only consumes
evidence committed to Wright. Produces a machine-readable report at
target/v1-gates-report.json and exits non-zero when a fixture cannot be shown
to preserve Workshop semantics.

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
    # The canonical emitter's `All Players` spelling is equivalent to
    # OverPy's explicit `All Players(All Teams)` selector.
    text = text.replace("All Players(All Teams)", "All Players")
    # The provider's canonical Workshop emitter spells the unit-up vector
    # explicitly; the pinned oracle uses the equivalent `Up` constant.
    text = re.sub(r"\bUp\b", "Vector(0, 1, 0)", text)
    return re.sub(r"\s+", "", text)


def fixture_hash(fixture_id: str) -> str:
    source = (ROOT / "compatibility/fixtures" / fixture_id / "source.opy").read_bytes()
    return hashlib.sha256(source).hexdigest()


def text_hash(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def compare_semantics(comparator: str, expected: Path, actual: str) -> tuple[int, dict]:
    result = subprocess.run(
        [comparator, "semantic-compare", str(expected), "-"],
        input=actual,
        capture_output=True,
        text=True,
    )
    try:
        report = json.loads(result.stdout)
    except json.JSONDecodeError:
        report = {
            "equivalent": False,
            "expected": {"parsed": False, "error": "invalid comparator output"},
            "actual": {"parsed": False, "error": result.stderr.strip()},
        }
    return result.returncode, report


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--wright", default=str(ROOT / "target/debug/wright"))
    parser.add_argument(
        "--semantic-compare",
        help="wright binary providing the internal semantic-compare command",
    )
    parser.add_argument("--profile", default="compat")
    args = parser.parse_args()
    semantic_comparator = args.semantic_compare or str(Path(args.wright))

    report = {
        "schemaVersion": 2,
        "gate": "semantic",
        "reference": {"frontend": "overpy@9.7.10", "recorded": True},
        "provider": {
            "language": "opy",
            "source": "wright-first-party-release",
            "resolution": "refresh latest first-party release, then compile with active provider",
            "versionPin": None,
        },
        "wright": {"commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip()},
        "comparison": {
            "contract": "workshop-rs::roundtrip::equivalent",
            "presentationAliases": [
                "All Players(All Teams) == All Players",
                "All Players(Team.ALL) == All Players",
                "Up == Vector(0, 1, 0)",
                "debug HUD display strings are presentation-only",
            ],
        },
        "fixtures": {},
    }
    failures = []
    actual_by_fixture = {}
    expected_dir = ROOT / "target" / "v1-gates-expected"
    expected_dir.mkdir(parents=True, exist_ok=True)

    provider_update = subprocess.run(
        [args.wright, "provider", "update", "opy"],
        capture_output=True,
        text=True,
    )
    if provider_update.returncode != 0:
        report["provider"]["refresh"] = "failed"
        report["provider"]["refreshError"] = provider_update.stderr.strip()
        failures.append("first-party OPY provider refresh failed")
        report["summary"] = {"passed": 0, "total": len(FIXTURES)}
        out = ROOT / "target"
        out.mkdir(exist_ok=True)
        (out / "v1-gates-report.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, indent=2))
        print("\nFAILURES:\n" + "\n".join(failures), file=sys.stderr)
        return 1
    provider_update_match = re.search(
        r"provider\s+(\d+\.\d+\.\d+)", provider_update.stdout
    )
    report["provider"]["refresh"] = "success"
    if provider_update_match:
        report["provider"]["version"] = provider_update_match.group(1)

    for fixture_id in FIXTURES:
        fixture_dir = ROOT / "compatibility/fixtures" / fixture_id
        source = fixture_dir / "source.opy"
        snapshot = json.loads((fixture_dir / "oracle.json").read_text())
        expected = snapshot["compile"]["workshop"]
        expected_path = expected_dir / (fixture_id.replace("/", "-") + ".ws")
        expected_path.write_text(expected)

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
        actual_by_fixture[fixture_id] = got
        _, semantic = compare_semantics(semantic_comparator, expected_path, got)
        equal = semantic.get("equivalent", False)
        entry["status"] = "pass" if equal else "fail"
        entry["semanticEquivalent"] = equal
        entry["emittedSha256"] = text_hash(got)
        entry["byteEqual"] = got.strip() == expected.strip()
        entry["legacyNormalizedEqual"] = normalize(got) == normalize(expected)
        entry["representationOnlyDelta"] = equal and not entry["byteEqual"]
        if not equal:
            entry["semanticEvidence"] = semantic
            failures.append(f"{fixture_id}: Workshop semantic divergence")
        report["fixtures"][fixture_id] = entry

    cake_id = "real-world/overpy-cake"
    cake_actual = actual_by_fixture.get(cake_id)
    if cake_actual is None:
        report["negativeRegression"] = {"status": "not-run"}
    else:
        broken = cake_actual.replace("0.75", "0.5", 1)
        _, semantic = compare_semantics(
            semantic_comparator,
            expected_dir / (cake_id.replace("/", "-") + ".ws"),
            broken,
        )
        detected = not semantic.get("equivalent", False)
        report["negativeRegression"] = {
            "fixture": cake_id,
            "mutation": "first Workshop numeric literal 0.75 -> 0.5",
            "detected": detected,
        }
        if not detected:
            failures.append(f"{cake_id}: semantic negative regression was not detected")

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
