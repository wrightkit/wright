import sys
import io
import json
from unittest.mock import patch, MagicMock
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "adapters"))

import devin
import pi
import codex
import agy
import direct
import opencode
import grok
import os
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer


class PiAdapterTest(unittest.TestCase):
    def test_usage_row_counts_cache_in_context(self):
        message = {"usage": {"input": 586, "output": 5, "cacheRead": 400, "cacheWrite": 100, "reasoning": 3}}
        row = pi.usage_row(message, 272_000, 1.0)
        self.assertEqual((row["input"], row["cache_read"], row["cache_write"], row["context"], row["context_limit"]), (586, 400, 100, 1086, 272_000))
        self.assertEqual((row["output"], row["reasoning"]), (2, 3))
        self.assertEqual(sum(row[key] or 0 for key in ("input", "output", "cache_read", "cache_write", "reasoning")), 1091)

    def test_recovered_provider_error_does_not_fail_completed_task(self):
        outage = {"role": "assistant", "stopReason": "error", "errorMessage": "WebSocket error"}
        recovered = {"role": "assistant", "stopReason": "stop", "content": [{"type": "text", "text": "done"}]}
        for messages, expected in (([outage, recovered], 0), ([recovered, outage], 75)):
            with self.subTest(expected=expected):
                proc = MagicMock()
                proc.stdin = io.StringIO()
                proc.stdout = io.StringIO("".join(json.dumps({"type": "message_end", "message": m}) + "\n" for m in messages))
                proc.stderr = io.StringIO()
                proc.wait.return_value = 0
                env = {
                    "HOME": "/isolated", "BENCH_MODEL": "provider/model", "BENCH_RUN_DIR": "/run",
                    "BENCH_KNOWLEDGE": "none", "BENCH_USAGE": "/run/usage",
                    "BENCH_TRANSCRIPT": "/run/transcript", "BENCH_CONTEXT": "/run/context", "BENCH_AGENT_INFO": "/run/agent-info",
                }
                with (
                    patch.dict(pi.os.environ, env, clear=True),
                    patch.object(pi.sys, "stdin", io.StringIO("task")),
                    patch.object(pi.sys, "stdout", io.StringIO()),
                    patch.object(pi.sys, "stderr", io.StringIO()),
                    patch.object(pi.shutil, "which", return_value="pi"),
                    patch.object(pi.Path, "mkdir"),
                    patch.object(pi.Path, "is_file", return_value=False),
                    patch.object(pi.Path, "write_text"),
                    patch.object(pi, "context_limit", return_value=None),
                    patch.object(pi, "cli_version", return_value="pi 0"),
                    patch.object(pi.subprocess, "Popen", return_value=proc),
                    patch("builtins.open", side_effect=lambda *a, **kw: io.StringIO()),
                ):
                    self.assertEqual(pi.main(), expected)

    def test_loaded_skills_and_final_text(self):
        system = {"sections": {"skills": "<available_skills><skill><name>wright</name></skill><skill><name>other</name></skill></available_skills>"}}
        self.assertEqual(pi.skill_names(system), ["wright", "other"])
        self.assertEqual(pi.skill_names({"sections": {}}), [])
        self.assertEqual(pi.message_text({"content": [{"type": "text", "text": "a"}, {"type": "tool_use"}, {"type": "text", "text": "b"}]}), "ab")


class DevinAdapterTest(unittest.TestCase):
    export = {"steps": [
        {"source": "system", "message": '<rules type="always-on">\n<rule name="AGENTS" path="/x/AGENTS.md">'},
        {"source": "system", "message": (
            "<available_skills>\n"
            "- **wright**: A guide. (source: /w/.agents/skills/wright/SKILL.md)\n"
            "- **devin-cli**: Docs. (source: /h/share/devin/docs)\n"
            "- **upload-secrets**: Secrets. (source: builtin:upload-secrets)\n"
            "- **context7-mcp**: Docs. (source: /h/cli/plugins/cache/x/skills/context7-mcp/SKILL.md)\n")},
        {"source": "user", "message": "task"},
        {"source": "agent", "timestamp": "2026-09-30T16:18:40+00:00", "metrics": {"prompt_tokens": 1000, "completion_tokens": 20, "cached_tokens": 600}},
        {"source": "agent", "timestamp": "2026-09-30T16:18:41+00:00", "message": "no metrics"},
    ]}

    def test_usage_rows_split_cached_prompt_tokens(self):
        rows, _, _ = devin.parse_export(self.export)
        self.assertEqual(len(rows), 1)
        self.assertEqual((rows[0]["input"], rows[0]["cache_read"], rows[0]["output"], rows[0]["context"]), (400, 600, 20, 1000))

    def test_loaded_context_excludes_builtins_and_lists_plugins_apart(self):
        _, loaded, plugins = devin.parse_export(self.export)
        self.assertEqual(loaded, ["AGENTS", "wright"])
        self.assertEqual(plugins, ["context7-mcp"])

    def test_config_reads_no_other_tools_and_denies_web_unless_asked(self):
        base = {"read_config_from": {"claude": True}, "permissions": {"allow": ["Exec(*)"]}, "agent": {"model": "old"}}
        closed = devin.isolated_config(base, "swe-2-max", web=False)
        opened = devin.isolated_config(base, "swe-2-max", web=True)
        self.assertFalse(any(closed["read_config_from"].values()))
        self.assertEqual(closed["agent"]["model"], "swe-2-max")
        self.assertIn("web_search", closed["permissions"]["deny"])
        self.assertIn("web_search", closed["disabled_tools"])
        self.assertIn("mcp_call_tool", closed["disabled_tools"])
        self.assertNotIn("web_search", opened["disabled_tools"])
        self.assertNotIn("web_search", opened["permissions"]["deny"])
        self.assertIn("mcp_call_tool", opened["permissions"]["deny"])
        self.assertNotIn("allow", closed["permissions"])


