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
        run = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1120)}}})
        violations, _ = metrics.compare_metrics(
            _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000)}}}), run
        )
        self.assertEqual(violations, [])

    def test_relative_jump_over_15_percent_needs_over_30_absolute(self):
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

    def test_skipped_run_project_fails_when_baseline_measured(self):
        # #643: a run-side project failure is a violation, not a warning —
        # a broken run must not pass `check`.
        base = _doc({"p": {"status": "ok", "metrics": {"m": _metric()}}})
        run = _doc({"p": {"status": "skipped", "reason": "no provider"}})
        violations, _ = metrics.compare_metrics(base, run)
        self.assertTrue(
            any(
                v["project"] == "p" and v["metric"] == "*" and v["change"] == "broken-in-run"
                for v in violations
            ),
            violations,
        )

    def test_project_skipped_on_both_sides_only_warns(self):
        base = _doc({"p": {"status": "skipped", "reason": "no provider"}})
        run = _doc({"p": {"status": "skipped", "reason": "no provider"}})
        violations, warnings = metrics.compare_metrics(base, run)
        self.assertEqual(violations, [])
        self.assertTrue(warnings)

    def test_run_side_error_metric_fails(self):
        base = _doc({"p": {"status": "ok", "metrics": {"m": _metric()}}})
        run = _doc({"p": {"status": "ok", "metrics": {"m": {"error": "timeout"}}}})
        violations, _ = metrics.compare_metrics(base, run)
        self.assertTrue(
            any(
                v["project"] == "p" and v["metric"] == "m" and v["change"] == "broken-in-run"
                for v in violations
            ),
            violations,
        )

    def test_matching_skip_marker_stays_a_warning(self):
        skip = {"skipped": "no addressable symbol/rule"}
        base = _doc({"p": {"status": "ok", "metrics": {"m": skip}}})
        run = _doc({"p": {"status": "ok", "metrics": {"m": dict(skip)}}})
        violations, warnings = metrics.compare_metrics(base, run)
        self.assertEqual(violations, [])
        self.assertTrue(any("still skipped" in w for w in warnings), warnings)

    def test_changed_error_marker_fails(self):
        base = _doc(
            {"p": {"status": "ok", "metrics": {"m": {"skipped": "provider-refused"}}}}
        )
        run = _doc({"p": {"status": "ok", "metrics": {"m": {"error": "timeout"}}}})
        violations, _ = metrics.compare_metrics(base, run)
        self.assertTrue(
            any(v["metric"] == "m" and v["change"] == "broken-in-run" for v in violations),
            violations,
        )

    def test_cli_address_discovery_avoids_duplicate_rule_names(self):
        self.assertEqual(metrics.unique_name([{"name": "duplicate"}, {"name": "unique"}, {"name": "duplicate"}]), "unique")
        self.assertIsNone(metrics.unique_name([{"name": "same"}, {"name": "same"}]))

    def test_snapshot_wrapped_listings_still_measure_the_list(self):
        wrapped = {"symbols": [{"id": 0, "name": "a"}, {"id": 1, "name": "b"}], "snapshot": 1}
        self.assertEqual(metrics._snapshot_listing(wrapped), wrapped["symbols"])
        self.assertEqual(metrics._first_id(wrapped), 0)
        self.assertEqual(metrics.unique_name(wrapped), "a")
        dup = {"rules": [{"name": "same"}, {"name": "same"}], "snapshot": 3}
        self.assertIsNone(metrics.unique_name(dup))
        self.assertEqual(metrics.agent_counts("symbols", wrapped), {"items": 2})
        # `snapshot` is contract metadata, never a counted field.
        self.assertNotIn("snapshot", metrics.default_counts({"snapshot": 2, "reads": 3}))
        # A capped listing keeps `truncation`/`selection` members; the list is
        # still found among them and measures in the legacy `items` vocabulary.
        capped = {
            "rules": [{"id": 0}, {"id": 1}],
            "selection": {"withheld": 5},
            "truncation": {"rules": {"total": 7, "shown": 2, "truncated": 5}},
            "snapshot": 2,
        }
        self.assertEqual(metrics._snapshot_listing(capped), capped["rules"])
        self.assertEqual(metrics._first_id(capped), 0)
        self.assertEqual(
            metrics._listing_measurement(capped),
            {
                "items": capped["rules"],
                "selection": {"withheld": 5},
                "truncation": {"items": {"total": 7, "shown": 2, "truncated": 5}},
            },
        )
        # Two list members make the payload ambiguous — no unwrapping.
        self.assertIsNone(metrics._snapshot_listing({"rules": [], "symbols": [], "snapshot": 1}))
        self.assertIsNone(metrics._snapshot_listing([{"id": 0}]))

    def test_default_counts_retains_complete_totals(self):
        result = {"counts": {"findings": {"total": 90, "error": 1}}, "findings": [{}, {}]}
        expected = {"counts.findings.total": 90, "counts.findings.error": 1, "findings": 2}
        self.assertEqual(metrics.default_counts(result), expected)
        self.assertEqual(metrics.default_counts({"result": result}), expected)


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
            | {f"cli:inspect {cmd}" for cmd in metrics.INSPECT_COMMANDS}
            | {f"cli:{cmd}?text" for cmd in metrics.CLI_COMMANDS if cmd != "compile"}
            | {f"cli:inspect {cmd}?text" for cmd in metrics.INSPECT_COMMANDS},
        )
        for key, record in project["metrics"].items():
            if "skipped" in record:
                continue
            self.assertIn("bytes", record, key)
            self.assertIn("tokens", record, key)
            if key.startswith("cli:") and key.endswith("?text"):
                self.assertNotIn("latencyMs", record, key)
            else:
                self.assertIn("latencyMs", record, key)

    def test_check_never_rewrites_baseline(self):
        with tempfile.TemporaryDirectory() as tmp:
            baseline_path = Path(tmp) / "baseline.json"
            baseline = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000)}}})
            baseline_path.write_text(json.dumps(baseline))
            before = baseline_path.read_bytes()
            drifting = _doc({"p": {"status": "ok", "metrics": {}}})
            with mock.patch.object(metrics, "run_metrics", return_value=drifting):
                code = _check_main(baseline_path)
            self.assertEqual(code, 1)
            self.assertEqual(baseline_path.read_bytes(), before)

    def test_check_from_shards_fails_when_a_project_has_no_shard(self):
        with tempfile.TemporaryDirectory() as tmp:
            baseline_path = Path(tmp) / "baseline.json"
            ok = {"status": "ok", "metrics": {"m": _metric(1000)}}
            baseline_path.write_text(
                json.dumps(_doc({"small-workshop": ok, "bastion": ok}))
            )
            shard = Path(tmp) / "small-workshop.json"
            shard.write_text(json.dumps(_doc({"small-workshop": ok})))
            with mock.patch.object(metrics, "run_metrics") as measure:
                code = _check_main(baseline_path, "--from", str(shard))
            self.assertEqual(code, 1)
            measure.assert_not_called()

    def test_size_and_latency_drift_report_without_failing(self):
        with tempfile.TemporaryDirectory() as tmp:
            baseline_path = Path(tmp) / "baseline.json"
            baseline = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000)}}})
            baseline_path.write_text(json.dumps(baseline))
            slower = _doc({"p": {"status": "ok", "metrics": {"m": _metric(1000, latency_ms=900.0)}}})
            bigger = _doc({"p": {"status": "ok", "metrics": {"m": _metric(4000)}}})
            with mock.patch.object(metrics, "run_metrics", return_value=slower) as run:
                self.assertEqual(_check_main(baseline_path), 0)
                self.assertEqual(_check_main(baseline_path, "--skip-latency"), 0)
            self.assertEqual(run.call_args.args[2:], (1, None, 1))
            with mock.patch.object(metrics, "run_metrics", return_value=bigger):
                self.assertEqual(_check_main(baseline_path, "--skip-latency"), 0)

    def test_split_keeps_only_independent_failures_blocking(self):
        base = _doc({"p": {"status": "ok", "metrics": {"m": {"error": "x"}, "n": _metric(1000)}}})
        run = _doc({"p": {"status": "ok", "metrics": {"m": _metric(), "n": _metric(4000)}}})
        blocking, drift = metrics.split_changes(metrics.compare_metrics(base, run)[0])
        self.assertEqual(blocking, [])
        self.assertEqual({(v["metric"], v["change"]) for v in drift}, {("m", "now-measured"), ("n", "increase")})

    def test_refusal_envelope_is_an_error_not_a_measurement(self):
        refusal = {"ok": False, "diagnostics": [{"code": "source-provider-unsupported"}]}
        self.assertEqual(metrics.envelope_failure(refusal), "source-provider-unsupported")
        self.assertIsNone(metrics.envelope_failure({"ok": False, "exit": 1, "result": {"findings": []}}))
        self.assertEqual(metrics.envelope_failure({"ok": False, "diagnostics": []}), "not-ok")
        self.assertIsNone(metrics.envelope_failure({"ok": True, "diagnostics": [{"code": "w"}]}))


