#!/usr/bin/env python3
"""Agent-facing drift metrics for Wright (#530).

Measures the output an agent-facing surface produces for every
``wright-agent/v1`` read operation and every common CLI command, per project
in a fixed corpus (``corpus.json``). Recorded per metric:

- ``bytes``: response/stdout size in bytes;
- ``tokens``: ``bytes / 4`` (the benchmark's estimate);
- ``latencyMs``: median wall time over ``--repeats`` runs;
- ``counts``: result-shape counts (items, per-kind, per-finding-code).

Commands:

- ``fetch``: clone the pinned ``repository`` entries into ``.cache/``;
- ``run``:   measure the corpus and write a ``wright-agent-metrics/v1`` file;
- ``check``: run, then compare against the committed baseline
  (``baseline-<version>.json``). Exits 1 when any metric leaves its band —
  tokens and counts: |delta| > 15% AND > 30 absolute; latency: |delta| > 50%
  AND > 1 ms absolute (sub-millisecond medians are measurement noise).
  Improvements are reported alongside regressions. ``check`` never rewrites
  the baseline; accepting a change means editing ``baseline-<version>.json``
  in a reviewed PR that documents old/new values and the reason.

Repository corpus entries are cloned shallow at their pinned commit; their
source is never vendored. Derived metrics are the only stored artifact.
"""

from __future__ import annotations

import argparse
import json
import os
import queue
import shutil
import statistics
import subprocess
import sys
import threading
import time
from pathlib import Path

CONTRACT = "wright-agent-metrics/v1"
CORPUS_CONTRACT = "wright-agent-metrics-corpus/v1"
TOKEN_BYTES = 4
SIZE_REL_BAND = 0.15
SIZE_ABS_BAND = 30.0
LATENCY_REL_BAND = 0.50
# Median latency below ~1 ms is measurement noise at our timer resolution;
# require a minimum absolute delta before the relative band applies.
LATENCY_ABS_BAND = 1.0
REQUEST_TIMEOUT_S = 180.0

METRICS_DIR = Path(__file__).resolve().parent
CACHE_DIR = METRICS_DIR / ".cache"

# Every wright-agent/v1 read operation, in measurement order. Mutation
# operations (semanticRename, validateEditTransaction, provider*) need
# caller-supplied transactions and are not measured.
AGENT_OPS = [
    "capabilities",
    "targetMetadata",
    "project",
    "rules",
    "symbols",
    "references",
    "usage",
    "cfg",
    "findings",
    "persistentObjects",
    "lint",
    "lintRules",
    "callGraph",
    "costEstimate",
    "check",
    "analyze",
    "inspect",
    "compile",
]

CLI_COMMANDS = ["check", "lint", "analyze", "inspect", "compile"]


