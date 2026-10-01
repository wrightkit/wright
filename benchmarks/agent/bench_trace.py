"""Trace capture and analysis for the agent benchmark (#414, SPEC-414): shim, snapshots, expectations, usage."""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import threading
import time
from pathlib import Path

TOKEN_BYTES = 4  # estimation only: bytes per token for Wright output attribution
DECISION_COMMANDS = ("check", "lint", "analyze", "inspect")
VALIDATING = ("check", "lint", "analyze", "compile")
OUTPUT_FORMAT_FLAGS = ("--format", "-f")


def command_of(argv: list[str]) -> str:
    return next((a for a in argv if not a.startswith("-")), "")


def wants_json(argv: list[str]) -> bool:
    return any(a in OUTPUT_FORMAT_FLAGS and i + 1 < len(argv) and argv[i + 1] == "json" for i, a in enumerate(argv)) or "--format=json" in argv


def envelope_summary(envelope: dict) -> dict:
    result = envelope.get("result") or {}
    output = result.get("output") if isinstance(result.get("output"), dict) else {}
    return {
        "command": envelope.get("command"),
        "ok": envelope.get("ok"),
        "exit": envelope.get("exit"),
        "codes": [d.get("code") for d in envelope.get("diagnostics", [])],
        "inputIdentity": result.get("input_identity") or output.get("input_identity"),
        "selection": result.get("selection") or envelope.get("selection"),
    }


def append_event(event: dict) -> None:
    with open(os.environ["WRIGHT_BENCH_TRACE"], "a") as trace:
        trace.write(json.dumps(event) + "\n")


def sidecar_name() -> str:
    return f"{time.time_ns()}-{os.getpid()}"


def shim_main(argv: list[str]) -> int:
    """Run the real Wright and record the call. `serve` sessions are teed line by line."""
    real = os.environ["WRIGHT_BENCH_REAL"]
    started = time.time()
    if command_of(argv) == "serve":
        return serve_tee(real, argv, started)
    proc = subprocess.run([real, *argv], capture_output=True)
    sys.stdout.buffer.write(proc.stdout)
    sys.stdout.flush()
    sys.stderr.buffer.write(proc.stderr)
    sys.stderr.flush()
    name = sidecar_name()
    sidecar = Path(os.environ["WRIGHT_BENCH_SIDECAR"])
    sidecar.mkdir(parents=True, exist_ok=True)
    (sidecar / f"{name}.out").write_bytes(proc.stdout)
    (sidecar / f"{name}.err").write_bytes(proc.stderr)
    envelope = None
    if wants_json(argv):
        try:
            envelope = envelope_summary(json.loads(proc.stdout))
        except json.JSONDecodeError:
            envelope = None
    append_event({
        "type": "call", "t": started, "argv": argv, "cwd": os.getcwd(), "exit": proc.returncode,
        "seconds": round(time.time() - started, 3), "stdoutBytes": len(proc.stdout), "stderrBytes": len(proc.stderr),
        "stderrHead": proc.stderr.decode(errors="replace")[:300], "envelope": envelope, "sidecar": name,
    })
    return proc.returncode


