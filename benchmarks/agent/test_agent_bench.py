import argparse
import hashlib
import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

import agent_bench
import bench_grade
import bench_report
import bench_trace
import wiki_skill

WRIGHT = os.environ.get("WRIGHT_BIN", str(agent_bench.ROOT / "target/debug/wright"))
SCENARIO = "repair-runaway-loop"


def reference(scenario: str = SCENARIO) -> str:
    return str(agent_bench.SCENARIOS / scenario / "reference")


@unittest.skipUnless(Path(WRIGHT).is_file(), "build wright first or set WRIGHT_BIN")
class AgentBenchTest(unittest.TestCase):
    def setUp(self):
        (agent_bench.ROOT / "target").mkdir(exist_ok=True)
        self.out = Path(tempfile.mkdtemp(dir=agent_bench.ROOT / "target")).resolve()
        self.addCleanup(shutil.rmtree, self.out, True)

    def trial(self, agent_cmd: str, wright: str = "bin", scenario: str = SCENARIO, knowledge: str = "none", **options) -> dict:
        args = argparse.Namespace(**{
            "wright": str(Path(WRIGHT).resolve()), "agent_id": "fake", "agent_cmd": agent_cmd, "timeout": 60, "infra_retries": 2,
            "env_pass": [], "canary_cmd": None, "skill_dir": None, "wiki_dir": None, "wiki_skill_dir": None, "check_ancestors": False, **options,
        })
        cell = {"wright": wright, "knowledge": knowledge, "network": "off"}
        return agent_bench.run_trial(agent_bench.load_scenario(scenario), cell, args, self.out / f"{scenario}-{wright}")

    def test_wiki_snapshot_is_linked_and_identified(self):
        snapshot = self.out / "snapshot"
        (snapshot / "articles").mkdir(parents=True)
        (snapshot / "articles/wait-until.md").write_text("# Wait Until\n")
        article_hash = hashlib.sha256((snapshot / "articles/wait-until.md").read_bytes()).hexdigest()
        snapshot_hash = hashlib.sha256(f"wait-until {article_hash}".encode()).hexdigest()
        (snapshot / "SNAPSHOT.json").write_text(json.dumps({"source": "https://mirror.example", "fetchedAt": "2026-09-30T00:00:00+00:00", "snapshotSha256": snapshot_hash, "documents": [{"slug": "wait-until", "sha256": article_hash}]}))
        result = self.trial("ls wiki/articles > listing.txt", knowledge="wiki", wiki_dir=snapshot)
        self.assertEqual(result["environment"]["wiki"]["snapshotSha256"], snapshot_hash)
        self.assertEqual(result["environment"]["wiki"]["documents"], 1)
        self.assertEqual((self.out / f"{SCENARIO}-bin/workspace/listing.txt").read_text().strip(), "wait-until.md")
        self.assertNotIn("wiki", " ".join(result["unsafeEdits"]))

    def test_wiki_skill_level_requires_a_built_skill_and_expects_only_that_skill(self):
        skill = self.out / "workshop-wiki"
        skill.mkdir()
        (skill / "SKILL.md").write_text("---\nname: workshop-wiki\n---\n")
        skill_hash = wiki_skill.content_hash(skill)
        (skill / "BUILD.json").write_text(json.dumps({"name": "workshop-wiki", "skillSha256": skill_hash, "articles": 3}))
        with self.assertRaises(SystemExit):
            self.trial("true", knowledge="wiki-skill")
        expected = self.trial("echo '{\"loaded\": [\"workshop-wiki\"]}' > \"$BENCH_CONTEXT\"; echo \"$BENCH_WIKI_SKILL_DIR\" > dir.txt", knowledge="wiki-skill", wiki_skill_dir=skill)
        self.assertNotIn("invalid", expected)
        self.assertEqual(expected["environment"]["wikiSkill"]["skillSha256"], skill_hash)
        self.assertEqual((self.out / f"{SCENARIO}-bin/workspace/dir.txt").read_text().strip(), str(skill))
        stray = self.trial("echo '{\"loaded\": [\"workshop-wiki\", \"other\"]}' > \"$BENCH_CONTEXT\"", knowledge="wiki-skill", wiki_skill_dir=skill)
        self.assertIn("unexpected loaded context", stray["invalid"])
        (skill / "SKILL.md").write_text("changed")
        with self.assertRaisesRegex(SystemExit, "wiki skill content mismatch"):
            self.trial("true", knowledge="wiki-skill", wiki_skill_dir=skill)

    def test_scenarios_are_solvable_and_not_vacuous(self):
        self.assertTrue(agent_bench.validate(WRIGHT, self.out / "validate"))

    @unittest.skipUnless(sys.platform == "darwin" and shutil.which("sandbox-exec"), "macOS file sandbox")
    def test_agent_file_writes_cannot_escape_the_trial_directory(self):
        code = 'from pathlib import Path; Path("allowed.txt").write_text("ok"); Path("../../escaped.txt").write_text("bad")'
        import shlex
        result = self.trial(f'{shlex.quote(sys.executable)} -c {shlex.quote(code)}', file_sandbox=True)
        self.assertNotEqual(result["agent"]["exit"], 0)
        self.assertFalse((self.out / "escaped.txt").exists())
        self.assertEqual((self.out / f"{SCENARIO}-bin/workspace/allowed.txt").read_text(), "ok")
        self.assertEqual(result["fileWriteEnforcement"], "trial-directory-only")

    def test_levels_differ_only_in_wright_availability(self):
        agent = f"cp {reference()}/* . && (wright check mode.ws >/dev/null 2>&1 || echo no-wright > missing-wright.txt)"
        none = self.trial(agent, wright="none")
        assisted = self.trial(agent, wright="bin")
        self.assertEqual(none["wrightUse"]["invocations"], 0)
        self.assertIn("missing-wright.txt", none["unsafeEdits"])
        self.assertEqual(assisted["wrightUse"]["byCommand"], {"check": 1})
        self.assertTrue(assisted["passed"])
        self.assertEqual(assisted["unsafeEdits"], [])

    def test_canary_rejects_reachable_wright_under_none(self):
        cell = {"wright": "none", "knowledge": "none", "network": "off"}
        args = argparse.Namespace(canary_cmd=None, check_ancestors=False)
        env = {"PATH": str(Path(WRIGHT).resolve().parent)}
        self.assertIn("reachable", agent_bench.canaries(cell, env, self.out, args))
        self.assertIsNone(agent_bench.canaries({**cell, "wright": "bin"}, env, self.out, args))

    def test_canary_rejects_instruction_files_above_the_workspace(self):
        outside = Path(tempfile.mkdtemp()).resolve()  # outside the repository, whose own AGENTS.md would match
        self.addCleanup(shutil.rmtree, outside, True)
        workspace = outside / "repo/runs/w"
        workspace.mkdir(parents=True)
        cell = {"wright": "bin", "knowledge": "none", "network": "on"}
        args = argparse.Namespace(canary_cmd=None, check_ancestors=True)
        self.assertIsNone(agent_bench.canaries(cell, {"PATH": ""}, workspace, args))
        (outside / "repo/AGENTS.md").write_text("instructions")
        self.assertIn("AGENTS.md", agent_bench.canaries(cell, {"PATH": ""}, workspace, args))

    def test_network_canary_invalidates_run(self):
        result = self.trial("true", canary_cmd="true")
        self.assertIn("network reachable", result["invalid"])
        self.assertIsNone(self.trial("true", canary_cmd="false").get("invalid"))

    def test_environment_is_scrubbed(self):
        os.environ["BENCH_LEAK_PROBE"] = "leak"
        self.addCleanup(os.environ.pop, "BENCH_LEAK_PROBE", None)
        self.trial("env > env.txt")
        env = (self.out / f"{SCENARIO}-bin/workspace/env.txt").read_text()
        self.assertNotIn("BENCH_LEAK_PROBE", env)
        self.assertIn("BENCH_KNOWLEDGE=none", env)
        self.assertIn(f"HOME={self.out}/{SCENARIO}-bin/home", env)

    def test_trace_records_envelope_and_serve_sessions(self):
        agent = (f"cp {reference()}/* . && wright lint mode.ws -f json >/dev/null; "
                 "printf '{\"op\":\"capabilities\"}\\n{\"op\":\"lint\"}\\n' | wright serve mode.ws >/dev/null")
        result = self.trial(agent)
        events = bench_trace.read_events(self.out / f"{SCENARIO}-bin/wright-trace.jsonl")
        lint = next(e for e in events if e["type"] == "call" and bench_trace.command_of(e["argv"]) == "lint")
        self.assertEqual(lint["envelope"]["command"], "lint")
        self.assertRegex(lint["envelope"]["inputIdentity"], r"^[0-9a-f]{64}$")
        self.assertEqual(bench_trace.serve_ops(events), ["capabilities", "lint"])
        self.assertEqual(result["expectations"]["E11"]["status"], "pass")
        self.assertEqual(result["expectations"]["E03"]["status"], "pass")

    def test_final_state_validation_expectation(self):
        edit = f"cp {reference()}/mode.ws mode.ws"
        late = self.trial(f"wright check mode.ws >/dev/null; sleep 0.6; {edit}")
        self.assertEqual(late["expectations"]["E04"]["status"], "fail")
        tight = self.trial(f"{edit}; sleep 0.6; wright check mode.ws >/dev/null")
        self.assertEqual(tight["expectations"]["E04"]["status"], "pass")
        self.assertEqual(tight["snapshots"]["firstValidIndex"], 1)

    def test_usage_context_and_first_valid(self):
        rows = [
            {"t": 1.0, "input": 10, "output": 5, "cache_read": 100, "cache_write": 0, "context": 110, "context_limit": 1000},
            {"t": 2.0, "input": 20, "output": 5, "cache_read": 200, "cache_write": 0, "context": 220, "context_limit": 1000},
        ]
        path = self.out / "usage.jsonl"
        path.write_text("\n".join(json.dumps(r) for r in rows))
        summary = bench_trace.usage_summary(path, first_valid_t=1.5)
        self.assertEqual(summary["totalTokens"], 340)
        self.assertEqual(summary["peakContext"], 220)
        self.assertEqual(summary["peakContextShare"], 0.22)
        self.assertEqual(summary["toFirstValid"], {"turns": 1, "tokens": 115})
        self.assertIsNone(bench_trace.usage_summary(self.out / "missing.jsonl", None))

    def test_adapter_usage_reaches_result(self):
        row = json.dumps({"t": 1.0, "input": 1, "output": 2, "cache_read": 3, "cache_write": 0, "context": 4, "context_limit": 8})
        result = self.trial(f"echo '{row}' > \"$BENCH_USAGE\"; echo '{{\"loaded\": [\"stray\"]}}' > \"$BENCH_CONTEXT\"")
        self.assertEqual(result["usage"]["totalTokens"], 6)
        self.assertIn("unexpected loaded context", result["invalid"])

    def test_infrastructure_failures_are_retried(self):
        agent = 'if [ -f "$BENCH_RUN_DIR/tried" ]; then exit 0; else touch "$BENCH_RUN_DIR/tried"; exit 75; fi'
        self.assertEqual(self.trial(agent)["infraRetries"], 1)

    def tiny_scenarios(self, negative_fails: list[str]) -> Path:
        scenarios = self.out / "scenarios"
        directory = scenarios / "tiny"
        for name, text in (("seed", ""), ("reference", "Y"), ("negative/nope", "N")):
            (directory / name).mkdir(parents=True)
            (directory / name / "mode.ws").write_text(text)
        spec = {
            "id": "tiny", "family": "modification", "language": "workshop", "entry": "mode.ws", "writable": ["mode.ws"],
            "checks": [{"id": "has-y", "kind": "contains", "file": "mode.ws", "text": "Y"}],
            "negatives": {"nope": {"fails": negative_fails}},
        }
        (directory / "scenario.json").write_text(json.dumps(spec))
        original = agent_bench.SCENARIOS
        agent_bench.SCENARIOS = scenarios
        self.addCleanup(setattr, agent_bench, "SCENARIOS", original)
        return directory

    def test_validate_checks_negatives_fail_exactly_as_declared(self):
        self.tiny_scenarios(["has-y"])
        self.assertTrue(agent_bench.validate(WRIGHT, self.out / "v1"))
        (agent_bench.SCENARIOS / "tiny/scenario.json").write_text(json.dumps({
            **json.loads((agent_bench.SCENARIOS / "tiny/scenario.json").read_text()), "negatives": {"nope": {"fails": []}},
        }))
        self.assertFalse(agent_bench.validate(WRIGHT, self.out / "v2"))

    def test_oracle_check_never_passes_silently(self):
        directory = self.tiny_scenarios([])
        spec = {**json.loads((directory / "scenario.json").read_text()), "checks": [{"id": "oracle", "kind": "oracle"}]}
        graded = bench_grade.grade({**spec, "dir": directory}, directory / "reference", WRIGHT)
        self.assertFalse(graded["checks"][0]["passed"])  # a raw Workshop entry has no upstream OverPy verdict
        self.assertRegex(graded["grader"]["hash"], r"^[0-9a-f]{64}$")

    @unittest.skipUnless(bench_grade.oracle_available(), "run `agent_bench.py setup-oracle`")
    def test_oracle_disagreement_is_reported(self):
        source = self.out / "n.opy"
        source.write_text('settings {"main": {"description": "t"}, "gamemodes": {"skirmish": {"enabledMaps": ["workshopIsland"]}}}\n'
                          'globalvar A\nrule "a":\n    @Event eachPlayer\n    A = allTankHeroes()\n')
        auth = bench_grade.authorities(WRIGHT, source, self.out / "auth")
        self.assertEqual(auth["oracle"]["status"], "error")
        if auth["wrightCompile"]["status"] == "ok":
            self.assertEqual(auth["disagreement"]["kind"], "wright-accepts-oracle-rejects")


