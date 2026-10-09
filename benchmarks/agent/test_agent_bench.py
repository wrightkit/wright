import argparse
import contextlib
import hashlib
import io
import json
import os
import shutil
import stat
import subprocess
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


class BoundedUsesTest(unittest.TestCase):
    """#532: `--brief` and the selection flags/fields count as bounded uses."""

    def test_cli_classification(self):
        self.assertTrue(bench_trace.bounded_cli(["lint", "m.ws", "--brief"]))
        self.assertTrue(bench_trace.bounded_cli(["lint", "m.ws", "--severity", "warning"]))
        self.assertTrue(bench_trace.bounded_cli(["inspect", "symbols", "m.ws", "--only", "rule"]))
        self.assertTrue(bench_trace.bounded_cli(["inspect", "refs", "x", "m.ws", "--rule", "r"]))
        # `lint --rule` loads a rule file; it does not select findings.
        self.assertFalse(bench_trace.bounded_cli(["lint", "m.ws", "--rule", "local.yaml"]))
        self.assertFalse(bench_trace.bounded_cli(["analyze", "m.ws"]))
        self.assertFalse(bench_trace.bounded_cli(["check", "m.ws", "-f", "json"]))

    def test_request_classification(self):
        self.assertTrue(bench_trace.bounded_request({"op": "analyze", "args": {"brief": True}}))
        self.assertTrue(bench_trace.bounded_request({"op": "lint", "args": {"severity": "warning"}}))
        self.assertTrue(bench_trace.bounded_request({"op": "references", "args": {"symbol": 0, "rule": "r"}}))
        self.assertFalse(bench_trace.bounded_request({"op": "analyze", "args": {"brief": False}}))
        # `rule` on cfg is the required address, not a selection.
        self.assertFalse(bench_trace.bounded_request({"op": "cfg", "args": {"rule": 0}}))
        self.assertFalse(bench_trace.bounded_request({"op": "project", "args": {}}))


