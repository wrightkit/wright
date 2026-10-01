"""Grading for the agent benchmark (#414, SPEC-414): check kinds, validity authorities, disagreement."""

from __future__ import annotations

import hashlib
import json
import re
import shutil
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
ORACLE = HERE / "oracle"
GRADER_FILES = ("bench_grade.py", "oracle/compile.js", "oracle/package-lock.json")
SUITE_VERSION = "v1"
UNSAFE_IGNORED = ("wiki", ".agents", ".devin", ".opencode")  # linked wiki and skills installed through the agent's own mechanism


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


def oracle_available() -> bool:
    return bool(shutil.which("node")) and (ORACLE / "node_modules/overpy/package.json").is_file()


def oracle_version() -> str | None:
    if not oracle_available():
        return None
    return json.loads((ORACLE / "node_modules/overpy/package.json").read_text())["version"]


def oracle_compile(source: Path, out: Path) -> dict:
    """Compile with the pinned upstream compiler; `unavailable` is explicit, never a pass."""
    if not oracle_available():
        return {"status": "unavailable"}
    proc = subprocess.run(["node", str(ORACLE / "compile.js"), str(source), str(out)], capture_output=True, text=True)
    try:
        reply = json.loads(proc.stdout.strip().splitlines()[-1])
    except (IndexError, json.JSONDecodeError):
        return {"status": "error", "error": (proc.stderr or proc.stdout).strip()[:300]}
    return {"status": "ok"} if reply.get("ok") else {"status": "error", "error": reply.get("error")}


def wright_compile(wright: str, source: Path, out: Path) -> dict:
    proc = subprocess.run([wright, "compile", str(source), "-o", str(out)], capture_output=True, text=True)
    if proc.returncode == 0:
        return {"status": "ok"}
    return {"status": "error", "error": proc.stderr.strip()[:300]}


def authorities(wright: str, source: Path, scratch: Path) -> dict:
    """Validity per authority, kept separate. Disagreement between upstream and Wright is an output."""
    scratch.mkdir(parents=True, exist_ok=True)
    wright_out = scratch / "wright.txt"
    result: dict = {
        "wrightCheck": subprocess.run([wright, "check", str(source)], capture_output=True).returncode == 0,
        "wrightCompile": wright_compile(wright, source, wright_out),
    }
    if result["wrightCompile"]["status"] == "ok":
        result["workshopCheck"] = subprocess.run([wright, "check", str(wright_out)], capture_output=True).returncode == 0
    if source.suffix == ".opy":
        result["oracle"] = oracle_compile(source, scratch / "oracle.txt")
        known = {"ok", "error"}
        theirs, ours = result["oracle"]["status"], result["wrightCompile"]["status"]
        if theirs in known and ours in known and theirs != ours:
            result["disagreement"] = {
                "kind": "wright-accepts-oracle-rejects" if ours == "ok" else "oracle-accepts-wright-rejects",
                "source": source.name,
                "wright": result["wrightCompile"],
                "oracle": result["oracle"],
            }
    return result


def missing_entry_authorities(entry: Path) -> dict:
    """The agent produced no entry file: every authority rejects it, and the oracle is only unavailable when it is not installed."""
    result: dict = {"wrightCompile": {"status": "error", "error": "entry file missing"}}
    if entry.suffix == ".opy" and oracle_available():
        result["oracle"] = {"status": "error", "error": "entry file missing"}
    return result


def compiled_text(state: dict, source: str) -> str | None:
    """Compiled Workshop text from `wright` or the upstream `oracle`, when that authority succeeded."""
    auth = state["authorities"]
    if source == "oracle":
        return (state["scratch"] / "oracle.txt").read_text() if auth.get("oracle", {}).get("status") == "ok" else None
    return (state["scratch"] / "wright.txt").read_text() if auth["wrightCompile"]["status"] == "ok" else None


def check_result(check: dict, passed: bool, detail: str, **extra) -> dict:
    return {"id": check["id"], "kind": check["kind"], "layer": check.get("layer", "agent"), "passed": passed, "detail": detail, **extra}


def matches(text: str, check: dict) -> bool:
    flags = re.I if check.get("ignoreCase") else 0
    return all(re.search(p, text, flags) for p in check.get("all", [])) and (
        not check.get("any") or any(re.search(p, text, flags) for p in check["any"])
    )


