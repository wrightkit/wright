#!/usr/bin/env python3
"""Codex exec adapter with an isolated HOME, session usage, and observed skill context."""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

from common import cli_version


def usage_row(usage: dict, timestamp: float, limit: int | None) -> dict:
    cached = usage.get("cached_input_tokens") or 0
    reasoning = usage.get("reasoning_output_tokens") or 0
    return {"t": timestamp, "input": max((usage.get("input_tokens") or 0) - cached, 0),
            "output": max((usage.get("output_tokens") or 0) - reasoning, 0),
            "cache_read": cached, "cache_write": usage.get("cache_write_input_tokens"),
            "reasoning": reasoning, "context": usage.get("input_tokens"), "context_limit": limit}


def loaded_skills(text: str) -> tuple[list[str], list[str]]:
    roots = dict(re.findall(r"^- `([^`]+)` = `([^`]+)`", text, re.M))
    custom, builtin = [], []
    for name, path in re.findall(r"^- ([^:\n]+): .*?\(file: ([^)]+)\)$", text, re.M):
        alias, _, relative = path.partition("/")
        resolved = str(Path(roots.get(alias, alias)) / relative)
        (builtin if "/skills/.system/" in resolved else custom).append(name)
    return custom, builtin


def session_usage(state: Path):
    for path in state.glob("sessions/**/*.jsonl"):
        for line in path.read_text().splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            payload = event.get("payload") or {}
            info = payload.get("info")
            if payload.get("type") == "token_count" and info:
                yield json.dumps(info["total_token_usage"], sort_keys=True), usage_row(info["last_token_usage"], datetime.fromisoformat(event["timestamp"]).timestamp(), info.get("model_context_window"))


def main() -> int:
    env = os.environ
    model = env["BENCH_MODEL"]
    effort = env["BENCH_THINKING"]
    run = Path(env["BENCH_RUN_DIR"])
    home = run / "codex-home"
    state = home / ".codex"
    state.mkdir(parents=True, exist_ok=True)
    shutil.copy(Path(env["HOME"]) / ".codex/auth.json", state / "auth.json")
    for skill in (Path(p) for p in env.get("BENCH_SKILL_DIRS", "").split(os.pathsep) if p):
        shutil.copytree(skill, Path.cwd() / ".agents/skills" / skill.name)
    binary = shutil.which("codex", path=env.get("BENCH_HOST_PATH")) or "codex"
    # Codex's own seatbelt cannot be applied inside the harness --file-sandbox (macOS refuses nested sandboxes), so it is off
    # here: the harness sandbox is the file-write boundary, and network `off` is declared-only for this adapter.
    command = [binary, "exec", "--json", "--ignore-user-config", "--ignore-rules", "--skip-git-repo-check",
               "--disable", "apps", "--disable", "plugins", "--disable", "remote_plugin",
               "--disable", "skill_mcp_dependency_install",
               "--sandbox", "danger-full-access", "-c", 'approval_policy="never"',
               "-c", 'web_search="disabled"', "-c", f'model_reasoning_effort="{effort}"', "-m", model, "-"]
    child_env = {**{k: v for k, v in env.items() if k != "BENCH_HOST_PATH"}, "HOME": str(home), "CODEX_HOME": str(state)}
    prompt = sys.stdin.read()
    final, errors, summary, seen, servers, item_types = "", [], None, set(), set(), set()
    with (run / "codex-stderr.log").open("w") as stderr, open(env["BENCH_TRANSCRIPT"], "w", buffering=1) as transcript, open(env["BENCH_USAGE"], "w", buffering=1) as usage:
        process = subprocess.Popen(command, env=child_env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr, text=True)
        process.stdin.write(prompt)
        process.stdin.close()
        for line in process.stdout:
            event = json.loads(line)
            transcript.write(json.dumps({"t": time.time(), **event}) + "\n")
            item = event.get("item") or {}
            item_types.add(item.get("type"))
            if item.get("type") == "mcp_tool_call":
                servers.add(item.get("server", "unknown"))
            if item.get("type") == "agent_message":
                final = item.get("text", final)
            if event["type"] in ("error", "turn.failed"):
                errors.append(str(event.get("message") or event.get("error")))
            if event["type"] == "turn.completed":
                summary = event.get("usage")
            for key, row in session_usage(state):
                if key not in seen:
                    seen.add(key)
                    usage.write(json.dumps(row) + "\n")
        code = process.wait()
        for key, row in session_usage(state):
            if key not in seen:
                seen.add(key)
                usage.write(json.dumps(row) + "\n")
        if not seen and summary:
            row = usage_row(summary, time.time(), None)
            row["context"] = None
            usage.write(json.dumps(row) + "\n")
    loaded, builtin, observed = [], [], {}
    for path in state.glob("sessions/**/*.jsonl"):
        for line in path.read_text().splitlines():
            event = json.loads(line)
            payload = event.get("payload") or {}
            if event["type"] == "turn_context":
                observed = {k: payload.get(k) for k in ("model", "effort")}
            if event["type"] == "response_item" and payload.get("role") == "developer":
                for part in payload.get("content", []):
                    names, builtins = loaded_skills(part.get("text", ""))
                    loaded += names
                    builtin += builtins
    Path(env["BENCH_CONTEXT"]).write_text(json.dumps({"loaded": sorted(set(loaded) | {f"unexpected-mcp:{server}" for server in servers}), "builtinSkills": sorted(set(builtin))}))
    Path(env["BENCH_AGENT_INFO"]).write_text(json.dumps({"agent": "codex", "version": cli_version(binary), "model": observed.get("model") or model, "effort": observed.get("effort") or effort, "sandbox": "danger-full-access inside the harness file sandbox", "toolsObserved": sorted(t for t in item_types if t)}, indent=2))
    (run / "adapter.json").write_text(json.dumps({"agent": "codex", "requestedModel": model, "requestedEffort": effort, "observed": observed, "usageSource": "session-token-count" if observed else "turn-summary"}, indent=2))
    sys.stdout.write(final)
    stderr_text = (run / "codex-stderr.log").read_text()
    sys.stderr.write(stderr_text)
    failure = " ".join(errors) + stderr_text
    if code or errors:
        return 75 if any(s in failure.lower() for s in ("rate limit", "usage limit", "429", "quota", "temporarily", "503")) else (code or 1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
