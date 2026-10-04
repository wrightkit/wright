import json
import shutil
import tempfile
import unittest
from pathlib import Path

import bench_score


def run(scenario, trial, usable, label=bench_score.CANONICAL, status="completed", split="test", language="opy", sha="a" * 64, model="m", **fields):
    return {
        "scenario": scenario, "family": "diagnosis" if scenario.endswith("1") else "modification", "language": language, "split": split,
        "condition": {"label": label}, "status": status, "usable": usable, "failedLayers": [] if usable else ["agent"],
        "agent": {"id": "codex", "seconds": 10.0}, "agentInfo": {"model": model, "effort": "high"}, "protocol": {"timeoutSeconds": 60, "infraRetries": 0},
        "environment": {"wright": "wright 0.5.0", "wrightSha256": sha, "skills": {"wright-skill": {"sha256": "b" * 64}}, "suite": {"version": "v1", "hash": "c" * 64}, "harness": "abc"},
        "networkEnforcement": "declared-only", "fileWriteEnforcement": "trial-directory-only",
        "fileReadEnforcement": {"mode": "allow-list", "hidden": ["/home"], "allowed": ["/run"]}, "usage": {"totalTokens": 1000}, "_trial": trial, **fields,
    }


SCENARIOS = [f"s{i}" for i in range(8)]


def runs(usable_scenarios, trials=3, **kw):
    return [run(s, t, s in usable_scenarios, **kw) for s in SCENARIOS for t in range(1, trials + 1)]


