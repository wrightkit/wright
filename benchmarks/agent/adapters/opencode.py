#!/usr/bin/env python3
"""Adapter for opencode (`opencode run`): run one benchmark trial and report per-turn usage.

BENCH_MODEL is `provider/model` as `opencode models` lists it, and must be set in --agent-cmd. The run uses an isolated HOME with
only the opencode credentials copied in, so no global instructions, agents, skills, or plugins load (`--pure`). Skills are installed in the
workspace under `.opencode/skills`; opencode also reads the real home's `~/.claude` and `~/.agents` skills and Claude Code instructions regardless of HOME,
so those scans are disabled by environment variable. Web tools are denied unless knowledge is `web`. BENCH_THINKING is passed as the model variant.
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

from common import as_dict, cli_version, feed_stdin, INFRA_EXIT, TRANSIENT
WEB_PERMISSIONS = {"webfetch": "deny", "websearch": "deny"}


def usage_row(tokens: dict, now: float) -> dict:
    cache = tokens.get("cache") or {}
    read, write = cache.get("read") or 0, cache.get("write") or 0
    return {"t": now, "input": tokens.get("input"), "output": tokens.get("output"), "cache_read": read, "cache_write": write,
            "reasoning": tokens.get("reasoning"), "context": (tokens.get("input") or 0) + read + write, "context_limit": None}


def available_skills(raw: str) -> list[str]:
    """Skill names opencode reports, without its built-in ones."""
    try:
        return [s["name"] for s in json.loads(raw) if s.get("location") != "<built-in>"]
    except (json.JSONDecodeError, TypeError):
        return []


def main() -> int:
    env = os.environ
    model = env.get("BENCH_MODEL") or sys.exit("BENCH_MODEL is required: set it in --agent-cmd, for example BENCH_MODEL=openai/gpt-6-luna python3 adapters/opencode.py")
    run_dir, workspace, real_home = Path(env["BENCH_RUN_DIR"]), Path.cwd(), Path(env["HOME"])
    home = run_dir / "opencode-home"
    (home / ".local/share/opencode").mkdir(parents=True)
    (home / ".config/opencode").mkdir(parents=True)
    shutil.copy(real_home / ".local/share/opencode/auth.json", home / ".local/share/opencode/auth.json")
    for skill in (Path(p) for p in env.get("BENCH_SKILL_DIRS", "").split(os.pathsep) if p):
        shutil.copytree(skill, workspace / ".opencode/skills" / skill.name)
    web = env["BENCH_KNOWLEDGE"] == "web"
    opencode = shutil.which("opencode", path=env.get("BENCH_HOST_PATH")) or "opencode"
    child_env = {**{k: v for k, v in env.items() if k != "BENCH_HOST_PATH"}, "HOME": str(home), "XDG_CONFIG_HOME": str(home / ".config"), "XDG_DATA_HOME": str(home / ".local/share"),
                 "OPENCODE_DISABLE_CLAUDE_CODE": "1", "OPENCODE_DISABLE_EXTERNAL_SKILLS": "1",
                 "OPENCODE_CONFIG_CONTENT": json.dumps({"$schema": "https://opencode.ai/config.json", "autoupdate": False, "share": "disabled", **({} if web else {"permission": WEB_PERMISSIONS})})}
    version = cli_version(opencode, child_env)
    listing = run_dir / "opencode-skills.json"  # a file, not a pipe: `debug skill` truncates large output when stdout is a pipe
    with open(listing, "w") as out:
        subprocess.run([opencode, "debug", "skill"], stdout=out, env=child_env)
    loaded = available_skills(listing.read_text())
    cmd = [opencode, "run", "--pure", "--format", "json", "--auto", "-m", model]
    if env.get("BENCH_THINKING"):
        cmd += ["--variant", env["BENCH_THINKING"]]
    proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=child_env)
    feed_stdin(proc, sys.stdin.read())
    final, error, tools = "", "", set()
    with open(env["BENCH_USAGE"], "w", buffering=1) as usage, open(env["BENCH_TRANSCRIPT"], "w", buffering=1) as transcript:  # line-buffered: a killed run keeps its usage
        for line in proc.stdout:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if not isinstance(event, dict):
                continue
            now, part, etype = time.time(), as_dict(event.get("part")), event.get("type")
            transcript.write(json.dumps({"t": now, **event}) + "\n")
            if etype == "step_finish" and part.get("tokens"):
                usage.write(json.dumps(usage_row(part["tokens"], now)) + "\n")
            elif etype == "text":
                final = part.get("text") or final
            elif etype == "tool_use" and part.get("tool"):
                tools.add(part["tool"])
            elif etype == "error":
                error = json.dumps(event.get("error"))
    stderr = proc.stderr.read()
    code = proc.wait()
    Path(env["BENCH_CONTEXT"]).write_text(json.dumps({"loaded": loaded}))
    Path(env["BENCH_AGENT_INFO"]).write_text(json.dumps({"agent": "opencode", "version": version, "model": model, "effort": env.get("BENCH_THINKING"), "tools": sorted(tools), "toolsNote": "tools the agent used; the CLI does not list its tools", "denied": [] if web else sorted(WEB_PERMISSIONS)}, indent=2))
    sys.stdout.write(final)
    sys.stderr.write(stderr)
    if error or code != 0:
        return INFRA_EXIT if any(s in (error + stderr).lower() for s in TRANSIENT) else (code or 1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