class ServeSession:
    """One `wright serve --transport stdio` process with line requests."""

    def __init__(self, wright: str, entry: Path, kind: str, cwd: Path):
        self.process = subprocess.Popen(
            [wright, "serve", str(entry), "--kind", kind, "--transport", "stdio"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            cwd=cwd,
        )
        self.lines: queue.Queue[str] = queue.Queue()
        self._reader = threading.Thread(target=self._pump, daemon=True)
        self._reader.start()
        self._next_id = 0

    def _pump(self) -> None:
        assert self.process.stdout is not None
        for line in self.process.stdout:
            self.lines.put(line)

    def request(self, op: str, **params) -> dict:
        self._next_id += 1
        body = {"id": self._next_id, "op": op, "contract": "wright-agent/v1"}
        body.update(params)
        assert self.process.stdin is not None
        self.process.stdin.write(json.dumps(body) + "\n")
        self.process.stdin.flush()
        line = self.lines.get(timeout=REQUEST_TIMEOUT_S)
        return json.loads(line)

    def request_timed(self, op: str, **params) -> tuple[str, float]:
        """One request; returns (raw line, wall ms)."""
        self._next_id += 1
        body = {"id": self._next_id, "op": op, "contract": "wright-agent/v1"}
        body.update(params)
        assert self.process.stdin is not None
        start = time.monotonic()
        self.process.stdin.write(json.dumps(body) + "\n")
        self.process.stdin.flush()
        line = self.lines.get(timeout=REQUEST_TIMEOUT_S)
        elapsed_ms = (time.monotonic() - start) * 1000.0
        return line.rstrip("\n"), elapsed_ms

    def close(self) -> None:
        if self.process.stdin is not None:
            try:
                self.process.stdin.close()
            except OSError:
                pass
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()

    def __enter__(self) -> "ServeSession":
        return self

    def __exit__(self, *_exc) -> None:
        self.close()


def load_corpus(manifest: Path) -> dict:
    corpus = json.loads(manifest.read_text())
    if corpus.get("contract") != CORPUS_CONTRACT:
        raise SystemExit(
            f"{manifest}: expected contract {CORPUS_CONTRACT}, got {corpus.get('contract')!r}"
        )
    return corpus


def fetch_project(project: dict) -> Path:
    """Return the project directory, cloning pinned repositories if needed."""
    if "repository" not in project:
        directory = (METRICS_DIR / project["path"]).resolve()
        if not directory.is_dir():
            raise FileNotFoundError(f"corpus project directory missing: {directory}")
        return directory

    directory = CACHE_DIR / project["id"]
    commit = project["commit"]
    head = subprocess.run(
        ["git", "-C", str(directory), "rev-parse", "HEAD"],
        capture_output=True,
        text=True,
    )
    if head.returncode == 0 and head.stdout.strip() == commit:
        return directory
    directory.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["git", "-C", str(directory), "init", "-q"], check=True, capture_output=True
    )
    remote = subprocess.run(
        ["git", "-C", str(directory), "remote", "get-url", "origin"],
        capture_output=True,
        text=True,
    )
    if remote.returncode != 0:
        subprocess.run(
            ["git", "-C", str(directory), "remote", "add", "origin", project["repository"]],
            check=True,
            capture_output=True,
        )
    subprocess.run(
        ["git", "-C", str(directory), "fetch", "-q", "--depth", "1", "origin", commit],
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(directory), "checkout", "-q", "FETCH_HEAD"],
        check=True,
    )
    return directory


def _finding_counts(findings: list) -> dict:
    counts: dict[str, float] = {"items": len(findings)}
    for finding in findings:
        code = finding.get("code") if isinstance(finding, dict) else None
        if isinstance(code, str) and code:
            counts[f"finding.{code}"] = counts.get(f"finding.{code}", 0) + 1
    return counts


def _typed_items(items: list, label: str) -> dict:
    counts: dict[str, float] = {"items": len(items)}
    for item in items:
        kind = item.get("kind") if isinstance(item, dict) else None
        if isinstance(kind, str) and kind:
            key = f"{label}.{kind}"
            counts[key] = counts.get(key, 0) + 1
    return counts


def _numeric_fields(result: dict, fields: list[str], prefix: str = "") -> dict:
    return {
        f"{prefix}{field}": result[field]
        for field in fields
        if isinstance(result.get(field), (int, float))
        and not isinstance(result.get(field), bool)
    }