class DetectorTest(unittest.TestCase):
    def call(self, argv, exit_code=0, t=0.0, **extra):
        return {"type": "call", "t": t, "argv": argv, "exit": exit_code, "seconds": 0.1, "stdoutBytes": 40, "stderrBytes": 0, "stderrHead": "", "envelope": None, **extra}

    def test_discovery_and_structured_output(self):
        good = bench_trace.detect_expectations([self.call(["--help"]), self.call(["check", "m.ws", "-f", "json"])], [], {}, None)
        self.assertEqual((good["E01"]["status"], good["E02"]["status"]), ("pass", "pass"))
        bad = bench_trace.detect_expectations([self.call(["check", "m.ws"])], [], {}, None)
        self.assertEqual((bad["E01"]["status"], bad["E02"]["status"]), ("fail", "fail"))

    def test_retry_storm_and_friction(self):
        events = [self.call(["update"], 4), self.call(["update"], 4), self.call(["update"], 4), self.call(["nope"], 2, stderrHead="unrecognized subcommand")]
        self.assertEqual(bench_trace.detect_expectations(events, [], {}, None)["E08"]["status"], "fail")
        friction = bench_trace.friction(events)
        self.assertEqual((friction["usageErrors"], friction["unknownSubcommands"], friction["identicalRepeats"]), (1, 1, 2))

    def test_unused_wright_is_not_applicable(self):
        result = bench_trace.detect_expectations([], [], {"stabilityRisk": True}, None)
        self.assertEqual(result["E03"]["status"], "na")
        self.assertEqual(result["E05"]["status"], "unavailable")