class DevinTransientTest(unittest.TestCase):
    def test_empty_model_catalog_is_a_provider_failure_but_a_wrong_model_is_not(self):
        self.assertTrue(devin.transient("Error: Unknown model: 'swe-2-max'\nAvailable:\n", ""))
        self.assertFalse(devin.transient("Error: Unknown model: 'nope'\nAvailable:\n  swe-2-max\n  swe-2\n", ""))
        self.assertTrue(devin.transient("", "429 rate limit"))

    def test_agent_text_on_stdout_does_not_classify_as_a_provider_failure(self):
        self.assertFalse(devin.transient("I could not finish: my test run timed out and the quota for retries is used up", ""))


class AgyTransientTest(unittest.TestCase):
    def test_a_dropped_connection_is_a_provider_failure_even_with_an_answer_written(self):
        self.assertTrue(agy.transient('API error (attempt 1): request failed: Post "https://x/v1internal:streamGenerateContent": EOF'))
        self.assertTrue(agy.transient("quota exceeded"))
        self.assertFalse(agy.transient("the agent wrote an invalid file"))


class DevinEffortTest(unittest.TestCase):
    def test_effort_is_read_from_the_model_id(self):
        self.assertEqual(devin.split_effort("swe-2-max"), ("swe-2", "max"))
        self.assertEqual(devin.split_effort("claude-opus-5-5-medium"), ("claude-opus-5-5", "medium"))
        self.assertEqual(devin.split_effort("swe-2"), ("swe-2", None))


class NativeAdapterUsageTest(unittest.TestCase):
    def test_codex_inclusive_counts_are_split_without_counting_reasoning_twice(self):
        row = codex.usage_row({"input_tokens": 100, "cached_input_tokens": 60, "output_tokens": 20, "reasoning_output_tokens": 12}, 1.0, 272000)
        self.assertEqual((row["input"], row["cache_read"], row["output"], row["reasoning"], row["context"]), (40, 60, 8, 12, 100))
        self.assertEqual(sum(row[key] or 0 for key in ("input", "cache_read", "cache_write", "output", "reasoning")), 120)

    def test_agy_cache_is_exclusive_and_thinking_is_included_in_output(self):
        row = agy.usage_row({"input_tokens": 278, "cache_read_tokens": 30214, "output_tokens": 4, "thinking_tokens": 3}, 1.0)
        self.assertEqual((row["input"], row["cache_read"], row["output"], row["reasoning"], row["context"]), (278, 30214, 1, 3, 30492))
        self.assertIsNone(row["context_limit"])

    def test_codex_builtin_skills_are_separate_from_observed_project_skills(self):
        text = ('- `r0` = `/isolated/.codex/skills/.system`\n- `r1` = `/workspace/.agents/skills`\n'
                '- openai-docs: Builtin. (file: r0/openai-docs/SKILL.md)\n'
                '- wright: Project. (file: r1/wright/SKILL.md)\n')
        self.assertEqual(codex.loaded_skills(text), (["wright"], ["openai-docs"]))


class GrokAdapterTest(unittest.TestCase):
    def test_usage_row_buckets_are_disjoint_and_carry_the_context_limit(self):
        row = grok.usage_row({"input_tokens": 12908, "output_tokens": 17, "cache_read_input_tokens": 1536, "cache_creation_input_tokens": 0}, 256000, 1.0)
        self.assertEqual((row["input"], row["cache_read"], row["output"], row["context"], row["context_limit"]), (12908, 1536, 17, 14444, 256000))


