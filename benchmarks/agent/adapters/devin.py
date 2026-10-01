#!/usr/bin/env python3
"""Adapter for the Devin CLI (`devin -p`): run one benchmark trial and report per-turn usage.

Reads the prompt on stdin and honors the BENCH_* contract (docs/agent-benchmark.md). BENCH_MODEL is required
(for example `swe-2-max`, see `devin models list`). The run uses an isolated HOME that contains only the Devin
credentials copied from the HOME the harness passed (`--env-pass HOME`), and a config that reads no other tools'
rules or skills, so the user's global skills, plugins, and instruction files do not load. BENCH_SKILL_DIR and
BENCH_WIKI_SKILL_DIR are installed as project skills in the workspace. Web tools are denied unless knowledge is `web`; the shell can
still reach the network, so network `off` is not enforced: use the harness --canary-cmd to check it.
Set BENCH_DEVIN_SANDBOX=1 to add `--sandbox`. Exit 75 marks a provider or infrastructure failure for a retry.
"""

from __future__ import annotations

import copy
import json
import os
import re
import shutil
import subprocess
import sys
from datetime import datetime
from pathlib import Path

INFRA_EXIT = 75
TRANSIENT = ("rate limit", "overloaded", "429", "503", "529", "timed out", "timeout", "temporarily", "usage limit", "quota", "insufficient credits", "fetch failed", "websocket error", "connection error")
WEB_TOOLS = ["WebFetch", "WebSearch", "webfetch", "web_search"]
MCP_TOOLS = ["mcp_call_tool", "mcp_list_tools", "mcp_list_servers", "mcp_read_resource"]  # org-managed plugins install MCP servers
NO_TOOL_CONFIG = {"claude": False, "cursor": False, "windsurf": False, "codex": False}


def isolated_config(user_config: dict, model: str, web: bool) -> dict:
    config = copy.deepcopy(user_config)
    config["read_config_from"] = NO_TOOL_CONFIG
    config["permissions"] = {"deny": MCP_TOOLS + ([] if web else WEB_TOOLS)}
    config.setdefault("agent", {})["model"] = model
    return config


def parse_export(export: dict) -> tuple[list[dict], list[str], list[str]]:
    """Usage rows (one per agent step with metrics), loaded skill and rule names, and plugin skill names.

    Built-in skills are not reported. Plugin skills come from the account's managed plugins, which cannot be
    switched off from a config; their MCP tools are denied, so they are listed apart from `loaded`."""
    rows = []
    for step in export.get("steps", []):
        metrics = step.get("metrics")
        if step.get("source") == "agent" and metrics:
            cached = metrics.get("cached_tokens") or 0
            prompt = metrics.get("prompt_tokens") or 0
            rows.append({
                "t": datetime.fromisoformat(step["timestamp"]).timestamp(), "input": max(prompt - cached, 0),
                "output": metrics.get("completion_tokens"), "cache_read": cached, "cache_write": None,
                "reasoning": None, "context": prompt, "context_limit": None,
            })
    loaded, plugins = [], []
    for step in export.get("steps", []):
        message = step.get("message") or ""
        if step.get("source") != "system":
            continue
        loaded += re.findall(r'<rule name="([^"]+)"', message)
        for name, source in re.findall(r"^- \*\*([^*]+)\*\*:.*?\(source: ([^)]*)\)\s*$", message, re.M):
            if source.startswith("builtin:") or "/share/devin/docs" in source:
                continue
            (plugins if "/plugins/cache/" in source else loaded).append(name)
    return rows, loaded, plugins


def main() -> int:
    env = os.environ
    model = env.get("BENCH_MODEL") or sys.exit("BENCH_MODEL is required: set it in --agent-cmd, for example BENCH_MODEL=swe-2-max python3 adapters/devin.py")
    run_dir, workspace = Path(env["BENCH_RUN_DIR"]), Path.cwd()
    real_home = Path(env["HOME"])
    home = run_dir / "devin-home"
    (home / ".local/share/devin").mkdir(parents=True)
    shutil.copy(real_home / ".local/share/devin/credentials.toml", home / ".local/share/devin/credentials.toml")
    config, export, prompt = run_dir / "devin-config.json", run_dir / "devin-export.json", run_dir / "devin-prompt.txt"
    config.write_text(json.dumps(isolated_config(json.loads((real_home / ".config/devin/config.json").read_text()), model, env["BENCH_KNOWLEDGE"] == "web")))
    prompt.write_text(sys.stdin.read())
    for key in ("BENCH_SKILL_DIR", "BENCH_WIKI_SKILL_DIR"):
        if env.get(key):
            skill = Path(env[key])
            shutil.copytree(skill, workspace / ".agents/skills" / skill.name)
    devin = shutil.which("devin", path=env.get("BENCH_HOST_PATH")) or "devin"
    cmd = [devin, "--config", str(config), "--model", model, "--respect-workspace-trust", "false",
           "--permission-mode", "dangerous", "--export", str(export), "--prompt-file", str(prompt), "-p"]
    if env.get("BENCH_DEVIN_SANDBOX") == "1":
        cmd.insert(1, "--sandbox")
    child_env = {**{k: v for k, v in env.items() if k != "BENCH_HOST_PATH"}, "HOME": str(home)}
    proc = subprocess.run(cmd, capture_output=True, text=True, env=child_env)
    rows, loaded, plugins = parse_export(json.loads(export.read_text())) if export.is_file() else ([], [], [])
    Path(env["BENCH_USAGE"]).write_text("".join(json.dumps(r) + "\n" for r in rows))
    Path(env["BENCH_TRANSCRIPT"]).write_text("".join(json.dumps(s) + "\n" for s in (json.loads(export.read_text())["steps"] if export.is_file() else [])))
    Path(env["BENCH_CONTEXT"]).write_text(json.dumps({"loaded": loaded, "ignoredPluginSkills": plugins}))
    sys.stdout.write(proc.stdout)
    sys.stderr.write(proc.stderr)
    if proc.returncode != 0:
        return INFRA_EXIT if any(s in (proc.stdout + proc.stderr).lower() for s in TRANSIENT) else proc.returncode
    return 0


if __name__ == "__main__":
    sys.exit(main())
