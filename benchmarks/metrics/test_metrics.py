"""Band-comparison and contract tests for benchmarks/metrics/metrics.py.

The heavy path (spawn wright, measure the corpus) is exercised once against
the smallest vendored project; the band logic itself is tested on synthetic
metric documents — the values below are independent expectations, not
echoes of the implementation.
"""

import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import metrics


def _metric(bytes_=1000, latency_ms=100.0, items=10):
    return {
        "bytes": bytes_,
        "tokens": bytes_ / metrics.TOKEN_BYTES,
        "latencyMs": latency_ms,
        "counts": {"items": items},
    }


def _doc(projects):
    return {
        "contract": metrics.CONTRACT,
        "corpusVersion": "v1",
        "wright": "wright 0.0.0",
        "opyProvider": "0.0.0",
        "projects": projects,
    }


class BandTest(unittest.TestCase):
    def test_within_band_is_clean(self):
        base = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000, 100.0, 10)}}})
        # tokens: 250 -> 285 (+14%, +35 tokens absolute -> inside 15%? no:
        # +35% of 250 = 37.5 allowed? band is 15% AND 30 abs; +35 tokens is
        # >30 absolute but +14% is within 15% -> clean)
        run = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1140, 140.0, 11)}}})
        violations, _ = metrics.compare_metrics(base, run)
        self.assertEqual(violations, [])

    def test_doubled_output_is_flagged(self):
        base = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000)}}})
        run = _doc({"p": {"status": "ok", "metrics": {"m": _metric(2000)}}})
        violations, _ = metrics.compare_metrics(base, run)
        self.assertTrue(
            any(
                v["metric"] == "m" and v["field"] == "tokens" and v["change"] == "increase"
                for v in violations
            ),
            violations,
        )

    def test_shrunk_output_is_reported_as_improvement(self):
        base = _doc({"p": {"status": "ok", "metrics": {"m": _metric(2000)}}})
        run = _doc({"p": {"status": "ok", "metrics": {"m": _metric(800)}}})
        violations, _ = metrics.compare_metrics(base, run)
        self.assertTrue(
            any(v["field"] == "tokens" and v["change"] == "decrease" for v in violations),
            violations,
        )

    def test_small_absolute_change_within_30_is_clean(self):
        # tokens 250 -> 280: +30 absolute is not > 30, and +12% < 15%
        run = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1120)}}})
        violations, _ = metrics.compare_metrics(
            _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000)}}}), run
        )
        self.assertEqual(violations, [])

    def test_relative_jump_over_15_percent_needs_over_30_absolute(self):
        # counts 10 -> 12 is +20% but only +2 absolute -> clean
        base = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000, 100.0, 10)}}})
        run = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000, 100.0, 12)}}})
        violations, _ = metrics.compare_metrics(base, run)
        self.assertEqual(violations, [])

    def test_count_jump_flagged(self):
        base = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000, 100.0, 100)}}})
        run = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000, 100.0, 200)}}})
        violations, _ = metrics.compare_metrics(base, run)
        self.assertTrue(any(v["field"] == "counts.items" for v in violations))

    def test_latency_band_is_relative_only(self):
        base = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000, 100.0)}}})
        run = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000, 160.0)}}})
        violations, _ = metrics.compare_metrics(base, run)
        self.assertTrue(any(v["field"] == "latencyMs" for v in violations))
        run2 = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000, 140.0)}}})
        violations2, _ = metrics.compare_metrics(base, run2)
        self.assertEqual(violations2, [])

    def test_uniform_latency_shift_is_machine_speed_not_drift(self):
        # Every metric at 1.6x — a slower host, not a per-operation
        # regression; the median factor normalizes it out and is reported.
        count = metrics.MIN_LATENCY_PAIRS + 2
        base_map = {f"m{i}": _metric(1000, 100.0) for i in range(count)}
        run_map = {f"m{i}": _metric(1000, 160.0) for i in range(count)}
        violations, warnings = metrics.compare_metrics(
            _doc({"p": {"status": "ok", "metrics": base_map}}),
            _doc({"p": {"status": "ok", "metrics": run_map}}),
        )
        self.assertFalse(any(v["field"] == "latencyMs" for v in violations), violations)
        self.assertTrue(any("machine factor: 1.60" in w for w in warnings), warnings)

    def test_one_latency_outlier_flags_against_the_fleet(self):
        # Eleven metrics at 1.6x and one at 5x: the outlier deviates from the
        # normalized baseline (160 * 1.5) while the fleet stays clean.
        count = metrics.MIN_LATENCY_PAIRS + 2
        base_map = {f"m{i}": _metric(1000, 100.0) for i in range(count)}
        run_map = {f"m{i}": _metric(1000, 160.0) for i in range(count)}
        run_map["m11"] = _metric(1000, 500.0)
        violations, _ = metrics.compare_metrics(
            _doc({"p": {"status": "ok", "metrics": base_map}}),
            _doc({"p": {"status": "ok", "metrics": run_map}}),
        )
        flagged = [v["metric"] for v in violations if v["field"] == "latencyMs"]
        self.assertEqual(flagged, ["m11"], violations)

    def test_too_few_pairs_falls_back_to_absolute_band(self):
        # Below MIN_LATENCY_PAIRS the machine factor is not trusted.
        base_map = {f"m{i}": _metric(1000, 100.0) for i in range(3)}
        run_map = {f"m{i}": _metric(1000, 160.0) for i in range(3)}
        violations, _ = metrics.compare_metrics(
            _doc({"p": {"status": "ok", "metrics": base_map}}),
            _doc({"p": {"status": "ok", "metrics": run_map}}),
        )
        self.assertEqual(
            len([v for v in violations if v["field"] == "latencyMs"]), 3, violations
        )

    def test_sub_millisecond_latency_jitter_is_ignored(self):
        base = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000, 0.1)}}})
        run = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000, 0.0)}}})
        violations, _ = metrics.compare_metrics(base, run)
        self.assertEqual(violations, [])

    def test_structural_changes_are_flagged(self):
        base = _doc({"p": {"status": "ok", "metrics": {"m": _metric()}}})
        run = _doc(
            {
                "p": {"status": "ok", "metrics": {"m": _metric(), "new": _metric()}},
                "q": {"status": "ok", "metrics": {"m": _metric()}},
            }
        )
        violations, _ = metrics.compare_metrics(base, run)
        kinds = {(v["project"], v["change"]) for v in violations}
        self.assertIn(("p", "added"), kinds)
        self.assertIn(("q", "added-to-run"), kinds)

    def test_skipped_project_warns_not_fails(self):
        base = _doc({"p": {"status": "ok", "metrics": {"m": _metric()}}})
        run = _doc({"p": {"status": "skipped", "reason": "no provider"}})
        violations, warnings = metrics.compare_metrics(base, run)
        self.assertEqual(violations, [])
        self.assertTrue(warnings)

    def test_brief_budget_is_a_violation_not_a_band(self):
        # #532: a brief result above the stated token budget fails even when
        # the drift band itself would pass.
        key = "cli:lint?brief"
        metric = _metric(metrics.BRIEF_TOKEN_BUDGET["cli"] * metrics.TOKEN_BYTES + 400)
        doc = {"p": {"status": "ok", "metrics": {key: metric}}}
        violations, _ = metrics.compare_metrics(_doc(doc), _doc(doc))
        self.assertTrue(
            any(v["metric"] == key and v["change"] == "exceeds-brief-budget" for v in violations),
            violations,
        )
        under = _metric(800)
        doc = {"p": {"status": "ok", "metrics": {key: under}}}
        violations, _ = metrics.compare_metrics(_doc(doc), _doc(doc))
        self.assertEqual(violations, [])

    def test_brief_counts_unwraps_envelopes(self):
        brief = {
            "brief": True,
            "counts": {"findings": {"total": 9, "error": 1}, "rules": 6},
            "items": [{}, {}],
            "expand": "...",
        }
        expected = {"items": 2, "counts.findings.total": 9, "counts.rules": 6}
        self.assertEqual(metrics.brief_counts(brief), expected)
        self.assertEqual(metrics.brief_counts({"result": brief}), expected)
        self.assertEqual(metrics.brief_counts({"result": {"result": brief}}), expected)
        self.assertEqual(metrics.brief_counts({"result": {"findings": []}}), {})