class ScoreTest(unittest.TestCase):
    def test_interval_is_deterministic_and_degenerate_when_nothing_varies(self):
        outcomes = {"a": [1, 1, 1], "b": [0, 0, 0]}
        self.assertEqual(bench_score.cluster_interval(outcomes), bench_score.cluster_interval(outcomes))
        lo, hi = bench_score.cluster_interval(outcomes)
        self.assertTrue(0 <= lo <= 50 <= hi <= 100)
        self.assertEqual(bench_score.cluster_interval({"a": [1, 1], "b": [1, 1]}), (100.0, 100.0))

    def test_pass_power_k(self):
        self.assertAlmostEqual(bench_score.pass_power_k(2, 3, 2), 1 / 3)
        self.assertEqual(bench_score.pass_power_k(3, 3, 3), 1.0)
        self.assertEqual(bench_score.pass_power_k(1, 3, 2), 0.0)

    def test_score_macro_averages_scenarios_with_equal_weight(self):
        data = runs({"s0", "s1", "s2", "s3"})
        data += [run("s0", 4, True)]  # an extra trial must not change a scenario's weight beyond its own rate
        c = bench_score.card([r for r in data if r["scenario"] != "s0" or r["_trial"] <= 3], "opy", SCENARIOS)
        self.assertEqual(c["score"], 50.0)
        self.assertEqual(c["scenarios"], 8)
        self.assertEqual(c["trialsPerScenario"], 3)
        self.assertEqual(c["provisional"], [])
        self.assertEqual(c["passPowK"], 50.0)
        unequal = bench_score.card(data, "opy", SCENARIOS)
        self.assertTrue(any("unequal valid trials" in p for p in unequal["provisional"]))
        self.assertEqual(unequal["score"], 50.0)

    def test_only_canonical_test_runs_of_the_language_count(self):
        data = runs({"s0"}) + runs(set(SCENARIOS), label="wright/none/off") + runs(set(SCENARIOS), split="train") + runs(set(SCENARIOS), language="workshop")
        c = bench_score.card(data, "opy", SCENARIOS)
        self.assertEqual(c["score"], 12.5)
        self.assertEqual(c["validRuns"], 24)

    def test_interrupted_invalid_and_agent_error_runs_are_excluded_and_published(self):
        data = runs({"s0", "s1"})
        data += [run("s0", 4, False, status="provider-interrupted"), run("s1", 4, False, status="invalid"), run("s2", 4, False, status="agent-error"), run("s3", 4, False, status="timeout")]
        c = bench_score.card(data, "opy", SCENARIOS)
        self.assertEqual(c["exclusions"], {"agent-error": 1, "invalid": 1, "provider-interrupted": 1})
        self.assertEqual(c["validRuns"], 25)  # a timeout is the agent's own outcome and counts

    def test_runs_from_different_environments_are_refused(self):
        mixed = runs({"s0"}) + [run("s0", 9, True, sha="d" * 64)]
        c = bench_score.card(mixed, "opy", SCENARIOS)
        self.assertIn("wrightSha256", c["refused"])
        self.assertIn("no score", bench_score.render(c))
        self.assertIn("model", bench_score.card(runs({"s0"}) + [run("s1", 9, True, model="other")], "opy", SCENARIOS)["refused"])

    def test_mixed_file_read_enforcement_is_refused(self):
        mixed = runs({"s0"}) + [run("s0", 9, True, fileReadEnforcement="unrestricted")]
        c = bench_score.card(mixed, "opy", SCENARIOS)
        self.assertIn("fileReadEnforcement", c["refused"])
        clean = bench_score.card(runs(set(SCENARIOS)), "opy", SCENARIOS)
        self.assertEqual(clean["fileReadEnforcement"], ["allow-list"])  # the allow-list mode, not the per-trial path lists
        self.assertIn("File reads:  allow-list", bench_score.render(clean))

    def test_small_suites_and_missing_scenarios_are_provisional(self):
        c = bench_score.card(runs(set(SCENARIOS))[:21], "opy", SCENARIOS)
        self.assertTrue(any("missing held-out scenarios" in p for p in c["provisional"]))
        few = ["s0", "s1"]
        small = bench_score.card([r for r in runs(set(SCENARIOS)) if r["scenario"] in few], "opy", few)
        self.assertTrue(any("fewer than 8" in p for p in small["provisional"]))
        self.assertIn("PROVISIONAL", bench_score.render(small))

    def test_card_discloses_network_enforcement_and_identity(self):
        c = bench_score.card(runs(set(SCENARIOS)), "opy", SCENARIOS)
        text = bench_score.render(c)
        self.assertEqual(c["networkEnforcement"], ["declared-only"])
        self.assertIn("declaration only", text)
        for needle in ("Suite:", "Wright:", "Skills:", "wright-skill", "Model:       m", "95% CI"):
            self.assertIn(needle, text)
        json.dumps(c)

    def test_suite_version_parts_and_mode_are_comparison_identity(self):
        suite = {"version": "v2", "publicHash": "p", "privateHash": "q", "hash": "combined", "mode": "official"}
        data = runs(set(SCENARIOS))
        for r in data:
            r["environment"]["suite"] = suite.copy()
        self.assertEqual(bench_score.card(data, "opy", SCENARIOS)["mode"], "official")
        for key, value in (("version", "v3"), ("publicHash", "other"), ("privateHash", "other"), ("mode", "public sample")):
            changed = run("s0", 9, True)
            changed["environment"]["suite"] = {**suite, key: value}
            self.assertIn("suite", bench_score.card([*data, changed], "opy", SCENARIOS)["refused"])
        sample = bench_score.card(runs(set(SCENARIOS)), "opy", SCENARIOS)
        self.assertEqual(sample["mode"], "public sample")
        self.assertIn("public sample", bench_score.render(sample))

    def test_published_private_results_do_not_contain_task_or_grader_material(self):
        import bench_report
        marker = "DO_NOT_PUBLISH_PRIVATE_CONTENT"
        private = run("private-1", 1, False, isPrivate=True, family=marker, prompt=marker, checks=[marker],
                      reference=marker, expectations={marker: {"status": "fail"}}, invalid=marker,
                      disagreement={"kind": marker}, friction={marker: 1}, failedLayers=[marker],
                      toolUse={"wright": {"invocations": 1, "outputTokensEstimate": {marker: 50}}},
                      context={"reported": True, "loaded": [marker]}, _dir=Path(marker))
        private["environment"]["suite"] = {"version": "v2", "publicHash": "p", "privateHash": "q", "hash": "h", "mode": "official", "path": marker}
        score = bench_score.card([private], "opy", ["private-1"])
        text, summary = bench_report.render([private, {**private, "status": "provider-interrupted"}], [marker])
        published = bench_score.render(score) + json.dumps(score) + text + json.dumps(summary)
        self.assertNotIn(marker, published)
        self.assertIn("private-1", published)
        self.assertEqual(summary["mode"], "official")
        self.assertEqual(summary["scenarios"], [{"id": "private-1", "runs": 1}])


    def test_main_writes_the_machine_readable_card_and_exit_status(self):
        root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, root, True)
        for i, r in enumerate(runs(set(SCENARIOS))):
            d = root / f"r{i}-{r['_trial']}"
            d.mkdir()
            payload = {k: v for k, v in r.items() if k != "_trial"}
            (d / "result.json").write_text(json.dumps({"contract": "wright-agent-bench/v3", **payload}))
        self.assertEqual(bench_score.main([root], ["opy"], {"opy": SCENARIOS}, None), 0)
        card = json.loads((root / "score.json").read_text())
        self.assertEqual(card["contract"], "wright-agent-score/v1")
        self.assertEqual(card["cards"][0]["score"], 100.0)
        self.assertEqual(bench_score.main([root], ["workshop"], {"workshop": SCENARIOS}, None), 2)

    def test_compare_tables_runs_and_warns_when_the_environment_differs(self):
        root = Path(tempfile.mkdtemp(dir=Path(__file__).resolve().parents[2] / "target"))
        self.addCleanup(shutil.rmtree, root, True)
        for name, sha, model in (("codex-run", "a" * 64, "luna"), ("pi-run", "d" * 64, "luna2")):
            directory = root / name
            directory.mkdir()
            card = bench_score.card(runs(SCENARIOS[:6], sha=sha, model=model), "opy", SCENARIOS)
            (directory / "score.json").write_text(json.dumps({"contract": bench_score.CONTRACT, "cards": [card]}))
        table = bench_score.compare([root / "codex-run", root / "pi-run"])
        self.assertIn("codex-run", table)
        self.assertIn("pi-run", table)
        self.assertIn("effort high", table)
        self.assertIn("runs differ in wrightSha256", table)
        same = bench_score.compare([root / "codex-run"])
        self.assertNotIn("WARNING", same)

    def test_compare_warns_when_the_file_read_policy_differs(self):
        root = Path(tempfile.mkdtemp(dir=Path(__file__).resolve().parents[2] / "target"))
        self.addCleanup(shutil.rmtree, root, True)
        for name, enforcement in (("sandboxed", {"mode": "allow-list"}), ("open", "unrestricted")):
            directory = root / name
            directory.mkdir()
            card = bench_score.card(runs(SCENARIOS[:6], fileReadEnforcement=enforcement), "opy", SCENARIOS)
            (directory / "score.json").write_text(json.dumps({"contract": bench_score.CONTRACT, "cards": [card]}))
        self.assertIn("runs differ in fileReadEnforcement", bench_score.compare([root / "sandboxed", root / "open"]))


if __name__ == "__main__":
    unittest.main()
