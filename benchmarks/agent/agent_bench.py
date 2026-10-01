#!/usr/bin/env python3
"""Wright agent benchmark harness (#414). Contract: docs/agent-benchmark.md; design: docs/specs/SPEC-414-agent-benchmark-comparison.md."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import random
import shutil
import signal
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
from pathlib import Path

import bench_grade
import bench_report
import bench_trace
import bench_wiki
import wiki_skill

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
SCENARIOS = HERE / "scenarios"
RESULT_CONTRACT = "wright-agent-bench/v2"
WRIGHT_LEVELS = ("none", "bin", "bin+skill")
KNOWLEDGE_LEVELS = ("none", "wiki", "wiki-skill", "web")
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
    """PATH without any directory that provides a `wright` executable."""
    kept = [d for d in path.split(os.pathsep) if d and not shutil.which("wright", path=d)]
    return os.pathsep.join(kept)


def cell_label(cell: dict) -> str:
    return f"{cell['wright']}/{cell['knowledge']}/{cell['network']}"


def check_cell(cell: dict, args: argparse.Namespace) -> None:
    if cell["wright"] not in WRIGHT_LEVELS or cell["knowledge"] not in KNOWLEDGE_LEVELS or cell["network"] not in ("off", "on"):
        raise SystemExit(f"invalid condition {cell_label(cell)}")
    if cell["knowledge"] == "web" and cell["network"] != "on":
        raise SystemExit("knowledge 'web' requires network 'on'")
    if cell["wright"] == "bin+skill" and not args.skill_dir:
        raise SystemExit("wright level 'bin+skill' requires --skill-dir")
    if cell["knowledge"] == "wiki-skill" and not (args.wiki_skill_dir and all((Path(args.wiki_skill_dir) / name).is_file() for name in ("SKILL.md", "BUILD.json"))):
        raise SystemExit("knowledge 'wiki-skill' requires --wiki-skill-dir pointing at a built skill (see `agent_bench.py wiki-skill`)")
    if cell["knowledge"] == "wiki" and not (args.wiki_dir and (Path(args.wiki_dir) / "SNAPSHOT.json").is_file()):
        raise SystemExit("knowledge 'wiki' requires --wiki-dir pointing at a snapshot (see `agent_bench.py wiki-snapshot`)")
    if cell["knowledge"] == "wiki":
        bench_wiki.identity(Path(args.wiki_dir))
    if cell["knowledge"] == "wiki-skill":
        wiki_skill.identity(Path(args.wiki_skill_dir))


def build_env(cell: dict, args: argparse.Namespace, out: Path, workspace: Path) -> dict:
    """Scrubbed environment: allowlisted host variables, a fresh HOME, and the BENCH_* contract for the adapter."""
    home = out / "home"
    home.mkdir(parents=True)
    env = {k: os.environ[k] for k in (*ENV_KEEP, *args.env_pass) if k in os.environ}
    env.setdefault("HOME", str(home))
    path = baseline_path(os.environ["PATH"])
    if cell["wright"] != "none":
        shim_dir = out / "bin"
        shim_dir.mkdir()
        shim = shim_dir / "wright"
        shim.write_text(f'#!/bin/sh\nexec "{sys.executable}" "{Path(__file__).resolve()}" shim "$@"\n')
        shim.chmod(0o755)
        env.update(WRIGHT_BENCH_REAL=args.wright, WRIGHT_BENCH_TRACE=str(out / "wright-trace.jsonl"), WRIGHT_BENCH_SIDECAR=str(out / "wright-calls"))
        path = f"{shim_dir}{os.pathsep}{path}"
    env.update(
        PATH=path,
        BENCH_HOST_PATH=os.environ["PATH"], BENCH_WORKSPACE=str(workspace), BENCH_RUN_DIR=str(out), BENCH_AGENT_ID=args.agent_id,
        BENCH_USAGE=str(out / "usage.jsonl"), BENCH_TRANSCRIPT=str(out / "transcript.jsonl"), BENCH_CONTEXT=str(out / "context.json"),
        BENCH_KNOWLEDGE=cell["knowledge"], BENCH_NETWORK=cell["network"], BENCH_WRIGHT=cell["wright"],
    )
    if cell["wright"] == "bin+skill":
        env["BENCH_SKILL_DIR"] = str(args.skill_dir)
    if cell["knowledge"] == "wiki-skill":
        env["BENCH_WIKI_SKILL_DIR"] = str(args.wiki_skill_dir)
    return env


INSTRUCTION_FILES = ("AGENTS.md", "CLAUDE.md", "GEMINI.md", ".cursorrules", ".github/copilot-instructions.md", ".windsurf/rules", ".cursor/rules")


def ancestor_instructions(workspace: Path) -> list[str]:
    """Instruction files an agent would discover by walking up from the workspace."""
    return [str(parent / name) for parent in workspace.resolve().parents for name in INSTRUCTION_FILES if (parent / name).exists()]


def canaries(cell: dict, env: dict, workspace: Path, args: argparse.Namespace) -> str | None:
    """A failed canary invalidates the run. Returns the reason, or None."""
    if args.check_ancestors and (found := ancestor_instructions(workspace)):
        return f"instruction files in ancestor directories of the workspace: {found}; use --out outside the repository"
    if cell["wright"] == "none" and shutil.which("wright", path=env["PATH"]):
        return "wright reachable under wright level 'none'"
    if cell["network"] == "off" and args.canary_cmd:
        if subprocess.run(args.canary_cmd, shell=True, cwd=workspace, env=env, capture_output=True).returncode == 0:
            return "network reachable under network 'off'"
    return None


def run_agent(args: argparse.Namespace, env: dict, workspace: Path, prompt: str) -> tuple[int | None, str, str]:
    command = args.agent_cmd
    if getattr(args, "file_sandbox", False):
        if sys.platform != "darwin" or not shutil.which("sandbox-exec"):
            raise SystemExit("--file-sandbox requires macOS sandbox-exec; refusing an unprotected run")
        run_dir = Path(env["BENCH_RUN_DIR"])
        temporary = run_dir / "tmp"
        temporary.mkdir(exist_ok=True)
        env = {**env, "TMPDIR": str(temporary), "PYTHONDONTWRITEBYTECODE": "1"}
        profile = run_dir / "agent.sb"
        profile.write_text('(version 1)\n(allow default)\n(deny file-write*)\n'
                           f'(allow file-write* (subpath {json.dumps(str(run_dir.resolve()))}) (subpath "/dev"))\n'
                           '(deny file-read-data (require-all (regex "/(AGENTS|CLAUDE|GEMINI)[.]md$") '
                           f'(require-not (subpath {json.dumps(str(workspace.resolve()))}))))\n')
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


def context_report(out: Path, cell: dict) -> dict:
    path = out / "context.json"
    if not path.is_file():
        return {"reported": False}
    report = json.loads(path.read_text())
    if "loaded" not in report:
        return {"reported": False, **report}
    loaded = report["loaded"]
    allowed = ({"wright"} if cell["wright"] == "bin+skill" else set()) | ({wiki_skill.SKILL_NAME} if cell["knowledge"] == "wiki-skill" else set())
    return {**report, "reported": True, "loaded": loaded, "unexpected": sorted(set(loaded) - allowed)}


def run_trial(scenario: dict, cell: dict, args: argparse.Namespace, out: Path) -> dict:
    check_cell(cell, args)
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    workspace = out / "workspace"
    prompt = (scenario["dir"] / "prompt.md").read_text()
    prompt += "\n\nBenchmark environment: Use only files in the current workspace, the supplied skills, and tools available on PATH. Do not read host repositories, caches, or other benchmark runs."
    if cell["network"] == "off":
        prompt += " Do not use web search, fetch URLs, download packages, or make network requests. Model-provider communication is handled by the harness."
    infra_retries = 0
    while True:
        shutil.rmtree(workspace, ignore_errors=True)
        for stale in ("home", "bin", "wright-trace.jsonl", "wright-calls", "usage.jsonl", "transcript.jsonl", "context.json", "snapshots"):
            target = out / stale
            shutil.rmtree(target, ignore_errors=True) if target.is_dir() else target.unlink(missing_ok=True)
        materialize(scenario, workspace)
        if cell["knowledge"] == "wiki":
            (workspace / "wiki").symlink_to(args.wiki_dir.resolve())
        env = build_env(cell, args, out, workspace)
        reason = canaries(cell, env, workspace, args)
        if reason:
            result = base_result(scenario, cell, args, out, 0.0, None)
            result.update(invalid=reason)
            (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
            return result
        snapshots = bench_trace.Snapshots(workspace, scenario.get("watch", [scenario["entry"]]), out / "snapshots")
        snapshots.start()
        start = time.monotonic()
        agent_exit, stdout, stderr = run_agent(args, env, workspace, prompt)
        seconds = round(time.monotonic() - start, 1)
        snaps = snapshots.finish()
        if agent_exit == INFRA_EXIT and infra_retries < args.infra_retries:
            infra_retries += 1
            continue
        break
    (out / "agent.log").write_text(f"exit={agent_exit}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}\n")
    result = base_result(scenario, cell, args, out, seconds, agent_exit)
    result["infraRetries"] = infra_retries
    result["networkEnforcement"] = "canary-checked" if cell["network"] == "off" and args.canary_cmd else "declared-only"
    result["fileWriteEnforcement"] = "trial-directory-only" if getattr(args, "file_sandbox", False) else "unrestricted"
    context = context_report(out, cell)
    result["context"] = context
    if context.get("unexpected"):
        result["invalid"] = f"unexpected loaded context: {context['unexpected']}"
    events = bench_trace.read_events(out / "wright-trace.jsonl")
    result["wrightUse"] = bench_trace.summarize_trace(events)
    result.update(bench_grade.grade(scenario, workspace, args.wright, out / "grading"))
    entry = workspace / scenario["entry"]
    final_sha = hashlib.sha256(entry.read_bytes()).hexdigest() if entry.is_file() else None
    result["friction"] = bench_trace.friction(events)
    result["expectations"] = bench_trace.detect_expectations(events, snaps, scenario, final_sha)
    result["snapshots"] = snapshot_validity(scenario, snaps, args.wright, out)
    first_valid = next((s["t"] for s in result["snapshots"]["series"] if s["valid"]), None)
    result["usage"] = bench_trace.usage_summary(out / "usage.jsonl", first_valid)
    (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def snapshot_validity(scenario: dict, snaps: list[dict], wright: str, out: Path) -> dict:
    """Strict validity of every snapshot of the entry file; regressions are valid -> invalid transitions."""
    series = []
    for snap in [s for s in snaps if s["file"] == scenario["entry"]]:
        valid = bench_grade.strict_valid(wright, Path(snap["path"]), out / "grading" / f"snapshot-{snap['i']:03d}")
        series.append({"i": snap["i"], "t": snap["t"], "valid": valid})
    regressions = sum(1 for a, b in zip(series, series[1:]) if a["valid"] and not b["valid"])
    return {"count": len(series), "firstValidIndex": next((s["i"] for s in series if s["valid"]), None), "regressions": regressions, "series": series}


def base_result(scenario: dict, cell: dict, args: argparse.Namespace, out: Path, seconds: float, agent_exit: int | None) -> dict:
    return {
        "contract": RESULT_CONTRACT,
        "scenario": scenario["id"],
        "family": scenario["family"],
        "language": scenario["language"],
        "split": scenario.get("split"),
        "condition": dict(cell),
        "agent": {"id": args.agent_id, "command": args.agent_cmd, "exit": agent_exit, "seconds": seconds},
        "environment": {
            "os": platform.platform(), "python": platform.python_version(),
            "wright": subprocess.run([args.wright, "--version"], capture_output=True, text=True).stdout.strip(),
            "timestamp": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            **({"wiki": bench_wiki.identity(Path(args.wiki_dir))} if cell["knowledge"] == "wiki" else {}),
            **({"wikiSkill": wiki_skill.identity(Path(args.wiki_skill_dir))} if cell["knowledge"] == "wiki-skill" else {}),
        },
    }


def trial_dir(base: Path, scenario_id: str, agent_id: str, cell: dict, trial: int) -> Path:
    return base / scenario_id / agent_id / f"{cell_label(cell).replace('/', '_')}-{trial}"


def cmd_run(args: argparse.Namespace) -> int:
    scenario = load_scenario(args.scenario)
    cell = {"wright": args.wright_level, "knowledge": args.knowledge, "network": args.network}
    ok = True
    for trial in range(1, args.trials + 1):
        result = run_trial(scenario, cell, args, trial_dir(args.out, args.scenario, args.agent_id, cell, trial))
        ok &= bool(result.get("passed")) and "invalid" not in result
        print(f"{args.scenario} {cell_label(cell)} trial {trial}: {'INVALID ' + result['invalid'] if 'invalid' in result else 'PASS' if result['passed'] else 'FAIL'}"
              f" layers={result.get('failedLayers')} wright-invocations={result.get('wrightUse', {}).get('invocations')}")
    return 0 if ok else 1


def cmd_matrix(args: argparse.Namespace) -> int:
    """Run every (scenario, agent, cell, trial) of a matrix file in randomized order; finished runs are skipped."""
    config = json.loads(args.config.read_text())
    jobs = [
        (s, agent, cell, t)
        for s in config.get("scenarios") or all_scenario_ids()
        for agent in config["agents"]
        for cell in config["cells"]
        for t in range(1, config.get("trials", 5) + 1)
    ]
    random.Random(config.get("seed", 0)).shuffle(jobs)

    def work(job: tuple) -> None:
        scenario_id, agent, cell, trial = job
        out = trial_dir(args.out, scenario_id, agent["id"], cell, trial)
        if (out / "result.json").is_file():
            return
        options = {k: Path(v) if k in ("skill_dir", "wiki_dir", "wiki_skill_dir") and v else v for k, v in config.get("options", {}).items()}
        trial_args = argparse.Namespace(**{**vars(args), "agent_id": agent["id"], "agent_cmd": agent["cmd"], **options})
        result = run_trial(load_scenario(scenario_id), cell, trial_args, out)
        print(f"{scenario_id} {agent['id']} {cell_label(cell)} #{trial}: {'INVALID' if 'invalid' in result else 'PASS' if result['passed'] else 'FAIL'}", flush=True)

    with ThreadPoolExecutor(max_workers=config.get("parallel", 2)) as pool:
        list(pool.map(work, jobs))
    return 0


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
    if len(sys.argv) > 1 and sys.argv[1] == "shim":
        return bench_trace.shim_main(sys.argv[2:])
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("validate", "run", "matrix"):
        p = sub.add_parser(name)
        p.add_argument("--wright", default=str(ROOT / "target/debug/wright"), help="Wright binary under test")
        p.add_argument("--out", type=Path, default=Path.home() / ".cache/wright-agent-bench", help="outside any repository, so agents cannot discover its instruction files")
    for name in ("run", "matrix"):
        p = sub.choices[name]
        p.add_argument("--skill-dir", type=Path, help="pinned guide directory, exposed to the adapter as BENCH_SKILL_DIR")
        p.add_argument("--wiki-dir", type=Path, help="pinned wiki snapshot, linked as ./wiki for knowledge 'wiki'; content hashes are verified")
        p.add_argument("--wiki-skill-dir", type=Path, help="built workshop-wiki skill, installed through the agent's skill mechanism for knowledge 'wiki-skill'")
        p.add_argument("--env-pass", nargs="*", default=[], help="host variables passed through the environment scrub")
        p.add_argument("--no-ancestor-check", dest="check_ancestors", action="store_false", help="skip the check for instruction files above the workspace")
        p.add_argument("--canary-cmd", help="shell command that must fail in the agent environment when the network is 'off'")
        p.add_argument("--timeout", type=int, default=1800)
        p.add_argument("--file-sandbox", action="store_true", help="macOS: restrict agent and descendant file writes to the trial directory")
        p.add_argument("--infra-retries", type=int, default=2, help="retries when the agent exits 75 (provider or infrastructure failure)")
    run = sub.choices["run"]
    run.add_argument("scenario", choices=all_scenario_ids())
    run.add_argument("--agent-cmd", required=True, help="shell command; the task prompt arrives on stdin, cwd is the workspace, BENCH_* describes the condition")
    run.add_argument("--agent-id", required=True, help="recorded agent/model/version label")
    run.add_argument("--wright-level", choices=WRIGHT_LEVELS, default="bin")
    run.add_argument("--knowledge", choices=KNOWLEDGE_LEVELS, default="none")
    run.add_argument("--network", choices=("off", "on"), default="off")
    run.add_argument("--trials", type=int, default=1)
    sub.choices["matrix"].add_argument("config", type=Path, help="JSON: agents[{id,cmd}], cells[{wright,knowledge,network}], scenarios, trials, parallel, seed, options")
    sub.add_parser("setup-oracle", help="install the pinned upstream OverPy oracle")
    skill = sub.add_parser("wiki-skill", help="build the progressive-disclosure workshop-wiki skill from a wiki snapshot")
    skill.add_argument("--snapshot", type=Path, required=True)
    skill.add_argument("--out-dir", type=Path, required=True, help="new skill directory (not overwritten)")
    skill.add_argument("--catalog", type=Path, required=True, help="workshop-rs catalog.json, for Workshop names and ids")
    skill.add_argument("--opy-manifest", type=Path, required=True, help="opy-rs manifest.json, for upstream OverPy spellings")
    wiki = sub.add_parser("wiki-snapshot", help="fetch the Workshop wiki Markdown mirror into a pinned local snapshot")
    wiki.add_argument("--dir", type=Path, default=Path.home() / ".cache/wright-agent-bench-wiki")
    wiki.add_argument("--base", default=bench_wiki.BASE)
    wiki.add_argument("--categories", nargs="+", default=list(bench_wiki.CATEGORIES), help="wiki categories to crawl (add tutorials for the second tier)")
    report = sub.add_parser("report", help="summarize result.json files")
    report.add_argument("dirs", nargs="+", type=Path)
    report.add_argument("--regrade", action="store_true", help="re-grade stored workspaces twice and flag unstable graders")
    report.add_argument("--wright", default=str(ROOT / "target/debug/wright"))
    args = parser.parse_args()
    if hasattr(args, "wright"):
        args.wright = str(Path(args.wright).resolve())
    if hasattr(args, "out"):
        args.out = args.out.resolve()
    if args.command == "validate":
        return 0 if validate(args.wright, args.out) else 1
    if args.command == "setup-oracle":
        return cmd_setup_oracle(args)
    if args.command == "wiki-snapshot":
        return cmd_wiki_snapshot(args)
    if args.command == "wiki-skill":
        return cmd_wiki_skill(args)
    if args.command == "report":
        return bench_report.main(args.dirs, args.wright, args.regrade, lambda s: load_scenario(s))
    return cmd_run(args) if args.command == "run" else cmd_matrix(args)


if __name__ == "__main__":
    sys.exit(main())
