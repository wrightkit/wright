#!/usr/bin/env python3
"""Reference adapter: run Claude Code for one benchmark trial and report per-turn usage.

Reads the prompt on stdin. Honors the BENCH_* contract (docs/agent-benchmark.md): tools follow BENCH_KNOWLEDGE,
BENCH_SKILL_DIRS (one directory per installed skill) are installed as plugin skills, and BENCH_USAGE / BENCH_TRANSCRIPT / BENCH_CONTEXT are written.
The agent binary is resolved on BENCH_HOST_PATH because the agent's own PATH hides Wright when the level is `none`.
The model comes from BENCH_MODEL (default `sonnet`). It removes web tools unless knowledge is `web`, but it does
not sandbox the network: pair it with the harness --canary-cmd to detect a reachable network under `off`.
Exit 75 signals a provider or infrastructure failure so the harness retries the trial.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

from common import as_dict, cli_version, drain, feed_stdin, INFRA_EXIT, TRANSIENT

TOOLS = ["Bash", "Read", "Edit", "Write", "Glob", "Grep"]
WEB_TOOLS = ["WebFetch", "WebSearch"]


def main() -> int:
    env = os.environ
    prompt = sys.stdin.read()
    web = env["BENCH_KNOWLEDGE"] == "web"
    cmd = [
        shutil.which("claude", path=env.get("BENCH_HOST_PATH")) or "claude", "-p", "--model", env.get("BENCH_MODEL", "sonnet"),
        "--output-format", "stream-json", "--verbose", "--setting-sources", "project", "--strict-mcp-config",
        "--no-session-persistence", "--permission-mode", "acceptEdits",
        "--allowedTools", *(TOOLS + WEB_TOOLS if web else TOOLS),
    ]
    if not web:
        cmd += ["--disallowedTools", *WEB_TOOLS]
    loaded: list[str] = []
    skills = [Path(p) for p in env.get("BENCH_SKILL_DIRS", "").split(os.pathsep) if p]
    if skills:
        plugin = Path(env["BENCH_RUN_DIR"]) / "claude-plugin"  # a fixed name: an infra retry would else orphan a mkdtemp dir
        shutil.rmtree(plugin, ignore_errors=True)
        (plugin / ".claude-plugin").mkdir(parents=True)
        (plugin / ".claude-plugin/plugin.json").write_text(json.dumps({"name": "bench", "version": "0.0.0", "description": "benchmark skills"}))
        for skill in skills:
            shutil.copytree(skill, plugin / "skills" / skill.name)
            match = re.search(r"^name:\s*(\S+)", (skill / "SKILL.md").read_text(), re.M)  # the name the harness checks, not the directory name
            loaded.append(match.group(1) if match else skill.name)
        cmd += ["--plugin-dir", str(plugin)]
    claude = cmd[0]
    init: dict = {}
    proc = subprocess.Popen(
        cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        env={**{k: v for k, v in env.items() if k != "BENCH_HOST_PATH"}, "CLAUDE_CODE_DISABLE_CLAUDE_MDS": "1"},
    )
    feed_stdin(proc, prompt)
    stderr_text = drain(proc.stderr)
    final, errored = "", False
    with open(env["BENCH_USAGE"], "w", buffering=1) as usage, open(env["BENCH_TRANSCRIPT"], "w", buffering=1) as transcript:  # line-buffered: a killed run keeps its usage
        for line in proc.stdout:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if not isinstance(event, dict):
                continue
            transcript.write(json.dumps({"t": time.time(), **event}) + "\n")
            if event.get("type") == "system" and event.get("subtype") == "init":
                init = event
            if event.get("type") == "assistant":
                u = as_dict(event.get("message")).get("usage") or {}
                cached = u.get("cache_read_input_tokens") or 0
                written = u.get("cache_creation_input_tokens") or 0
                usage.write(json.dumps({
                    "t": time.time(), "input": u.get("input_tokens"), "output": u.get("output_tokens"),
                    "cache_read": cached, "cache_write": written, "reasoning": None,
                    "context": (u.get("input_tokens") or 0) + cached + written, "context_limit": None,
                }) + "\n")
            if event.get("type") == "result":
                result_value = event.get("result")
                final, errored = (result_value if isinstance(result_value, str) else final), bool(event.get("is_error"))
    stderr = stderr_text()
    code = proc.wait()
    Path(env["BENCH_CONTEXT"]).write_text(json.dumps({"loaded": loaded}))
    Path(env["BENCH_AGENT_INFO"]).write_text(json.dumps({"agent": "claude-code", "version": cli_version(claude), "model": init.get("model") or env.get("BENCH_MODEL", "sonnet"), "tools": init.get("tools") or TOOLS + (WEB_TOOLS if web else []), "toolsSource": "init" if init.get("tools") else "requested", "mcpServers": init.get("mcp_servers")}, indent=2))
    sys.stdout.write(final)
    sys.stderr.write(stderr)
    if errored or code != 0:
        return INFRA_EXIT if any(s in stderr.lower() for s in TRANSIENT) else (code or 1)  # `final` is the agent's own text; only provider channels classify
    return 0


if __name__ == "__main__":
    sys.exit(main())