def run_check(check: dict, workspace: Path, entry: Path, wright: str, state: dict) -> dict:
    kind = check["kind"]
    if kind == "check":
        code, envelope = wright_json(wright, ["check", str(entry)])
        state["diagnostics"] = envelope.get("diagnostics", [])
        errors = [d for d in state["diagnostics"] if d.get("severity") == "error"]
        return check_result(check, code == 0 and not errors, f"exit {code}, {len(errors)} error diagnostic(s)")
    if kind == "lint":
        findings = [f for f in state["lint"] if f["code"] == check["code"]]
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
    if kind == "oracle":
        status = state["authorities"].get("oracle", {"status": "unavailable"})
        detail = status.get("error") or status["status"]
        if status["status"] == "unavailable":
            detail = "upstream oracle not installed; run `agent_bench.py setup-oracle`"
        return check_result(check, status["status"] == "ok", detail, unavailable=status["status"] == "unavailable")
    if kind == "wright-compile":
        status = state["authorities"]["wrightCompile"]
        return check_result(check, status["status"] == "ok", status.get("error") or status["status"])
    if kind in ("compiled-contains", "compiled-absent"):
        source = check.get("source", "oracle" if entry.suffix == ".opy" else "wright")
        text = compiled_text(state, source)
        if text is None:
            return check_result(check, False, f"no compiled text from {source}", unavailable=source == "oracle" and not oracle_available())
        found = matches(text, check)
        return check_result(check, found if kind == "compiled-contains" else not found, f"{'matched' if found else 'no match'} in {source} output")
    raise SystemExit(f"unknown check kind '{kind}'")


def tree(root: Path) -> dict[str, bytes]:
    return {str(p.relative_to(root)): p.read_bytes() for p in sorted(root.rglob("*")) if p.is_file()}


def unsafe_edits(scenario: dict, workspace: Path) -> list[str]:
    seed = tree(scenario["dir"] / "seed")
    now = tree(workspace)
    writable = set(scenario["writable"])
    return sorted(
        name for name in seed.keys() | now.keys()
        if seed.get(name) != now.get(name) and name not in writable and name.split("/")[0] not in UNSAFE_IGNORED
    )


def grader_hash(scenario: dict) -> str:
    digest = hashlib.sha256()
    for path in [scenario["dir"] / "scenario.json", *(HERE / f for f in GRADER_FILES)]:
        digest.update(path.name.encode() + (path.read_bytes() if path.is_file() else b""))
    return digest.hexdigest()


def suite_identity(scenarios: Path) -> dict:
    """Version and content hash of the whole suite: every scenario file, the grader sources, and the oracle lock."""
    digest = hashlib.sha256()
    files = sorted(p for p in scenarios.rglob("*") if p.is_file())
    for path in files:
        digest.update(str(path.relative_to(scenarios)).encode() + b"\0" + path.read_bytes())
    for name in GRADER_FILES:
        path = HERE / name
        digest.update(name.encode() + b"\0" + (path.read_bytes() if path.is_file() else b""))
    return {"version": SUITE_VERSION, "hash": digest.hexdigest(), "scenarios": sum(1 for p in scenarios.iterdir() if (p / "scenario.json").is_file())}


def usable_verdict(checks: list[dict], lint_errors: int, unsafe: list[str]) -> list[str]:
    """Blocking conditions of `usable`. Empty means usable: all checks pass, no error-severity lint finding, no edit outside
    the writable files, and every required grader was available."""
    blocking = []
    if not all(c["passed"] for c in checks):
        blocking.append("checks-failed")
    if lint_errors:
        blocking.append("lint-error")
    if unsafe:
        blocking.append("unsafe-edits")
    if any(c.get("unavailable") for c in checks):
        blocking.append("grader-unavailable")
    return blocking


def grade(scenario: dict, workspace: Path, wright: str, scratch: Path | None = None) -> dict:
    entry = workspace / scenario["entry"]
    scratch = scratch or workspace.parent / f"{workspace.name}-grading"
    shutil.rmtree(scratch, ignore_errors=True)
    state: dict = {"scratch": scratch, "authorities": authorities(wright, entry, scratch) if entry.is_file() else missing_entry_authorities(entry)}
    _, lint = wright_json(wright, ["lint", str(entry)])
    state["lint"] = (lint.get("result") or {}).get("findings") or []
    checks = [run_check(c, workspace, entry, wright, state) for c in scenario["checks"]]
    lint_errors = sum(1 for f in state["lint"] if f.get("severity") == "error")
    passed = all(c["passed"] for c in checks)
    unsafe = unsafe_edits(scenario, workspace)
    blocking = usable_verdict(checks, lint_errors, unsafe)
    auth = state["authorities"]
    return {
        "checks": checks,
        "passed": passed,
        "usable": not blocking,
        "usableReason": blocking,
        "failedLayers": sorted({c["layer"] for c in checks if not c["passed"]}),
        "diagnostics": state.get("diagnostics", []),
        "authorities": {k: v for k, v in auth.items() if k != "disagreement"},
        "disagreement": auth.get("disagreement"),
        "lintFindings": sorted({f["code"] for f in state["lint"]}),
        "unsafeEdits": unsafe,
        "unverifiedRuntimeClaims": scenario.get("runtimeOnly", []),
        "grader": {"hash": grader_hash(scenario), "oracle": oracle_version()},
    }


def strict_valid(wright: str, source: Path, scratch: Path) -> bool:
    """Validity of one source snapshot: the upstream oracle when it applies, else Wright."""
    auth = authorities(wright, source, scratch)
    if source.suffix == ".opy" and auth["oracle"]["status"] != "unavailable":
        return auth["oracle"]["status"] == "ok"
    return auth["wrightCompile"]["status"] == "ok"
