#!/usr/bin/env python3
"""Adapter for pi (`pi -p --mode json`): run one benchmark trial and report per-turn usage.

Reads the prompt on stdin and honors the BENCH_* contract (docs/agent-benchmark.md). BENCH_MODEL is required
(`provider/id`, see `pi --list-models`); BENCH_THINKING optionally sets `--thinking`. Discovery of context files,
extensions, prompt templates, themes, and skills is disabled; BENCH_SKILL_DIRS (one directory per installed skill) are loaded explicitly. A provider that
is registered by an extension (for example Gemini through pi-antigravity) needs BENCH_PI_EXTENSIONS, a comma-separated
list of extension paths. Web tools come from BENCH_PI_WEB_EXTENSIONS (for example pi-web-access), loaded only when
knowledge is `web`. Network `off` is not enforced, because the shell can still reach the network: use the harness
--canary-cmd to check it. Exit 75 marks a provider or infrastructure failure for a retry.
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
SCALE = {"K": 1_000, "M": 1_000_000}


def context_limit(pi: str, model: str, env: dict, extensions: list[str]) -> int | None:
    """Context window of the model from `pi --list-models`, or None."""
    command = [pi, "--no-extensions"]
    for extension in filter(None, extensions):
        command += ["-e", extension]
    try:
        listing = subprocess.run([*command, "--list-models", model.split("/")[-1]], env=env, capture_output=True, text=True, timeout=30).stdout
    except (OSError, subprocess.TimeoutExpired):
        return None
    for line in listing.splitlines():
        cols = line.split()
        if len(cols) > 2 and cols[0] == model.split("/")[0] and cols[1] == model.split("/")[-1]:
            match = re.fullmatch(r"([\d.]+)([KM])", cols[2])
            return int(float(match.group(1)) * SCALE[match.group(2)]) if match else None
    return None


def skill_names(system_message: dict) -> list[str]:
    return re.findall(r"<name>([^<]+)</name>", (system_message.get("sections") or {}).get("skills", ""))


def usage_row(message: dict, limit: int | None, now: float) -> dict:
    u = message.get("usage") or {}
    read, write = u.get("cacheRead") or 0, u.get("cacheWrite") or 0
    return {
        "t": now, "input": u.get("input"), "output": max((u.get("output") or 0) - (u.get("reasoning") or 0), 0), "cache_read": read, "cache_write": write,
        "reasoning": u.get("reasoning"), "context": (u.get("input") or 0) + read + write, "context_limit": limit,
    }


def message_text(message: dict) -> str:
    return "".join(part.get("text", "") for part in message.get("content", []) if isinstance(part, dict) and part.get("type") == "text")


def main() -> int:
    env = os.environ
    prompt = sys.stdin.read()
    pi = shutil.which("pi", path=env.get("BENCH_HOST_PATH")) or "pi"
    model = env.get("BENCH_MODEL") or sys.exit("BENCH_MODEL is required: set it in --agent-cmd, for example BENCH_MODEL=provider/id python3 adapters/pi.py")
    home = Path(env["BENCH_RUN_DIR"]) / "pi-home"
    state = home / ".pi/agent"
    state.mkdir(parents=True, exist_ok=True)
    for name in ("auth.json", "antigravity-accounts.json"):
        source = Path(env["HOME"]) / ".pi/agent" / name
        if source.is_file():
            shutil.copy(source, state / name)
    cmd = [pi, "-p", "--mode", "json", "--no-session", "--no-context-files", "--no-extensions", "--no-prompt-templates",
           "--no-themes", "--no-skills", "--tools", "read,bash,edit,write", "--model", model]
    extensions = env.get("BENCH_PI_EXTENSIONS", "").split(",") + (env.get("BENCH_PI_WEB_EXTENSIONS", "").split(",") if env["BENCH_KNOWLEDGE"] == "web" else [])
    for extension in filter(None, extensions):
        cmd += ["-e", extension]
    for skill in (p for p in env.get("BENCH_SKILL_DIRS", "").split(os.pathsep) if p):
        cmd += ["--skill", skill]
    if env.get("BENCH_THINKING"):
        cmd += ["--thinking", env["BENCH_THINKING"]]
    child_env = {**{k: v for k, v in env.items() if k != "BENCH_HOST_PATH"}, "HOME": str(home), "PI_CODING_AGENT_DIR": str(state)}
    limit = context_limit(pi, model, child_env, extensions)
    Path(env["BENCH_AGENT_INFO"]).write_text(json.dumps({"agent": "pi", "version": cli_version(pi), "model": model, "effort": env.get("BENCH_THINKING"), "tools": ["read", "bash", "edit", "write"], "extensions": [e for e in extensions if e]}, indent=2))
    proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=child_env)
    stderr_text = drain(proc.stderr)
    feed_stdin(proc, prompt)
    loaded: list[str] = []
    final, error = "", ""
    with open(env["BENCH_USAGE"], "w", buffering=1) as usage, open(env["BENCH_TRANSCRIPT"], "w", buffering=1) as transcript:  # line-buffered: a killed run keeps its usage
        for line in proc.stdout:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if not isinstance(event, dict):
                continue
            now = time.time()
            message, etype = as_dict(event.get("message")), event.get("type")
            if etype != "message_update":
                transcript.write(json.dumps({"t": now, **event}) + "\n")
            if etype == "message_start" and message.get("role") == "system":
                loaded = skill_names(message)
            if etype == "message_end" and message.get("role") == "assistant":
                usage.write(json.dumps(usage_row(message, limit, now)) + "\n")
                final = message_text(message) or final
                if message.get("stopReason") == "error":
                    error = str(message.get("errorMessage") or "error")  # only the structured error classifies; message text is the agent's own
                else:
                    error = ""
    stderr = stderr_text()
    code = proc.wait()
    Path(env["BENCH_CONTEXT"]).write_text(json.dumps({"loaded": loaded}))
    sys.stdout.write(final)
    sys.stderr.write(stderr)
    if error or code != 0:
        return INFRA_EXIT if any(s in (error + stderr).lower() for s in TRANSIENT) else (code or 1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