class ScenarioTest(unittest.TestCase):
    def test_sequence_sums_every_step(self):
        sizes = {"check": 400, "lint": 200, "analyze": 600}
        record = metrics.accumulate_scenario(
            [["check"], ["lint"], ["analyze"], ["check"]], lambda step: (sizes[step[0]], None)
        )
        self.assertEqual((record["bytes"], record["tokens"]), (1600, 400.0))
        self.assertEqual(record["counts"], {"steps": 4})

    def test_failed_or_refused_step_is_an_error_not_a_partial_sum(self):
        def execute(step):
            return (500, "exit-3") if step == ["lint"] else (100, None)

        record = metrics.accumulate_scenario([["check"], ["lint"], ["analyze"]], execute)
        self.assertEqual(set(record), {"error"})
        self.assertIn("step 2", record["error"])

    def test_scenario_changes_never_block(self):
        base = _doc({"p": {"status": "ok", "metrics": {"scenario:s": _metric(1000)}}})
        for run_metric in ({"error": "step 1 (check): exit-3"}, _metric(9000)):
            run = _doc({"p": {"status": "ok", "metrics": {"scenario:s": run_metric}}})
            blocking, drift = metrics.split_changes(metrics.compare_metrics(base, run)[0])
            self.assertEqual(blocking, [])
            self.assertTrue(drift)

    def test_ratio_compares_largest_project_to_each_seed(self):
        projects = {
            "seed-a": {"metrics": {"scenario:s": {"tokens": 100.0}}},
            "seed-b": {"metrics": {"scenario:s": {"tokens": 50.0}}},
            "big": {"metrics": {"scenario:s": {"tokens": 1000.0}}},
        }
        corpus_projects = [{"id": i, "scenarios": ["s"]} for i in projects]
        ratios = metrics.scenario_ratios(projects, [{"id": "s"}], corpus_projects)
        self.assertEqual(
            ratios["s"], {"largest": "big", "tokens": 1000.0, "vsSeed": {"seed-a": 10.0, "seed-b": 20.0}}
        )


