#!/usr/bin/env python3
"""Wright agent benchmark harness (#414). Contract: docs/agent-benchmark.md; design: docs/specs/SPEC-414-agent-benchmark-comparison.md."""

from __future__ import annotations

import argparse
import functools
import hashlib
import json
import os
import platform
import random
import re
import shlex
import shutil
import signal
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
from pathlib import Path

import bench_grade
import bench_leaderboard
import bench_report
import bench_score
import bench_trace
import bench_wiki
import wiki_skill

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
SCENARIOS = HERE / "scenarios"
RESULT_CONTRACT = "wright-agent-bench/v3"
TOOLS = ("none", "wright", "overpy")
SKILLS = ("wright-skill", "workshop-skill", "opy-skill", "workshop-format-skill")
SKILL_LANGUAGE = {"opy-skill": "opy", "workshop-format-skill": "workshop"}  # skills that teach one language and apply to its scenarios only
KNOWLEDGE_LEVELS = ("none", "wiki", "web")
INFRA_EXIT = 75  # EX_TEMPFAIL: the adapter reports a provider or infrastructure failure, not an agent failure
ENV_KEEP = ("LANG", "LC_ALL", "TERM", "TMPDIR", "USER", "LOGNAME")


def load_scenario(scenario_id: str) -> dict:
    directory = SCENARIOS / scenario_id
    scenario = json.loads((directory / "scenario.json").read_text())
    scenario["dir"] = directory
    return scenario


def all_scenario_ids() -> list[str]:
    return sorted(p.name for p in SCENARIOS.iterdir() if (p / "scenario.json").is_file())


def materialize(scenario: dict, workspace: Path, overlay: str | None = None) -> None:
    shutil.copytree(scenario["dir"] / "seed", workspace)
    if overlay:
        shutil.copytree(scenario["dir"] / overlay, workspace, dirs_exist_ok=True)


def validate(wright: str, out: Path) -> bool:
    """Graders are calibrated: the reference passes, the seed fails, each negative fails exactly its named checks."""
    ok = True
    for scenario_id in all_scenario_ids():
        scenario = load_scenario(scenario_id)
        runs = [("seed", None), ("reference", "reference")] + [(f"negative-{n}", f"negative/{n}") for n in scenario.get("negatives", {})]
        results = {}
        for name, overlay in runs:
            workspace = out / scenario_id / name
            shutil.rmtree(workspace, ignore_errors=True)
            materialize(scenario, workspace, overlay)
            results[name] = bench_grade.grade(scenario, workspace, wright)
        problems = []
        if not results["reference"]["passed"]:
            problems.append(f"reference failures={[c['id'] for c in results['reference']['checks'] if not c['passed']]}")
        if results["seed"]["passed"]:
            problems.append("seed passed")
        if results["reference"]["unsafeEdits"]:
            problems.append(f"unsafe={results['reference']['unsafeEdits']}")
        for name, expected in scenario.get("negatives", {}).items():
            failed = sorted(c["id"] for c in results[f"negative-{name}"]["checks"] if not c["passed"])
            if failed != sorted(expected["fails"]):
                problems.append(f"negative '{name}' failed {failed}, expected {sorted(expected['fails'])}")
        if problems:
            ok = False
            print(f"INVALID {scenario_id}: {'; '.join(problems)}")
        else:
            print(f"ok      {scenario_id}")
    return ok


def baseline_path(path: str) -> str:
    """PATH without any directory that provides a benchmark tool (`wright` or `overpy`)."""
    kept = [d for d in path.split(os.pathsep) if d and not any(shutil.which(tool, path=d) for tool in ("wright", "overpy"))]
    return os.pathsep.join(kept)


def normalize_cell(raw: dict) -> dict:
    return {"tool": raw["tool"], "skills": sorted(raw.get("skills") or []), "knowledge": raw["knowledge"], "network": raw["network"]}


def cell_label(cell: dict) -> str:
    """`tool[+skill...]/knowledge/network`, for example `wright+wright-skill/none/off`; the baseline is `none/none/off`."""
    return f"{'+'.join([cell['tool'], *cell['skills']])}/{cell['knowledge']}/{cell['network']}"


def applicable(scenario: dict, cell: dict) -> bool:
    """The `overpy` tool and the language skills apply to their own language only; a raw Workshop scenario has no OverPy cell."""
    if cell["tool"] == "overpy" and scenario["language"] != "opy":
        return False
    return all(SKILL_LANGUAGE.get(s, scenario["language"]) == scenario["language"] for s in cell["skills"])


def skill_name(directory: Path) -> str:
    match = re.search(r"^name:\s*(\S+)", (directory / "SKILL.md").read_text(), re.M)
    return match.group(1) if match else directory.name


def skill_identity(logical: str, directory: Path) -> dict:
    """Name, content hash, and build record (when the skill has one) of an installed skill."""
    record = {"name": logical, "skillName": skill_name(directory), "sha256": wiki_skill.content_hash(directory)}
    build = directory / "BUILD.json"
    if build.is_file():
        record["build"] = json.loads(build.read_text())
        if record["build"].get("skillSha256") != record["sha256"]:
            raise SystemExit(f"skill content mismatch for {logical}: {directory}")
    return record


def overpy_launcher(out: Path, host_path: str) -> Path:
    """A launcher for the pinned OverPy CLI, so the tool shim has a real executable to call."""
    node = shutil.which("node", path=host_path)
    cli = bench_grade.ORACLE / "node_modules/overpy/cli.js"
    if not node or not cli.is_file():
        raise SystemExit("tool 'overpy' requires node and the pinned oracle; run `agent_bench.py setup-oracle`")
    launcher = out / "real" / "overpy"
    launcher.parent.mkdir(exist_ok=True)
    launcher.write_text(f'#!/bin/sh\nexec "{node}" "{cli}" "$@"\n')
    launcher.chmod(0o755)
    return launcher


