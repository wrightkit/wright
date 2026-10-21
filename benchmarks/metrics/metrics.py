#!/usr/bin/env python3
"""Agent-facing drift metrics for Wright (#530).

Measures the output an agent-facing surface produces for every
``wright-agent/v1`` read operation and every common CLI command, per project
in a fixed corpus (``corpus.json``). Recorded per metric:

- ``bytes``: response/stdout size in bytes;
- ``tokens``: ``bytes / 4`` (the benchmark's estimate);
- ``latencyMs``: median wall time over ``--repeats`` runs (agent operations)
  or ``--cli-repeats`` runs (CLI commands; each run is a full pipeline). The
  Text forms run once for size; JSON forms also measure latency;
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
  ``--skip-latency`` compares size and shape only: every metric runs once and
  latency is neither measured for nor compared. Wall time on shared CI hosts
  varies by a factor of two between runs, so a blocking gate uses this form.
  ``--out`` also writes the measured run, for trend records.
  ``--from`` compares the merged ``run --project`` outputs of per-project
  shards instead of measuring, so a partitioned run reaches the verdict of an
  unpartitioned one; a project no shard covered fails as ``removed-from-run``.

Repository corpus entries are cloned shallow at their pinned commit; their
source is never vendored. Derived metrics are the only stored artifact.
"""

from __future__ import annotations

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import json
import os
import queue
import shutil
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

CONTRACT = "wright-agent-metrics/v1"
CORPUS_CONTRACT = "wright-agent-metrics-corpus/v1"
TOKEN_BYTES = 4
SIZE_REL_BAND = 0.15
SIZE_ABS_BAND = 30.0
LATENCY_REL_BAND = 0.50
# Median latency under ~1 ms is timer-resolution noise; the relative band
# applies only once the absolute delta clears this floor.
LATENCY_ABS_BAND = 1.0
# A baseline recorded on another host differs by a uniform speed factor, so
# `check` bands each metric against `baseline * median(run/baseline)`. Below
# MIN_LATENCY_PAIRS comparable pairs that factor is untrusted.
MIN_LATENCY_PAIRS = 10
REQUEST_TIMEOUT_S = 180.0

METRICS_DIR = Path(__file__).resolve().parent
CACHE_DIR = METRICS_DIR / ".cache"

# Mutation operations (validateEditTransaction and the provider* ops)
# are absent: they need caller-supplied transactions and are not measured.
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

DEFAULT_OPS = {"check", "lint", "analyze", "inspect", "rules", "symbols", "references", "cfg", "callGraph", "costEstimate"}
INSPECT_COMMANDS = ["rules", "symbols", "refs", "cfg", "callgraph", "cost"]

BLOCKING_CHANGES = {"removed-from-run", "broken-in-run", "removed"}


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


_LISTING_KEYS = ("symbols", "rules", "persistentObjects", "findings", "items")


def _snapshot_listing(result):
    """The id list a `{key: [...], snapshot}` listing carries (#595)."""
    if not isinstance(result, dict) or "snapshot" not in result:
        return None
    lists = [key for key, value in result.items() if isinstance(value, list)]
    return result[lists[0]] if len(lists) == 1 and lists[0] in _LISTING_KEYS else None


def _listing_measurement(result):
    """Stamped listings measure in the legacy `items` vocabulary (#595)."""
    listed = _snapshot_listing(result)
    if listed is None:
        return None
    measured = {"items": listed}
    for key, value in result.items():
        if key == "snapshot" or isinstance(value, list):
            continue
        if key == "truncation" and isinstance(value, dict) and len(value) == 1:
            value = {"items": next(iter(value.values()))}
        measured[key] = value
    return listed if len(measured) == 1 else measured


def agent_counts(op: str, result) -> dict:
    """Result-shape counts for one agent operation's `result` payload."""
    if not isinstance(result, (list, dict)):
        return {}
    if op in _LISTING_KEYS[:4] and (measured := _listing_measurement(result)) is not None:
        result = measured
    if isinstance(result, list):
        if op == "symbols":
            return _typed_items(result, "symbol")
        if op == "references":
            return _typed_items(result, "reference")
        if op in ("rules", "persistentObjects", "callGraph"):
            return _typed_items(result, "item")
        return {"items": len(result)}
    if op in DEFAULT_OPS:
        return default_counts(result)
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
        return cli_counts(op, result)
    return {}


