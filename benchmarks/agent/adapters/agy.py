#!/usr/bin/env python3
"""Antigravity print adapter: isolated HOME and per-step streaming usage."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path


def usage_row(usage: dict, timestamp: float) -> dict:
    cached = usage.get("cache_read_tokens") or 0
    thinking = usage.get("thinking_tokens") or 0
    return {"t": timestamp, "input": usage.get("input_tokens"),
            "output": max((usage.get("output_tokens") or 0) - thinking, 0),
            "cache_read": cached, "cache_write": None, "reasoning": thinking,
            "context": (usage.get("input_tokens") or 0) + cached, "context_limit": None}


def main() -> int:
    env = os.environ
    run = Path(env["BENCH_RUN_DIR"])
    home = run / "agy-home"
    state = home / ".gemini/antigravity-cli"
    state.mkdir(parents=True, exist_ok=True)
    for name in ("antigravity-oauth-token", "installation_id"):
        shutil.copy(Path(env["HOME"]) / ".gemini/antigravity-cli" / name, state / name)
    deny = ["mcp(*)", "execute_url(*)"]
    if env["BENCH_KNOWLEDGE"] != "web":
        deny.append("read_url(*)")
    (state / "settings.json").write_text(json.dumps({"allowNonWorkspaceAccess": True, "permissions": {"allow": ["command(*)", "read_file(*)"], "deny": deny}}))
    installed = []
    for key in ("BENCH_SKILL_DIR", "BENCH_WIKI_SKILL_DIR"):
        if env.get(key):
            skill = Path(env[key])
            shutil.copytree(skill, Path.cwd() / ".agents/skills" / skill.name)
            installed.append(skill.name)
    binary = shutil.which("agy", path=env.get("BENCH_HOST_PATH")) or "agy"
    command = [binary, "--model", env["BENCH_MODEL"], "--effort", env["BENCH_THINKING"],
               "--output-format", "stream-json", "--print-timeout", "0", "--print", sys.stdin.read()]
    child_env = {**{k: v for k, v in env.items() if k != "BENCH_HOST_PATH"}, "HOME": str(home)}
    seen, result, init, unexpected = set(), {}, {}, set()
    with (run / "agy-stderr.log").open("w") as stderr, open(env["BENCH_USAGE"], "w", buffering=1) as usage, open(env["BENCH_TRANSCRIPT"], "w", buffering=1) as transcript:
        process = subprocess.Popen(command, env=child_env, stdout=subprocess.PIPE, stderr=stderr, text=True)
        for line in process.stdout:
            event = json.loads(line)
            now = time.time()
            transcript.write(json.dumps({"t": now, **event}) + "\n")
            if event["event"] == "init":
                init = event["init"]
            step = event.get("step_update") or {}
            tool = step.get("tool_name", "")
            if env["BENCH_NETWORK"] == "off" and tool in {"search_web", "read_url_content", "browser_subagent", "open_browser_url"}:
                unexpected.add("network-tool:" + tool)
                process.terminate()

            if step.get("state") == "DONE" and step.get("usage") and step["step_index"] not in seen:
                seen.add(step["step_index"])
                usage.write(json.dumps(usage_row(step["usage"], now)) + "\n")
            if event["event"] == "result":
                result = event["result"]
        code = process.wait()
    Path(env["BENCH_CONTEXT"]).write_text(json.dumps({"installed": installed, "audit": "isolated-home; CLI does not export loaded skill context", "unexpected": sorted(unexpected)}))
    (run / "adapter.json").write_text(json.dumps({"agent": "agy", "requestedModel": env["BENCH_MODEL"], "requestedEffort": env["BENCH_THINKING"], "observedModel": init.get("model"), "status": result.get("status"), "usageSource": "stream-step-usage", "providerUsage": result.get("usage")}, indent=2))
    sys.stdout.write(result.get("response", ""))
    stderr_text = (run / "agy-stderr.log").read_text()
    sys.stderr.write(stderr_text)
    error = str(result.get("error", "")) + stderr_text
    if code or result.get("status") != "SUCCESS":
        return 75 if any(s in error.lower() for s in ("quota", "rate limit", "429", "temporarily", "503", "credits")) else (code or 1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