def check_cell(cell: dict, args: argparse.Namespace) -> None:
    if cell["tool"] not in TOOLS or cell["knowledge"] not in KNOWLEDGE_LEVELS or cell["network"] not in ("off", "on") or any(s not in SKILLS for s in cell["skills"]):
        raise SystemExit(f"invalid condition {cell_label(cell)}")
    if cell["knowledge"] == "web" and cell["network"] != "on":
        raise SystemExit("knowledge 'web' requires network 'on'")
    for name in cell["skills"]:
        directory = (args.skill_dirs or {}).get(name)
        if not directory or not (Path(directory) / "SKILL.md").is_file():
            raise SystemExit(f"skill '{name}' requires --skill {name}=DIR with a SKILL.md")
        skill_identity(name, Path(directory))
    if cell["tool"] == "overpy" and not bench_grade.oracle_available():
        raise SystemExit("tool 'overpy' requires the pinned oracle; run `agent_bench.py setup-oracle`")
    if cell["knowledge"] == "wiki":
        if not (args.wiki_dir and (Path(args.wiki_dir) / "SNAPSHOT.json").is_file()):
            raise SystemExit("knowledge 'wiki' requires --wiki-dir pointing at a snapshot (see `agent_bench.py wiki-snapshot`)")
        bench_wiki.identity(Path(args.wiki_dir))


def build_env(cell: dict, args: argparse.Namespace, out: Path, workspace: Path) -> dict:
    """Scrubbed environment: allowlisted host variables, a fresh HOME, and the BENCH_* contract for the adapter."""
    home = out / "home"
    home.mkdir(parents=True)
    env = {k: os.environ[k] for k in (*ENV_KEEP, *args.env_pass) if k in os.environ}
    env.setdefault("HOME", str(home))
    path = baseline_path(os.environ["PATH"])
    if cell["tool"] != "none":
        shim_dir = out / "bin"
        shim_dir.mkdir()
        shim = shim_dir / cell["tool"]
        shim.write_text(f'#!/bin/sh\nexec "{sys.executable}" "{(HERE / "bench_trace.py").resolve()}" shim {cell["tool"]} "$@"\n')
        shim.chmod(0o755)
        real = args.wright if cell["tool"] == "wright" else str(overpy_launcher(out, os.environ["PATH"]))
        env.update({f"BENCH_TOOL_REAL_{cell['tool'].upper()}": real, "BENCH_TOOL_TRACE": str(out / "tool-trace.jsonl"), "BENCH_TOOL_SIDECAR": str(out / "tool-calls")})
        path = f"{shim_dir}{os.pathsep}{path}"
    env.update(
        PATH=path,
        BENCH_HOST_PATH=os.environ["PATH"], BENCH_WORKSPACE=str(workspace), BENCH_RUN_DIR=str(out), BENCH_AGENT_ID=args.agent_id,
        BENCH_USAGE=str(out / "usage.jsonl"), BENCH_TRANSCRIPT=str(out / "transcript.jsonl"), BENCH_CONTEXT=str(out / "context.json"),
        BENCH_AGENT_INFO=str(out / "agent-info.json"),
        BENCH_KNOWLEDGE=cell["knowledge"], BENCH_NETWORK=cell["network"], BENCH_TOOL=cell["tool"], BENCH_SKILLS=",".join(cell["skills"]),
    )
    if cell["skills"]:
        env["BENCH_SKILL_DIRS"] = os.pathsep.join(str(Path(args.skill_dirs[name]).resolve()) for name in cell["skills"])
    return env


INSTRUCTION_FILES = ("AGENTS.md", "CLAUDE.md", "GEMINI.md", ".cursorrules", ".github/copilot-instructions.md", ".windsurf/rules", ".cursor/rules")


def ancestor_instructions(workspace: Path) -> list[str]:
    """Instruction files an agent would discover by walking up from the workspace."""
    return [str(parent / name) for parent in workspace.resolve().parents for name in INSTRUCTION_FILES if (parent / name).exists()]


def canaries(cell: dict, env: dict, workspace: Path, args: argparse.Namespace) -> str | None:
    """A failed canary invalidates the run. Returns the reason, or None."""
    if args.check_ancestors and (found := ancestor_instructions(workspace)):
        return f"instruction files in ancestor directories of the workspace: {found}; use --out outside the repository"
    for tool in ("wright", "overpy"):
        if cell["tool"] != tool and shutil.which(tool, path=env["PATH"]):
            return f"{tool} reachable although the tool is '{cell['tool']}'"
    if cell["network"] == "off" and args.canary_cmd:
        if subprocess.run(args.canary_cmd, shell=True, cwd=workspace, env=env, capture_output=True).returncode == 0:
            return "network reachable under network 'off'"
    return None


BENCH_HOME = Path(os.environ.get("WRIGHT_BENCH_HOME", Path.home() / ".local/share/wright-agent-bench"))  # the one place for runs (`runs/`), wiki snapshots, and pinned skills
DATA_ROOTS = (BENCH_HOME, Path.home() / ".cache/wright-agent-bench")  # hidden from agents; the second is where earlier versions wrote runs
HIDDEN_ROOTS = (Path("/Users"), Path("/Volumes"), Path.home())  # the host's home directories and external drives are hidden unless listed below
ADAPTER_READS = {  # what each adapter reads from the real home before the agent starts: credentials, configuration, installation (relative to the home directory)
    "devin": [".local/share/devin", ".config/devin"], "pi": [".pi"], "codex": [".codex/auth.json"], "agy": [".gemini/antigravity-cli"],
    "opencode": [".local/share/opencode/auth.json"], "grok": [".grok/auth.json"], "claude-code": [".claude.json", ".claude/.credentials.json"], "direct": [],
}


CREDENTIALS = {  # (real file relative to the home directory, its isolated copy relative to the run directory) per adapter
    "codex": [(".codex/auth.json", "codex-home/.codex/auth.json")],
    "pi": [(".pi/agent/auth.json", "pi-home/.pi/agent/auth.json"), (".pi/agent/antigravity-accounts.json", "pi-home/.pi/agent/antigravity-accounts.json")],
    "devin": [(".local/share/devin/credentials.toml", "devin-home/.local/share/devin/credentials.toml")],
    "agy": [(".gemini/antigravity-cli/antigravity-oauth-token", "agy-home/.gemini/antigravity-cli/antigravity-oauth-token")],
    "opencode": [(".local/share/opencode/auth.json", "opencode-home/.local/share/opencode/auth.json")],
    "grok": [(".grok/auth.json", "grok-home/auth.json")],
}