def envelope_failure(envelope) -> str | None:
    """Code of the first diagnostic when a CLI result envelope reports
    ``ok: false`` without a result (a refusal, not a completed verdict)."""
    if not isinstance(envelope, dict) or envelope.get("ok") is not False:
        return None
    if envelope.get("result") is not None and envelope.get("exit") == 1:
        return None
    for diagnostic in envelope.get("diagnostics") or []:
        if isinstance(diagnostic, dict) and diagnostic.get("code"):
            return str(diagnostic["code"])
    return "not-ok"


def cli_counts(command: str, envelope: dict) -> dict:
    """Result-shape counts for one CLI command's JSON envelope."""
    if not isinstance(envelope, dict):
        return {}
    counts: dict[str, float] = {"diagnostics": len(envelope.get("diagnostics") or [])}
    result = envelope.get("result")
    listed = _listing_measurement(result)
    if listed is not None:
        result = listed
    if isinstance(result, list):
        counts["items"] = len(result)
        return counts
    if not isinstance(result, dict):
        return counts
    counts.update(default_counts(result))
    return counts


def metric_record(
    payloads_bytes: list[int], latencies_ms: list[float] | None, counts: dict
) -> dict:
    raw = statistics.median(payloads_bytes)
    record = {
        "bytes": raw,
        "tokens": round(raw / TOKEN_BYTES, 1),
        "counts": counts,
    }
    if latencies_ms:
        record["latencyMs"] = round(statistics.median(latencies_ms), 1)
    return record


def run_agent_ops(
    wright: str, entry: Path, kind: str, project_dir: Path, repeats: int
) -> dict:
    metrics: dict[str, dict] = {}
    scratch = tempfile.TemporaryDirectory(prefix="wright-metrics-")
    # `compile` writes the program to a file (ADR-0025); keep it out of the
    # measured project and replace it on every repeat.
    targets: dict[str, dict] = {
        "compile": {"output": str(Path(scratch.name) / "compiled.ow"), "force": True}
    }
    with scratch, ServeSession(wright, entry, kind, project_dir) as session:
        # Warm-up is not optional: the first envelope drains load-time
        # diagnostics, so a repeats=1 run without it measures a different
        # shape than a repeats=N median.
        try:
            session.request("check")
        except (queue.Empty, TimeoutError):
            pass
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
                    failure = envelope_failure(last_result)
                    if failure:
                        last_error = {"code": failure}
                        break
                    counts = agent_counts(op, last_result)
                if last_error is not None:
                    metrics[key] = {
                        "error": last_error.get("code", "error"),
                        "message": last_error.get("message", ""),
                    }
                    continue
                metrics[key] = metric_record(lines_bytes, latencies, counts)
                # Numeric addresses echo the listing's generation (#595).
                snap = last_result.get("snapshot") if isinstance(last_result, dict) else None
                if op == "symbols":
                    sid = _first_id(last_result)
                    if sid is not None and snap is not None:
                        targets["references"] = {"symbol": sid, "snapshot": snap}
                        targets["usage"] = {"symbol": sid, "snapshot": snap}
                if op == "rules":
                    rid = _first_id(last_result)
                    if rid is not None and snap is not None:
                        targets["cfg"] = {"rule": rid, "snapshot": snap}
            except (queue.Empty, TimeoutError):
                metrics[key] = {"error": "timeout"}
                break
    return metrics


def _first_id(items) -> int | None:
    """Numeric id of the first listed item — unambiguous within the session
    where a name may resolve to several rules/symbols (`ambiguous-rule`)."""
    if isinstance(items, dict):
        items = _snapshot_listing(items) or items.get("items", [])
    for item in items if isinstance(items, list) else []:
        ident = item.get("id") if isinstance(item, dict) else None
        if isinstance(ident, int):
            return ident
    return None


def unique_name(items) -> str | None:
    """Choose an address that stays unambiguous across the complete query."""
    if isinstance(items, dict):
        items = _snapshot_listing(items) or items.get("items", [])
    names = [item.get("name") for item in items if isinstance(item, dict)] if isinstance(items, list) else []
    counts = Counter(names)
    return next((name for name in names if isinstance(name, str) and counts[name] == 1), None)


