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

from common import as_dict, cli_version, INFRA_EXIT, TRANSIENT


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
    (state / "settings.json").write_text(json.dumps({"allowNonWorkspaceAccess": True, "permissions": {"allow": ["command(*)", "read_file(*)", f"write_file({Path.cwd()})"], "deny": deny}}))
    installed = []
    for skill in (Path(p) for p in env.get("BENCH_SKILL_DIRS", "").split(os.pathsep) if p):
        shutil.copytree(skill, Path.cwd() / ".agents/skills" / skill.name)
        installed.append(skill.name)
    binary = shutil.which("agy", path=env.get("BENCH_HOST_PATH")) or "agy"
    effort = env.get("BENCH_THINKING")
    command = [binary, "--model", env["BENCH_MODEL"], *(["--effort", effort] if effort else []),
               "--output-format", "stream-json", "--print-timeout", "0", "--print", sys.stdin.read()]
    child_env = {**{k: v for k, v in env.items() if k != "BENCH_HOST_PATH"}, "HOME": str(home)}
    seen, result, init, unexpected = set(), {}, {}, set()
    with (run / "agy-stderr.log").open("w") as stderr, open(env["BENCH_USAGE"], "w", buffering=1) as usage, open(env["BENCH_TRANSCRIPT"], "w", buffering=1) as transcript:
        process = subprocess.Popen(command, env=child_env, stdout=subprocess.PIPE, stderr=stderr, text=True)
        for line in process.stdout:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if not isinstance(event, dict):
                continue
            now = time.time()
            transcript.write(json.dumps({"t": now, **event}) + "\n")
            if event.get("event") == "init":
                init = event.get("init") or {}
            step = as_dict(event.get("step_update"))
            tool = step.get("tool_name", "")
            if env["BENCH_NETWORK"] == "off" and tool in {"search_web", "read_url_content", "browser_subagent", "open_browser_url"}:
                unexpected.add("network-tool:" + tool)
                process.terminate()

            if step.get("state") == "DONE" and step.get("usage") and (index := step.get("step_index")) is not None and index not in seen:
                seen.add(index)
                usage.write(json.dumps(usage_row(step["usage"], now)) + "\n")
            if event.get("event") == "result":
                result = as_dict(event.get("result"))
        code = process.wait()
    Path(env["BENCH_CONTEXT"]).write_text(json.dumps({"installed": installed, "audit": "isolated-home; CLI does not export loaded skill context", "unexpected": sorted(unexpected)}))
    Path(env["BENCH_AGENT_INFO"]).write_text(json.dumps({"agent": "agy", "version": cli_version(binary), "model": env["BENCH_MODEL"], "effort": effort, "tools": None, "note": "the CLI does not expose its tool list"}, indent=2))
    (run / "adapter.json").write_text(json.dumps({"agent": "agy", "requestedModel": env["BENCH_MODEL"], "requestedEffort": effort, "observedModel": init.get("model"), "status": result.get("status"), "usageSource": "stream-step-usage", "providerUsage": result.get("usage")}, indent=2))
    sys.stdout.write(result.get("response", ""))
    stderr_text = (run / "agy-stderr.log").read_text()
    sys.stderr.write(stderr_text)
    error = str(result.get("error", "")) + stderr_text
    if "no output produced" in stderr_text and "headless" in stderr_text:
        return INFRA_EXIT
    if code or result.get("status") != "SUCCESS":
        return INFRA_EXIT if any(s in error.lower() for s in TRANSIENT) else (code or 1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
