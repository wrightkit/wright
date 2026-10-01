import argparse
import hashlib
import json
import os
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch
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

    def trial(self, agent_cmd: str, tool: str = "wright", skills: tuple = (), knowledge: str = "none", scenario: str = SCENARIO, **options) -> dict:
        args = argparse.Namespace(**{
            "wright": str(Path(WRIGHT).resolve()), "agent_id": "fake", "agent_cmd": agent_cmd, "timeout": 60, "infra_retries": 2,
            "env_pass": [], "canary_cmd": None, "skill_dirs": {}, "wiki_dir": None, "check_ancestors": False, "out": self.out, **options,
        })
        cell = agent_bench.normalize_cell({"tool": tool, "skills": list(skills), "knowledge": knowledge, "network": "off"})
        return agent_bench.run_trial(agent_bench.load_scenario(scenario), cell, args, self.out / f"{scenario}-{tool}")

    def test_wiki_snapshot_is_copied_and_identified(self):
        snapshot = self.out / "snapshot"
        (snapshot / "articles").mkdir(parents=True)
        (snapshot / "articles/wait-until.md").write_text("# Wait Until\n")
        article_hash = hashlib.sha256((snapshot / "articles/wait-until.md").read_bytes()).hexdigest()
        snapshot_hash = hashlib.sha256(f"wait-until {article_hash}".encode()).hexdigest()
        (snapshot / "SNAPSHOT.json").write_text(json.dumps({"source": "https://mirror.example", "fetchedAt": "2026-09-30T00:00:00+00:00", "snapshotSha256": snapshot_hash, "documents": [{"slug": "wait-until", "sha256": article_hash}]}))
        result = self.trial("ls wiki/articles > listing.txt", knowledge="wiki", wiki_dir=snapshot)
        self.assertEqual(result["environment"]["wiki"]["snapshotSha256"], snapshot_hash)
        self.assertEqual(result["environment"]["wiki"]["documents"], 1)
        self.assertEqual((self.out / f"{SCENARIO}-wright/workspace/listing.txt").read_text().strip(), "wait-until.md")
        self.assertNotIn("wiki", " ".join(result["unsafeEdits"]))

    def test_skills_are_installed_identified_and_the_only_ones_expected(self):
        skill = self.out / "workshop-wiki"
        skill.mkdir()
        (skill / "SKILL.md").write_text("---\nname: workshop-wiki\n---\n")
        skill_hash = wiki_skill.content_hash(skill)
        with self.assertRaisesRegex(SystemExit, "requires --skill"):
            self.trial("true", skills=("workshop-skill",))
        expected = self.trial("echo '{\"loaded\": [\"workshop-wiki\"]}' > \"$BENCH_CONTEXT\"; echo \"$BENCH_SKILL_DIRS\" > dirs.txt", skills=("workshop-skill",), skill_dirs={"workshop-skill": skill})
        self.assertNotIn("invalid", expected)
        self.assertEqual(expected["condition"]["label"], "wright+workshop-skill/none/off")
        self.assertEqual(expected["environment"]["skills"]["workshop-skill"]["sha256"], skill_hash)
        self.assertEqual((self.out / f"{SCENARIO}-wright/workspace/dirs.txt").read_text().strip(), str(skill.resolve()))
        stray = self.trial("echo '{\"loaded\": [\"workshop-wiki\", \"other\"]}' > \"$BENCH_CONTEXT\"", skills=("workshop-skill",), skill_dirs={"workshop-skill": skill})
        self.assertIn("unexpected loaded context", stray["invalid"])
        (skill / "BUILD.json").write_text(json.dumps({"name": "workshop-wiki", "skillSha256": "0" * 64}))
        with self.assertRaisesRegex(SystemExit, "skill content mismatch"):
            self.trial("true", skills=("workshop-skill",), skill_dirs={"workshop-skill": skill})

    def test_the_prompt_is_exactly_the_scenario_prompt(self):
        self.trial('cat > received.txt')
        received = (self.out / f"{SCENARIO}-wright/workspace/received.txt").read_text()
        self.assertEqual(received, (agent_bench.SCENARIOS / SCENARIO / "prompt.md").read_text())

    def test_cell_labels_applicability_and_baseline(self):
        cell = agent_bench.normalize_cell({"tool": "wright", "skills": ["wright-skill"], "knowledge": "none", "network": "off"})
        self.assertEqual(agent_bench.cell_label(cell), "wright+wright-skill/none/off")
        base = agent_bench.normalize_cell({"tool": "none", "skills": [], "knowledge": "none", "network": "off"})
        self.assertEqual(agent_bench.cell_label(base), bench_report.BASELINE)
        opy = agent_bench.normalize_cell({"tool": "overpy", "skills": ["opy-skill"], "knowledge": "none", "network": "off"})
        self.assertTrue(agent_bench.applicable({"language": "opy"}, opy))
        self.assertFalse(agent_bench.applicable({"language": "workshop"}, opy))
        self.assertTrue(agent_bench.applicable({"language": "workshop"}, cell))
        fmt = agent_bench.normalize_cell({"tool": "none", "skills": ["workshop-format-skill"], "knowledge": "none", "network": "off"})
        self.assertTrue(agent_bench.applicable({"language": "workshop"}, fmt))
        self.assertFalse(agent_bench.applicable({"language": "opy"}, fmt))

    @unittest.skipUnless(bench_grade.oracle_available(), "run `agent_bench.py setup-oracle`")
    def test_overpy_tool_is_traced_and_wright_is_hidden(self):
        scenario = "repair-opy-runaway-loop"
        agent = (f"cp {reference(scenario)}/* . && overpy compile -i mode.opy -o out.txt; "
                 "(command -v wright >/dev/null && echo found || echo hidden) > wright.txt")
        result = self.trial(agent, tool="overpy", skills=("opy-skill",), scenario=scenario, skill_dirs={"opy-skill": self.skill_dir("overpy")})
        self.assertEqual(result["toolUse"]["overpy"]["byCommand"], {"compile": 1})
        self.assertNotIn("wright", result["toolUse"])
        self.assertEqual((self.out / f"{scenario}-overpy/workspace/wright.txt").read_text().strip(), "hidden")
        self.assertTrue((self.out / f"{scenario}-overpy/workspace/out.txt").is_file())
        self.assertEqual(result["condition"]["label"], "overpy+opy-skill/none/off")
        with self.assertRaisesRegex(SystemExit, "does not apply"):
            self.trial("true", tool="overpy", scenario=SCENARIO)

    def skill_dir(self, name: str) -> Path:
        directory = self.out / "skills" / name
        directory.mkdir(parents=True, exist_ok=True)
        (directory / "SKILL.md").write_text(f"---\nname: {name}\n---\n")
        return directory

    def test_result_records_protocol_identity_and_status(self):
        result = self.trial(f"cp {reference()}/* .")
        self.assertEqual(result["status"], "completed")
        self.assertEqual(result["protocol"], {"timeoutSeconds": 60, "infraRetries": 2})
        env = result["environment"]
        self.assertEqual(env["wrightSha256"], hashlib.sha256(Path(WRIGHT).read_bytes()).hexdigest())
        self.assertEqual((env["suite"]["version"], len(env["suite"]["hash"])), ("v1", 64))
        self.assertTrue(env["harness"])
        interrupted = self.trial("exit 75", infra_retries=0)
        self.assertEqual(interrupted["status"], "provider-interrupted")
        timeout = self.trial("sleep 5", timeout=1)
        self.assertEqual((timeout["status"], timeout["agent"]["exit"]), ("timeout", None))
        self.assertEqual(self.trial("exit 3")["status"], "agent-error")
        info = self.trial('echo \'{"agent": "fake", "model": "m"}\' > "$BENCH_AGENT_INFO"')
        self.assertEqual(info["agentInfo"]["model"], "m")

    def test_unsafe_edits_block_usable_even_when_every_check_passes(self):
        result = self.trial(f"cp {reference()}/* . && echo stray > stray.txt")
        self.assertTrue(result["passed"])
        self.assertFalse(result["usable"])
        self.assertEqual(result["usableReason"], ["unsafe-edits"])
        clean = self.trial(f"cp {reference()}/* .")
        self.assertEqual((clean["usable"], clean["usableReason"]), (True, []))

    def test_an_unavailable_required_grader_makes_the_run_invalid_not_a_failure(self):
        from unittest.mock import patch
        scenario = "repair-opy-runaway-loop"
        with patch.object(bench_grade, "oracle_available", return_value=False):
            result = self.trial(f"cp {reference(scenario)}/* .", scenario=scenario)
        self.assertEqual(result["status"], "invalid")
        self.assertIn("grader", result["invalid"])
        self.assertIn("grader-unavailable", result["usableReason"])

    def test_matrix_skips_inapplicable_cells_and_stops_after_repeated_interruptions(self):
        import contextlib
        import io
        config = self.out / "matrix.json"
        config.write_text(json.dumps({
            "agents": [{"id": "fake", "cmd": "exit 75"}],
            "cells": [{"tool": "none", "skills": ["opy-skill"], "knowledge": "none", "network": "off"}],
            "scenarios": ["repair-runaway-loop", "repair-opy-runaway-loop"], "trials": 4, "parallel": 1, "seed": 1,
            "options": {"skill_dirs": {"opy-skill": str(self.skill_dir("overpy"))}, "timeout": 30, "infra_retries": 0},
        }))
        args = argparse.Namespace(config=config, out=self.out / "m", wright=str(Path(WRIGHT).resolve()), skill_dirs={}, wiki_dir=None, env_pass=[],
                                  check_ancestors=False, canary_cmd=None, timeout=30, infra_retries=0, file_sandbox=False)
        printed = io.StringIO()
        with contextlib.redirect_stdout(printed):
            code = agent_bench.cmd_matrix(args)
        self.assertEqual(code, 3)
        self.assertIn("not applicable: repair-runaway-loop", printed.getvalue())
        self.assertEqual(len(list((self.out / "m").rglob("result.json"))), 2)  # the third and fourth jobs were left unattempted
        self.assertIn("left unattempted", printed.getvalue())

    def test_scenarios_are_solvable_and_not_vacuous(self):
        self.assertTrue(agent_bench.validate(WRIGHT, self.out / "validate"))

    @unittest.skipUnless(sys.platform == "darwin" and shutil.which("sandbox-exec"), "macOS file sandbox")
    def test_agent_file_writes_cannot_escape_the_trial_directory(self):
        code = 'from pathlib import Path; Path("allowed.txt").write_text("ok"); Path("../../escaped.txt").write_text("bad")'
        import shlex
        result = self.trial(f'{shlex.quote(sys.executable)} -c {shlex.quote(code)}', file_sandbox=True)
        self.assertNotEqual(result["agent"]["exit"], 0)
        self.assertFalse((self.out / "escaped.txt").exists())
        self.assertEqual((self.out / f"{SCENARIO}-wright/workspace/allowed.txt").read_text(), "ok")
        self.assertEqual(result["fileWriteEnforcement"], "trial-directory-only")

    @unittest.skipUnless(sys.platform == "darwin" and shutil.which("sandbox-exec"), "macOS file sandbox")
    def test_host_instructions_are_unreadable_but_workspace_instructions_are_allowed(self):
        import shlex
        host = self.out / "AGENTS.md"
        host.write_text("host-only instructions")
        code = (
            'from pathlib import Path; '
            'Path("AGENTS.md").write_text("workspace instructions"); '
            'assert Path("AGENTS.md").read_text() == "workspace instructions"; '
            f'Path({str(host)!r}).read_text()'
        )
        result = self.trial(f'{shlex.quote(sys.executable)} -c {shlex.quote(code)}', file_sandbox=True)
        self.assertNotEqual(result["agent"]["exit"], 0)
        self.assertIn("PermissionError", (self.out / f"{SCENARIO}-wright/agent.log").read_text())
        self.assertEqual(host.read_text(), "host-only instructions")
        self.assertEqual((self.out / f"{SCENARIO}-wright/workspace/AGENTS.md").read_text(), "workspace instructions")

    @unittest.skipUnless(sys.platform == "darwin" and shutil.which("sandbox-exec"), "macOS file sandbox")
    def test_agent_cannot_read_answer_keys_other_runs_or_denied_paths(self):
        import shlex
        denied = self.out / "checkout"
        denied.mkdir()
        (denied / "secret.txt").write_text("sibling repository")
        answer = agent_bench.SCENARIOS / SCENARIO / "reference" / "mode.ws"
        probes = {"answer": answer, "denied": denied / "secret.txt"}
        code = ("from pathlib import Path\nimport json\nout = {}\n"
                + "".join(f"try:\n    Path({str(p)!r}).read_text(); out[{k!r}] = 'read'\nexcept PermissionError:\n    out[{k!r}] = 'blocked'\n" for k, p in probes.items())
                + "Path('probe.json').write_text(json.dumps(out))\nPath('own.txt').write_text('ok'); assert Path('own.txt').read_text() == 'ok'\n")
        result = self.trial(f'{shlex.quote(sys.executable)} -c {shlex.quote(code)}', file_sandbox=True, deny_read=[str(denied)])
        workspace = self.out / f"{SCENARIO}-wright/workspace"
        self.assertEqual(json.loads((workspace / "probe.json").read_text()), {"answer": "blocked", "denied": "blocked"})
        self.assertIn(str(denied.resolve()), result["fileReadEnforcement"])

    def test_user_defaults_are_read_from_the_config_file(self):
        config = self.out / "config.json"
        config.write_text(json.dumps({"skill_dirs": {"wright-skill": "/s/wright"}, "deny_read": ["~/Repos"], "wright": "/bin/wright"}))
        with patch.object(agent_bench, "CONFIG_PATH", config):
            defaults = agent_bench.user_defaults()
        self.assertEqual((defaults["skill_dir"], defaults["deny_read"], defaults["wright"]), (["wright-skill=/s/wright"], ["~/Repos"], "/bin/wright"))
        config.write_text(json.dumps({"skil_dirs": {}}))
        with patch.object(agent_bench, "CONFIG_PATH", config), self.assertRaises(SystemExit):
            agent_bench.user_defaults()

    def test_preflight_names_what_is_missing_before_a_run_starts(self):
        args = argparse.Namespace(wright=str(self.out / "nope"), adapter="devin", skill_dirs={})
        with patch.object(agent_bench.shutil, "which", return_value=None):
            problems = agent_bench.preflight(args, [agent_bench.normalize_cell({"tool": "wright", "skills": ["wright-skill"], "knowledge": "none", "network": "off"})])
        self.assertEqual(len(problems), 3)
        self.assertTrue(any("wright binary not found" in p for p in problems) and any("`devin` is not on PATH" in p for p in problems) and any("--skill-dir wright-skill" in p for p in problems))

    def test_tools_differ_only_in_availability(self):
        agent = f"cp {reference()}/* . && (wright check mode.ws >/dev/null 2>&1 || echo no-wright > missing-wright.txt)"
        none = self.trial(agent, tool="none")
        assisted = self.trial(agent, tool="wright")
        self.assertEqual(none["toolUse"], {})
        self.assertIn("missing-wright.txt", none["unsafeEdits"])
        self.assertEqual(assisted["toolUse"]["wright"]["byCommand"], {"check": 1})
        self.assertTrue(assisted["passed"])
        self.assertEqual(assisted["unsafeEdits"], [])

    def test_canary_rejects_a_tool_that_is_not_part_of_the_condition(self):
        cell = agent_bench.normalize_cell({"tool": "none", "skills": [], "knowledge": "none", "network": "off"})
        args = argparse.Namespace(canary_cmd=None, check_ancestors=False)
        env = {"PATH": str(Path(WRIGHT).resolve().parent)}
        self.assertIn("reachable", agent_bench.canaries(cell, env, self.out, args))
        self.assertIsNone(agent_bench.canaries({**cell, "tool": "wright"}, env, self.out, args))

    def test_canary_rejects_instruction_files_above_the_workspace(self):
        outside = Path(tempfile.mkdtemp()).resolve()  # outside the repository, whose own AGENTS.md would match
        self.addCleanup(shutil.rmtree, outside, True)
        workspace = outside / "repo/runs/w"
        workspace.mkdir(parents=True)
        cell = agent_bench.normalize_cell({"tool": "wright", "skills": [], "knowledge": "none", "network": "on"})
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
        env = (self.out / f"{SCENARIO}-wright/workspace/env.txt").read_text()
        self.assertNotIn("BENCH_LEAK_PROBE", env)
        self.assertIn("BENCH_KNOWLEDGE=none", env)
        self.assertIn(f"HOME={self.out}/{SCENARIO}-wright/home", env)

    def test_trace_records_envelope_and_serve_sessions(self):
        agent = (f"cp {reference()}/* . && wright lint mode.ws -f json >/dev/null; "
                 "printf '{\"op\":\"capabilities\"}\\n{\"op\":\"lint\"}\\n' | wright serve mode.ws >/dev/null")
        result = self.trial(agent)
        events = bench_trace.read_events(self.out / f"{SCENARIO}-wright/tool-trace.jsonl")
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
    def test_missing_entry_is_an_agent_failure_not_an_unavailable_grader(self):
        workspace = self.out / "empty"
        workspace.mkdir()
        scenario = agent_bench.load_scenario("widow-headshots")
        graded = bench_grade.grade(scenario, workspace, WRIGHT)
        self.assertEqual(graded["authorities"]["oracle"]["status"], "error")
        self.assertNotIn("grader-unavailable", graded["usableReason"])
        self.assertIn("checks-failed", graded["usableReason"])

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
        return {"tool": "wright", "type": "call", "t": t, "argv": argv, "exit": exit_code, "seconds": 0.1, "stdoutBytes": 40, "stderrBytes": 0, "stderrHead": "", "envelope": None, **extra}

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

    def test_friction_reports_unparseable_serve_responses_without_losing_errors(self):
        responses = ['{"result":"unfinished', '{"error":{"code":"malformed-request"}}', '{"result":{}}']
        events = [{"tool": "wright", "type": "serve", "dir": "res", "line": line} for line in responses]
        result = bench_trace.friction(events)
        self.assertEqual(result["malformedServeRequests"], 1)
        self.assertEqual(result["unparsedServeResponses"], 1)

    def test_serve_trace_preserves_large_json_responses(self):
        import io
        from types import SimpleNamespace
        from unittest.mock import Mock, patch
        response = json.dumps({"result": {"value": "x" * 4096}}) + "\n"
        process = SimpleNamespace(stdin=io.BytesIO(), stdout=io.BytesIO(response.encode()), wait=lambda: 0)
        output = SimpleNamespace(buffer=io.BytesIO(), flush=lambda: None)
        with patch.object(bench_trace.subprocess, "Popen", return_value=process), \
             patch.object(bench_trace.sys, "stdin", SimpleNamespace(buffer=io.BytesIO())), \
             patch.object(bench_trace.sys, "stdout", output), \
             patch.object(bench_trace, "append_event", Mock()) as capture:
            self.assertEqual(bench_trace.serve_tee("wright", ["serve"], 0), 0)
        recorded = next(call.args[0] for call in capture.call_args_list if call.args[0]["type"] == "serve")
        self.assertEqual(json.loads(recorded["line"]), json.loads(response))
        self.assertEqual(output.buffer.getvalue(), response.encode())

    def test_unused_wright_is_not_applicable(self):
        result = bench_trace.detect_expectations([], [], {"stabilityRisk": True}, None)
        self.assertEqual(result["E03"]["status"], "na")
        self.assertEqual(result["E05"]["status"], "unavailable")