def default_counts(payload) -> dict:
    """Count structured default lists and the complete totals they report."""
    if isinstance(payload, list):
        return {"items": len(payload)}
    if not isinstance(payload, dict):
        return {}
    if "result" in payload:
        return default_counts(payload["result"])
    counts = {}
    for key, value in payload.items():
        if isinstance(value, list):
            counts[key] = len(value)
        elif isinstance(value, dict):
            counts.update({f"{key}.{k}": v for k, v in default_counts(value).items()})
        elif isinstance(value, (int, float)) and not isinstance(value, bool):
            if key != "snapshot":  # generation stamp is metadata, not content
                counts[key] = value
    return counts


def discover_addresses(wright: str, entry: Path, kind: str, project_dir: Path) -> dict:
    """Globally unique rule/symbol names, keyed by the `inspect` subcommand
    (`cfg`, `refs`) that addresses them."""
    addresses = {}
    with ServeSession(wright, entry, kind, project_dir) as session:
        for op, address in (("rules", "cfg"), ("symbols", "refs")):
            try:
                name = unique_name(session.request(op, all=True).get("result"))
                if name is not None:
                    addresses[address] = name
            except (queue.Empty, TimeoutError):
                break
    return addresses


def run_cli_commands(
    wright: str, entry: Path, kind: str, project_dir: Path, repeats: int, jobs: int = 1
) -> dict:
    """Measure default JSON and text forms; address refs/cfg through queries.

    Each command/form is an independent CLI invocation, so ``jobs`` > 1 runs
    them concurrently; the result order does not depend on ``jobs``."""
    scratch = tempfile.TemporaryDirectory(prefix="wright-metrics-")
    addresses = discover_addresses(wright, entry, kind, project_dir)

    def measure(command: str, form: str) -> dict:
        sub = command.split()[-1]
        if sub in ("refs", "cfg") and sub not in addresses:
            return {"skipped": "no globally unique declared name"}
        argv = [wright, *command.split()]
        if sub in addresses:
            argv.append(addresses[sub])
        argv += [str(entry), "--kind", kind, "-f", form]
        if command == "compile":
            argv += ["-o", str(Path(scratch.name) / "compiled.ow"), "--force"]
        outputs_bytes = []
        latencies = []
        counts = {}
        failure = None
        for _ in range(repeats if form == "json" else 1):
            start = time.monotonic()
            completed = subprocess.run(argv, capture_output=True, cwd=project_dir)
            latencies.append((time.monotonic() - start) * 1000.0)
            outputs_bytes.append(len(completed.stdout))
            if form == "text":
                if completed.returncode not in (0, 1):
                    failure = f"exit-{completed.returncode}"
                continue
            try:
                envelope = json.loads(completed.stdout)
                failure = envelope_failure(envelope)
                counts = cli_counts(command, envelope)
            except (json.JSONDecodeError, TypeError):
                failure = "unparsed-stdout"
        return {"error": failure} if failure else metric_record(
            outputs_bytes, latencies if form == "json" else None, counts
        )

    tasks = [
        (f"cli:{command}" + ("?text" if form == "text" else ""), command, form)
        for command in CLI_COMMANDS + [f"inspect {sub}" for sub in INSPECT_COMMANDS]
        for form in (["json"] if command == "compile" else ["json", "text"])
    ]
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        records = list(pool.map(lambda task: measure(task[1], task[2]), tasks))
    scratch.cleanup()
    return {task[0]: record for task, record in zip(tasks, records)}


def accumulate_scenario(steps: list[list[str]], execute) -> dict:
    """Cumulative size of one scenario's call sequence.

    ``execute(step)`` returns ``(output bytes, failure or None)``. Any failing
    step makes the whole scenario an ``error``: a partial sequence is not a
    measurement."""
    total = 0
    for index, step in enumerate(steps, 1):
        size, failure = execute(step)
        if failure:
            return {"error": f"step {index} ({' '.join(step)}): {failure}"}
        total += size
    return metric_record([total], None, {"steps": len(steps)})