class OpencodeAdapterTest(unittest.TestCase):
    def test_usage_row_keeps_cache_apart_from_input(self):
        row = opencode.usage_row({"total": 6679, "input": 1042, "output": 5, "reasoning": 0, "cache": {"write": 0, "read": 5632}}, 1.0)
        self.assertEqual((row["input"], row["cache_read"], row["output"], row["context"]), (1042, 5632, 5, 6674))

    def test_builtin_skills_are_not_loaded_context(self):
        raw = json.dumps([{"name": "customize-opencode", "location": "<built-in>"}, {"name": "wright", "location": "/w/.agents/skills/wright/SKILL.md"}])
        self.assertEqual(opencode.available_skills(raw), ["wright"])
        self.assertEqual(opencode.available_skills("not json"), [])


class DirectAdapterTest(unittest.TestCase):
    def serve(self, replies):
        seen = []

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                request = json.loads(self.rfile.read(int(self.headers["content-length"])))
                seen.append(request)
                status, body = replies[min(len(seen), len(replies)) - 1]
                self.send_response(status)
                self.end_headers()
                self.wfile.write(json.dumps(body(request) if callable(body) else body).encode())

            def log_message(self, *args):
                pass

        server = HTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.shutdown)
        return f"http://127.0.0.1:{server.server_port}", seen

    def run_direct(self, model, base_var, replies, extra_env=None):
        base, seen = self.serve(replies)
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as tmp:
            tmp = Path(tmp)
            skill = tmp / "demo"
            skill.mkdir()
            (skill / "SKILL.md").write_text("---\nname: demo\ndescription: A demo skill\n---\nbody\n")
            env = {"BENCH_MODEL": model, "BENCH_KNOWLEDGE": "none", "BENCH_SKILL_DIRS": str(skill), "ANTHROPIC_API_KEY": "k", "OPENAI_API_KEY": "k", base_var: base,
                   **{f"BENCH_{n}": str(tmp / n.lower()) for n in ("USAGE", "TRANSCRIPT", "CONTEXT", "AGENT_INFO")}, "PATH": os.environ["PATH"], **(extra_env or {})}
            cwd = os.getcwd()
            os.makedirs(tmp / "work")
            os.chdir(tmp / "work")
            out = io.StringIO()
            try:
                with patch.dict(direct.os.environ, env, clear=True), patch.object(direct.sys, "stdin", io.StringIO("task")), patch.object(direct.sys, "stdout", out):
                    code = direct.main()
            finally:
                os.chdir(cwd)
            read = lambda n: (tmp / n).read_text()
            return code, out.getvalue(), seen, [json.loads(l) for l in read("usage").splitlines()], json.loads(read("context")), json.loads(read("agent_info"))

    def test_anthropic_loop_runs_a_tool_and_records_usage(self):
        use = {"content": [{"type": "tool_use", "id": "t1", "name": "bash", "input": {"command": "echo hi"}}], "usage": {"input_tokens": 10, "output_tokens": 2, "cache_read_input_tokens": 5}}
        done = {"content": [{"type": "text", "text": "done"}], "usage": {"input_tokens": 20, "output_tokens": 3}}
        code, final, seen, usage, context, info = self.run_direct("anthropic/m", "ANTHROPIC_BASE_URL", [(200, use), (200, done)])
        self.assertEqual((code, final, len(seen)), (0, "done", 2))
        self.assertEqual(seen[1]["messages"][-1]["content"][0]["content"].strip(), "hi")
        self.assertIn("demo: A demo skill", seen[0]["system"])
        self.assertEqual((usage[0]["input"], usage[0]["cache_read"], usage[0]["context"]), (10, 5, 15))
        self.assertEqual((context["loaded"], info["agent"], [t["name"] for t in seen[0]["tools"]]), (["demo"], "direct", ["bash"]))

    def test_openai_usage_splits_cached_and_reasoning_tokens(self):
        reply = {"choices": [{"message": {"role": "assistant", "content": "ok"}}], "usage": {"prompt_tokens": 100, "completion_tokens": 20, "prompt_tokens_details": {"cached_tokens": 60}, "completion_tokens_details": {"reasoning_tokens": 12}}}
        code, final, _, usage, _, _ = self.run_direct("openai/m", "OPENAI_BASE_URL", [(200, reply)])
        self.assertEqual((code, final), (0, "ok"))
        self.assertEqual((usage[0]["input"], usage[0]["cache_read"], usage[0]["output"], usage[0]["reasoning"]), (40, 60, 8, 12))

    def test_client_error_is_not_an_infrastructure_failure(self):
        code, *_ = self.run_direct("anthropic/m", "ANTHROPIC_BASE_URL", [(400, {"error": "bad"})])
        self.assertEqual(code, 1)

    def test_a_malformed_payload_is_a_clean_provider_error_not_a_traceback(self):
        for model, base_var, replies in (("anthropic/m", "ANTHROPIC_BASE_URL", [(200, {"unexpected": "shape"})]),
                                         ("openai/m", "OPENAI_BASE_URL", [(200, {"choices": []})])):
            with self.subTest(model=model):
                code, *_ = self.run_direct(model, base_var, replies)
                self.assertEqual(code, 1)

    FAKE_MCP = '''\
import json, os, sys
log = open(os.environ["MCP_CALLS"], "a")
for line in sys.stdin:
    try:
        msg = json.loads(line)
    except json.JSONDecodeError:
        continue
    method = msg.get("method")
    if method == "initialize":
        result = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}, "serverInfo": {"name": "fake", "version": "0"}}
    elif method == "tools/list":
        result = {"tools": [{"name": "wright_check", "description": "Check the project", "inputSchema": {"type": "object", "properties": {"file": {"type": "string"}}}}]}
    elif method == "tools/call":
        log.write(msg["params"]["name"] + "\\n")
        log.flush()
        result = {"content": [{"type": "text", "text": "{\\"ok\\": true}"}]}
    else:
        result = {}
    if "id" in msg:
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": result}) + "\\n")
        sys.stdout.flush()
'''

    def test_mcp_tools_reach_the_model_dispatch_to_the_server_and_bill_schemas(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as tmp:
            tmp = Path(tmp)
            server_py = tmp / "fake_mcp.py"
            server_py.write_text(self.FAKE_MCP)
            calls = tmp / "calls.txt"
            use = {"content": [{"type": "tool_use", "id": "t1", "name": "wright_check", "input": {"file": "mode.ws"}}],
                   "usage": {"input_tokens": 1, "output_tokens": 1}}
            billed = lambda request: {"content": [{"type": "text", "text": "done"}],
                                      "usage": {"input_tokens": len(json.dumps(request["tools"])), "output_tokens": 1}}
            code, final, seen, usage, context, info = self.run_direct(
                "anthropic/m", "ANTHROPIC_BASE_URL", [(200, use), (200, billed)],
                {"BENCH_MCP_CMD": f"{sys.executable} {server_py}", "MCP_CALLS": str(calls), "BENCH_TOOL_LEVEL": "mcp"})
            self.assertEqual((code, final), (0, "done"))
            tools = {t["name"]: t for t in seen[0]["tools"]}
            self.assertEqual([t["name"] for t in seen[0]["tools"]], ["bash", "wright_check"])
            self.assertEqual(tools["wright_check"]["input_schema"], {"type": "object", "properties": {"file": {"type": "string"}}})
            self.assertEqual(calls.read_text().strip(), "wright_check")
            self.assertIn('"ok": true', seen[1]["messages"][-1]["content"][0]["content"])
            self.assertEqual(info["toolLevel"], "mcp")
            # fixture: the billed input covers the whole serialized tool list, so the MCP tool schema counts toward mcp input tokens
            self.assertEqual(usage[1]["input"], len(json.dumps(seen[1]["tools"])))

    def test_mcp_setup_failure_is_not_an_infrastructure_exit(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as tmp:
            env = {"BENCH_MODEL": "anthropic/m", "BENCH_KNOWLEDGE": "none", "BENCH_MCP_CMD": "false", "ANTHROPIC_API_KEY": "k",
                   **{f"BENCH_{n}": str(Path(tmp) / n.lower()) for n in ("USAGE", "TRANSCRIPT", "CONTEXT", "AGENT_INFO")}, "PATH": os.environ["PATH"]}
            with patch.dict(direct.os.environ, env, clear=True), patch.object(direct.sys, "stdin", io.StringIO("task")), patch.object(direct.sys, "stdout", io.StringIO()):
                self.assertEqual(direct.main(), 1)


class PipeTest(unittest.TestCase):
    def test_drain_keeps_a_chatty_stderr_from_deadlocking_the_stdout_read(self):
        # a child that floods stderr past the pipe buffer blocks unless someone drains it concurrently with stdout
        import subprocess
        import common
        child = subprocess.Popen(
            [sys.executable, "-c", "import sys; sys.stderr.write('x' * 262144); sys.stderr.flush(); print('ok')"],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        stderr_text = common.drain(child.stderr)
        self.assertEqual(child.stdout.read(), "ok\n")
        child.wait()
        self.assertEqual(stderr_text(), "x" * 262144)


if __name__ == "__main__":
    unittest.main()
