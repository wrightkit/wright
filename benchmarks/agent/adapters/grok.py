#!/usr/bin/env python3
"""Adapter for the Grok CLI (`grok -p`): run one benchmark trial and report per-turn usage.

BENCH_MODEL is a model id from `grok models` and must be set in --agent-cmd. The run uses an isolated GROK_HOME that holds only the
login (auth.json) and the benchmark skills, so no global config, rules, skills, or plugins load. The prompt is sent verbatim.
Subagents are disabled so each assistant message is one model call, and web tools are disabled unless knowledge is `web`.
The tools and skills the agent actually had come from the stream's init line. BENCH_THINKING is the reasoning effort.
The network is not sandboxed: pair this with the harness --canary-cmd. Exit 75 marks a provider or infrastructure failure.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

from common import cli_version, INFRA_EXIT, TRANSIENT


def usage_row(usage: dict, limit: int | None, now: float) -> dict:
    read, write = usage.get("cache_read_input_tokens") or 0, usage.get("cache_creation_input_tokens") or 0
    return {"t": now, "input": usage.get("input_tokens"), "output": usage.get("output_tokens"), "cache_read": read, "cache_write": write,
            "reasoning": None, "context": (usage.get("input_tokens") or 0) + read + write, "context_limit": limit}


def main() -> int:
    env = os.environ
    model = env.get("BENCH_MODEL") or sys.exit("BENCH_MODEL is required: set it in --agent-cmd, for example BENCH_MODEL=grok-4.7 python3 adapters/grok.py")
    run_dir, real_home = Path(env["BENCH_RUN_DIR"]), Path(env["HOME"])
    grok_home = run_dir / "grok-home"
    (grok_home / "skills").mkdir(parents=True)
    shutil.copy(real_home / ".grok/auth.json", grok_home / "auth.json")
    for skill in (Path(p) for p in env.get("BENCH_SKILL_DIRS", "").split(os.pathsep) if p):
        shutil.copytree(skill, grok_home / "skills" / skill.name)
    web = env["BENCH_KNOWLEDGE"] == "web"
    grok = shutil.which("grok", path=env.get("BENCH_HOST_PATH")) or "grok"
    child_env = {**{k: v for k, v in env.items() if k != "BENCH_HOST_PATH"}, "GROK_HOME": str(grok_home), "HOME": str(run_dir / "grok-user"), "GROK_TELEMETRY_ENABLED": "0", "GROK_DISABLE_AUTOUPDATER": "1"}
    (run_dir / "grok-user").mkdir()
    prompt = run_dir / "grok-prompt.txt"
    prompt.write_text(sys.stdin.read())
    cmd = [grok, "--verbatim", "--output-format", "streaming-messages-json", "--permission-mode", "bypassPermissions", "--no-subagents", "-m", model, "--prompt-file", str(prompt)]
    if not web:
        cmd.append("--disable-web-search")
    if env.get("BENCH_THINKING"):
        cmd += ["--reasoning-effort", env["BENCH_THINKING"]]
    version = cli_version(grok, child_env)
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=child_env)
    init: dict = {}
    pending: list[dict] = []
    final, error, limit = "", "", None
    with open(env["BENCH_USAGE"], "w", buffering=1) as usage, open(env["BENCH_TRANSCRIPT"], "w", buffering=1) as transcript:  # line-buffered: a killed run keeps its usage
        for line in proc.stdout:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            now = time.time()
            transcript.write(json.dumps({"t": now, **event}) + "\n")
            if event["type"] == "system" and event.get("subtype") == "init":
                init = event
            elif event["type"] == "assistant":
                pending.append(event["message"].get("usage") or {})
                text = "".join(b.get("text", "") for b in event["message"].get("content", []) if b.get("type") == "text")
                final = text or final
            elif event["type"] == "result":
                final = event.get("result") or final
                if event.get("is_error"):
                    error = json.dumps(event.get("errors") or event.get("result") or "error")
                limit = next((m.get("contextWindow") for m in (event.get("modelUsage") or {}).values()), None)
        for u in pending:  # the context window is only known from the final result line
            usage.write(json.dumps(usage_row(u, limit, time.time())) + "\n")
    stderr = proc.stderr.read()
    code = proc.wait()
    Path(env["BENCH_CONTEXT"]).write_text(json.dumps({"loaded": init.get("skills") or []}))
    Path(env["BENCH_AGENT_INFO"]).write_text(json.dumps({"agent": "grok", "version": version, "model": init.get("model") or model, "effort": env.get("BENCH_THINKING"), "tools": init.get("tools"),
                                                          "mcpServers": init.get("mcp_servers"), "disabled": ["subagents"] + ([] if web else ["web"])}, indent=2))
    sys.stdout.write(final)
    sys.stderr.write(stderr)
    if error or code != 0:
        return INFRA_EXIT if any(s in (error + stderr).lower() for s in TRANSIENT) else (code or 1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