@unittest.skipUnless(Path(WRIGHT).is_file(), "build wright first or set WRIGHT_BIN")
class AgentBenchTest(unittest.TestCase):
    def setUp(self):
        (agent_bench.ROOT / "target").mkdir(exist_ok=True)
        self.out = Path(tempfile.mkdtemp(dir=agent_bench.ROOT / "target")).resolve()
        self.addCleanup(shutil.rmtree, self.out, True)

    def trial(self, agent_cmd: str, tool: str = "wright", skills: tuple = (), knowledge: str = "none", scenario: str = SCENARIO, level: str = "bin", **options) -> dict:
        args = argparse.Namespace(**{
            "wright": str(Path(WRIGHT).resolve()), "agent_id": "fake", "agent_cmd": agent_cmd, "timeout": 60, "infra_retries": 2,
            "env_pass": [], "canary_cmd": None, "skill_dirs": {}, "wiki_dir": None, "check_ancestors": False, "out": self.out, **options,
        })
        cell = agent_bench.normalize_cell({"tool": tool, "level": level, "skills": list(skills), "knowledge": knowledge, "network": "off"})
        return agent_bench.run_trial(agent_bench.load_scenario(scenario, getattr(args, "private_suite", None)), cell, args, self.out / f"{scenario}-{tool}")

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
        skill = self.out / "workshop-skill"
        skill.mkdir()
        (skill / "SKILL.md").write_text("---\nname: workshop-skill\n---\n")
        skill_hash = wiki_skill.content_hash(skill)
        with self.assertRaisesRegex(SystemExit, "requires --skill"):
            self.trial("true", skills=("workshop-skill",))
        expected = self.trial("echo '{\"loaded\": [\"workshop-skill\"]}' > \"$BENCH_CONTEXT\"; echo \"$BENCH_SKILL_DIRS\" > dirs.txt", skills=("workshop-skill",), skill_dirs={"workshop-skill": skill})
        self.assertNotIn("invalid", expected)
        self.assertEqual(expected["condition"]["label"], "wright+workshop-skill/none/off")
        self.assertEqual(expected["environment"]["skills"]["workshop-skill"]["sha256"], skill_hash)
        self.assertEqual((self.out / f"{SCENARIO}-wright/workspace/dirs.txt").read_text().strip(), str(skill.resolve()))
        stray = self.trial("echo '{\"loaded\": [\"workshop-skill\", \"other\"]}' > \"$BENCH_CONTEXT\"", skills=("workshop-skill",), skill_dirs={"workshop-skill": skill})
        self.assertIn("unexpected loaded context", stray["invalid"])
        (skill / "BUILD.json").write_text(json.dumps({"name": "workshop-skill", "skillSha256": "0" * 64}))
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

    def test_mcp_level_label_and_cell_validation(self):
        cell = agent_bench.normalize_cell({"tool": "wright", "level": "mcp", "skills": ["wright-skill"], "knowledge": "none", "network": "off"})
        self.assertEqual(agent_bench.cell_label(cell), "wright-mcp+wright-skill/none/off")
        self.assertEqual(agent_bench.normalize_cell({"tool": "wright", "skills": [], "knowledge": "none", "network": "off"})["level"], "bin")
        self.assertEqual(agent_bench.cell_label(agent_bench.normalize_cell({"tool": "wright", "skills": [], "knowledge": "none", "network": "off"})), "wright/none/off")
        args = argparse.Namespace(skill_dirs={}, wiki_dir=None)
        with self.assertRaisesRegex(SystemExit, "level 'mcp' is a `wright` level"):
            agent_bench.check_cell(agent_bench.normalize_cell({"tool": "overpy", "level": "mcp", "skills": [], "knowledge": "none", "network": "off"}), args)
        with self.assertRaisesRegex(SystemExit, "level 'mcp' is a `wright` level"):
            agent_bench.check_cell(agent_bench.normalize_cell({"tool": "none", "level": "mcp", "skills": [], "knowledge": "none", "network": "off"}), args)
        with self.assertRaisesRegex(SystemExit, "invalid condition"):
            agent_bench.check_cell(agent_bench.normalize_cell({"tool": "wright", "level": "bogus", "skills": [], "knowledge": "none", "network": "off"}), args)

    def test_mcp_env_hides_the_cli_and_describes_the_server(self):
        workspace = self.out / "ws"
        workspace.mkdir()
        args = argparse.Namespace(wright=str(Path(WRIGHT).resolve()), agent_id="fake", env_pass=[], skill_dirs={}, wiki_dir=None, check_ancestors=False, canary_cmd=None)
        cell = agent_bench.normalize_cell({"tool": "wright", "level": "mcp", "skills": [], "knowledge": "none", "network": "off"})
        env = agent_bench.build_env(cell, args, self.out / "run", workspace)
        self.assertEqual(env["BENCH_TOOL_LEVEL"], "mcp")
        self.assertIn("serve --transport mcp", env["BENCH_MCP_CMD"])
        self.assertIn("bench_trace.py", env["BENCH_MCP_CMD"])
        self.assertIn(str(workspace), env["BENCH_MCP_CMD"])
        self.assertIsNone(shutil.which("wright", path=env["PATH"]))
        self.assertFalse((self.out / "run/bin/wright").exists())  # no wright CLI shim; the package-manager blockers live in the same directory
        shims = self.out / "shims"
        shims.mkdir()
        (shims / "wright").write_text("#!/bin/sh\nexit 0\n")
        (shims / "wright").chmod(0o755)
        self.assertIn("reachable", agent_bench.canaries(cell, {**env, "PATH": str(shims)}, workspace, args))
        env_bin = agent_bench.build_env({**cell, "level": "bin"}, args, self.out / "run2", workspace)
        self.assertTrue((self.out / "run2/bin").is_dir())
        self.assertTrue(str(shutil.which("wright", path=env_bin["PATH"])).startswith(str(self.out / "run2")))

    def test_mcp_level_is_invalid_when_the_adapter_never_registers_the_server(self):
        result = self.trial("true", level="mcp")
        self.assertEqual(result["status"], "invalid")
        self.assertIn("BENCH_MCP_CMD", result["invalid"])

    def test_mcp_tools_calls_are_traced_as_wright_uses(self):
        lines = [
            '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{}}}',
            '{"jsonrpc":"2.0","method":"notifications/initialized"}',
            '{"jsonrpc":"2.0","id":2,"method":"tools/list"}',
            '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"wright_check","arguments":{}}}',
            '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"wright_nope","arguments":{}}}',
        ]
        agent = "printf '%s\\n' " + " ".join(f"'{line}'" for line in lines) + ' | sh -c "$BENCH_MCP_CMD" >/dev/null'
        result = self.trial(agent, level="mcp")
        use = result["toolUse"]["wright"]
        self.assertEqual((use["invocations"], use["byCommand"]["check"]), (2, 1))
        self.assertEqual(use["byCommand"]["nope"], 1)
        self.assertEqual(use["failedInvocations"], 1)
        self.assertNotIn("invalid", result)
        self.assertEqual(result["expectations"]["E11"]["status"], "pass")
        self.assertEqual(result["expectations"]["E01"]["status"], "pass")
        self.assertEqual(result["expectations"]["E02"]["status"], "pass")
        self.assertGreater(use["outputTokensEstimate"]["mcp:tools/list"], 0)

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
        self.assertEqual((env["suite"]["version"], len(env["suite"]["hash"])), ("v2", 64))
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
        interrupted = {p for p in (self.out / "m").rglob("result.json") if json.loads(p.read_text())["status"] == "provider-interrupted"}
        self.assertEqual(len(interrupted), 2)
        config.write_text(config.read_text().replace('"cmd": "exit 75"', '"cmd": "exit 0"'))  # the provider is back
        with contextlib.redirect_stdout(io.StringIO()):
            agent_bench.cmd_matrix(args)
        self.assertTrue(all(json.loads(p.read_text())["status"] != "provider-interrupted" for p in (self.out / "m").rglob("result.json")))
        self.assertEqual(len(list((self.out / "m").rglob("result.json"))), 4)  # the interrupted two were rerun and the rest ran

    def test_a_partial_result_from_a_killed_run_is_retried_instead_of_crashing(self):
        import io
        cell = agent_bench.normalize_cell({"tool": "none", "skills": [], "knowledge": "none", "network": "off"})
        config = self.out / "matrix.json"
        config.write_text(json.dumps({
            "agents": [{"id": "fake", "cmd": "exit 0"}],
            "cells": [cell], "scenarios": [SCENARIO], "trials": 1, "parallel": 1, "seed": 1,
            "options": {"timeout": 30, "infra_retries": 0},
        }))
        out = agent_bench.trial_dir(self.out / "m", SCENARIO, "fake", cell, 1)
        out.mkdir(parents=True)
        (out / "result.json").write_text('{"status": "com')  # a kill mid-write left this
        args = argparse.Namespace(config=config, out=self.out / "m", wright=str(Path(WRIGHT).resolve()), skill_dirs={}, wiki_dir=None, env_pass=[],
                                  check_ancestors=False, canary_cmd=None, timeout=30, infra_retries=0, file_sandbox=False)
        with contextlib.redirect_stdout(io.StringIO()):
            code = agent_bench.cmd_matrix(args)
        self.assertEqual(code, 0)
        self.assertEqual(json.loads((out / "result.json").read_text())["status"], "completed")

    @unittest.skipUnless(sys.platform == "darwin" and shutil.which("sandbox-exec"), "macOS file sandbox")
    def test_evaluate_runs_private_scenarios_and_records_versioned_parts(self):
        import io
        root = self.private_suite()
        script = agent_bench.HERE / "adapters" / "_test_private_adapter.py"
        script.write_text("import sys\nsys.stdin.read()\n")
        self.addCleanup(script.unlink, True)
        args = argparse.Namespace(
            adapter="fake", model="m", effort=None, name="eval", out=self.out,
            wright=str(Path(WRIGHT).resolve()), private_suite=root,
            skill_dirs={"wright-skill": self.skill_dir("wright-skill")}, wiki_dir=None, env_pass=[],
            allow_read=[], deny_read=[], canary_cmd=None, check_ancestors=False,
            timeout=30, infra_retries=0, infra_backoff=0, no_file_sandbox=False, dry_run=False,
            cells="score", split="test", scenarios=None, trials=1, parallel=1, seed=1)
        with patch.dict(agent_bench.ADAPTERS, {"fake": script.name}), patch.dict(agent_bench.ADAPTER_READS, {"fake": []}), \
                patch.object(agent_bench, "all_scenario_ids", return_value=[SCENARIO, "private-001"]), \
                contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(agent_bench.cmd_evaluate(args), 0)
        records = [json.loads(p.read_text()) for p in (self.out / "eval").glob("*/*/*/result.json")]
        self.assertEqual({r["scenario"] for r in records}, {SCENARIO, "private-001"})
        self.assertTrue(all(r["environment"]["suite"]["privateHash"] for r in records))
        self.assertTrue(any(r["isPrivate"] for r in records))
        self.assertEqual(json.loads((self.out / "eval/matrix.json").read_text())["options"]["private_suite"], str(root))

    def test_evaluate_serializes_the_run_options_so_its_matrix_reproduces_the_run(self):
        import contextlib
        import io
        script = agent_bench.HERE / "adapters" / "_test_fake_adapter.py"
        script.write_text("import os, sys\nsys.stdin.read()\nopen('probe.txt', 'w').write(os.environ.get('BENCH_PROBE', '') + '|' + os.environ.get('HOME', ''))\n")
        self.addCleanup(script.unlink, True)
        self.skill_dir("wright-skill")  # reached as a relative path below, resolved against the evaluate cwd
        (self.out / "wiki-snap").mkdir()
        args = argparse.Namespace(  # relative paths must serialize resolved: the file resolves its own against its directory
            adapter="fake", model="m", effort=None, name="eval", out=self.out, wright=str(Path(WRIGHT).resolve()),
            skill_dirs={"wright-skill": Path("skills/wright-skill")}, wiki_dir=Path("wiki-snap"), env_pass=["BENCH_PROBE"],
            allow_read=["allow-this"], deny_read=[], canary_cmd=None, check_ancestors=False,
            timeout=30, infra_retries=3, infra_backoff=0, no_file_sandbox=True, dry_run=False,
            cells="score", split="test", scenarios=[SCENARIO], trials=1, parallel=1, seed=1)
        run_dir = self.out / "eval"
        with patch.dict(agent_bench.ADAPTERS, {"fake": "_test_fake_adapter.py"}), patch.dict(agent_bench.ADAPTER_READS, {"fake": []}), \
                patch.dict(os.environ, {"BENCH_PROBE": "present"}), contextlib.chdir(self.out), contextlib.redirect_stdout(io.StringIO()):
            code = agent_bench.cmd_evaluate(args)
        self.assertEqual(code, 0)
        options = json.loads((run_dir / "matrix.json").read_text())["options"]
        self.assertEqual((options["out"], options["out_root"], options["adapter"]), (".", "..", "fake"))
        self.assertEqual((options["wright"], options["timeout"], options["file_sandbox"]), (str(Path(WRIGHT).resolve()), 30, False))
        self.assertIn("BENCH_PROBE", options["env_pass"])
        self.assertIn("HOME", options["env_pass"])
        self.assertEqual(options["skill_dirs"]["wright-skill"], str((self.out / "skills/wright-skill").resolve()))
        self.assertEqual(options["wiki_dir"], str((self.out / "wiki-snap").resolve()))
        self.assertEqual(options["deny_read"], [])
        self.assertEqual(options["infra_retries"], 3)
        self.assertIn(str((self.out / "allow-this").resolve()), options["allow_read"])
        trial = run_dir / SCENARIO / "fake-m" / "wright+wright-skill_none_off-1"
        self.assertEqual((trial / "workspace" / "probe.txt").read_text(), f"present|{Path.home()}")
        elsewhere = self.out / "elsewhere"  # flag values that would all be wrong; every serialized option must win
        margs = argparse.Namespace(config=run_dir / "matrix.json", out=elsewhere, wright="/missing", skill_dirs={}, wiki_dir=None,
                                   env_pass=[], check_ancestors=True, canary_cmd=None, timeout=99, infra_retries=5, infra_backoff=5,
                                   file_sandbox=True, deny_read=[], allow_read=[])
        (trial / "result.json").unlink()
        with patch.dict(os.environ, {"BENCH_PROBE": "present"}), contextlib.redirect_stdout(io.StringIO()):
            code = agent_bench.cmd_matrix(margs)
        self.assertEqual(code, 0)
        self.assertFalse(elsewhere.exists())  # the run's own directory, not --out, receives the rerun trial
        result = json.loads((trial / "result.json").read_text())
        self.assertEqual((result["protocol"]["timeoutSeconds"], result["fileWriteEnforcement"]), (30, "unrestricted"))
        self.assertEqual((trial / "workspace" / "probe.txt").read_text(), f"present|{Path.home()}")  # env_pass reached the agent again

    def test_evaluate_reports_a_missing_wright_binary_instead_of_crashing(self):
        import io
        self.skill_dir("wright-skill")
        args = argparse.Namespace(
            adapter="fake", model="m", effort=None, name="eval", out=self.out, wright=str(self.out / "no-such-wright"),
            skill_dirs={"wright-skill": self.out / "skills" / "wright-skill"}, wiki_dir=None, env_pass=[],
            allow_read=[], deny_read=[], canary_cmd=None, check_ancestors=False,
            timeout=30, infra_retries=0, infra_backoff=0, no_file_sandbox=True, dry_run=False,
            cells="score", split="test", scenarios=[SCENARIO], trials=1, parallel=1, seed=1)
        with patch.dict(agent_bench.ADAPTERS, {"fake": "x.py"}), patch.dict(agent_bench.ADAPTER_READS, {"fake": []}), \
                contextlib.redirect_stdout(io.StringIO()), self.assertRaises(SystemExit) as stop:
            agent_bench.cmd_evaluate(args)
        self.assertIn("cannot start", str(stop.exception.code))
        self.assertIn("wright binary not found", str(stop.exception.code))

    def test_scenarios_are_solvable_and_not_vacuous(self):
        self.assertTrue(agent_bench.validate(WRIGHT, self.out / "validate"))

    def private_suite(self):
        root = self.out / "private"
        scenario = root / "scenarios" / "private-001"
        shutil.copytree(agent_bench.SCENARIOS / SCENARIO, scenario)
        data = json.loads((scenario / "scenario.json").read_text())
        data["id"] = "private-001"
        (scenario / "scenario.json").write_text(json.dumps(data))
        (root / "suite.json").write_text(json.dumps({"version": "v2"}))
        return root

    def test_private_loading_canary_and_separate_content_hashes(self):
        root = self.private_suite()
        self.assertIn("private-001", agent_bench.all_scenario_ids(root))
        loaded = agent_bench.load_scenario("private-001", root)
        self.assertTrue(loaded["isPrivate"])
        public = agent_bench.suite_identity()
        combined = agent_bench.suite_identity(root)
        self.assertEqual(public["mode"], "public sample")
        self.assertEqual(combined["mode"], "official")
        self.assertEqual(public["publicHash"], combined["publicHash"])
        prompt = loaded["dir"] / "prompt.md"
        prompt.write_text(prompt.read_text() + "changed")
        changed = agent_bench.suite_identity(root)
        self.assertEqual(combined["publicHash"], changed["publicHash"])
        self.assertNotEqual(combined["privateHash"], changed["privateHash"])
        data = json.loads((loaded["dir"] / "scenario.json").read_text())
        del data["canary"]
        (loaded["dir"] / "scenario.json").write_text(json.dumps(data))
        with self.assertRaisesRegex(SystemExit, "canary"):
            agent_bench.load_scenario("private-001", root)

    @unittest.skipUnless(sys.platform == "darwin" and shutil.which("sandbox-exec"), "macOS file sandbox")
    def test_private_suite_reads_denied_even_under_an_allowed_directory(self):
        import shlex
        root = self.private_suite()
        target = root / "scenarios/private-001/prompt.md"
        code = f"from pathlib import Path; Path({str(target)!r}).read_text()"
        result = self.trial(f'{shlex.quote(sys.executable)} -c {shlex.quote(code)}', file_sandbox=True,
                            private_suite=root, allow_read=[str(self.out)], scenario="private-001")
        self.assertNotEqual(result["agent"]["exit"], 0)
        self.assertIn("PermissionError", (self.out / "private-001-wright/agent.log").read_text())
        self.assertTrue(result["isPrivate"])
        self.assertEqual(result["environment"]["suite"]["version"], "v2")
        with self.assertRaisesRegex(SystemExit, "requires the file sandbox"):
            self.trial("true", private_suite=root)

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
        self.assertIn(str(denied.resolve()), result["fileReadEnforcement"]["hidden"])

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
        with patch.object(agent_bench.shutil, "which", return_value=None), patch.dict(agent_bench.CREDENTIALS, {"devin": [(".missing-bench-cred/auth.json", "x")]}):
            problems = agent_bench.preflight(args, [agent_bench.normalize_cell({"tool": "wright", "skills": ["wright-skill"], "knowledge": "none", "network": "off"})])
        self.assertEqual(len(problems), 4)
        self.assertTrue(any("wright binary not found" in p for p in problems) and any("`devin` is not on PATH" in p for p in problems) and any("--skill-dir wright-skill" in p for p in problems) and any("~/.missing-bench-cred/auth.json" in p for p in problems))

    def test_preflight_names_a_missing_adapter_login_and_an_unusable_sandbox(self):
        args = argparse.Namespace(wright=str(Path(WRIGHT).resolve()), adapter="grok", skill_dirs={}, file_sandbox=True, credentials=[])
        creds = {"grok": [(".missing-bench-cred/auth.json", "x"), (".missing-bench-cred/secondary", "y")]}
        with patch.object(agent_bench.shutil, "which", side_effect=lambda b: f"/bin/{b}" if b != "sandbox-exec" else None), patch.dict(agent_bench.CREDENTIALS, creds, clear=True):
            problems = agent_bench.preflight(args, [agent_bench.normalize_cell({"tool": "wright", "skills": [], "knowledge": "none", "network": "off"})])
        self.assertTrue(any("sandbox-exec" in p for p in problems))
        self.assertTrue(any("~/.missing-bench-cred/auth.json" in p for p in problems))
        self.assertTrue(any("~/.missing-bench-cred/secondary" in p for p in problems))  # every file the adapter needs is named

    def test_read_policy_hides_the_host_and_allows_only_what_the_run_needs(self):
        root = self.out
        skills = {"wright-skill": root / "s1", "opy-skill": root / "s2"}
        args = argparse.Namespace(out=root / "run-a", out_root=root, wright=WRIGHT, skill_dirs=skills, wiki_dir=None, deny_read=[], allow_read=[str(root / "creds")], adapter="devin")
        env = {"BENCH_RUN_DIR": str(root / "run-a" / "t"), "BENCH_SKILLS": "wright-skill", "BENCH_TOOL": "wright"}
        hidden, allowed = agent_bench.read_policy(args, env)
        self.assertTrue(all(p in hidden for p in (Path("/Users"), root.resolve(), agent_bench.HERE)))
        self.assertTrue(all(d.resolve() in hidden for d in agent_bench.DATA_ROOTS))
        self.assertIn(skills["opy-skill"].resolve(), hidden)
        self.assertNotIn(skills["wright-skill"].resolve(), hidden)
        for needed in (root / "run-a" / "t", skills["wright-skill"], root / "creds", agent_bench.HERE / "adapters", agent_bench.HERE / "bench_trace.py", Path(WRIGHT).resolve().parent):
            self.assertIn(needed.resolve(), allowed)
        self.assertNotIn(agent_bench.HERE / "bench_grade.py", allowed)
        self.assertNotIn((agent_bench.HERE / "oracle").resolve(), allowed)  # the grading authority is readable only where the tool needs it
        _, allowed = agent_bench.read_policy(args, {**env, "BENCH_TOOL": "overpy"})
        self.assertIn((agent_bench.HERE / "oracle").resolve(), allowed)

    @unittest.skipUnless(sys.platform == "darwin" and shutil.which("sandbox-exec"), "macOS file sandbox")
    def test_the_agent_sees_only_its_run_the_allowed_paths_and_the_shim(self):
        import shlex
        other = self.out / "elsewhere"
        other.mkdir()
        (other / "note.txt").write_text("a file the agent was not given")
        granted = self.out / "granted"
        granted.mkdir()
        (granted / "note.txt").write_text("a file the adapter needs")
        probes = {"unlisted": other / "note.txt", "granted": granted / "note.txt", "grader": agent_bench.HERE / "bench_grade.py", "shim": agent_bench.HERE / "bench_trace.py",
                  "answer": agent_bench.SCENARIOS / SCENARIO / "reference" / "mode.ws", "oracle": agent_bench.HERE / "oracle" / "compile.js",
                  "profile": self.out / f"{SCENARIO}-wright" / "agent.sb"}  # the sandbox profile itself, which lists the hidden paths
        code = ("import json\nfrom pathlib import Path\nout = {}\n"
                + "".join(f"try:\n    Path({str(p)!r}).read_text(); out[{k!r}] = 'read'\nexcept PermissionError:\n    out[{k!r}] = 'blocked'\n" for k, p in probes.items())
                # a read deny on the profile's own path is not enough: inside the writable run dir the agent can move or
                # link the file to a name the deny does not cover, so the probes must try the rename and link themselves
                + f"try:\n    moved = Path('agent-moved.sb')\n    Path({str(probes['profile'])!r}).rename(moved)\n    moved.read_text(); out['profile-renamed'] = 'read'\nexcept PermissionError:\n    out['profile-renamed'] = 'blocked'\n"
                + f"try:\n    linked = Path('agent-linked.sb')\n    linked.hardlink_to({str(probes['profile'])!r})\n    linked.read_text(); out['profile-linked'] = 'read'\nexcept PermissionError:\n    out['profile-linked'] = 'blocked'\n"
                + "Path('probe.json').write_text(json.dumps(out))\n")
        result = self.trial(f'{shlex.quote(sys.executable)} -c {shlex.quote(code)}', file_sandbox=True, allow_read=[str(granted)])
        seen = json.loads((self.out / f"{SCENARIO}-wright/workspace/probe.json").read_text())
        self.assertEqual(seen, {"unlisted": "blocked", "granted": "read", "grader": "blocked", "shim": "read", "answer": "blocked", "oracle": "blocked",
                                "profile": "blocked", "profile-renamed": "blocked", "profile-linked": "blocked"})  # wright cells must not read the grading authority, and no cell reads its own sandbox profile
        self.assertEqual(result["fileReadEnforcement"]["mode"], "allow-list")

    @unittest.skipUnless(sys.platform == "darwin", "file flags are the macOS enforcement")
    def test_a_locked_profile_left_by_a_killed_run_does_not_block_the_next_trial(self):
        out = self.out / f"{SCENARIO}-wright"
        locked = {"profile": out / "agent.sb", "agent-file": out / "workspace" / "agent-locked.txt"}
        for name, path in locked.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f"left by a run killed before cleanup ({name})")
            os.chflags(path, stat.UF_IMMUTABLE)  # the harness locks agent.sb; the agent can lock anything inside its run dir
        try:
            result = self.trial("true")
            self.assertNotIn("invalid", result)
        finally:
            for path in locked.values():  # if drop left them, free the test's own cleanup
                with contextlib.suppress(OSError):
                    os.chflags(path, 0)

    def test_the_tool_shim_runs_without_the_grader(self):
        shim = (agent_bench.HERE / "bench_trace.py").read_text()
        self.assertNotIn("import bench_grade", shim)
        self.assertIn("shim_main(sys.argv[2:])", shim)

    def test_a_refreshed_login_is_written_back_to_the_real_one_only_when_newer(self):
        home, run = self.out / "home", self.out / "run"
        (home / ".grok").mkdir(parents=True)
        run.mkdir()
        real, isolated = home / ".grok/auth.json", run / "grok-home/auth.json"
        real.write_text("old-token")
        os.chmod(real, 0o600)
        isolated.parent.mkdir()
        isolated.write_text("old-token")
        pairs = [(".grok/auth.json", "grok-home/auth.json")]
        self.assertEqual(agent_bench.sync_credentials_back(pairs, run, home), [])  # unchanged
        isolated.write_text("refreshed-token")
        os.utime(isolated, (real.stat().st_mtime + 10, real.stat().st_mtime + 10))
        self.assertEqual(agent_bench.sync_credentials_back(pairs, run, home), [".grok/auth.json"])
        self.assertEqual((real.read_text(), oct(real.stat().st_mode & 0o777)), ("refreshed-token", "0o600"))
        real.write_text("newer-real-token")  # the user logged in again meanwhile: never overwrite a newer real login
        os.utime(isolated, (real.stat().st_mtime - 10, real.stat().st_mtime - 10))
        self.assertEqual(agent_bench.sync_credentials_back(pairs, run, home), [])
        self.assertEqual(real.read_text(), "newer-real-token")

    def test_suite_runs_each_model_in_turn_continues_past_a_wait_or_a_skip_and_writes_the_page(self):
        models = [{"adapter": "devin", "model": "swe-2-max"}, {"adapter": "codex", "model": "gpt-6-luna", "effort": "xhigh"}, {"adapter": "pi", "model": "m"}]
        args = argparse.Namespace(models=models, only=None, out=self.out, suite_name="s", dry_run=False)
        calls = []

        def evaluate(sub):
            calls.append((sub.adapter, sub.name, sub.effort, sub.out))
            if sub.adapter == "codex":
                return 3
            if sub.adapter == "pi":
                raise SystemExit("cannot start: pi missing")
            return 0

        with patch.object(agent_bench, "cmd_evaluate", evaluate), patch.object(agent_bench.bench_leaderboard, "main") as page:
            status = agent_bench.cmd_suite(args)
        self.assertEqual(status, 3)  # codex is waiting for a rerun
        self.assertEqual([c[1] for c in calls], ["devin-swe-2-max", "codex-gpt-6-luna-xhigh", "pi-m"])
        self.assertTrue(all(c[3] == self.out / "s" for c in calls))
        page.assert_called_once()
        with patch.object(agent_bench, "cmd_evaluate", evaluate):
            only = agent_bench.cmd_suite(argparse.Namespace(models=models, only=["devin"], out=self.out, suite_name="s2", dry_run=True))
        self.assertEqual(only, 0)
        calls.clear()  # --only matches the adapter alone or adapter:model exactly — a prefix must not pick another model
        with patch.object(agent_bench, "cmd_evaluate", evaluate):
            only = agent_bench.cmd_suite(argparse.Namespace(models=models, only=["codex:gpt-6-luna"], out=self.out, suite_name="s4", dry_run=True))
        self.assertEqual(only, 3)  # only codex ran, and it is waiting for a rerun
        self.assertEqual([c[1] for c in calls], ["codex-gpt-6-luna-xhigh"])
        with self.assertRaisesRegex(SystemExit, "no models"):
            agent_bench.cmd_suite(argparse.Namespace(models=models, only=["codex:gpt-6"], out=self.out, suite_name="s5", dry_run=True))
        with patch.object(agent_bench, "cmd_evaluate", return_value=1):
            failed = agent_bench.cmd_suite(argparse.Namespace(models=[{"adapter": "devin", "model": "m"}], only=None, out=self.out, suite_name="s3", dry_run=True))
        self.assertEqual(failed, 1)  # a model that finished with errors fails the suite, it does not pass silently

        models = [{"adapter": "devin", "model": "m"}]
        for bad in ("..", "a/b", ""):
            with self.assertRaises(SystemExit):
                agent_bench.cmd_suite(argparse.Namespace(models=models, only=None, out=self.out, suite_name=bad, dry_run=True))

    def test_a_repeated_run_refuses_a_different_wright_binary(self):
        run = self.out / "r"
        trial = run / "s" / "agent" / "cell-1"
        trial.mkdir(parents=True)
        (trial / "result.json").write_text(json.dumps({"environment": {"wright": "wright 0.7.0", "wrightSha256": "a" * 64}}))
        other = self.out / "wright-other"
        other.write_text("a different binary")
        message = agent_bench.wright_mismatch(run, str(other))
        self.assertIn("wright 0.7.0", message)
        self.assertIn("--wright", message)
        self.assertIsNone(agent_bench.wright_mismatch(self.out / "fresh", str(other)))  # a new run has nothing to disagree with
        (trial / "result.json").write_text(json.dumps({"environment": {"wright": "x", "wrightSha256": agent_bench.file_sha256(other)}}))
        self.assertIsNone(agent_bench.wright_mismatch(run, str(other)))
        self.assertIsNone(agent_bench.wright_mismatch(run, str(self.out / "no-such-binary")))  # preflight names the missing binary; hashing it must not crash first
        (trial / "result.json").write_text('{"environment": {"wrightSha')  # a mid-write kill left a partial file: the trial is unfinished, not evidence
        self.assertIsNone(agent_bench.wright_mismatch(run, str(other)))

    def test_an_effort_the_model_id_already_names_is_not_repeated_in_the_run_name(self):
        self.assertEqual(agent_bench.model_slug({"adapter": "agy", "model": "gemini-3.8-flash-high", "effort": "high"}), "agy-gemini-3.8-flash-high")
        self.assertEqual(agent_bench.model_slug({"adapter": "codex", "model": "gpt-6-luna", "effort": "xhigh"}), "codex-gpt-6-luna-xhigh")

    def test_the_lift_cells_leave_out_the_canonical_cell_and_the_skill_probe(self):
        labels = [agent_bench.cell_label(agent_bench.normalize_cell(c)) for c in agent_bench.CELL_SETS["lift"]]
        self.assertEqual(labels, ["none/none/off", "wright/none/off", "overpy/none/off"])
        self.assertEqual([agent_bench.cell_label(agent_bench.normalize_cell(c)) for c in agent_bench.CELL_SETS["score"]], ["wright+wright-skill/none/off"])

    def test_a_report_over_several_runs_never_overwrites_a_runs_own_report(self):
        one, two = self.out / "one", self.out / "two"
        for run in (one, two):
            trial = run / "s" / "agent" / "cell-1"
            trial.mkdir(parents=True)
            (trial / "result.json").write_text(json.dumps({**ReportTest.result(ReportTest(), "none/none/off", 1, True, 100), "contract": "wright-agent-bench/v3", "environment": {}}, default=str))
            (run / "report.md").write_text("the run's own report")
        with contextlib.redirect_stdout(io.StringIO()):
            bench_report.main([one, two], WRIGHT, False, lambda s: {})
        self.assertEqual((one / "report.md").read_text(), "the run's own report")
        combined = self.out / "combined"
        with contextlib.redirect_stdout(io.StringIO()):
            bench_report.main([one, two], WRIGHT, False, lambda s: {}, None, combined)
        self.assertIn("Agent benchmark report", (combined / "report.md").read_text())

    def test_wait_for_limits_continues_after_a_provider_limit_and_gives_up_after_max_waits(self):
        waited = []
        with patch.object(agent_bench, "cmd_matrix", side_effect=[3, 3, 0]) as matrix, patch.object(agent_bench.time, "sleep", side_effect=waited.append), contextlib.redirect_stdout(io.StringIO()):
            status = agent_bench.matrix_waiting_for_limits(argparse.Namespace(wait_for_limits=True, limits_poll=2100, max_waits=5))
        self.assertEqual((status, matrix.call_count, waited), (0, 3, [2100, 2100]))
        with patch.object(agent_bench, "cmd_matrix", return_value=3) as matrix, patch.object(agent_bench.time, "sleep"), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(agent_bench.matrix_waiting_for_limits(argparse.Namespace(wait_for_limits=True, limits_poll=1, max_waits=2)), 3)
        self.assertEqual(matrix.call_count, 3)  # the first run and two waits
        with patch.object(agent_bench, "cmd_matrix", return_value=3) as matrix:  # without the option the exit code is left for the caller
            self.assertEqual(agent_bench.matrix_waiting_for_limits(argparse.Namespace(wait_for_limits=False, max_waits=5)), 3)
        self.assertEqual(matrix.call_count, 1)

    def test_wait_for_limits_rejects_a_negative_poll_or_wait_count(self):
        for bad in (argparse.Namespace(wait_for_limits=True, limits_poll=-1, max_waits=5), argparse.Namespace(wait_for_limits=True, limits_poll=2100, max_waits=-1)):
            with patch.object(agent_bench, "cmd_matrix", return_value=3), self.assertRaises(SystemExit):
                agent_bench.matrix_waiting_for_limits(bad)
        with patch.object(agent_bench, "cmd_matrix", return_value=0):  # a namespace without the options takes the defaults and does not wait
            self.assertEqual(agent_bench.matrix_waiting_for_limits(argparse.Namespace()), 0)
        argv = ["evaluate", "--adapter", "devin", "--model", "m"]
        with self.assertRaises(SystemExit) as neg:
            with contextlib.redirect_stderr(io.StringIO()):
                agent_bench.build_parser().parse_args(argv + ["--limits-poll", "-1"])
        self.assertNotEqual(neg.exception.code, 0)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            agent_bench.build_parser().parse_args(argv + ["--wait-for-limits", "30"])  # the flag takes no argument

    def test_network_off_makes_package_managers_and_downloaders_fail(self):
        # no real installs here: PATH resolution shows the shadowing and the blocker itself is the only thing executed
        for network, blocked in (("off", True), ("on", False)):
            out = self.out / f"env-{network}"
            out.mkdir()
            cell = agent_bench.normalize_cell({"tool": "none", "skills": [], "knowledge": "web" if network == "on" else "none", "network": network})
            args = argparse.Namespace(wright=WRIGHT, env_pass=[], agent_id="a", skill_dirs={})
            env = agent_bench.build_env(cell, args, out, out / "ws")
            shims = out / "bin"
            self.assertEqual(env["PATH"].split(os.pathsep)[0] == str(shims), blocked)
            if blocked:
                for name in agent_bench.NETWORK_TOOLS:
                    self.assertEqual(shutil.which(name, path=env["PATH"]), str(shims / name), f"{name} is not shadowed")
                found = subprocess.run([str(shims / "npm"), "install", "overpy"], env=env, capture_output=True, text=True)
                self.assertEqual(found.returncode, 1)
                self.assertIn("blocked", found.stderr)
            else:
                self.assertFalse(shims.exists())
                resolved = shutil.which("npm", path=env["PATH"])
                self.assertTrue(resolved is None or not resolved.startswith(str(out)))  # a real npm or none, never a shim

    def test_the_canary_flags_a_network_tool_the_blockers_do_not_shadow(self):
        out = self.out / "env-canary"
        out.mkdir()
        cell = agent_bench.normalize_cell({"tool": "none", "skills": [], "knowledge": "none", "network": "off"})
        args = argparse.Namespace(check_ancestors=False, canary_cmd=None, env_pass=[], agent_id="agent", wright=WRIGHT, skill_dirs={})
        env = agent_bench.build_env(cell, args, out, out / "ws")
        self.assertIsNone(agent_bench.canaries(cell, env, out / "ws", args))
        env["PATH"] = env["PATH"].split(os.pathsep, 1)[1]  # drop the blocker dir: every blocked name resolves unshadowed or missing
        self.assertIn("not shadowed", agent_bench.canaries(cell, env, out / "ws", args))

    def test_a_withheld_tool_fetched_through_a_package_manager_is_detected_in_native_transcripts(self):
        lines = [
            {"source": "agent", "tool_calls": [{"function_name": "exec", "arguments": {"command": "npm init -y && npm install --save-dev @wrightkit/wright overpy"}}]},
            {"payload": {"type": "function_call", "arguments": json.dumps({"cmd": "pip3 install overpy"})}},
            {"type": "toolCall", "arguments": {"command": "npx wright check mode.opy"}},
            {"source": "user", "message": "skill text mentioning `npm install -g overpy` is not a command"},
            {"source": "agent", "tool_calls": [{"function_name": "exec", "arguments": {"command": "wright check mode.ws"}}]},
        ]
        path = self.out / "transcript.jsonl"
        path.write_text("".join(json.dumps(line) + "\n" for line in lines))
        found = bench_trace.contraband_installs(path, "none")
        self.assertEqual([tool for tool, _ in found], ["wright", "overpy", "overpy", "wright"])
        self.assertEqual([tool for tool, _ in bench_trace.contraband_installs(path, "wright")], ["overpy", "overpy"])  # wright is the condition's own tool
        self.assertEqual([tool for tool, _ in bench_trace.contraband_installs(path, "overpy")], ["wright", "wright"])

    def test_every_blocked_tool_has_a_matching_fetch_pattern_and_every_call_shape_is_read(self):
        fetched = [
            "npm install overpy", "npm i overpy", "npm pack overpy", "pnpm add overpy", "pnpm dlx overpy", "yarn add overpy",
            "yarn global add overpy", "bun add overpy", "bun x overpy", "npx overpy --help", "npx -y overpy", "bunx overpy",
            "pip install overpy", "pip3 install overpy", "python3 -m pip install overpy", "pip download overpy",
            "pipx install overpy", "pipx run overpy", "uv add overpy", "uv pip install overpy", "uv tool install overpy",
            "uv tool run overpy", "uv run --with overpy python app.py", "uvx overpy",
            "cargo install overpy", "cargo add overpy", "brew install overpy", "gem install overpy",
            "go install overpy.dev/cmd/overpy@latest", "go run overpy.dev/cmd/overpy@latest",
            "apt install overpy", "apt-get install overpy", "apt download overpy", "composer require overpy/cli",
            "docker pull example.io/overpy:latest", "podman pull example.io/overpy:latest",
            "git clone https://example.com/overpy.git", "git clone https://github.com/wrightkit/opy-rs",
            "wget https://example.com/overpy.tgz",
            "curl -fsSLo out https://example.com/overpy.tar.gz", "curl https://example.com/overpy.tar.gz > out",
            "gh release download -R wrightkit/opy-rs", "gh repo clone wrightkit/opy-rs",
            "rsync host:/srv/overpy.tar.gz .", "scp host:overpy .",
        ]
        path = self.out / "transcript.jsonl"
        calls = [{"type": "toolCall", "arguments": {"command": c}} for c in fetched]
        path.write_text("".join(json.dumps(c) + "\n" for c in calls))
        self.assertEqual([tool for tool, _ in bench_trace.contraband_installs(path, "wright")], ["overpy"] * len(fetched))
        # `overpy` as a path/document/repo token is not a fetch — only install-verb arguments and fetch targets count
        clean = ["apt update", "cargo build", "npm test", "go build ./...", "composer dump-autoload", "git fetch origin",
                 "curl -s https://api.example.com/overpy-docs | jq .",
                 "npx prettier --write docs/overpy-notes.md", "uv run report.py --project overpy",
                 "wget https://site.example/overpy-guide.html", "git clone https://example.com/overpy-docs",
                 "gh repo clone wrightkit/wright-docs", "pipx run black --check .",
                 "curl -o page.html https://site.example/overpy-notes.html"]
        path.write_text("".join(json.dumps({"type": "toolCall", "arguments": {"command": c}}) + "\n" for c in clean))
        self.assertEqual(bench_trace.contraband_installs(path, "wright"), [])

    def test_mcp_level_withholds_the_cli_so_either_tool_is_contraband(self):
        self.assertEqual(agent_bench.allowed_cli_tool({"tool": "wright", "level": "bin"}), "wright")
        self.assertEqual(agent_bench.allowed_cli_tool({"tool": "overpy", "level": "bin"}), "overpy")
        self.assertEqual(agent_bench.allowed_cli_tool({"tool": "wright", "level": "mcp"}), "none")  # the MCP server gives wright without its CLI
        self.assertEqual(agent_bench.allowed_cli_tool({"tool": "none", "level": "bin"}), "none")

    def test_quoted_or_malformed_payloads_are_not_shell_commands(self):
        lines = [
            {"source": "agent", "message": "proposed but declined: {\"command\": \"npm install overpy\"}"},
            {"source": "user", "text": json.dumps({"command": "pip install wright"})},
            {"note": {"command": "gem install overpy"}},  # a command field outside any tool-call shape
            {"type": "assistant", "text": "run npm install overpy next"},
            {"calls": [{"name": "bash", "input": {"command": "npm install overpy"}}]},  # the normalized call shape still counts
            {"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Bash", "input": {"command": "wget https://x/overpy.tgz"}}]}},
            {"item": {"type": "command_execution", "command": "go install overpy.dev/cmd/overpy@latest"}},
            {"type": "function_call", "name": "shell", "arguments": "{\"command\": \"pipx install overpy\"}"},
        ]
        path = self.out / "transcript.jsonl"
        path.write_text("".join(json.dumps(line) + "\n" for line in lines))
        found = bench_trace.contraband_installs(path, "wright")
        self.assertEqual([tool for tool, _ in found], ["overpy"] * 4)  # only the real call structures

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
        args = argparse.Namespace(canary_cmd=None, check_ancestors=False, env_pass=[], agent_id="agent", wright=WRIGHT, skill_dirs={})
        wright_dir = str(Path(WRIGHT).resolve().parent)
        env = agent_bench.build_env(cell, args, self.out, self.out / "ws")
        env["PATH"] = os.pathsep.join([str(self.out / "bin"), wright_dir])  # blockers first, then a dir holding wright
        self.assertIn("reachable", agent_bench.canaries(cell, env, self.out, args))
        out_w = self.out / "with-wright"
        env_w = agent_bench.build_env({**cell, "tool": "wright"}, args, out_w, out_w / "ws")
        env_w["PATH"] = os.pathsep.join([str(out_w / "bin"), wright_dir])
        self.assertIsNone(agent_bench.canaries({**cell, "tool": "wright"}, env_w, self.out, args))

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

    def test_bounded_forms_are_counted_in_tool_use(self):
        # #532: `--brief` and selection flags/fields show up as bounded uses.
        agent = (f"cp {reference()}/* . && wright lint mode.ws --brief -f json >/dev/null; "
                 "wright analyze mode.ws -f json >/dev/null; "
                 "printf '{\"op\":\"inspect\",\"brief\":true}\\n{\"op\":\"lint\",\"severity\":\"warning\"}\\n{\"op\":\"cfg\",\"rule\":0}\\n' "
                 "| wright serve mode.ws >/dev/null")
        result = self.trial(agent)
        use = result["toolUse"]["wright"]
        self.assertEqual(use["invocations"], 5)
        # lint --brief, inspect?brief, lint?severity — analyze stays full and
        # cfg's `rule` is its address, not a selection.
        self.assertEqual(use["boundedUses"], 3)

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

    def test_a_retry_starts_without_the_previous_attempts_adapter_home(self):
        agent = ('if [ -f "$BENCH_RUN_DIR/tried" ]; then test ! -e "$BENCH_RUN_DIR/devin-home" || exit 1; exit 0; '
                 'else touch "$BENCH_RUN_DIR/tried"; mkdir -p "$BENCH_RUN_DIR/devin-home/.local"; exit 75; fi')
        result = self.trial(agent)
        self.assertEqual((result["agent"]["exit"], result["infraRetries"]), (0, 1))

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
            "canary": agent_bench.CANARY, "id": "tiny", "family": "modification", "language": "workshop", "entry": "mode.ws", "writable": ["mode.ws"],
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
        scenario = agent_bench.load_scenario("ana-paintball")
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