class RunTest(unittest.TestCase):
    def test_run_small_project_records_all_surfaces(self):
        wright = Path(__file__).resolve().parents[2] / "target" / "debug" / "wright"
        if not wright.is_file():
            self.skipTest("wright binary not built")
        corpus = {
            "contract": metrics.CORPUS_CONTRACT,
            "version": "v1",
            "projects": [
                {
                    "id": "small-workshop",
                    "kind": "workshop",
                    "path": "corpus/small-workshop",
                    "entry": "main.ws",
                }
            ],
        }
        result = metrics.run_metrics(
            str(wright), corpus, repeats=1, only=None
        )
        project = result["projects"]["small-workshop"]
        self.assertEqual(project["status"], "ok")
        keys = set(project["metrics"])
        self.assertEqual(
            keys,
            {f"agent:{op}" for op in metrics.AGENT_OPS}
            | {f"cli:{cmd}" for cmd in metrics.CLI_COMMANDS}
            | {f"agent:{op}?brief" for op in metrics.BRIEF_OPS}
            | {f"cli:{cmd}?brief" for cmd in metrics.BRIEF_OPS},
        )
        self.assertEqual(result["briefTokenBudget"], metrics.BRIEF_TOKEN_BUDGET)
        for key, record in project["metrics"].items():
            if "skipped" in record:
                continue
            self.assertIn("bytes", record, key)
            self.assertIn("tokens", record, key)
            self.assertIn("latencyMs", record, key)

    def test_check_never_rewrites_baseline(self):
        with tempfile.TemporaryDirectory() as tmp:
            baseline_path = Path(tmp) / "baseline.json"
            baseline = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000)}}})
            baseline_path.write_text(json.dumps(baseline))
            before = baseline_path.read_bytes()
            drifting = _doc({"p": {"status": "ok", "metrics": {"m": _metric(4000)}}})
            with mock.patch.object(metrics, "run_metrics", return_value=drifting):
                code = _check_main(baseline_path)
            self.assertEqual(code, 1)
            self.assertEqual(baseline_path.read_bytes(), before)


def _check_main(baseline_path: Path) -> int:
    """Invoke `check` against a canned baseline without touching wright."""
    argv = ["metrics.py", "check", "--baseline", str(baseline_path)]
    with mock.patch("sys.argv", argv):
        return metrics.main()


if __name__ == "__main__":
    unittest.main()