class ReportTest(unittest.TestCase):
    def result(self, cell, trial, usable, tokens, split=None, scenario="s", language="opy", status="completed"):
        tool = cell.split("/")[0].split("+")[0]
        return {
            "contract": "wright-agent-bench/v3", "scenario": scenario, "family": "diagnosis", "language": language, "split": split, "_trial": trial, "_dir": Path("d"),
            "condition": {"label": cell}, "agent": {"id": "m", "exit": 0, "seconds": 1.0}, "status": status,
            "usable": usable, "passed": usable, "usage": {"totalTokens": tokens, "peakContext": tokens // 2},
            "toolUse": {"wright": {"invocations": 1}} if tool == "wright" else {},
        }

    def test_wilson_interval(self):
        low, high = bench_report.wilson(5, 10)
        self.assertAlmostEqual(low, 0.237, places=2)
        self.assertAlmostEqual(high, 0.763, places=2)
        self.assertEqual(bench_report.wilson(0, 0), (0.0, 0.0))

    def test_paired_efficiency_counts_only_both_usable_and_failures_cost(self):
        runs = [self.result("none/none/off", 1, True, 1000), self.result("wright/none/off", 1, True, 600),
                self.result("none/none/off", 2, False, 900), self.result("wright/none/off", 2, True, 700)]
        text, summary = bench_report.render(runs)
        self.assertIn("+1 / -0", text)
        self.assertIn("+40% tokens (n=1)", text)
        self.assertEqual(summary["cells"]["m|wright/none/off"]["tokensPerUsable"], 650)
        self.assertEqual(summary["cells"]["m|none/none/off"]["tokensPerUsable"], 1900)

    def test_headroom_and_invalid_runs_are_reported(self):
        runs = [self.result("none/none/off", t, True, 100) for t in range(1, 5)]
        runs.append({**self.result("wright/none/off", 1, True, 100, status="invalid"), "invalid": "canary"})
        text, _ = bench_report.render(runs)
        self.assertIn("HEADROOM", text)
        self.assertIn("INVALID: 1 run(s) excluded", text)

    def test_report_shows_what_the_agent_was_actually_given(self):
        run = self.result("wright+wright-skill/none/off", 1, True, 100)
        run["agentInfo"] = {"version": "tool 1.2", "model": "m-1", "effort": "high", "tools": ["bash", "read"]}
        run["context"] = {"loaded": ["wright"]}
        bare = self.result("none/none/off", 1, True, 100)
        text, _ = bench_report.render([run, bare])
        self.assertIn("| m | wright+wright-skill/none/off | tool 1.2 | m-1 | high | 2: bash, read | wright |", text)
        self.assertIn("| m | none/none/off | not recorded | not recorded | not recorded | not recorded | none |", text)

    def test_provider_failures_do_not_count_as_agent_failures(self):
        good = self.result("none/none/off", 1, True, 100)
        provider_failure = self.result("none/none/off", 2, False, 0)
        provider_failure.update(status="provider-interrupted")
        provider_failure["agent"]["exit"] = 75
        text, summary = bench_report.render([good, provider_failure])
        self.assertEqual(summary["cells"]["m|none/none/off"]["n"], 1)
        self.assertEqual(summary["infrastructureFailures"], 1)
        self.assertIn("excluded from outcome metrics", text)


if __name__ == "__main__":
    unittest.main()