def sync_credentials_back(pairs: list[tuple[str, str]], run_dir: Path, home: Path | None = None) -> list[str]:
    """Copy a login the agent's CLI refreshed inside its isolated home back to the real one.

    Adapters hand the CLI a copy of the user's OAuth login. Refresh tokens rotate, so a refresh in the copy that is then discarded
    leaves the user's own login holding a dead token. Only a changed, newer copy is written back, atomically."""
    home = home or Path.home()
    updated = []
    for real_rel, isolated_rel in pairs:
        real, isolated = home / real_rel, run_dir / isolated_rel
        if not isolated.is_file() or not real.is_file() or isolated.read_bytes() == real.read_bytes() or isolated.stat().st_mtime <= real.stat().st_mtime:
            continue
        temporary = real.with_name(f".{real.name}.bench-sync")
        temporary.write_bytes(isolated.read_bytes())
        temporary.chmod(real.stat().st_mode & 0o777)
        os.replace(temporary, real)
        updated.append(real_rel)
    return updated


CREDENTIALS = {  # (real file relative to the home directory, its isolated copy relative to the run directory) per adapter
    "codex": [(".codex/auth.json", "codex-home/.codex/auth.json")],
    "pi": [(".pi/agent/auth.json", "pi-home/.pi/agent/auth.json"), (".pi/agent/antigravity-accounts.json", "pi-home/.pi/agent/antigravity-accounts.json")],
    "devin": [(".local/share/devin/credentials.toml", "devin-home/.local/share/devin/credentials.toml")],
    "agy": [(".gemini/antigravity-cli/antigravity-oauth-token", "agy-home/.gemini/antigravity-cli/antigravity-oauth-token")],
    "opencode": [(".local/share/opencode/auth.json", "opencode-home/.local/share/opencode/auth.json")],
    "grok": [(".grok/auth.json", "grok-home/auth.json")],
}


def sync_credentials_back(pairs: list[tuple[str, str]], run_dir: Path, home: Path | None = None) -> list[str]:
    """Copy a login the agent's CLI refreshed inside its isolated home back to the real one.

    Adapters hand the CLI a copy of the user's OAuth login. Refresh tokens rotate, so a refresh in the copy that is then discarded
    leaves the user's own login holding a dead token. Only a changed, newer copy is written back, atomically."""
    home = home or Path.home()
    updated = []
    for real_rel, isolated_rel in pairs:
        real, isolated = home / real_rel, run_dir / isolated_rel
        if not isolated.is_file() or not real.is_file() or isolated.read_bytes() == real.read_bytes() or isolated.stat().st_mtime <= real.stat().st_mtime:
            continue
        temporary = real.with_name(f".{real.name}.bench-sync")
        temporary.write_bytes(isolated.read_bytes())
        temporary.chmod(real.stat().st_mode & 0o777)
        os.replace(temporary, real)
        updated.append(real_rel)
    return updated


def read_policy(args: argparse.Namespace, env: dict) -> tuple[list[Path], list[Path]]:
    """What the agent may not read, and the exceptions it needs.

    An allow-list: the host's home directories and drives are hidden, so the agent sees the machine as a clean one holding only its run
    directory, the condition's skills, the tool and runtime binaries, and what its adapter needs. Agents otherwise read the whole machine:
    the answer keys, other runs, the grader, installed copies of the OverPy source, and checkouts of the repositories under test, so the
    benchmark would measure what the agent could find instead of what it was given."""
    run_dir = Path(env["BENCH_RUN_DIR"]).resolve()
    selected = [name for name in env["BENCH_SKILLS"].split(",") if name]
    hidden = [*HIDDEN_ROOTS, args.out, getattr(args, "out_root", args.out), *DATA_ROOTS, HERE, *(Path(p).expanduser() for p in getattr(args, "deny_read", []) or [])]
    if getattr(args, "wiki_dir", None):
        hidden.append(Path(args.wiki_dir))
    hidden += [Path(d) for name, d in (args.skill_dirs or {}).items() if name not in selected]
    runtime = [Path(args.wright), Path(sys.executable)]
    for binary in (shutil.which("node"), shutil.which(ADAPTER_BINARY.get(getattr(args, "adapter", ""), ""))):
        if binary:
            runtime.append(Path(binary))
    allowed = [run_dir, HERE / "adapters", HERE / "bench_trace.py", HERE / "oracle", Path(sys.prefix), Path(sys.base_prefix),
               *(Path(args.skill_dirs[name]) for name in selected), *(Path(p).expanduser() for p in getattr(args, "allow_read", []) or [])]
    for binary in runtime:  # the directory of the binary and of the file its symlink resolves to
        allowed += [binary.parent, binary.resolve().parent]
    unique = lambda paths: list(dict.fromkeys(p.resolve() for p in paths))
    return unique(hidden), unique(allowed)


def sandbox_read_rules(hidden: list[Path], allowed: list[Path]) -> str:
    def rule(action: str, path: Path) -> str:
        return f"({action} file-read-data ({'literal' if path.is_file() else 'subpath'} {json.dumps(str(path))}))\n"
    # the allow-list overrides the hidden roots, and the answer keys are hidden again whatever else is allowed
    return "".join(rule("deny", p) for p in hidden) + "".join(rule("allow", p) for p in allowed) + rule("deny", SCENARIOS.resolve())


def run_agent(args: argparse.Namespace, env: dict, workspace: Path, prompt: str) -> tuple[int | None, str, str]:
    command = args.agent_cmd
    if getattr(args, "file_sandbox", False):
        if sys.platform != "darwin" or not shutil.which("sandbox-exec"):
            raise SystemExit("--file-sandbox requires macOS sandbox-exec; refusing an unprotected run")
        run_dir = Path(env["BENCH_RUN_DIR"])
        temporary = run_dir / "tmp"
        temporary.mkdir(exist_ok=True)
        env = {**env, "TMPDIR": str(temporary), "PYTHONDONTWRITEBYTECODE": "1"}
        rules = sandbox_read_rules(*read_policy(args, env))
        profile = run_dir / "agent.sb"
        profile.write_text('(version 1)\n(allow default)\n(deny file-write*)\n'
                           f'(allow file-write* (subpath {json.dumps(str(run_dir.resolve()))}) (subpath "/dev"))\n'
                           '(deny file-read-data (require-all (regex "/(AGENTS|CLAUDE|GEMINI)[.]md$") '
                           f'(require-not (subpath {json.dumps(str(workspace.resolve()))}))))\n' + rules)
        command = ["sandbox-exec", "-f", str(profile), "/bin/sh", "-c", args.agent_cmd]
    proc = subprocess.Popen(
        command, shell=isinstance(command, str), cwd=workspace, env=env, text=True,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True,
    )
    try:
        stdout, stderr = proc.communicate(input=prompt, timeout=args.timeout)
        return proc.returncode, stdout, stderr
    except subprocess.TimeoutExpired:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        stdout, stderr = proc.communicate()
        return None, stdout, f"{stderr}\ntimeout" if stderr else "timeout"