def run_scenarios(
    wright: str, entry: Path, kind: str, project_dir: Path, scenarios: list[dict], jobs: int = 1
) -> dict:
    """`scenario:<id>` metrics: each recommended call sequence in its default
    text form, run as separate CLI invocations."""
    addresses = discover_addresses(wright, entry, kind, project_dir)
    placeholders = {"{symbol}": addresses.get("refs"), "{rule}": addresses.get("cfg")}

    def execute(step: list[str]) -> tuple[int, str | None]:
        argv = [wright]
        for word in step:
            if word in placeholders:
                if placeholders[word] is None:
                    return 0, "no globally unique declared name"
                word = placeholders[word]
            argv.append(word)
        argv += [str(entry), "--kind", kind, "-f", "text"]
        completed = subprocess.run(argv, capture_output=True, cwd=project_dir)
        failure = None
        if completed.returncode not in (0, 1):
            failure = f"exit-{completed.returncode}"
        return len(completed.stdout), failure

    with ThreadPoolExecutor(max_workers=jobs) as pool:
        records = list(
            pool.map(lambda scenario: accumulate_scenario(scenario["steps"], execute), scenarios)
        )
    return {f"scenario:{scenario['id']}": record for scenario, record in zip(scenarios, records)}


def scenario_ratios(projects: dict, scenarios: list[dict], corpus_projects: list[dict]) -> dict:
    """Per scenario: the largest measured project and its token ratio to each
    seed project that ran the same scenario."""
    ratios: dict[str, dict] = {}
    for scenario in scenarios:
        key = f"scenario:{scenario['id']}"
        tokens = {
            pid: p["metrics"][key]["tokens"]
            for pid, p in projects.items()
            if "tokens" in (p.get("metrics") or {}).get(key, {})
        }
        if not tokens:
            continue
        largest = max(tokens, key=tokens.get)
        seeds = [
            p["id"] for p in corpus_projects
            if p["id"].startswith("seed-") and p["id"] in tokens
            and scenario["id"] in p.get("scenarios", [])
        ]
        ratios[scenario["id"]] = {
            "largest": largest,
            "tokens": tokens[largest],
            "vsSeed": {
                seed: round(tokens[largest] / tokens[seed], 1)
                for seed in seeds if tokens[seed]
            },
        }
    return ratios


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


def strip_latency(document: dict) -> dict:
    """Copy of a metrics document without any ``latencyMs`` field."""
    stripped = json.loads(json.dumps(document))
    for project in (stripped.get("projects") or {}).values():
        for record in (project.get("metrics") or {}).values():
            record.pop("latencyMs", None)
    return stripped


def run_metrics(
    wright: str,
    corpus: dict,
    repeats: int,
    only: set[str] | None,
    cli_repeats: int | None = None,
    jobs: int = 1,
) -> dict:
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
        selected = [
            sc for sc in corpus.get("scenarios", []) if sc["id"] in project.get("scenarios", [])
        ]
        cli_runs = repeats if cli_repeats is None else cli_repeats
        # Parallel surfaces overlap and blur latency, so only a run that
        # discards latency may request jobs > 1.
        with ThreadPoolExecutor(max_workers=3 if jobs > 1 else 1) as pool:
            surfaces = [
                pool.submit(run_agent_ops, wright, entry, project["kind"], directory, repeats),
                pool.submit(
                    run_cli_commands, wright, entry, project["kind"], directory, cli_runs, jobs
                ),
                pool.submit(
                    run_scenarios, wright, entry, project["kind"], directory, selected, jobs
                ),
            ]
            metrics: dict[str, dict] = {}
            for surface in surfaces:
                metrics.update(surface.result())
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
        "scenarioRatios": scenario_ratios(
            projects, corpus.get("scenarios", []), corpus["projects"]
        ),
    }


MERGE_IDENTITY_KEYS = (
    "contract",
    "corpusVersion",
    "wright",
    "opyProvider",
)


def merge_runs(documents: list[dict], corpus: dict) -> dict:
    """One run document from per-project shard runs of the same build.

    Shards must agree on every identity key and must not overlap; scenario
    ratios are recomputed over the merged projects so they never depend on how
    the corpus was partitioned.
    """
    if not documents:
        raise ValueError("no shard runs to merge")
    first = documents[0]
    projects: dict[str, dict] = {}
    for document in documents:
        for key in MERGE_IDENTITY_KEYS:
            if document.get(key) != first.get(key):
                raise ValueError(
                    f"shards disagree on {key}: {first.get(key)!r} vs {document.get(key)!r}"
                )
        for pid, project in (document.get("projects") or {}).items():
            if pid in projects:
                raise ValueError(f"project {pid} appears in more than one shard")
            projects[pid] = project
    return {
        **{key: first.get(key) for key in MERGE_IDENTITY_KEYS},
        "projects": projects,
        "scenarioRatios": scenario_ratios(
            projects, corpus.get("scenarios", []), corpus["projects"]
        ),
    }