class MergeTest(unittest.TestCase):
    CORPUS = {
        "version": "v1",
        "projects": [
            {"id": "seed", "scenarios": ["s"]},
            {"id": "big", "scenarios": ["s"]},
        ],
        "scenarios": [{"id": "s"}],
    }

    def _shard(self, pid, tokens, **overrides):
        document = _doc({pid: {"status": "ok", "metrics": {"scenario:s": {"tokens": tokens}}}})
        document.update(overrides)
        return document

    def test_merge_equals_the_unpartitioned_run(self):
        whole = self._shard("seed", 100.0)
        whole["projects"].update(self._shard("big", 1000.0)["projects"])
        whole["scenarioRatios"] = metrics.scenario_ratios(
            whole["projects"], self.CORPUS["scenarios"], self.CORPUS["projects"]
        )
        shards = [self._shard("big", 1000.0), self._shard("seed", 100.0)]
        self.assertEqual(metrics.merge_runs(shards, self.CORPUS), whole)

    def test_shards_that_disagree_on_build_identity_do_not_merge(self):
        shards = [self._shard("seed", 1.0), self._shard("big", 1.0, opyProvider="0.0.1")]
        with self.assertRaisesRegex(ValueError, "opyProvider"):
            metrics.merge_runs(shards, self.CORPUS)

    def test_a_project_in_two_shards_does_not_merge(self):
        shards = [self._shard("seed", 1.0), self._shard("seed", 2.0)]
        with self.assertRaisesRegex(ValueError, "more than one shard"):
            metrics.merge_runs(shards, self.CORPUS)

    def test_a_project_no_shard_covered_blocks(self):
        base = _doc({p: {"status": "ok", "metrics": {"m": _metric()}} for p in ("seed", "big")})
        merged = metrics.merge_runs(
            [_doc({"seed": {"status": "ok", "metrics": {"m": _metric()}}})], self.CORPUS
        )
        blocking, _ = metrics.split_changes(metrics.compare_metrics(base, merged)[0])
        self.assertEqual(
            [(v["project"], v["change"]) for v in blocking], [("big", "removed-from-run")]
        )


def _check_main(baseline_path: Path, *extra: str) -> int:
    """Invoke `check` against a canned baseline without touching wright."""
    argv = ["metrics.py", "check", "--baseline", str(baseline_path), *extra]
    with mock.patch("sys.argv", argv):
        return metrics.main()


if __name__ == "__main__":
    unittest.main()