def serve_tee(real: str, argv: list[str], started: float) -> int:
    proc = subprocess.Popen([real, *argv], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    counts = {"req": 0, "res": 0}

    def log(direction: str, line: bytes) -> None:
        counts[direction] += 1
        append_event({"type": "serve", "dir": direction, "t": time.time(), "line": line.decode(errors="replace")})

    def pump() -> None:
        for line in proc.stdout:
            sys.stdout.buffer.write(line)
            sys.stdout.flush()
            log("res", line)

    reader = threading.Thread(target=pump)
    reader.start()
    for line in sys.stdin.buffer:
        proc.stdin.write(line)
        proc.stdin.flush()
        log("req", line)
    proc.stdin.close()
    code = proc.wait()
    reader.join()
    append_event({"type": "call", "t": started, "argv": argv, "cwd": os.getcwd(), "exit": code, "seconds": round(time.time() - started, 3),
                  "requests": counts["req"], "envelope": None})
    return code


def read_events(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text().splitlines()] if path.is_file() else []


class Snapshots(threading.Thread):
    """Poll watched files during a run; keep a copy of every distinct content, with its time."""

    def __init__(self, workspace: Path, files: list[str], out: Path, interval: float = 0.2):
        super().__init__(daemon=True)
        self.workspace, self.files, self.out, self.interval = workspace, files, out, interval
        self.events: list[dict] = []
        self.stop_event = threading.Event()
        self.last = {f: self.digest(f) for f in files}
        out.mkdir(parents=True, exist_ok=True)

    def digest(self, name: str) -> str | None:
        path = self.workspace / name
        return hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None

    def scan(self) -> None:
        for name in self.files:
            digest = self.digest(name)
            if digest is not None and digest != self.last[name]:
                self.last[name] = digest
                index = len(self.events) + 1
                target = self.out / f"{index:03d}-{Path(name).name}"
                target.write_bytes((self.workspace / name).read_bytes())
                self.events.append({"i": index, "t": time.time(), "file": name, "sha256": digest, "path": str(target)})

    def run(self) -> None:
        while not self.stop_event.wait(self.interval):
            self.scan()

    def finish(self) -> list[dict]:
        self.stop_event.set()
        self.join()
        self.scan()
        (self.out / "snapshots.json").write_text(json.dumps(self.events, indent=2) + "\n")
        return self.events


def summarize_trace(events: list[dict]) -> dict:
    calls = [e for e in events if e["type"] == "call"]
    by_command: dict[str, int] = {}
    for call in calls:
        by_command[command_of(call["argv"])] = by_command.get(command_of(call["argv"]), 0) + 1
    return {
        "invocations": len(calls),
        "byCommand": by_command,
        "failedInvocations": sum(1 for c in calls if c["exit"] != 0),
        "ownerOrEnvironmentGaps": [c["argv"] for c in calls if c["exit"] >= 3],
        "outputTokensEstimate": {
            cmd: sum(c.get("stdoutBytes", 0) + c.get("stderrBytes", 0) for c in calls if command_of(c["argv"]) == cmd) // TOKEN_BYTES
            for cmd in by_command if cmd
        },
    }


def friction(events: list[dict]) -> dict:
    calls = [e for e in events if e["type"] == "call"]
    seen: list[tuple] = []
    repeats = 0
    for call in calls:
        key = tuple(call["argv"])
        repeats += key in seen
        seen.append(key)
    serve_responses = []
    unparsed = 0
    for event in events:
        if event["type"] == "serve" and event["dir"] == "res":
            try:
                serve_responses.append(json.loads(event["line"]))
            except json.JSONDecodeError:
                unparsed += 1
    return {
        "usageErrors": sum(1 for c in calls if c["exit"] == 2),
        "unknownSubcommands": sum(1 for c in calls if "unrecognized subcommand" in c.get("stderrHead", "")),
        "helpLookups": sum(1 for c in calls if any(a in ("--help", "-h", "help") for a in c["argv"])),
        "retriesAfterUnsupported": sum(1 for i, c in enumerate(calls) if c["exit"] >= 3 and tuple(c["argv"]) in [tuple(x["argv"]) for x in calls[i + 1:]]),
        "malformedServeRequests": sum(1 for r in serve_responses if r.get("error", {}).get("code") == "malformed-request"),
        "unparsedServeResponses": unparsed,
        "identicalRepeats": repeats,
        "callsToFirstSuccess": next((i + 1 for i, c in enumerate(calls) if c["exit"] == 0 and not any(a in ("--help", "-h", "--version") for a in c["argv"])), None),
    }


def serve_ops(events: list[dict]) -> list[str]:
    ops = []
    for e in events:
        if e["type"] == "serve" and e["dir"] == "req":
            try:
                ops.append(json.loads(e["line"]).get("op", ""))
            except json.JSONDecodeError:
                ops.append("")
    return ops


def expectation(status: str, detail: str = "") -> dict:
    return {"status": status, "detail": detail}


def detect_expectations(events: list[dict], snapshots: list[dict], scenario: dict, final_sha256: str | None) -> dict:
    """SPEC-414 E01-E12 over the Wright trace. E05, E07, E09, E10 need the agent transcript: `unavailable`."""
    calls = [e for e in events if e["type"] == "call"]
    ops = serve_ops(events)
    used = bool(calls)
    unavailable = expectation("unavailable", "needs the normalized agent transcript")
    result = {k: unavailable for k in ("E05", "E07", "E09", "E10")}
    if not used:
        return {**result, **{k: expectation("na", "Wright not used") for k in ("E01", "E02", "E03", "E04", "E06", "E08", "E11", "E12")}}
    first = calls[0]
    discovery = any(a in ("--help", "-h", "help", "--version") for a in first["argv"]) or "capabilities" in ops[:1]
    result["E01"] = expectation("pass" if discovery else "fail", f"first call: {' '.join(first['argv'])}")
    decision = [c for c in calls if command_of(c["argv"]) in DECISION_COMMANDS]
    structured = [c for c in decision if wants_json(c["argv"])]
    if decision or ops:
        rate = (len(structured) + len(ops)) / (len(decision) + len(ops))
        result["E02"] = expectation("pass" if rate >= 0.5 else "fail", f"structured share {rate:.2f}")
    else:
        result["E02"] = expectation("na", "no decision-driving calls")
    if scenario.get("stabilityRisk"):
        stability = any(command_of(c["argv"]) in ("lint", "analyze") for c in calls) or any(o in ("lint", "findings", "analyze") for o in ops)
        result["E03"] = expectation("pass" if stability else "fail", "lint or analyze run" if stability else "only check-level validation")
    else:
        result["E03"] = expectation("na", "scenario has no stability risk")
    last_edit = max((s["t"] for s in snapshots), default=None)
    if last_edit is None:
        result["E04"] = expectation("na", "no edits observed")
    else:
        after = [c for c in calls if command_of(c["argv"]) in VALIDATING and c["t"] + c["seconds"] >= last_edit]
        matched = [c for c in after if c.get("envelope") and c["envelope"].get("inputIdentity") == final_sha256]
        result["E04"] = expectation("pass" if after else "fail", f"{len(after)} validation(s) after last edit; {len(matched)} match the final content")
    withheld = [c for c in calls if ((c.get("envelope") or {}).get("selection") or {}).get("withheld")]
    flags = sum(1 for c in calls if any(a in ("--severity", "--rule-id", "--file", "--max") for a in c["argv"]))
    result["E06"] = expectation("na" if not withheld and not flags else "info", f"{flags} selection-flag call(s), {len(withheld)} withheld result(s)")
    unsupported = [c for c in calls if c["exit"] >= 3]
    excess = [c for c in unsupported if sum(1 for x in calls if x["argv"] == c["argv"]) > 2]
    result["E08"] = expectation("na" if not unsupported else ("fail" if excess else "pass"), f"{len(unsupported)} exit 3/4 call(s), {len(excess)} retried more than twice")
    if ops:
        malformed = friction(events)["malformedServeRequests"]
        result["E11"] = expectation("pass" if ops[0] == "capabilities" and not malformed else "fail", f"first op '{ops[0]}', {malformed} malformed")
    else:
        result["E11"] = expectation("na", "no serve session")
    edit_times = [s["t"] for s in snapshots]
    wasted = 0
    for i, c in enumerate(calls):
        for later in calls[i + 1:]:
            if later["argv"] == c["argv"]:
                wasted += not any(c["t"] <= t <= later["t"] for t in edit_times)
                break
    result["E12"] = expectation("pass" if wasted == 0 else "fail", f"{wasted} identical repeat(s) with no edit between")
    return result


def usage_summary(path: Path, first_valid_t: float | None) -> dict | None:
    """Per-turn usage rows written by the adapter: t, input, output, cache_read, cache_write, reasoning, context, context_limit."""
    if not path.is_file():
        return None
    rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    if not rows:
        return None

    def total(row: dict) -> int:
        return sum(row.get(k) or 0 for k in ("input", "output", "cache_read", "cache_write", "reasoning"))

    peak = max(rows, key=lambda r: r.get("context") or 0)
    limit = peak.get("context_limit")
    to_first = None
    if first_valid_t is not None:
        upto = [r for r in rows if r.get("t") is not None and r["t"] <= first_valid_t]
        to_first = {"turns": len(upto), "tokens": sum(total(r) for r in upto)}
    return {
        "turns": len(rows),
        "tokens": {k: sum(r.get(k) or 0 for r in rows) for k in ("input", "output", "cache_read", "cache_write", "reasoning")},
        "totalTokens": sum(total(r) for r in rows),
        "peakContext": peak.get("context"),
        "peakContextShare": round(peak["context"] / limit, 4) if limit and peak.get("context") else None,
        "toFirstValid": to_first,
    }