def agent_counts(op: str, result) -> dict:
    """Result-shape counts for one agent operation's `result` payload."""
    if not isinstance(result, (list, dict)):
        return {}
    if isinstance(result, list):
        if op == "symbols":
            return _typed_items(result, "symbol")
        if op == "references":
            return _typed_items(result, "reference")
        if op in ("rules", "persistentObjects", "callGraph"):
            return _typed_items(result, "item")
        return {"items": len(result)}
    if op == "project":
        return _numeric_fields(
            result,
            [
                "files",
                "findings",
                "globalVariables",
                "playerVariables",
                "rules",
                "subroutines",
                "symbols",
            ],
        )
    if op == "usage":
        return _numeric_fields(result, ["reads", "writes", "calls", "rules"])
    if op == "cfg":
        return {"blocks": len(result.get("blocks") or [])}
    if op in ("findings", "lint"):
        return _finding_counts(result.get("findings") or [])
    if op == "costEstimate":
        counts = _finding_counts(result.get("findings") or [])
        counts.update(
            _numeric_fields(
                result.get("exact") or {},
                ["programActions", "programRules", "waitActions"],
            )
        )
        return counts
    if op == "lintRules":
        return {"rules": len(result.get("rules") or [])}
    if op in ("check", "analyze", "inspect", "compile"):
        # These operations return the CLI result envelope
        # ({command, diagnostics, exit, ok, result, wright}).
        return cli_counts(op, result)
    return {}


def cli_counts(command: str, envelope: dict) -> dict:
    """Result-shape counts for one CLI command's JSON envelope."""
    if not isinstance(envelope, dict):
        return {}
    counts: dict[str, float] = {"diagnostics": len(envelope.get("diagnostics") or [])}
    result = envelope.get("result")
    if not isinstance(result, dict):
        return counts
    if command == "lint":
        counts.update(
            {f"lint.{k}": v for k, v in _finding_counts(result.get("findings") or []).items()}
        )
    elif command == "analyze":
        facts = result.get("facts") or {}
        for key, value in facts.items():
            if isinstance(value, list):
                counts[f"facts.{key}"] = len(value)
        counts.update(
            _numeric_fields(
                result.get("program") or {},
                ["files", "findings", "globalVariables", "playerVariables", "rules", "subroutines"],
                "program.",
            )
        )
    elif command == "inspect":
        for key in ("symbols", "rules", "references"):
            value = result.get(key)
            if isinstance(value, list):
                counts[key] = len(value)
        counts.update(
            _numeric_fields(
                result.get("program") or {},
                ["files", "findings", "globalVariables", "playerVariables", "rules", "subroutines"],
                "program.",
            )
        )
    elif command == "compile":
        text = (result.get("output") or {}).get("text")
        if isinstance(text, str):
            counts["emittedBytes"] = len(text)
    return counts


def metric_record(payloads_bytes: list[int], latencies_ms: list[float], counts: dict) -> dict:
    raw = statistics.median(payloads_bytes)
    return {
        "bytes": raw,
        "tokens": round(raw / TOKEN_BYTES, 1),
        "latencyMs": round(statistics.median(latencies_ms), 1),
        "counts": counts,
    }


def run_agent_ops(
    wright: str, entry: Path, kind: str, project_dir: Path, repeats: int
) -> dict:
    metrics: dict[str, dict] = {}
    targets: dict[str, dict] = {}
    with ServeSession(wright, entry, kind, project_dir) as session:
        for op in AGENT_OPS:
            key = f"agent:{op}"
            if op in ("references", "usage", "cfg") and op not in targets:
                metrics[key] = {"skipped": "no addressable symbol/rule"}
                continue
            try:
                lines_bytes: list[int] = []
                latencies: list[float] = []
                counts: dict = {}
                last_result = None
                last_error: dict | None = None
                for _ in range(repeats):
                    line, elapsed = session.request_timed(op, **targets.get(op, {}))
                    lines_bytes.append(len(line.encode("utf-8")))
                    latencies.append(elapsed)
                    response = json.loads(line)
                    if "error" in response:
                        last_error = response["error"]
                        break
                    last_result = response.get("result")
                    counts = agent_counts(op, last_result)
                if last_error is not None:
                    metrics[key] = {
                        "error": last_error.get("code", "error"),
                        "message": last_error.get("message", ""),
                    }
                    continue
                metrics[key] = metric_record(lines_bytes, latencies, counts)
                if op == "symbols":
                    sid = _first_id(last_result)
                    if sid is not None:
                        targets["references"] = {"symbol": sid}
                        targets["usage"] = {"symbol": sid}
                if op == "rules":
                    rid = _first_id(last_result)
                    if rid is not None:
                        targets["cfg"] = {"rule": rid}
            except (queue.Empty, TimeoutError):
                metrics[key] = {"error": "timeout"}
                break
    return metrics