def _size_out_of_band(base: float, current: float) -> bool:
    delta = abs(current - base)
    return delta > SIZE_ABS_BAND and delta > SIZE_REL_BAND * base


def _latency_out_of_band(base: float, current: float) -> bool:
    delta = abs(current - base)
    return delta > LATENCY_ABS_BAND and delta > LATENCY_REL_BAND * base


def _latency_factor(baseline: dict, current: dict) -> tuple[float, int]:
    """(median run/baseline latency ratio, pair count): the machine-speed
    offset between the baseline host and this host."""
    ratios = []
    base_projects = baseline.get("projects") or {}
    run_projects = current.get("projects") or {}
    for pid in set(base_projects) & set(run_projects):
        base_p = base_projects[pid]
        run_p = run_projects[pid]
        if base_p.get("status") != "ok" or run_p.get("status") != "ok":
            continue
        base_metrics = base_p.get("metrics") or {}
        run_metrics = run_p.get("metrics") or {}
        for key in set(base_metrics) & set(run_metrics):
            base_m = base_metrics[key]
            run_m = run_metrics[key]
            if any("error" in m or "skipped" in m for m in (base_m, run_m)):
                continue
            base_v = base_m.get("latencyMs")
            run_v = run_m.get("latencyMs")
            if (
                isinstance(base_v, (int, float))
                and isinstance(run_v, (int, float))
                and base_v >= LATENCY_ABS_BAND
            ):
                ratios.append(run_v / base_v)
    if len(ratios) < MIN_LATENCY_PAIRS:
        return 1.0, len(ratios)
    return statistics.median(ratios), len(ratios)


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
    latency_factor, latency_pairs = _latency_factor(baseline, current)
    if latency_pairs >= MIN_LATENCY_PAIRS and abs(latency_factor - 1.0) > 0.02:
        warnings.append(
            f"latency machine factor: {latency_factor:.2f} "
            f"(median run/baseline over {latency_pairs} metrics; "
            "bands apply to the normalized baseline)"
        )
    for pid in sorted(set(base_projects) | set(run_projects)):
        base_p = base_projects.get(pid)
        run_p = run_projects.get(pid)
        if run_p is None:
            violations.append(
                {"project": pid, "metric": "*", "change": "removed-from-run"}
            )
            continue
        if run_p.get("status") != "ok":
            if base_p is not None and base_p.get("status") != "ok":
                warnings.append(
                    f"{pid}: still skipped ({run_p.get('reason', 'unavailable')})"
                )
            else:
                violations.append(
                    {
                        "project": pid,
                        "metric": "*",
                        "field": "status",
                        "baseline": "ok" if base_p is not None else "absent",
                        "current": run_p.get("reason") or run_p.get("status"),
                        "change": "broken-in-run",
                    }
                )
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
                run_state = run_m.get("error") or run_m.get("skipped")
                base_state = base_m.get("error") or base_m.get("skipped")
                if base_state is not None and base_state == run_state:
                    warnings.append(f"{pid}/{key}: still skipped ({run_state})")
                else:
                    violations.append(
                        {
                            "project": pid,
                            "metric": key,
                            "field": "status",
                            "baseline": base_state or "measured",
                            "current": run_state,
                            "change": "broken-in-run",
                        }
                    )
                continue
            if "error" in base_m or "skipped" in base_m:
                violations.append(
                    {"project": pid, "metric": key, "change": "now-measured"}
                )
                continue
            violations.extend(_compare_metric(pid, key, base_m, run_m, latency_factor))
    return violations, warnings


