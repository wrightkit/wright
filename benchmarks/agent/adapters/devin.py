#!/usr/bin/env python3
"""Adapter for the Devin CLI (`devin -p`): run one benchmark trial and report per-turn usage.

Reads the prompt on stdin and honors the BENCH_* contract (docs/agent-benchmark.md). BENCH_MODEL is required
(for example `swe-2-max`, see `devin models list`). The run uses an isolated HOME that contains only the Devin
credentials copied from the HOME the harness passed (`--env-pass HOME`), and a config that reads no other tools'
rules or skills. Managed plugin skills are reported separately; --file-sandbox blocks outside-workspace
instruction files. BENCH_SKILL_DIRS (one directory per installed skill) are installed as project skills in the workspace. Web tools are denied unless knowledge is `web`; the shell can
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

from common import cli_version, INFRA_EXIT, TRANSIENT
WEB_TOOLS = ["WebFetch", "WebSearch", "webfetch", "web_search"]
MCP_TOOLS = ["mcp_call_tool", "mcp_list_tools", "mcp_list_servers", "mcp_read_resource"]  # org-managed plugins install MCP servers
NO_TOOL_CONFIG = {"claude": False, "cursor": False, "windsurf": False, "codex": False}


EFFORTS = ("low", "medium", "high", "xhigh", "max")


def split_effort(model_id: str) -> tuple[str, str | None]:
    """Devin names the effort in the model id (`swe-2-max` is model `swe-2` at effort `max`), so the CLI never reports it separately."""
    for effort in EFFORTS:
        if model_id.endswith(f"-{effort}"):
            return model_id[: -len(effort) - 1], effort
    return model_id, None


def isolated_config(user_config: dict, model: str, web: bool) -> dict:
    config = copy.deepcopy(user_config)
    config["read_config_from"] = NO_TOOL_CONFIG
    config["permissions"] = {"deny": MCP_TOOLS + ([] if web else WEB_TOOLS)}
    # A denied call ends a headless session, so the tools are also hidden from the agent: it never reaches for what the condition withholds.
    config["disabled_tools"] = MCP_TOOLS + ([] if web else [t for t in WEB_TOOLS if t.islower()])
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


def transient(stdout: str, stderr: str) -> bool:
    """A provider or infrastructure failure. The agent's own text on stdout can mimic provider wording, so only stderr and the
    CLI's anchored model-catalog error count. An empty catalog means the catalog could not be fetched, not that the model is wrong."""
    return any(s in stderr.lower() for s in TRANSIENT) or re.search(r"unknown model.*\navailable:\s*$", (stdout + stderr).lower().strip(), re.S) is not None


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
    for skill in (Path(p) for p in env.get("BENCH_SKILL_DIRS", "").split(os.pathsep) if p):
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
    exported = json.loads(export.read_text()) if export.is_file() else {}
    Path(env["BENCH_AGENT_INFO"]).write_text(json.dumps({"agent": "devin", "version": cli_version(devin), "model": split_effort(model)[0], "effort": split_effort(model)[1], "modelId": model, "tools": [t["function"]["name"] for t in exported.get("agent", {}).get("tool_definitions", [])], "denied": MCP_TOOLS + ([] if env["BENCH_KNOWLEDGE"] == "web" else WEB_TOOLS)}, indent=2))
    Path(env["BENCH_CONTEXT"]).write_text(json.dumps({"loaded": loaded, "ignoredPluginSkills": plugins}))
    sys.stdout.write(proc.stdout)
    sys.stderr.write(proc.stderr)
    if proc.returncode != 0:
        return INFRA_EXIT if transient(proc.stdout, proc.stderr) else proc.returncode
    return 0


if __name__ == "__main__":
    sys.exit(main())