def _first_id(items) -> int | None:
    """Numeric id of the first listed item — unambiguous within the session
    where a name may resolve to several rules/symbols (`ambiguous-rule`)."""
    for item in items if isinstance(items, list) else []:
        ident = item.get("id") if isinstance(item, dict) else None
        if isinstance(ident, int):
            return ident
    return None


def run_cli_commands(
    wright: str, entry: Path, kind: str, project_dir: Path, repeats: int
) -> dict:
    metrics: dict[str, dict] = {}
    for command in CLI_COMMANDS:
        outputs_bytes: list[int] = []
        latencies: list[float] = []
        counts: dict = {}
        for _ in range(repeats):
            start = time.monotonic()
            completed = subprocess.run(
                [
                    wright,
                    command,
                    str(entry),
                    "--kind",
                    kind,
                    "-f",
                    "json",
                ],
                capture_output=True,
                cwd=project_dir,
            )
            latencies.append((time.monotonic() - start) * 1000.0)
            outputs_bytes.append(len(completed.stdout))
            try:
                counts = cli_counts(command, json.loads(completed.stdout))
            except json.JSONDecodeError:
                counts = {"unparsedStdout": len(completed.stdout)}
        metrics[f"cli:{command}"] = metric_record(outputs_bytes, latencies, counts)
    return metrics


def wright_version(wright: str) -> str:
    try:
        out = subprocess.run(
            [wright, "--version"], capture_output=True, text=True, timeout=10
        )
        return out.stdout.strip().splitlines()[0] if out.stdout.strip() else "unknown"
    except (OSError, subprocess.SubprocessError):
        return "unknown"


def _opy_provider_store() -> Path:
    """Mirror wright-driver's `default_store_dir` resolution (read-only)."""
    override = os.environ.get("WRIGHT_PROVIDER_DATA_DIR")
    if override:
        return Path(override) / "providers" / "opy"
    if sys.platform == "darwin":
        base = Path.home() / "Library" / "Application Support"
    elif sys.platform.startswith("win"):
        base = Path(os.environ.get("APPDATA", "."))
    else:
        base = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local"))
    return base / "wright" / "providers" / "opy"


def provider_version() -> str | None:
    """Active OPY provider version, resolved like wright resolves it."""
    try:
        store = _opy_provider_store()
        active = store / "active"
        if active.is_symlink() or active.is_dir():
            return Path(os.readlink(active)).name if active.is_symlink() else None
        if active.is_file():
            return active.read_text().strip() or None
    except OSError:
        pass
    return None


def run_metrics(wright: str, corpus: dict, repeats: int, only: set[str] | None) -> dict:
    projects: dict[str, dict] = {}
    for project in corpus["projects"]:
        pid = project["id"]
        if only and pid not in only:
            continue
        try:
            directory = fetch_project(project)
        except (FileNotFoundError, subprocess.CalledProcessError) as error:
            projects[pid] = {
                "status": "skipped",
                "reason": f"project unavailable: {error}",
            }
            continue
        entry = directory / project["entry"]
        if not entry.is_file():
            projects[pid] = {
                "status": "skipped",
                "reason": f"entry missing: {project['entry']}",
            }
            continue
        metrics = run_agent_ops(wright, entry, project["kind"], directory, repeats)
        metrics.update(
            run_cli_commands(wright, entry, project["kind"], directory, repeats)
        )
        failed = all("error" in m or "skipped" in m for m in metrics.values())
        projects[pid] = {
            "status": "error" if failed else "ok",
            "kind": project["kind"],
            "metrics": metrics,
        }
        if failed:
            first = next(iter(metrics.values()))
            projects[pid]["reason"] = first.get("error") or first.get("skipped") or ""
    return {
        "contract": CONTRACT,
        "corpusVersion": corpus["version"],
        "wright": wright_version(wright),
        "opyProvider": provider_version(),
        "projects": projects,
    }