def _compare_metric(
    pid: str, key: str, base: dict, current: dict, latency_factor: float = 1.0
) -> list[dict]:
    diffs: list[dict] = []
    for field in ("tokens", "latencyMs"):
        base_v = base.get(field)
        run_v = current.get(field)
        if not (
            isinstance(base_v, (int, float)) and isinstance(run_v, (int, float))
        ):
            continue
        if field == "latencyMs":
            adjusted = base_v * latency_factor
            out = _latency_out_of_band(adjusted, run_v)
            shown_base: float = round(adjusted, 1)
        else:
            out = _size_out_of_band(base_v, run_v)
            shown_base = base_v
        if out:
            diffs.append(
                {
                    "project": pid,
                    "metric": key,
                    "field": field,
                    "baseline": shown_base,
                    "current": run_v,
                    "delta": round(run_v - shown_base, 1),
                    "change": "increase" if run_v > shown_base else "decrease",
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


def split_changes(violations: list[dict]) -> tuple[list[dict], list[dict]]:
    """(blocking, drift): drift is every change that is not in BLOCKING_CHANGES."""
    # Scenario metrics are not yet a gate (#665): every change is drift.
    def blocks(v: dict) -> bool:
        return v["change"] in BLOCKING_CHANGES and not v["metric"].startswith("scenario:")

    return [v for v in violations if blocks(v)], [v for v in violations if not blocks(v)]


def _format_change(v: dict) -> str:
    if "field" in v:
        return (
            f"  {v['project']}/{v['metric']} {v['field']}: "
            f"{v['baseline']} -> {v['current']} ({v['change']})"
        )
    return f"  {v['project']}/{v['metric']}: {v['change']}"


def print_report(
    blocking: list[dict], drift: list[dict], warnings: list[str], ratios: dict | None = None
) -> None:
    for scenario, ratio in (ratios or {}).items():
        vs = ", ".join(f"{seed} x{value}" for seed, value in ratio["vsSeed"].items())
        print(f"scenario {scenario}: largest {ratio['largest']} {ratio['tokens']} tokens; vs {vs}")
    for warning in warnings:
        print(f"warning: {warning}", file=sys.stderr)
    if drift:
        print(f"metrics: {len(drift)} drift change(s), not blocking:")
        for v in drift:
            print(_format_change(v))
        print(
            "Refresh baseline-<version>.json when the change is intended; "
            "it does not block."
        )
    if blocking:
        print(f"metrics: {len(blocking)} blocking change(s):")
        for v in blocking:
            print(_format_change(v))
    elif not drift:
        print("metrics: within bands")


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
        help="metrics output file for `run` (default: stdout); `check` also writes it",
    )
    parser.add_argument(
        "--skip-latency",
        action="store_true",
        help="`check` only: measure each metric once and compare size and shape, not latency",
    )
    parser.add_argument(
        "--repeats",
        type=int,
        default=5,
        help="measurements per agent-operation metric (median)",
    )
    parser.add_argument(
        "--cli-repeats",
        type=int,
        default=3,
        help="measurements per CLI-command metric (median); each is a full pipeline run",
    )
    parser.add_argument(
        "--project",
        action="append",
        default=None,
        help="limit to one corpus project id (repeatable)",
    )
    parser.add_argument(
        "--jobs",
        type=int,
        default=1,
        help="`run` only: measure independent invocations concurrently; latency is not "
        "meaningful with more than one job, so use it with `check --skip-latency` or "
        "shard runs whose latency is discarded",
    )
    parser.add_argument(
        "--from",
        dest="shards",
        type=Path,
        action="append",
        default=None,
        help="`check` only: compare these `run --project` outputs, merged, instead of measuring",
    )
    args = parser.parse_args()

    corpus = load_corpus(args.manifest)
    only = set(args.project) if args.project else None
    unknown = (only or set()) - {project["id"] for project in corpus["projects"]}
    if unknown:
        print(f"unknown corpus project(s): {', '.join(sorted(unknown))}", file=sys.stderr)
        return 1
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
        metrics = run_metrics(wright, corpus, args.repeats, only, args.cli_repeats, args.jobs)
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
    repeats, cli_repeats = (1, 1) if args.skip_latency else (args.repeats, args.cli_repeats)
    if args.shards:
        try:
            current = merge_runs([json.loads(path.read_text()) for path in args.shards], corpus)
        except (OSError, ValueError) as error:
            print(f"cannot merge shard runs: {error}", file=sys.stderr)
            return 1
    else:
        current = run_metrics(wright, corpus, repeats, only, cli_repeats)
    if args.out:
        args.out.write_text(json.dumps(current, indent=2, sort_keys=True) + "\n")
    if args.skip_latency:
        baseline, current = strip_latency(baseline), strip_latency(current)
    violations, warnings = compare_metrics(baseline, current)
    blocking, drift = split_changes(violations)
    print_report(blocking, drift, warnings, current.get("scenarioRatios"))
    return 1 if blocking else 0


if __name__ == "__main__":
    raise SystemExit(main())