class ReportTest(unittest.TestCase):
    def result(self, cell, trial, usable, tokens, split=None):
        wright, knowledge, network = cell.split("/")
        return {
            "contract": "wright-agent-bench/v2", "scenario": "s", "split": split, "_trial": trial, "_dir": Path("d"),
            "condition": {"wright": wright, "knowledge": knowledge, "network": network}, "agent": {"id": "m", "exit": 0, "seconds": 1.0},
            "usable": usable, "passed": usable, "usage": {"totalTokens": tokens, "peakContext": tokens // 2}, "wrightUse": {"invocations": 1 if wright != "none" else 0},
        }

    def test_wilson_interval(self):
        low, high = bench_report.wilson(5, 10)
        self.assertAlmostEqual(low, 0.237, places=2)
        self.assertAlmostEqual(high, 0.763, places=2)
        self.assertEqual(bench_report.wilson(0, 0), (0.0, 0.0))

    def test_paired_efficiency_counts_only_both_usable_and_failures_cost(self):
        runs = [self.result("none/none/off", 1, True, 1000), self.result("bin/none/off", 1, True, 600),
                self.result("none/none/off", 2, False, 900), self.result("bin/none/off", 2, True, 700)]
        text, summary = bench_report.render(runs)
        self.assertIn("+1 / -0", text)
        self.assertIn("+40% tokens (n=1)", text)
        self.assertEqual(summary["cells"]["m|bin/none/off"]["tokensPerUsable"], 650)
        self.assertEqual(summary["cells"]["m|none/none/off"]["tokensPerUsable"], 1900)

    def test_headroom_and_invalid_runs_are_reported(self):
        runs = [self.result("none/none/off", t, True, 100) for t in range(1, 5)]
        runs.append({**self.result("bin/none/off", 1, True, 100), "invalid": "canary"})
        text, _ = bench_report.render(runs)
        self.assertIn("HEADROOM", text)
        self.assertIn("INVALID: 1 run(s) excluded", text)

    def test_provider_failures_do_not_count_as_agent_failures(self):
        good = self.result("none/none/off", 1, True, 100)
        provider_failure = self.result("none/none/off", 2, False, 0)
        provider_failure["agent"]["exit"] = 75
        text, summary = bench_report.render([good, provider_failure])
        self.assertEqual(summary["cells"]["m|none/none/off"]["n"], 1)
        self.assertEqual(summary["infrastructureFailures"], 1)
        self.assertIn("excluded from outcome metrics", text)


if __name__ == "__main__":
    unittest.main()