# ---------------------------------------------------------------------------
# Band comparison
# ---------------------------------------------------------------------------


def _size_out_of_band(base: float, current: float) -> bool:
    delta = abs(current - base)
    return delta > SIZE_ABS_BAND and delta > SIZE_REL_BAND * base


def _latency_out_of_band(base: float, current: float) -> bool:
    delta = abs(current - base)
    return delta > LATENCY_ABS_BAND and delta > LATENCY_REL_BAND * base


def compare_metrics(baseline: dict, current: dict) -> tuple[list[dict], list[str]]:
    """(violations, warnings): every out-of-band metric and structural change."""
    violations: list[dict] = []
    warnings: list[str] = []
    if baseline.get("corpusVersion") != current.get("corpusVersion"):
        warnings.append(
            "corpus version differs: "
            f"baseline={baseline.get('corpusVersion')} run={current.get('corpusVersion')}"
        )
    if baseline.get("opyProvider") != current.get("opyProvider"):
        warnings.append(
            "OPY provider differs: "
            f"baseline={baseline.get('opyProvider')} run={current.get('opyProvider')}"
        )
    base_projects = baseline.get("projects") or {}
    run_projects = current.get("projects") or {}
    for pid in sorted(set(base_projects) | set(run_projects)):
        base_p = base_projects.get(pid)
        run_p = run_projects.get(pid)
        if run_p is None:
            violations.append(
                {"project": pid, "metric": "*", "change": "removed-from-run"}
            )
            continue
        if run_p.get("status") != "ok":
            warnings.append(f"{pid}: skipped ({run_p.get('reason', 'unavailable')})")
            continue
        if base_p is None or base_p.get("status") != "ok":
            violations.append({"project": pid, "metric": "*", "change": "added-to-run"})
            continue
        base_metrics = base_p.get("metrics") or {}
        run_metrics_map = run_p.get("metrics") or {}
        for key in sorted(set(base_metrics) | set(run_metrics_map)):
            base_m = base_metrics.get(key)
            run_m = run_metrics_map.get(key)
            if run_m is None:
                violations.append(
                    {"project": pid, "metric": key, "change": "removed"}
                )
                continue
            if base_m is None:
                violations.append({"project": pid, "metric": key, "change": "added"})
                continue
            if "error" in run_m or "skipped" in run_m:
                warnings.append(
                    f"{pid}/{key}: skipped "
                    f"({run_m.get('error') or run_m.get('skipped')})"
                )
                continue
            if "error" in base_m or "skipped" in base_m:
                violations.append(
                    {"project": pid, "metric": key, "change": "now-measured"}
                )
                continue
            violations.extend(_compare_metric(pid, key, base_m, run_m))
    return violations, warnings