def context_report(out: Path, skill_names: list[str]) -> dict:
    path = out / "context.json"
    if not path.is_file():
        return {"reported": False}
    report = json.loads(path.read_text())
    if "loaded" not in report:
        return {"reported": False, **report}
    loaded = report["loaded"]
    allowed = set(skill_names or [])
    return {**report, "reported": True, "loaded": loaded, "unexpected": sorted(set(loaded) - allowed)}


def run_trial(scenario: dict, cell: dict, args: argparse.Namespace, out: Path) -> dict:
    check_cell(cell, args)
    if not applicable(scenario, cell):
        raise SystemExit(f"condition {cell_label(cell)} does not apply to the {scenario['language']} scenario {scenario['id']}")
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    workspace = out / "workspace"
    prompt = (scenario["dir"] / "prompt.md").read_text()  # the exact pinned prompt: the harness adds no text
    infra_retries = 0
    while True:
        shutil.rmtree(workspace, ignore_errors=True)
        for stale in ("home", "bin", "real", "tool-trace.jsonl", "tool-calls", "usage.jsonl", "transcript.jsonl", "context.json", "agent-info.json", "snapshots", "tmp",
                      *(child.name for child in out.iterdir() if child.name.endswith(("-home", "-user")))):  # adapters keep their isolated homes here; a retry starts from none
            target = out / stale
            shutil.rmtree(target, ignore_errors=True) if target.is_dir() else target.unlink(missing_ok=True)
        materialize(scenario, workspace)
        if cell["knowledge"] == "wiki":
            shutil.copytree(Path(args.wiki_dir), workspace / "wiki")  # a real copy: tools that skip symlinks (rg) must see it
        env = build_env(cell, args, out, workspace)
        reason = canaries(cell, env, workspace, args)
        if reason:
            result = base_result(scenario, cell, args, out, 0.0, None)
            result.update(invalid=reason, status="invalid")
            (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
            return result
        snapshots = bench_trace.Snapshots(workspace, scenario.get("watch", [scenario["entry"]]), out / "snapshots")
        snapshots.start()
        start = time.monotonic()
        agent_exit, stdout, stderr = run_agent(args, env, workspace, prompt)
        sync_credentials_back(getattr(args, "credentials", []), out)
        sync_credentials_back(getattr(args, "credentials", []), out)
        seconds = round(time.monotonic() - start, 1)
        snaps = snapshots.finish()
        if agent_exit == INFRA_EXIT and infra_retries < args.infra_retries:
            infra_retries += 1
            time.sleep(getattr(args, "infra_backoff", 0) * infra_retries)  # an outage lasts longer than an immediate retry
            continue
        break
    (out / "agent.log").write_text(f"exit={agent_exit}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}\n")
    result = base_result(scenario, cell, args, out, seconds, agent_exit)
    result["infraRetries"] = infra_retries
    result["networkEnforcement"] = "canary-checked" if cell["network"] == "off" and args.canary_cmd else "declared-only"
    result["fileWriteEnforcement"] = "trial-directory-only" if getattr(args, "file_sandbox", False) else "unrestricted"
    if getattr(args, "file_sandbox", False):
        hidden, allowed = read_policy(args, env)
        result["fileReadEnforcement"] = {"mode": "allow-list", "hidden": [str(p) for p in hidden], "allowed": [str(p) for p in allowed]}
    else:
        result["fileReadEnforcement"] = "unrestricted"
    context = context_report(out, [skill_name(Path(args.skill_dirs[name])) for name in cell["skills"]])
    result["context"] = context
    if context.get("unexpected"):
        result["invalid"] = f"unexpected loaded context: {context['unexpected']}"
    if (out / "agent-info.json").is_file():
        result["agentInfo"] = json.loads((out / "agent-info.json").read_text())
    events = bench_trace.read_events(out / "tool-trace.jsonl")
    result["toolUse"] = bench_trace.summarize_trace(events)
    result.update(bench_grade.grade(scenario, workspace, args.wright, out / "grading"))
    if any(c.get("unavailable") for c in result["checks"]):
        result["invalid"] = "a required grader was unavailable"
    entry = workspace / scenario["entry"]
    final_sha = hashlib.sha256(entry.read_bytes()).hexdigest() if entry.is_file() else None
    result["friction"] = bench_trace.friction(events)
    result["expectations"] = bench_trace.detect_expectations(events, snaps, scenario, final_sha)
    result["snapshots"] = snapshot_validity(scenario, snaps, args.wright, out)
    first_valid = next((s["t"] for s in result["snapshots"]["series"] if s["valid"]), None)
    result["usage"] = bench_trace.usage_summary(out / "usage.jsonl", first_valid)
    result["status"] = run_status(result)
    (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def run_status(result: dict) -> str:
    """How the run ended, separate from whether its artifact was usable."""
    if "invalid" in result:
        return "invalid"
    code = result["agent"]["exit"]
    if code == INFRA_EXIT:
        return "provider-interrupted"
    if code is None:
        return "timeout"
    return "agent-error" if code else "completed"


def snapshot_validity(scenario: dict, snaps: list[dict], wright: str, out: Path) -> dict:
    """Strict validity of every snapshot of the entry file; regressions are valid -> invalid transitions."""
    series = []
    for snap in [s for s in snaps if s["file"] == scenario["entry"]]:
        valid = bench_grade.strict_valid(wright, Path(snap["path"]), out / "grading" / f"snapshot-{snap['i']:03d}")
        series.append({"i": snap["i"], "t": snap["t"], "valid": valid})
    regressions = sum(1 for a, b in zip(series, series[1:]) if a["valid"] and not b["valid"])
    return {"count": len(series), "firstValidIndex": next((s["i"] for s in series if s["valid"]), None), "regressions": regressions, "series": series}


@functools.lru_cache(maxsize=None)
def harness_commit() -> str:
    proc = subprocess.run(["git", "-C", str(HERE), "rev-parse", "HEAD"], capture_output=True, text=True)
    dirty = subprocess.run(["git", "-C", str(HERE), "status", "--porcelain", "--", "."], capture_output=True, text=True).stdout.strip()
    return proc.stdout.strip() + ("+dirty" if dirty else "")


@functools.lru_cache(maxsize=None)
def file_sha256(path: str) -> str:
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


@functools.lru_cache(maxsize=None)
def cached_suite(scenarios: str) -> tuple:
    return tuple(bench_grade.suite_identity(Path(scenarios)).items())


def base_result(scenario: dict, cell: dict, args: argparse.Namespace, out: Path, seconds: float, agent_exit: int | None) -> dict:
    return {
        "contract": RESULT_CONTRACT,
        "scenario": scenario["id"],
        "family": scenario["family"],
        "language": scenario["language"],
        "split": scenario.get("split"),
        "condition": {**cell, "label": cell_label(cell)},
        "agent": {"id": args.agent_id, "command": args.agent_cmd, "exit": agent_exit, "seconds": seconds},
        "protocol": {"timeoutSeconds": args.timeout, "infraRetries": args.infra_retries},
        "environment": {
            "os": platform.platform(), "python": platform.python_version(),
            "wright": subprocess.run([args.wright, "--version"], capture_output=True, text=True).stdout.strip(),
            "wrightSha256": file_sha256(args.wright),
            "harness": harness_commit(),
            "suite": dict(cached_suite(str(SCENARIOS))),
            "timestamp": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "skills": {name: skill_identity(name, Path(args.skill_dirs[name])) for name in cell["skills"]},
            **({"wiki": bench_wiki.identity(Path(args.wiki_dir))} if cell["knowledge"] == "wiki" else {}),
        },
    }


def trial_dir(base: Path, scenario_id: str, agent_id: str, cell: dict, trial: int) -> Path:
    return base / scenario_id / agent_id / f"{cell_label(cell).replace('/', '_')}-{trial}"


def verdict_word(result: dict) -> str:
    return result["status"].upper() if result["status"] != "completed" else "PASS" if result["passed"] else "FAIL"


def cmd_run(args: argparse.Namespace) -> int:
    scenario = load_scenario(args.scenario)
    cell = normalize_cell({"tool": args.tool, "skills": args.skills, "knowledge": args.knowledge, "network": args.network})
    code = 0
    for trial in range(1, args.trials + 1):
        result = run_trial(scenario, cell, args, trial_dir(args.out, args.scenario, args.agent_id, cell, trial))
        used = sum(u["invocations"] for u in result.get("toolUse", {}).values())
        print(f"{args.scenario} {cell_label(cell)} trial {trial}: {verdict_word(result)} layers={result.get('failedLayers')} tool-invocations={used}")
        code = max(code, 3 if result["status"] == "provider-interrupted" else 1 if result["status"] != "completed" or not result["passed"] else 0)
    return code


STOP_AFTER_INTERRUPTIONS = 2


def cmd_matrix(args: argparse.Namespace) -> int:
    """Run every applicable (scenario, agent, cell, trial) of a matrix file in randomized order; finished runs are skipped.

    Exit 0 when every run completed (graded failures are results, not errors), 3 when provider interruptions occurred, 1 otherwise.
    After repeated provider interruptions in a row the remaining jobs are left unattempted instead of burning through them."""
    config = json.loads(args.config.read_text())
    cells = [normalize_cell(c) for c in config["cells"]]
    jobs, skipped = [], []
    for scenario_id in config.get("scenarios") or all_scenario_ids():
        scenario = load_scenario(scenario_id)
        for agent in config["agents"]:
            for cell in cells:
                if not applicable(scenario, cell):
                    skipped.append((scenario_id, cell_label(cell)))
                    continue
                jobs += [(scenario_id, agent, cell, t) for t in range(1, config.get("trials", 5) + 1)]
    random.Random(config.get("seed", 0)).shuffle(jobs)
    for scenario_id, label in sorted(set(skipped)):
        print(f"not applicable: {scenario_id} {label}", flush=True)
    state = {"streak": 0, "interrupted": 0, "unattempted": 0, "failed": 0}
    lock = threading.Lock()

    def work(job: tuple) -> None:
        scenario_id, agent, cell, trial = job
        out = trial_dir(args.out, scenario_id, agent["id"], cell, trial)
        finished = out / "result.json"
        if finished.is_file() and json.loads(finished.read_text()).get("status") != "provider-interrupted":  # an interrupted trial is retried on the next run
            return
        with lock:
            if state["streak"] >= STOP_AFTER_INTERRUPTIONS:
                state["unattempted"] += 1
                return
        options = {k: ({n: Path(v) for n, v in val.items()} if k == "skill_dirs" else Path(val) if k == "wiki_dir" and val else val) for k, val in config.get("options", {}).items()}
        trial_args = argparse.Namespace(**{**vars(args), "agent_id": agent["id"], "agent_cmd": agent["cmd"], **options})
        result = run_trial(load_scenario(scenario_id), cell, trial_args, out)
        with lock:
            state["streak"] = state["streak"] + 1 if result["status"] == "provider-interrupted" else 0
            state["interrupted"] += result["status"] == "provider-interrupted"
            state["failed"] += result["status"] in ("invalid", "agent-error")
        print(f"{scenario_id} {agent['id']} {cell_label(cell)} #{trial}: {verdict_word(result)}", flush=True)

    with ThreadPoolExecutor(max_workers=config.get("parallel", 2)) as pool:
        list(pool.map(work, jobs))
    if state["unattempted"]:
        print(f"stopped after {STOP_AFTER_INTERRUPTIONS} provider interruptions in a row: {state['unattempted']} job(s) left unattempted; rerun to resume", flush=True)
    return 3 if state["interrupted"] or state["unattempted"] else 1 if state["failed"] else 0


ADAPTERS = {"claude-code": "claude_code.py", "pi": "pi.py", "devin": "devin.py", "codex": "codex.py", "agy": "agy.py", "opencode": "opencode.py", "grok": "grok.py", "direct": "direct.py"}
CANONICAL_CELL = {"tool": "wright", "skills": ["wright-skill"], "knowledge": "none", "network": "off"}
CONTROL_CELLS = [
    {"tool": "none", "skills": [], "knowledge": "none", "network": "off"},
    {"tool": "wright", "skills": [], "knowledge": "none", "network": "off"},
    CANONICAL_CELL,
    {"tool": "overpy", "skills": [], "knowledge": "none", "network": "off"},
    {"tool": "overpy", "skills": ["opy-skill"], "knowledge": "none", "network": "off"},
]


ADAPTER_BINARY = {"claude-code": "claude", "pi": "pi", "devin": "devin", "codex": "codex", "agy": "agy", "opencode": "opencode", "grok": "grok"}
DIRECT_ENV = ("ANTHROPIC_API_KEY", "ANTHROPIC_BASE_URL", "OPENAI_API_KEY", "OPENAI_BASE_URL")
CONFIG_PATH = Path(os.environ.get("WRIGHT_BENCH_CONFIG", Path.home() / ".config/wright-agent-bench/config.json"))


def user_defaults() -> dict:
    """Per-user defaults for the long options, so a run is `evaluate --adapter A --model M`: keys wright, out, wiki_dir, env_pass, deny_read, skill_dirs {name: dir}."""
    if not CONFIG_PATH.is_file():
        return {}
    config = json.loads(CONFIG_PATH.read_text())
    unknown = set(config) - {"wright", "out", "wiki_dir", "env_pass", "deny_read", "allow_read", "skill_dirs", "models"}
    if unknown:
        raise SystemExit(f"{CONFIG_PATH}: unknown key(s) {sorted(unknown)}")
    defaults = {k: v for k, v in config.items() if k not in ("skill_dirs", "out", "wiki_dir")}
    defaults.update({k: Path(v).expanduser() for k, v in config.items() if k in ("out", "wiki_dir")})
    if "skill_dirs" in config:
        defaults["skill_dir"] = [f"{name}={Path(d).expanduser()}" for name, d in config["skill_dirs"].items()]
    return defaults


def preflight(args: argparse.Namespace, cells: list[dict]) -> list[str]:
    """Problems that would waste a run, found before it starts."""
    problems = []
    if not Path(args.wright).is_file():
        problems.append(f"wright binary not found: {args.wright} (pass --wright or put `wright` on PATH)")
    binary = ADAPTER_BINARY.get(args.adapter)
    if binary and not shutil.which(binary):
        problems.append(f"`{binary}` is not on PATH, which adapter '{args.adapter}' needs")
    if args.adapter == "direct" and not any(os.environ.get(k) for k in ("ANTHROPIC_API_KEY", "OPENAI_API_KEY")):
        problems.append("adapter 'direct' needs ANTHROPIC_API_KEY or OPENAI_API_KEY in the environment")
    for name in sorted({s for c in cells for s in c["skills"]}):
        if not (Path(args.skill_dirs.get(name, "/nonexistent")) / "SKILL.md").is_file():
            problems.append(f"skill '{name}' needs --skill-dir {name}=DIR (or skill_dirs in {CONFIG_PATH})")
    if any(c["tool"] == "overpy" for c in cells) and not bench_grade.oracle_available():
        problems.append("tool 'overpy' needs the pinned oracle: run `agent_bench.py setup-oracle`")
    return problems


def cmd_evaluate(args: argparse.Namespace) -> int:
    """One command from agent and model to data and document: run the matrix, then write report, score cards, and RESULTS.md.

    Needs no agent harness around it: isolation comes from the harness's own scrubbed environment, so it runs the same from a
    terminal or from inside another agent's shell."""
    args.file_sandbox = not args.no_file_sandbox  # evaluation hides the answer keys and the rest of the host from the agent by default
    args.credentials = CREDENTIALS.get(args.adapter, [])
    args.credentials = CREDENTIALS.get(args.adapter, [])
    args.allow_read = [*(str(Path.home() / rel) for rel in ADAPTER_READS[args.adapter]), *args.allow_read]
    script = Path(__file__).parent / "adapters" / ADAPTERS[args.adapter]
    effort = f"BENCH_THINKING={shlex.quote(args.effort)} " if args.effort else ""
    agent_id = "-".join(filter(None, (args.adapter, args.model.replace("/", "_"), args.effort)))
    cmd = f"BENCH_MODEL={shlex.quote(args.model)} {effort}{shlex.quote(sys.executable)} {shlex.quote(str(script))}"
    wanted = [CANONICAL_CELL] if args.cells == "score" else CONTROL_CELLS
    cells = [c for c in wanted if all(s in args.skill_dirs for s in c["skills"])]
    dropped = [cell_label(normalize_cell(c)) for c in wanted if c not in cells]
    if dropped:
        print(f"skipped cells without --skill-dir: {', '.join(dropped)}", flush=True)
    if not cells:
        raise SystemExit("no cell can run: pass --skill-dir wright-skill=DIR")
    args.env_pass = sorted({*args.env_pass, *(DIRECT_ENV if args.adapter == "direct" else ("HOME",))})  # the credentials the adapter copies or reads
    problems = preflight(args, [normalize_cell(c) for c in cells])
    if problems:
        raise SystemExit("cannot start:\n  " + "\n  ".join(problems))
    scenarios = args.scenarios or [s for s in all_scenario_ids() if args.split == "all" or load_scenario(s).get("split") == args.split]
    runnable = sum(args.trials for s in scenarios for c in cells if applicable(load_scenario(s), normalize_cell(c)))
    print(f"{args.adapter} {args.model}: {len(scenarios)} scenario(s), cells {', '.join(cell_label(normalize_cell(c)) for c in cells)}, {runnable} trial(s) into {args.out / args.name}", flush=True)
    if args.dry_run:
        return 0
    args.out_root = args.out  # sibling evaluation runs must stay unreadable too
    args.out = args.out / args.name
    args.out.mkdir(parents=True, exist_ok=True)
    config = {"agents": [{"id": agent_id, "cmd": cmd}], "cells": cells, "scenarios": scenarios, "trials": args.trials, "parallel": args.parallel, "seed": args.seed,
              "options": {"skill_dirs": {k: str(v) for k, v in args.skill_dirs.items()}, "wiki_dir": str(args.wiki_dir) if args.wiki_dir else None}}
    args.config = args.out / "matrix.json"
    args.config.write_text(json.dumps(config, indent=2) + "\n")
    status = cmd_matrix(args)
    if not list(args.out.glob("*/*/*/result.json")):
        return status or 1
    bench_report.main([args.out], args.wright, False, load_scenario, bench_report.BASELINE)
    languages = ["workshop", "opy"]
    expected = {lang: [s for s in all_scenario_ids() if load_scenario(s)["language"] == lang and load_scenario(s).get("split") == "test"] for lang in languages}
    bench_score.main([args.out], languages, expected, None)
    (args.out / "RESULTS.md").write_text(f"# Agent benchmark results: {args.name}\n\nAgent `{agent_id}`. Generated by `agent_bench.py evaluate`; `matrix.json` reproduces it.\n\n"
                                         f"## Score\n\n```\n{(args.out / 'score.txt').read_text()}```\n\n{(args.out / 'report.md').read_text()}")
    print(f"wrote {args.out / 'RESULTS.md'}")
    return status


def model_slug(entry: dict) -> str:
    return "-".join(filter(None, (entry["adapter"], entry["model"].replace("/", "_"), entry.get("effort"))))


def cmd_suite(args: argparse.Namespace) -> int:
    """Evaluate every model of the user's list one after another, then write the publishable results page.

    Safe to repeat: finished runs are skipped, so quota or time limits only postpone the rest. Exit 3 when something is waiting for a rerun."""
    models = [m for m in (args.models or []) if not args.only or any(f"{m['adapter']}:{m['model']}".startswith(o) for o in args.only)]
    if not models:
        raise SystemExit(f'no models: add "models": [{{"adapter": "devin", "model": "swe-2-max"}}, ...] to {CONFIG_PATH}')
    root = args.out / args.suite_name
    outcome = {}
    for entry in models:
        slug = model_slug(entry)
        print(f"\n=== {slug}", flush=True)
        sub = argparse.Namespace(**{**vars(args), "adapter": entry["adapter"], "model": entry["model"], "effort": entry.get("effort"), "name": slug, "out": root})
        try:
            outcome[slug] = {0: "done", 3: "waiting: provider limit or outage, rerun later"}.get(cmd_evaluate(sub), "finished with errors")
        except SystemExit as stop:
            outcome[slug] = f"skipped: {stop.code}"
    print("\n" + "\n".join(f"{slug}: {state}" for slug, state in outcome.items()))
    if not args.dry_run:
        bench_leaderboard.main(sorted(root.glob("*/")), root / "leaderboard")
    return 3 if any(state.startswith("waiting") for state in outcome.values()) else 0


def cmd_wiki_snapshot(args: argparse.Namespace) -> int:
    record = bench_wiki.snapshot(args.base, args.dir, tuple(args.categories))
    print(f"{len(record['documents'])} document(s) from {record['source']} into {args.dir}\nsnapshotSha256 {record['snapshotSha256']}")
    return 0


def cmd_wiki_skill(args: argparse.Namespace) -> int:
    record = wiki_skill.build(args.snapshot, args.out_dir, json.loads(args.catalog.read_text()), json.loads(args.opy_manifest.read_text()), wiki_skill.upstream_source_text())
    print(json.dumps(record, indent=2))
    return 0


def cmd_setup_oracle(_: argparse.Namespace) -> int:
    return subprocess.call(["npm", "ci", "--silent"], cwd=bench_grade.ORACLE)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("suite", help="evaluate every model listed in the user config in turn, then write the results page")
    for name in ("validate", "run", "matrix", "evaluate", "suite"):
        p = sub.choices[name] if name == "suite" else sub.add_parser(name)
        p.add_argument("--wright", default=str(ROOT / "target/debug/wright"), help="Wright binary under test")
        p.add_argument("--out", type=Path, default=BENCH_HOME / "runs", help="outside any repository, so agents cannot discover its instruction files")
    for name in ("run", "matrix", "evaluate", "suite"):
        p = sub.choices[name]
        p.add_argument("--skill-dir", action="append", default=[], metavar="NAME=DIR", help=f"pinned skill directory for one of {', '.join(SKILLS)}; repeatable")
        p.add_argument("--wiki-dir", type=Path, help="pinned wiki snapshot, linked as ./wiki for knowledge 'wiki'; content hashes are verified")
        p.add_argument("--env-pass", nargs="*", default=[], help="host variables passed through the environment scrub")
        p.add_argument("--no-ancestor-check", dest="check_ancestors", action="store_false", help="skip the check for instruction files above the workspace")
        p.add_argument("--canary-cmd", help="shell command that must fail in the agent environment when the network is 'off'")
        p.add_argument("--timeout", type=int, default=1800)
        p.add_argument("--file-sandbox", action="store_true", help="macOS: restrict agent and descendant file writes to the trial directory, and hide the scenarios, other runs, the wiki, unselected skills, and --deny-read paths")
        p.add_argument("--deny-read", nargs="*", default=[], metavar="PATH", help="extra directories hidden from the agent (the home directories and drives already are); needs --file-sandbox")
        p.add_argument("--allow-read", nargs="*", default=[], metavar="PATH", help="paths the agent's CLI needs inside the hidden home directories (credentials, installation); `evaluate` adds its adapter's")
        p.add_argument("--infra-retries", type=int, default=2, help="retries when the agent exits 75 (provider or infrastructure failure)")
        p.add_argument("--infra-backoff", type=int, default=60, help="seconds before the first retry; each further retry waits one more multiple")
    run = sub.choices["run"]
    run.add_argument("scenario", choices=all_scenario_ids())
    run.add_argument("--agent-cmd", required=True, help="shell command; the task prompt arrives on stdin, cwd is the workspace, BENCH_* describes the condition")
    run.add_argument("--agent-id", required=True, help="recorded agent/model/version label")
    run.add_argument("--tool", choices=TOOLS, default="wright")
    run.add_argument("--skills", nargs="*", choices=SKILLS, default=[], help="skills installed in this condition")
    run.add_argument("--knowledge", choices=KNOWLEDGE_LEVELS, default="none")
    run.add_argument("--network", choices=("off", "on"), default="off")
    run.add_argument("--trials", type=int, default=1)
    sub.choices["matrix"].add_argument("config", type=Path, help="JSON: agents[{id,cmd}], cells[{tool,skills,knowledge,network}], scenarios, trials, parallel, seed, options")
    ev = sub.choices["evaluate"]
    ev.add_argument("--adapter", choices=sorted(ADAPTERS), required=True, help="agent adapter; `direct` is the built-in loop that needs no agent harness")
    ev.add_argument("--model", required=True, help="BENCH_MODEL, in the form the adapter expects")
    ev.add_argument("--effort", help="BENCH_THINKING, where the adapter supports it")
    ev.add_argument("--name", default=time.strftime("%Y%m%d-%H%M%S"), help="run directory under --out")
    su = sub.choices["suite"]
    su.add_argument("--suite-name", default="results", help="directory under --out holding every model's run and the results page")
    su.add_argument("--only", nargs="*", metavar="ADAPTER[:MODEL]", help="evaluate only these entries of the models list")
    for shared in (su,):
        shared.add_argument("--cells", choices=("score", "controls"), default="score")
        shared.add_argument("--split", choices=("test", "train", "all"), default="test")
        shared.add_argument("--scenarios", nargs="*", choices=all_scenario_ids())
        shared.add_argument("--trials", type=int, default=3)
        shared.add_argument("--parallel", type=int, default=1)
        shared.add_argument("--seed", type=int, default=1)
        shared.add_argument("--dry-run", action="store_true")
        shared.add_argument("--no-file-sandbox", action="store_true")
    lb = sub.add_parser("leaderboard", help="write the publishable results page (Markdown, HTML, JSON) from evaluation run directories")
    lb.add_argument("dirs", nargs="+", type=Path)
    lb.add_argument("--page-out", type=Path, help="directory for the page; `leaderboard` inside the first directory's parent by default")
    ev.add_argument("--cells", choices=("score", "controls"), default="score", help="score: the canonical cell only; controls: also baseline and language-appropriate controls")
    ev.add_argument("--split", choices=("test", "train", "all"), default="test")
    ev.add_argument("--scenarios", nargs="*", choices=all_scenario_ids())
    ev.add_argument("--trials", type=int, default=3)
    ev.add_argument("--parallel", type=int, default=1, help="trials at a time; sequential by default so provider limits are not hit, and a run can continue across sessions")
    ev.add_argument("--seed", type=int, default=1)
    ev.add_argument("--dry-run", action="store_true", help="check the setup and print what would run, without running it")
    ev.add_argument("--no-file-sandbox", action="store_true", help="run without the macOS file sandbox: the agent can then read the scenario answer keys")
    sub.add_parser("setup-oracle", help="install the pinned upstream OverPy oracle")
    skill = sub.add_parser("wiki-skill", help="build the progressive-disclosure workshop-wiki skill from a wiki snapshot")
    skill.add_argument("--snapshot", type=Path, required=True)
    skill.add_argument("--out-dir", type=Path, required=True, help="new skill directory (not overwritten)")
    skill.add_argument("--catalog", type=Path, required=True, help="workshop-rs catalog.json, for Workshop names and ids")
    skill.add_argument("--opy-manifest", type=Path, required=True, help="opy-rs manifest.json, for upstream OverPy spellings")
    wiki = sub.add_parser("wiki-snapshot", help="fetch the Workshop wiki Markdown mirror into a pinned local snapshot")
    wiki.add_argument("--dir", type=Path, default=BENCH_HOME / "wiki")
    wiki.add_argument("--base", default=bench_wiki.BASE)
    wiki.add_argument("--categories", nargs="+", default=list(bench_wiki.CATEGORIES), help="wiki categories to crawl (add tutorials for the second tier)")
    report = sub.add_parser("report", help="summarize result.json files")
    report.add_argument("dirs", nargs="+", type=Path)
    report.add_argument("--regrade", action="store_true", help="re-grade stored workspaces twice and flag unstable graders")
    report.add_argument("--wright", default=str(ROOT / "target/debug/wright"))
    report.add_argument("--reference", default=bench_report.BASELINE, help="condition label the paired comparison is made against")
    compare = sub.add_parser("compare", help="one table from the score.json of several evaluation runs, warning when they are not comparable")
    compare.add_argument("dirs", nargs="+", type=Path)
    score = sub.add_parser("score", help="compute the Wright Agent Score card of each language track from canonical test runs")
    score.add_argument("dirs", nargs="+", type=Path)
    score.add_argument("--language", choices=("workshop", "opy"), action="append", help="track to score; both when omitted")
    defaults = user_defaults()
    for choice in sub.choices.values():
        known = {a.dest for a in choice._actions}
        choice.set_defaults(**{k: v for k, v in defaults.items() if k in known})
    for name in ("evaluate", "suite"):
        sub.choices[name].set_defaults(wright=defaults.get("wright") or shutil.which("wright") or str(ROOT / "target/debug/wright"))
    sub.choices["suite"].set_defaults(models=defaults.get("models"))
    args = parser.parse_args()
    if hasattr(args, "wright"):
        args.wright = str(Path(args.wright).resolve())
    if hasattr(args, "out"):
        args.out = args.out.resolve()
    if hasattr(args, "skill_dir"):
        args.skill_dirs = {}
        for item in args.skill_dir:
            name, _, directory = item.partition("=")
            if name not in SKILLS or not directory:
                raise SystemExit(f"--skill-dir expects NAME=DIR with NAME one of {', '.join(SKILLS)}: {item}")
            args.skill_dirs[name] = Path(directory)
    if getattr(args, "deny_read", None) and not getattr(args, "file_sandbox", False) and args.command not in ("evaluate", "suite"):
        raise SystemExit("--deny-read needs --file-sandbox")
    if args.command == "validate":
        return 0 if validate(args.wright, args.out) else 1
    if args.command == "setup-oracle":
        return cmd_setup_oracle(args)
    if args.command == "wiki-snapshot":
        return cmd_wiki_snapshot(args)
    if args.command == "wiki-skill":
        return cmd_wiki_skill(args)
    if args.command == "report":
        return bench_report.main(args.dirs, args.wright, args.regrade, lambda s: load_scenario(s), args.reference)
    if args.command == "leaderboard":
        return bench_leaderboard.main(args.dirs, args.page_out or args.dirs[0].parent / "leaderboard")
    if args.command == "compare":
        print(bench_score.compare(args.dirs))
        return 0
    if args.command == "score":
        languages = args.language or ["workshop", "opy"]
        expected = {lang: [s for s in all_scenario_ids() if load_scenario(s)["language"] == lang and load_scenario(s).get("split") == "test"] for lang in languages}
        return bench_score.main(args.dirs, languages, expected, None)
    return {"run": cmd_run, "matrix": cmd_matrix, "evaluate": cmd_evaluate, "suite": cmd_suite}[args.command](args)


if __name__ == "__main__":
    sys.exit(main())
