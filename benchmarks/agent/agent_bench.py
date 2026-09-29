#!/usr/bin/env python3
"""Wright agent benchmark harness (#414). Contract: docs/agent-benchmark.md."""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import signal
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
SCENARIOS = HERE / "scenarios"
RESULT_CONTRACT = "wright-agent-bench/v1"
SHIM_ENV = ("WRIGHT_BENCH_REAL", "WRIGHT_BENCH_TRACE")


def load_scenario(scenario_id: str) -> dict:
    directory = SCENARIOS / scenario_id
    scenario = json.loads((directory / "scenario.json").read_text())
    scenario["dir"] = directory
    return scenario


def all_scenario_ids() -> list[str]:
    return sorted(p.name for p in SCENARIOS.iterdir() if (p / "scenario.json").is_file())


def wright_json(wright: str, args: list[str]) -> tuple[int, dict]:
    proc = subprocess.run([wright, *args, "--format", "json"], capture_output=True, text=True)
    try:
        return proc.returncode, json.loads(proc.stdout)
    except json.JSONDecodeError:
        return proc.returncode, {"diagnostics": [{"code": "harness-output", "severity": "error", "message": proc.stderr.strip() or proc.stdout.strip()}]}


def serve_request(wright: str, entry: Path, request: dict) -> dict:
    proc = subprocess.run([wright, "serve", str(entry)], input=json.dumps(request) + "\n", capture_output=True, text=True)
    try:
        return json.loads(proc.stdout.splitlines()[0])
    except (IndexError, json.JSONDecodeError):
        return {"error": {"code": "harness-output", "message": proc.stderr.strip()}}


def check_result(check: dict, passed: bool, detail: str) -> dict:
    return {"id": check["id"], "kind": check["kind"], "layer": check.get("layer", "agent"), "passed": passed, "detail": detail}


def run_check(check: dict, workspace: Path, entry: Path, wright: str, state: dict) -> dict:
    kind = check["kind"]
    if kind == "check":
        code, envelope = wright_json(wright, ["check", str(entry)])
        state["diagnostics"] = envelope.get("diagnostics", [])
        errors = [d for d in state["diagnostics"] if d.get("severity") == "error"]
        return check_result(check, code == 0 and not errors, f"exit {code}, {len(errors)} error diagnostic(s)")
    if kind == "lint":
        _, envelope = wright_json(wright, ["lint", str(entry)])
        findings = [f for f in envelope.get("result", {}).get("findings", []) if f["code"] == check["code"]]
        return check_result(check, len(findings) <= check.get("max", 0), f"{len(findings)} '{check['code']}' finding(s)")
    if kind == "symbols":
        response = serve_request(wright, entry, {"op": "symbols", "kind": check["symbolKind"]})
        found = len(response.get("result", []))
        return check_result(check, found >= check.get("min", 1), f"{found} '{check['symbolKind']}' symbol(s)")
    if kind in ("contains", "absent"):
        path = workspace / check["file"]
        text = path.read_text() if path.is_file() else ""
        texts = check["text"] if isinstance(check["text"], list) else [check["text"]]
        count = sum(text.count(t) for t in texts)
        if kind == "contains":
            passed = count >= check.get("min", 1) and count <= check.get("max", count)
        else:
            passed = count == 0
        return check_result(check, passed, f"{count} occurrence(s) in {check['file']}")
    if kind == "answer":
        path = workspace / "answer.json"
        try:
            actual = json.loads(path.read_text()).get(check["key"])
        except (OSError, json.JSONDecodeError, AttributeError):
            actual = None
        return check_result(check, actual == check["expected"], f"answer[{check['key']}] = {json.dumps(actual)}")
    raise SystemExit(f"unknown check kind '{kind}'")


def tree(root: Path) -> dict[str, bytes]:
    return {str(p.relative_to(root)): p.read_bytes() for p in sorted(root.rglob("*")) if p.is_file()}


def unsafe_edits(scenario: dict, workspace: Path) -> list[str]:
    seed = tree(scenario["dir"] / "seed")
    now = tree(workspace)
    writable = set(scenario["writable"])
    return sorted(name for name in seed.keys() | now.keys() if seed.get(name) != now.get(name) and name not in writable)


def grade(scenario: dict, workspace: Path, wright: str) -> dict:
    entry = workspace / scenario["entry"]
    state: dict = {}
    checks = [run_check(c, workspace, entry, wright, state) for c in scenario["checks"]]
    return {
        "checks": checks,
        "passed": all(c["passed"] for c in checks),
        "failedLayers": sorted({c["layer"] for c in checks if not c["passed"]}),
        "diagnostics": state.get("diagnostics", []),
        "unsafeEdits": unsafe_edits(scenario, workspace),
        "unverifiedRuntimeClaims": scenario.get("runtimeOnly", []),
    }


def materialize(scenario: dict, workspace: Path, overlay: str | None = None) -> None:
    shutil.copytree(scenario["dir"] / "seed", workspace)
    if overlay:
        shutil.copytree(scenario["dir"] / overlay, workspace, dirs_exist_ok=True)


def validate(wright: str, out: Path) -> bool:
    """Each scenario must be solvable by the reference and unsolved by its seed."""
    ok = True
    for scenario_id in all_scenario_ids():
        scenario = load_scenario(scenario_id)
        results = {}
        for name, overlay in (("seed", None), ("reference", "reference")):
            workspace = out / scenario_id / name
            shutil.rmtree(workspace, ignore_errors=True)
            materialize(scenario, workspace, overlay)
            results[name] = grade(scenario, workspace, wright)
        good = results["reference"]["passed"] and not results["seed"]["passed"] and not results["reference"]["unsafeEdits"]
        if not good:
            ok = False
            failed = [c for c in results["reference"]["checks"] if not c["passed"]]
            print(f"INVALID {scenario_id}: seed passed={results['seed']['passed']}, reference failures={failed}, unsafe={results['reference']['unsafeEdits']}")
        else:
            print(f"ok      {scenario_id}")
    return ok