def _compare_metric(pid: str, key: str, base: dict, current: dict) -> list[dict]:
    diffs: list[dict] = []
    for field, out_of_band in (
        ("tokens", _size_out_of_band),
        ("latencyMs", _latency_out_of_band),
    ):
        base_v = base.get(field)
        run_v = current.get(field)
        if (
            isinstance(base_v, (int, float))
            and isinstance(run_v, (int, float))
            and out_of_band(base_v, run_v)
        ):
            diffs.append(
                {
                    "project": pid,
                    "metric": key,
                    "field": field,
                    "baseline": base_v,
                    "current": run_v,
                    "delta": round(run_v - base_v, 1),
                    "change": "increase" if run_v > base_v else "decrease",
                }
            )
    base_counts = base.get("counts") or {}
    run_counts = current.get("counts") or {}
    for count_key in sorted(set(base_counts) | set(run_counts)):
        base_v = base_counts.get(count_key)
        run_v = run_counts.get(count_key)
        if base_v is None or run_v is None:
            diffs.append(
                {
                    "project": pid,
                    "metric": key,
                    "field": f"counts.{count_key}",
                    "baseline": base_v,
                    "current": run_v,
                    "change": "added" if base_v is None else "removed",
                }
            )
            continue
        if _size_out_of_band(base_v, run_v):
            diffs.append(
                {
                    "project": pid,
                    "metric": key,
                    "field": f"counts.{count_key}",
                    "baseline": base_v,
                    "current": run_v,
                    "delta": run_v - base_v,
                    "change": "increase" if run_v > base_v else "decrease",
                }
            )
    return diffs


def print_report(violations: list[dict], warnings: list[str]) -> None:
    for warning in warnings:
        print(f"warning: {warning}", file=sys.stderr)
    if not violations:
        print("metrics: within bands")
        return
    print(f"metrics: {len(violations)} out-of-band change(s):")
    for v in violations:
        if "field" in v:
            print(
                f"  {v['project']}/{v['metric']} {v['field']}: "
                f"{v['baseline']} -> {v['current']} ({v['change']})"
            )
        else:
            print(f"  {v['project']}/{v['metric']}: {v['change']}")
    print(
        "Update baseline-<version>.json in a reviewed PR documenting old/new "
        "values and the reason to accept these changes."
    )


def default_baseline_path(corpus_version: str) -> Path:
    return METRICS_DIR / f"baseline-{corpus_version}.json"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "command", choices=["fetch", "run", "check"], help="fetch corpus, run metrics, or check drift"
    )
    parser.add_argument(
        "--wright", default="wright", help="wright binary (default: PATH)"
    )
    parser.add_argument(
        "--manifest", type=Path, default=METRICS_DIR / "corpus.json"
    )
    parser.add_argument(
        "--baseline",
        type=Path,
        default=None,
        help="baseline metrics file (default: baseline-<corpusVersion>.json)",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=None,
        help="metrics output file for `run` (default: stdout)",
    )
    parser.add_argument(
        "--repeats", type=int, default=5, help="measurements per metric (median)"
    )
    parser.add_argument(
        "--project",
        action="append",
        default=None,
        help="limit to one corpus project id (repeatable)",
    )
    args = parser.parse_args()

    corpus = load_corpus(args.manifest)
    only = set(args.project) if args.project else None
    wright = (
        str(Path(args.wright).resolve())
        if os.sep in args.wright
        else (shutil.which(args.wright) or args.wright)
    )

    if args.command == "fetch":
        for project in corpus["projects"]:
            if only and project["id"] not in only:
                continue
            directory = fetch_project(project)
            print(f"{project['id']}: {directory}")
        return 0

    if args.command == "run":
        metrics = run_metrics(wright, corpus, args.repeats, only)
        text = json.dumps(metrics, indent=2, sort_keys=True) + "\n"
        if args.out:
            args.out.write_text(text)
            print(f"wrote {args.out}")
        else:
            sys.stdout.write(text)
        return 0

    baseline_path = args.baseline or default_baseline_path(corpus["version"])
    if not baseline_path.is_file():
        print(f"baseline missing: {baseline_path}", file=sys.stderr)
        return 1
    baseline = json.loads(baseline_path.read_text())
    if baseline.get("contract") != CONTRACT:
        print(
            f"{baseline_path}: expected contract {CONTRACT}",
            file=sys.stderr,
        )
        return 1
    current = run_metrics(wright, corpus, args.repeats, only)
    violations, warnings = compare_metrics(baseline, current)
    print_report(violations, warnings)
    return 1 if violations else 0


if __name__ == "__main__":
    raise SystemExit(main())