@unittest.skipUnless(sys.platform == "darwin", "file flags are the macOS enforcement")
class DropTest(unittest.TestCase):
    def setUp(self):
        (agent_bench.ROOT / "target").mkdir(exist_ok=True)
        self.out = Path(tempfile.mkdtemp(dir=agent_bench.ROOT / "target")).resolve()
        self.addCleanup(shutil.rmtree, self.out, True)

    def test_repairs_never_reach_through_a_symlink_to_a_host_file(self):
        host_file = self.out / "host-file.txt"
        host_file.write_text("host data the run must not mutate")
        host_file.chmod(0o400)
        os.chflags(host_file, stat.UF_IMMUTABLE)
        link = self.out / "run" / "stale-link"
        link.parent.mkdir(parents=True)
        link.symlink_to(host_file)
        try:
            agent_bench.drop(link)
            self.assertFalse(os.path.lexists(link))
            self.assertTrue(os.lstat(host_file).st_flags & stat.UF_IMMUTABLE, "the link redirected the flag repair onto the host file")
            self.assertEqual(host_file.stat().st_mode & 0o777, 0o400)
        finally:
            with contextlib.suppress(OSError):
                os.chflags(host_file, 0)
            host_file.chmod(0o600)

    def test_nested_chmodded_directories_come_down_a_level_per_pass(self):
        root = self.out / "run" / "denied"
        inner = root / "inner"
        inner.mkdir(parents=True)
        (inner / "left.txt").write_text("agent-owned")
        for directory in (inner, root):
            directory.chmod(0)  # the agent can make its own directories untraversable — deepest first, the parent must stay resolvable
        try:
            agent_bench.drop(self.out / "run")
            self.assertFalse(os.path.lexists(self.out / "run"))
        finally:
            for directory in (root, inner):
                with contextlib.suppress(OSError):
                    directory.chmod(0o700)


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

    def serve(self, line, direction="req", transport="stdio", session=7, t=0.0):
        return {"tool": "wright", "type": "serve", "dir": direction, "t": t, "line": line, "transport": transport, "session": session}

    def test_correction_rounds_counts_failed_validation_then_edit(self):
        events = [
            self.call(["check", "mode.ws"], exit_code=1, t=1.0),
            self.call(["check", "mode.ws"], exit_code=1, t=3.0),
            self.call(["check", "mode.ws"], exit_code=0, t=5.0),
        ]
        snapshots = [{"t": 2.0}, {"t": 4.0}, {"t": 6.0}]
        # fail -> edit, fail -> edit: two rounds; the pass at t=5 and the edit at t=6 are not one.
        self.assertEqual(bench_trace.correction_rounds(events, snapshots), 2)
        # consecutive failures before one edit are one round
        self.assertEqual(bench_trace.correction_rounds(events[:2], snapshots[:1]), 1)
        # a pass between failure and edit is not a correction
        self.assertEqual(bench_trace.correction_rounds([events[0], events[2]], [{"t": 6.0}]), 0)
        self.assertEqual(bench_trace.correction_rounds([], snapshots), 0)
        # usage errors and crashes are neutral: the fail -> edit round still counts, and they are not corrections
        errored = self.call(["check", "mode.ws", "--bogus"], exit_code=2, t=4.0)
        crashed = self.call(["check", "mode.ws"], exit_code=4, t=4.5)
        self.assertEqual(bench_trace.correction_rounds([events[0], errored, crashed], [{"t": 6.0}]), 1)
        self.assertEqual(bench_trace.correction_rounds([errored, crashed], [{"t": 6.0}]), 0)
        # `overpy compile` is the validating op of the opy cell
        opy = [self.call(["compile", "-i", "mode.opy"], exit_code=1, t=1.0, tool="overpy")]
        self.assertEqual(bench_trace.correction_rounds(opy, [{"t": 2.0}]), 1)

    def test_correction_rounds_covers_serve_ops_and_ignores_refusals(self):
        request = '{"op": "check"}'
        failed = '{"result": {"ok": false, "exit": 1, "diagnostics": []}}'
        passed = '{"result": {"ok": true, "exit": 0, "diagnostics": []}}'
        refused = '{"error": {"code": -32602, "message": "bad params"}}'
        events = [
            self.serve(request, t=1.0), self.serve(failed, direction="res", t=1.1),
            self.serve(request, t=3.0), self.serve(passed, direction="res", t=3.1),
            self.serve(request, t=5.0), self.serve(refused, direction="res", t=5.1),
            self.serve(request, t=7.0),  # unanswered: not a completed validation
        ]
        # t=1 fail -> t=2 edit; t=3 pass clears; t=5 refusal is not a correction signal; t=7 pending with no edit.
        self.assertEqual(bench_trace.correction_rounds(events, [{"t": 2.0}, {"t": 6.0}, {"t": 8.0}]), 1)

    def test_correction_rounds_unwraps_mcp_tool_payloads(self):
        request = '{"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "wright_check", "arguments": {}}}'
        result = '{"jsonrpc": "2.0", "id": 1, "result": {"content": [{"type": "text", "text": "{\\"ok\\": false, \\"exit\\": 1}"}]}}'
        events = [self.serve(request, transport="mcp", t=1.0), self.serve(result, direction="res", transport="mcp", t=1.1)]
        self.assertEqual(bench_trace.correction_rounds(events, [{"t": 2.0}]), 1)
        # an isError refusal (`{code, message}` payload) is neutral: it neither fails nor clears a pending correction
        refusal = '{"jsonrpc": "2.0", "id": 2, "result": {"isError": true, "content": [{"type": "text", "text": "{\\"code\\": \\"refused\\"}"}]}}'
        neutral = [self.serve(request, transport="mcp", t=3.0), self.serve(refusal, direction="res", transport="mcp", t=3.1)]
        self.assertEqual(bench_trace.correction_rounds(neutral, [{"t": 4.0}]), 0)
        self.assertEqual(bench_trace.correction_rounds(events + neutral, [{"t": 2.0}, {"t": 4.0}]), 1)

    def test_serve_request_mirrors_the_servers_answer_rule(self):
        notification = '{"jsonrpc":"2.0","method":"notifications/initialized"}'
        self.assertFalse(bench_trace.serve_request("", "mcp")["expects"])  # blank lines are skipped silently
        self.assertFalse(bench_trace.serve_request("   ", "stdio")["expects"])
        self.assertFalse(bench_trace.serve_request(notification, "mcp")["expects"])
        self.assertTrue(bench_trace.serve_request("not json", "stdio")["expects"])  # answered with a parse error
        self.assertTrue(bench_trace.serve_request('{"foo":1}', "mcp")["expects"])  # no method: answered -32600
        self.assertTrue(bench_trace.serve_request(notification, "stdio")["expects"])  # stdio answers every non-blank line
        request = bench_trace.serve_request('{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"wright_call_graph","arguments":{"depth":2}}}', "mcp")
        self.assertEqual((request["op"], request["args"], request["expects"]), ("callGraph", {"depth": 2}, True))
        # the jsonrpc transport's wright methods are compile/check/analyze/inspect — lint is a CLI op the server rejects
        self.assertEqual(bench_trace.serve_request('{"jsonrpc":"2.0","id":4,"method":"compile"}', "jsonrpc")["op"], "compile")
        self.assertEqual(bench_trace.serve_request('{"jsonrpc":"2.0","id":5,"method":"lint"}', "jsonrpc")["op"], "jsonrpc:lint")

    def test_serve_pairs_do_not_desynchronize_on_silent_lines(self):
        res1, res2, res3 = (json.dumps({"jsonrpc": "2.0", "id": i, "result": {}}) for i in (1, 2, 3))
        events = [
            self.serve(""),  # skipped silently by the server
            self.serve('{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"wright_check"}}', transport="mcp"),
            self.serve('{"jsonrpc":"2.0","method":"notifications/initialized"}', transport="mcp"),  # notification: no answer
            self.serve('{"foo":1}', transport="mcp"),  # answered -32600
            self.serve(res1, "res", "mcp"), self.serve('{"error":{"code":-32600}}', "res", "mcp"),
            self.serve('{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"wright_symbols"}}', transport="mcp"),
            self.serve(res2, "res", "mcp"),
            self.serve('{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"wright_lint"}}', transport="mcp"),  # unanswered
        ]
        pairs = bench_trace.serve_pairs(events)
        self.assertEqual([r["op"] for _, r, _ in pairs], ["check", None, "symbols", "lint"])
        self.assertEqual(pairs[0][2]["line"], res1)
        self.assertEqual(pairs[1][2]["line"], '{"error":{"code":-32600}}')
        self.assertEqual(pairs[2][2]["line"], res2)
        self.assertIsNone(pairs[3][2])

    def test_serve_pairs_separate_concurrent_sessions(self):
        events = [
            self.serve('{"op":"check"}', session=1, t=1), self.serve('{"op":"lint"}', session=2, t=2),
            self.serve('{"r":1}', "res", session=1, t=3), self.serve('{"r":2}', "res", session=2, t=4),
        ]
        pairs = bench_trace.serve_pairs(events)
        self.assertEqual([(r["op"], s["line"]) for _, r, s in pairs], [("check", '{"r":1}'), ("lint", '{"r":2}')])

    def test_serve_error_classifies_refusals_and_malformed(self):
        self.assertEqual(bench_trace.serve_error('{"error":{"code":-32700}}'), "malformed")
        self.assertEqual(bench_trace.serve_error('{"error":{"code":"malformed-request"}}'), "malformed")
        self.assertEqual(bench_trace.serve_error('{"error":{"code":-32602,"message":"unknown tool"}}'), "refused")
        self.assertEqual(bench_trace.serve_error('{"result":{"isError":true,"content":[]}}'), "refused")
        self.assertIsNone(bench_trace.serve_error('{"result":{"ok":true}}'))

    def test_call_counts_and_shell_search_reads(self):
        (agent_bench.ROOT / "target").mkdir(exist_ok=True)
        transcript = Path(tempfile.mkdtemp(dir=agent_bench.ROOT / "target")) / "transcript.jsonl"
        self.addCleanup(shutil.rmtree, transcript.parent, True)
        transcript.write_text("\n".join(json.dumps(e) for e in [
            {"type": "assistant", "calls": [{"name": "bash", "input": {"command": "rg foo && cat mode.ws"}},
                                            {"name": "wright_check", "input": {"file": "mode.ws"}}]},
            {"type": "assistant", "calls": [{"name": "bash", "input": {"command": "wright check mode.ws"}},
                                            {"name": "bash", "input": "a string input"},
                                            {"name": "bash", "input": {"command": "ls"}}]},
            "not json",
        ]))
        self.assertEqual(bench_trace.call_counts(transcript), {"bash": 4, "wright_check": 1})
        self.assertEqual(bench_trace.shell_search_reads(transcript), 2)  # the wright-CLI call is counted by toolUse, not here

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
    def test_load_skips_a_partial_result_left_by_a_killed_run(self):
        import tempfile
        (agent_bench.ROOT / "target").mkdir(exist_ok=True)
        root = Path(tempfile.mkdtemp(dir=agent_bench.ROOT / "target")).resolve()
        self.addCleanup(shutil.rmtree, root, True)
        finished = {k: v for k, v in {**self.result("wright/none/off", 2, True, 5), "environment": {}}.items() if not k.startswith("_")}  # _dir/_trial are stamped on load, not stored
        for name, content in (("partial-1", '{"contract":'), ("done-2", json.dumps(finished))):
            directory = root / "s" / "a" / name
            directory.mkdir(parents=True)
            (directory / "result.json").write_text(content)
        loaded = bench_report.load([root])
        self.assertEqual(len(loaded), 1)
        self.assertEqual(loaded[0]["_trial"], 2)

    def result(self, cell, trial, usable, tokens, split=None, scenario="s", language="opy", status="completed", **environment):
        tool = cell.split("/")[0].split("+")[0]
        return {
            "contract": "wright-agent-bench/v3", "scenario": scenario, "family": "diagnosis", "language": language, "split": split, "_trial": trial, "_dir": Path("d"),
            "condition": {"label": cell}, "agent": {"id": "m", "exit": 0, "seconds": 1.0}, "status": status,
            "usable": usable, "passed": usable, "usage": {"totalTokens": tokens, "peakContext": tokens // 2},
            "toolUse": {"wright": {"invocations": 1}} if tool == "wright" else {},
            "environment": environment,
        }

    def test_wilson_interval(self):
        low, high = bench_report.wilson(5, 10)
        self.assertAlmostEqual(low, 0.237, places=2)
        self.assertAlmostEqual(high, 0.763, places=2)
        self.assertEqual(bench_report.wilson(0, 0), (0.0, 0.0))

    def test_paired_lift_against_each_named_reference(self):
        runs = [
            self.result("none/none/off", 1, False, 100),
            self.result("none/wiki/off", 1, True, 200),
            self.result("wright/none/off", 1, True, 80),
        ]
        text, _ = bench_report.render(runs, references=["none/none/off", "none/wiki/off"])
        self.assertIn("Paired against `none/none/off`", text)
        self.assertIn("Paired against `none/wiki/off`", text)
        self.assertIn("wright/none/off vs none/none/off", text)
        self.assertIn("wright/none/off vs none/wiki/off", text)
        self.assertIn("none/wiki/off vs none/none/off", text)
        text, _ = bench_report.render(runs)  # the default pairs only against the baseline
        self.assertIn("Paired against `none/none/off`", text)
        self.assertNotIn("Paired against `none/wiki/off`", text)
        text, _ = bench_report.render(runs, references=["missing/cell/here"])
        self.assertNotIn("Paired against", text)

    def test_paired_lift_reports_an_interval_and_drops_pairs_from_a_different_environment(self):
        runs = [
            self.result("none/none/off", 1, False, 100, scenario="a", wrightSha256="x"),
            self.result("wright/none/off", 1, True, 80, scenario="a", wrightSha256="x"),
            self.result("none/none/off", 2, True, 100, scenario="b", wrightSha256="x"),
            self.result("wright/none/off", 2, True, 90, scenario="b", wrightSha256="y"),  # a different binary: not one experiment
            self.result("wright+wright-skill/none/off", 1, True, 70, scenario="a", wrightSha256="x", skills={"wright-skill": {"sha256": "s"}}),
        ]
        text, _ = bench_report.render(runs)
        self.assertIn("paired lift [95% CI]", text)
        self.assertIn("+100pp", text)  # only pair `a` survives: baseline unusable, wright usable
        self.assertIn("wrightSha256", text)  # the b pair is dropped and reported, not silently mixed
        self.assertIn("Non-comparable pairs dropped", text)
        self.assertIn("| wright/none/off vs none/none/off | 1 | +1 / -0 |", text)  # only the comparable `a` pair counts
        self.assertIn("wright+wright-skill/none/off vs none/none/off", text)  # the skill difference is the variable being measured

    def test_scenario_discrimination_flags(self):
        runs = [
            self.result("none/none/off", 1, True, 100, scenario="always-pass"),
            self.result("wright/none/off", 1, True, 100, scenario="always-pass"),
            self.result("none/none/off", 1, False, 100, scenario="always-fail"),
            self.result("wright/none/off", 1, False, 100, scenario="always-fail"),
            self.result("none/none/off", 1, False, 100, scenario="wright-helps"),
            self.result("wright/none/off", 1, True, 100, scenario="wright-helps"),
            self.result("none/none/off", 1, True, 100, scenario="one-cell"),
        ]
        flags = bench_report.discrimination(runs)
        self.assertEqual(flags["always-pass"]["discrimination"], "smoke")
        self.assertEqual(flags["always-fail"]["discrimination"], "smoke")
        self.assertEqual(flags["wright-helps"]["discrimination"], "discriminating")
        self.assertEqual(flags["wright-helps"]["differingConditions"], ["none/none/off", "wright/none/off"])
        self.assertEqual(flags["one-cell"]["discrimination"], "indeterminate")
        rates = flags["wright-helps"]["conditions"]
        self.assertEqual(rates["none/none/off"], {"usable": 0, "runs": 1, "rate": 0.0})
        self.assertEqual(rates["wright/none/off"], {"usable": 1, "runs": 1, "rate": 1.0})

    def test_lift_report_shows_both_denominators_and_names_smoke_scenarios(self):
        runs = [
            self.result("none/none/off", 1, False, 100, scenario="wright-helps"),
            self.result("wright/none/off", 1, True, 100, scenario="wright-helps"),
            self.result("none/none/off", 1, True, 100, scenario="always-pass"),
            self.result("wright/none/off", 1, True, 100, scenario="always-pass"),
        ]
        text, summary = bench_report.render(runs)
        self.assertIn("usable gained/lost (all)", text)
        self.assertIn("usable gained/lost (discriminating)", text)
        self.assertIn("+1 / -0 | +1 / -0 (n=1)", text)
        self.assertIn("Smoke scenarios kept out of the discriminating column: always-pass", text)
        entries = {s["id"]: s for s in summary["scenarios"]}
        self.assertEqual(entries["always-pass"]["discrimination"], "smoke")
        self.assertEqual(entries["wright-helps"]["discrimination"], "discriminating")

    def test_correction_rounds_reach_the_cell_summary(self):
        run = self.result("wright/none/off", 1, True, 100)
        run["correctionRounds"] = 2.0
        text, summary = bench_report.render([run])
        self.assertEqual(summary["cells"]["m|wright/none/off"]["correctionRounds"], 2.0)
        self.assertIn("| corr |", text)

    def test_references_of_normalizes_flag_and_config_forms(self):
        ns = argparse.Namespace(reference=["a/b/c", "d/e/f"])
        self.assertEqual(agent_bench.references_of(ns), ["a/b/c", "d/e/f"])
        self.assertEqual(agent_bench.references_of(argparse.Namespace(reference="a/b/c")), ["a/b/c"])
        self.assertEqual(agent_bench.references_of(argparse.Namespace(reference=None)), [bench_report.BASELINE])

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

    def level_result(self, level, trial, usable, tokens, bash=(), wright_invocations=0, tool_calls=None, scenario="s"):
        run = self.result("wright-mcp/none/off" if level == "mcp" else "wright/none/off", trial, usable, tokens, scenario=scenario)
        run["condition"].update(tool="wright", level=level, knowledge="none", network="off", skills=[])
        run["toolUse"] = {"wright": {"invocations": wright_invocations}}
        run["toolCalls"] = tool_calls or {}
        (agent_bench.ROOT / "target").mkdir(exist_ok=True)
        directory = Path(tempfile.mkdtemp(dir=agent_bench.ROOT / "target"))
        self.addCleanup(shutil.rmtree, directory, True)
        transcript = [{"type": "assistant", "calls": [{"name": "bash", "input": {"command": command}} for command in bash]}]
        (directory / "transcript.jsonl").write_text("\n".join(json.dumps(e) for e in transcript))
        run["_dir"] = directory
        return run

    def test_level_comparison_pairs_mcp_with_bin(self):
        runs = [
            self.level_result("bin", 1, True, 1000, bash=("rg foo mode.ws", "wright check mode.ws", "cat mode.ws"), wright_invocations=2, tool_calls={"bash": 3}),
            self.level_result("mcp", 1, True, 800, wright_invocations=3, tool_calls={"wright_check": 2, "wright_symbols": 1, "bash": 1}),
            self.level_result("bin", 2, False, 900, bash=("ls",), wright_invocations=1, tool_calls={"bash": 1}),
            self.level_result("mcp", 2, True, 700, wright_invocations=2, tool_calls={"wright_check": 2}),
        ]
        text, summary = bench_report.render(runs)
        self.assertIn("Level comparison", text)
        self.assertIn("+1 / -0", text)  # mcp turned trial 2 usable
        stats = {row["cell"]: row["stats"] for row in summary["levels"]}
        self.assertEqual(stats["wright/none/off"]["mcp"]["searchReads"], 2.5)  # wright invocations only; no shell search commands
        self.assertEqual(stats["wright/none/off"]["bin"]["searchReads"], 3.0)  # (2 wright + 2 shell) and (1 wright + 1 shell)
        self.assertIn("[", text.split("Level comparison")[1])  # Wilson intervals on the rates
        # trial keys do not cross-pair: bin trial 1 and mcp trial 2 are different trials
        text, _ = bench_report.render([runs[0], runs[3]])
        self.assertNotIn("Level comparison", text)


class TrackingDefinitionTest(unittest.TestCase):
    """#540: the committed per-release tracking definition must only name cells and models the harness can run."""

    def definition(self, **overrides) -> dict:
        base = {
            "contract": agent_bench.TRACKING_CONTRACT,
            "suite": bench_grade.SUITE_VERSION,
            "models": [{"adapter": "devin", "model": "swe-2-max"}],
            "cells": [dict(agent_bench.CANONICAL_CELL), {"tool": "none", "skills": [], "knowledge": "none", "network": "off"}],
            "split": "test", "trials": 3, "parallel": 1, "seed": 1,
        }
        base.update(overrides)
        return base

    def load(self, definition: dict) -> dict:
        (agent_bench.ROOT / "target").mkdir(exist_ok=True)
        directory = Path(tempfile.mkdtemp(dir=agent_bench.ROOT / "target"))
        self.addCleanup(shutil.rmtree, directory, True)
        path = directory / "tracking.json"
        path.write_text(json.dumps(definition))
        return agent_bench.load_tracking(path)

    def test_committed_definition_is_runnable(self):
        definition = agent_bench.load_tracking()
        self.assertEqual(definition["suite"], bench_grade.SUITE_VERSION)
        for model in definition["models"]:
            self.assertIn(model["adapter"], agent_bench.ADAPTERS)
            self.assertTrue(model["model"])
        labels = [agent_bench.cell_label(agent_bench.normalize_cell(cell)) for cell in definition["cells"]]
        canonical = agent_bench.cell_label(agent_bench.normalize_cell(agent_bench.CANONICAL_CELL))
        self.assertIn(canonical, labels)
        self.assertGreater(len(labels), 1)  # the canonical cell alone cannot produce a lift figure

    def test_unrunnable_cell_is_refused(self):
        with self.assertRaises(SystemExit):
            self.load(self.definition(cells=[{"tool": "bogomips", "skills": [], "knowledge": "none", "network": "off"}]))

    def test_unrunnable_model_is_refused(self):
        with self.assertRaises(SystemExit):
            self.load(self.definition(models=[{"adapter": "bogomips", "model": "x"}]))

    def test_foreign_suite_is_refused(self):
        with self.assertRaises(SystemExit):
            self.load(self.definition(suite="v0"))

    def test_canonical_cell_is_required(self):
        cells = [{"tool": "none", "skills": [], "knowledge": "none", "network": "off"}, {"tool": "wright", "skills": [], "knowledge": "none", "network": "off"}]
        with self.assertRaises(SystemExit):
            self.load(self.definition(cells=cells))


if __name__ == "__main__":
    unittest.main()