def baseline_path(path: str) -> str:
    """PATH without any directory that provides a `wright` executable."""
    kept = [d for d in path.split(os.pathsep) if d and not shutil.which("wright", path=d)]
    return os.pathsep.join(kept)


def shim_main(argv: list[str]) -> int:
    start = time.monotonic()
    code = subprocess.call([os.environ["WRIGHT_BENCH_REAL"], *argv])
    with open(os.environ["WRIGHT_BENCH_TRACE"], "a") as trace:
        trace.write(json.dumps({"argv": argv, "exit": code, "seconds": round(time.monotonic() - start, 3)}) + "\n")
    return code


def summarize_trace(trace: Path) -> dict:
    calls = [json.loads(line) for line in trace.read_text().splitlines()] if trace.is_file() else []
    commands = [next((a for a in c["argv"] if not a.startswith("-")), "") for c in calls]
    by_command: dict[str, int] = {}
    for command in commands:
        by_command[command] = by_command.get(command, 0) + 1
    return {
        "invocations": len(calls),
        "byCommand": by_command,
        "failedInvocations": sum(1 for c in calls if c["exit"] != 0),
        "ownerOrEnvironmentGaps": [c for c in calls if c["exit"] >= 3],
    }


def run_trial(scenario: dict, condition: str, args: argparse.Namespace, out: Path) -> dict:
    shutil.rmtree(out, ignore_errors=True)
    workspace = out / "workspace"
    materialize(scenario, workspace)
    prompt = (scenario["dir"] / "prompt.md").read_text()
    trace = out / "wright-trace.jsonl"
    env = {k: v for k, v in os.environ.items() if k not in SHIM_ENV}
    if condition == "wright":
        shim_dir = out / "bin"
        shim_dir.mkdir(parents=True)
        shim = shim_dir / "wright"
        shim.write_text(f'#!/bin/sh\nexec "{sys.executable}" "{Path(__file__).resolve()}" shim "$@"\n')
        shim.chmod(0o755)
        env.update(WRIGHT_BENCH_REAL=args.wright, WRIGHT_BENCH_TRACE=str(trace), PATH=f"{shim_dir}{os.pathsep}{baseline_path(env['PATH'])}")
    else:
        env["PATH"] = baseline_path(env["PATH"])
    start = time.monotonic()
    proc = subprocess.Popen(
        args.agent_cmd, shell=True, cwd=workspace, env=env, text=True,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        start_new_session=True,
    )
    try:
        stdout, stderr = proc.communicate(input=prompt, timeout=args.timeout)
        agent_exit = proc.returncode
    except subprocess.TimeoutExpired:
        agent_exit = None
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        stdout, stderr = proc.communicate()
        stderr = f"{stderr}\ntimeout" if stderr else "timeout"
    seconds = round(time.monotonic() - start, 1)
    (out / "agent.log").write_text(f"exit={agent_exit}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}\n")
    wright_version = subprocess.run([args.wright, "--version"], capture_output=True, text=True).stdout.strip()
    result = {
        "contract": RESULT_CONTRACT,
        "scenario": scenario["id"],
        "family": scenario["family"],
        "language": scenario["language"],
        "condition": condition,
        "agent": {"id": args.agent_id, "command": args.agent_cmd, "exit": agent_exit, "seconds": seconds},
        "environment": {"os": platform.platform(), "python": platform.python_version(), "wright": wright_version, "timestamp": datetime.now(timezone.utc).isoformat(timespec="seconds")},
        "wrightUse": summarize_trace(trace),
        **grade(scenario, workspace, args.wright),
    }
    (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def cmd_run(args: argparse.Namespace) -> int:
    scenario = load_scenario(args.scenario)
    ok = True
    for condition in args.conditions:
        for trial in range(1, args.trials + 1):
            result = run_trial(scenario, condition, args, args.out / args.scenario / f"{condition}-{trial}")
            ok &= result["passed"]
            print(f"{args.scenario} {condition} trial {trial}: {'PASS' if result['passed'] else 'FAIL'} layers={result['failedLayers']} wright-invocations={result['wrightUse']['invocations']}")
    return 0 if ok else 1


def main() -> int:
    if len(sys.argv) > 1 and sys.argv[1] == "shim":
        return shim_main(sys.argv[2:])
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("validate", "run"):
        p = sub.add_parser(name)
        p.add_argument("--wright", default=str(ROOT / "target/debug/wright"), help="Wright binary under test")
        p.add_argument("--out", type=Path, default=ROOT / "target/agent-bench")
    sub.choices["run"].add_argument("scenario", choices=all_scenario_ids())
    sub.choices["run"].add_argument("--agent-cmd", required=True, help="shell command; the task prompt arrives on stdin, cwd is the workspace")
    sub.choices["run"].add_argument("--agent-id", required=True, help="recorded agent/model/version label")
    sub.choices["run"].add_argument("--conditions", nargs="+", choices=("baseline", "wright"), default=["baseline", "wright"])
    sub.choices["run"].add_argument("--trials", type=int, default=1)
    sub.choices["run"].add_argument("--timeout", type=int, default=1800)
    args = parser.parse_args()
    args.wright = str(Path(args.wright).resolve())
    if args.command == "validate":
        return 0 if validate(args.wright, args.out) else 1
    return cmd_run(args)


if __name__ == "__main__":
    sys.exit(main())
