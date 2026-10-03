"""Trace capture and analysis for the agent benchmark (#414, SPEC-414): shim, snapshots, expectations, usage."""

from __future__ import annotations

import hashlib
import json
import os
import re
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
    with open(os.environ["BENCH_TOOL_TRACE"], "a") as trace:
        trace.write(json.dumps({"tool": os.environ["BENCH_TOOL_NAME"], **event}) + "\n")


def sidecar_name() -> str:
    return f"{time.time_ns()}-{os.getpid()}"


def shim_main(argv: list[str]) -> int:
    """Run the real tool (`wright` or `overpy`, named by argv[0]) and record the call. Wright `serve` sessions are teed line by line."""
    tool, argv = argv[0], argv[1:]
    os.environ["BENCH_TOOL_NAME"] = tool
    real = os.environ[f"BENCH_TOOL_REAL_{tool.upper()}"]
    started = time.time()
    if tool == "wright" and command_of(argv) == "serve":
        return serve_tee(real, argv, started)
    proc = subprocess.run([real, *argv], capture_output=True)
    sys.stdout.buffer.write(proc.stdout)
    sys.stdout.flush()
    sys.stderr.buffer.write(proc.stderr)
    sys.stderr.flush()
    name = sidecar_name()
    sidecar = Path(os.environ["BENCH_TOOL_SIDECAR"])
    sidecar.mkdir(parents=True, exist_ok=True)
    (sidecar / f"{name}.out").write_bytes(proc.stdout)
    (sidecar / f"{name}.err").write_bytes(proc.stderr)
    envelope = None
    if tool == "wright" and wants_json(argv):
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


def serve_transport(argv: list[str]) -> str:
    """The transport a `wright serve` argv selected; stdio is the server default."""
    for i, a in enumerate(argv):
        if a == "--transport" and i + 1 < len(argv):
            return argv[i + 1]
        if a.startswith("--transport="):
            return a.split("=", 1)[1]
    return "stdio"


def serve_tee(real: str, argv: list[str], started: float) -> int:
    proc = subprocess.Popen([real, *argv], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    counts = {"req": 0, "res": 0}
    session = {"session": os.getpid(), "transport": serve_transport(argv)}  # separates this session's lines from a concurrent one

    def log(direction: str, line: bytes) -> None:
        counts[direction] += 1
        append_event({"type": "serve", "dir": direction, "t": time.time(), "line": line.decode(errors="replace"), **session})

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


def mcp_op(name) -> str | None:
    """The Wright operation an MCP tool name carries: `wright_call_graph` -> `callGraph`."""
    if not isinstance(name, str):
        return None
    return re.sub(r"_([a-z])", lambda m: m.group(1).upper(), name.removeprefix("wright_"))


def serve_request(line: str, transport: str = "stdio") -> dict:
    """One serve request line: `{op, args, expects}`.

    `expects` mirrors the server's answer rule for the session's transport (`serve.rs`/`mcp.rs`): a blank line is skipped
    silently; under jsonrpc/mcp a notification — a JSON-RPC object with a string `method` and no `id` — is answered with
    silence; every other line is answered, including unparseable or malformed input (a parse/invalid-request error).
    `tools/call` maps to the Wright operation its tool name carries, and the jsonrpc transport's wright methods
    (`compile`, `check`, `analyze`, `inspect`) map to their names; every other method keeps a `<transport>:` name so
    transport traffic (handshake, `tools/list`) is distinguishable from Wright operations."""
    if not line.strip():
        return {"op": None, "args": None, "expects": False}
    try:
        message = json.loads(line)
    except json.JSONDecodeError:
        message = None
    expects = transport == "stdio" or not (
        isinstance(message, dict) and message.get("jsonrpc") == "2.0" and isinstance(message.get("method"), str) and "id" not in message
    )
    if not isinstance(message, dict):
        return {"op": None, "args": None, "expects": expects}
    if transport == "stdio":  # a bare {op, ...} request per line; anything else is answered malformed-request
        if "op" in message:
            op = message["op"]
            return {"op": op if isinstance(op, str) else "", "args": {k: v for k, v in message.items() if k != "op"}, "expects": expects}
        return {"op": None, "args": None, "expects": expects}
    method = message.get("method")
    params = message.get("params") if isinstance(message.get("params"), dict) else {}
    if transport == "mcp" and method == "tools/call":
        return {"op": mcp_op(params.get("name")), "args": params.get("arguments"), "expects": expects}
    if transport == "jsonrpc" and (method == "request" and (op := params.get("op")) or method in DECISION_COMMANDS and (op := method)):
        return {"op": op if isinstance(op, str) else None, "args": params or None, "expects": expects}
    return {"op": f"{transport}:{method}" if isinstance(method, str) else None, "args": None, "expects": expects}


def serve_pairs(events: list[dict]) -> list[tuple[dict, dict, dict | None]]:
    """`(request event, parsed request, response event)` for every serve request that expects an answer, in request order.

    Serve lines carry their session's id and transport (the shim tags them); pairing is FIFO within a session because the
    server answers its lines strictly in order. A request left without a response pairs with None."""
    sessions: dict = {}
    for event in events:
        if event.get("type") == "serve":
            sessions.setdefault(event.get("session"), []).append(event)
    pairs: list[tuple[dict, dict, dict | None]] = []
    for stream in sessions.values():
        pending: list[tuple[dict, dict]] = []
        for event in stream:
            if event["dir"] == "req":
                request = serve_request(event["line"], event.get("transport", "stdio"))
                if request["expects"]:
                    pending.append((event, request))
            elif pending:
                pairs.append((*pending.pop(0), event))
        pairs += [(*pair, None) for pair in pending]
    return sorted(pairs, key=lambda pair: pair[0]["t"])


def serve_error(line: str) -> str | None:
    """Why a serve response failed: `malformed` when the request could not be understood at all, `refused` for a structured
    rejection the server understood (unknown tool, bad params, or a Wright refusal), else None."""
    try:
        message = json.loads(line)
    except json.JSONDecodeError:
        return None  # counted by `unparsedServeResponses`, not here
    if not isinstance(message, dict):
        return None
    error = message.get("error")
    if isinstance(error, dict):
        code = error.get("code")
        return "malformed" if code in (-32700, -32600, "malformed-request") else "refused"
    result = message.get("result")
    if isinstance(result, dict) and result.get("isError") and "content" in result:  # an MCP tool result carrying a refusal
        return "refused"
    return None


def serve_use(request: dict) -> bool:
    """A serve request counts as a Wright use when it carried a Wright operation; `<transport>:` methods are transport traffic."""
    return bool(request["op"]) and ":" not in request["op"]


def request_key(request: dict) -> tuple:
    """Identity for repeat/retry detection: the operation plus its serialized arguments."""
    return (request["op"], json.dumps(request["args"], sort_keys=True))


def serve_ops(events: list[dict]) -> list[str]:
    """The operation names a serve session carried, in order: Wright ops and `<transport>:` methods."""
    return [request["op"] for _, request, _ in serve_pairs(events) if request["op"]]


def transcript_events(path: Path):
    """Parsed transcript events (dicts only); empty when the transcript does not exist."""
    if not path.is_file():
        return
    for line in path.read_text().splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(event, dict):
            yield event


def call_counts(path: Path) -> dict[str, int] | None:
    """Model tool calls by name from a normalized adapter transcript (`calls` on its events), None when it has none."""
    counts: dict[str, int] = {}
    for event in transcript_events(path):
        for call in event.get("calls") or []:
            name = call.get("name") if isinstance(call, dict) else None
            if name:
                counts[name] = counts.get(name, 0) + 1
    return counts or None


SEARCH_COMMAND = re.compile(r"(?<![\w./-])(?:rg|grep|find|fd|cat|bat|head|tail|less|more|sed|awk|ls|tree|wc|file|stat|strings|diff|du)\b")
WRIGHT_IN_SHELL = re.compile(r"(?<![\w./-])wright\b")


def shell_search_reads(path: Path) -> int:
    """`bash` transcript calls that ran a search/read command; a call that invokes the wright CLI is excluded (`toolUse` counts it)."""
    count = 0
    for event in transcript_events(path):
        for call in event.get("calls") or []:
            if not isinstance(call, dict) or call.get("name") != "bash":
                continue
            command = call.get("input").get("command") if isinstance(call.get("input"), dict) else None
            if isinstance(command, str) and SEARCH_COMMAND.search(command) and not WRIGHT_IN_SHELL.search(command):
                count += 1
    return count


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


def tool_events(events: list[dict], tool: str) -> list[dict]:
    return [e for e in events if e.get("tool") == tool]


def summarize_trace(events: list[dict]) -> dict:
    """Per tool: invocations, failures, and output size, for every tool that was called."""
    return {tool: summarize_tool(tool_events(events, tool)) for tool in sorted({e["tool"] for e in events if "tool" in e})}


def summarize_tool(events: list[dict]) -> dict:
    """Wright uses: every CLI invocation, plus each serve request's operation (`check`, `lint`, ...).

    A `serve` session is a container — its spawn is not itself a use unless it never carried a request — so a
    `wright serve` session and an MCP `tools/call` count the same way. The `<transport>:` handshake is not a use
    either, but its response bytes (`mcp:tools/list` carries the schemas) count as output."""
    calls = [e for e in events if e["type"] == "call"]
    cli = [c for c in calls if "requests" not in c]
    sessions = [c for c in calls if "requests" in c]
    pairs = serve_pairs(events)
    uses = [(request, response) for _, request, response in pairs if serve_use(request)]
    by_command: dict[str, int] = {}
    for call in calls:
        by_command[command_of(call["argv"])] = by_command.get(command_of(call["argv"]), 0) + 1
    for _, request, _ in pairs:
        if request["op"]:
            by_command[request["op"]] = by_command.get(request["op"], 0) + 1
    output: dict[str, int] = {}
    for call in calls:
        command = command_of(call["argv"])
        if command:
            output[command] = output.get(command, 0) + call.get("stdoutBytes", 0) + call.get("stderrBytes", 0)
    for _, request, response in pairs:
        if request["op"] and response:
            output[request["op"]] = output.get(request["op"], 0) + len(response["line"])
    return {
        "invocations": len(cli) + len(uses) + sum(1 for c in sessions if not c["requests"]),
        "byCommand": by_command,
        "failedInvocations": sum(1 for c in calls if c["exit"] != 0) + sum(1 for request, response in uses if response and serve_error(response["line"])),
        "ownerOrEnvironmentGaps": [c["argv"] for c in calls if c["exit"] >= 3],
        "outputTokensEstimate": {cmd: total // TOKEN_BYTES for cmd, total in output.items()},
    }


def friction(events: list[dict]) -> dict:
    """Wright friction; other tools are summarized by `summarize_trace` only."""
    events = tool_events(events, "wright")
    calls = [e for e in events if e["type"] == "call"]
    pairs = serve_pairs(events)
    requests = [(request, response) for _, request, response in pairs if serve_use(request)]
    seen: list = []
    repeats = 0
    for key in [tuple(c["argv"]) for c in calls] + [request_key(request) for request, _ in requests]:
        repeats += key in seen
        seen.append(key)
    errors = [serve_error(e["line"]) for e in events if e["type"] == "serve" and e["dir"] == "res"]
    unparsed = 0
    for event in events:
        if event["type"] == "serve" and event["dir"] == "res":
            try:
                json.loads(event["line"])
            except json.JSONDecodeError:
                unparsed += 1
    keys = [request_key(request) for request, _ in requests]
    return {
        "usageErrors": sum(1 for c in calls if c["exit"] == 2),
        "unknownSubcommands": sum(1 for c in calls if "unrecognized subcommand" in c.get("stderrHead", "")),
        "helpLookups": sum(1 for c in calls if any(a in ("--help", "-h", "help") for a in c["argv"])),
        "retriesAfterUnsupported": sum(1 for i, c in enumerate(calls) if c["exit"] >= 3 and tuple(c["argv"]) in [tuple(x["argv"]) for x in calls[i + 1:]])
        + sum(1 for i, (request, response) in enumerate(requests) if response and serve_error(response["line"]) == "refused" and keys[i] in keys[i + 1:]),
        "malformedServeRequests": errors.count("malformed"),
        "unparsedServeResponses": unparsed,
        "identicalRepeats": repeats,
        "callsToFirstSuccess": next((i + 1 for i, c in enumerate(calls) if c["exit"] == 0 and not any(a in ("--help", "-h", "--version") for a in c["argv"])), None),
    }


def expectation(status: str, detail: str = "") -> dict:
    return {"status": status, "detail": detail}


def detect_expectations(events: list[dict], snapshots: list[dict], scenario: dict, final_sha256: str | None) -> dict:
    """SPEC-414 E01-E12 over the Wright trace. E05, E07, E09, E10 need the agent transcript: `unavailable`."""
    events = tool_events(events, "wright")
    calls = [e for e in events if e["type"] == "call"]
    pairs = serve_pairs(events)
    ops = serve_ops(events)
    requests = [request for _, request, _ in pairs if serve_use(request)]
    real_ops = [request["op"] for request in requests]
    used = bool(calls)
    unavailable = expectation("unavailable", "needs the normalized agent transcript")
    result = {k: unavailable for k in ("E05", "E07", "E09", "E10")}
    if not used:
        return {**result, **{k: expectation("na", "Wright not used") for k in ("E01", "E02", "E03", "E04", "E06", "E08", "E11", "E12")}}
    first = calls[0]
    discovery = any(a in ("--help", "-h", "help", "--version") for a in first["argv"]) or "capabilities" in ops[:1] or (ops[:1] and ":" in ops[0])
    result["E01"] = expectation("pass" if discovery else "fail", f"first call: {' '.join(first['argv'])}")
    decision = [c for c in calls if command_of(c["argv"]) in DECISION_COMMANDS]
    structured = [c for c in decision if wants_json(c["argv"])]
    if decision or real_ops:
        rate = (len(structured) + len(real_ops)) / (len(decision) + len(real_ops))
        result["E02"] = expectation("pass" if rate >= 0.5 else "fail", f"structured share {rate:.2f}")
    else:
        result["E02"] = expectation("na", "no decision-driving calls")
    if scenario.get("stabilityRisk"):
        stability = any(command_of(c["argv"]) in ("lint", "analyze") for c in calls) or any(o in ("lint", "findings", "analyze") for o in real_ops)
        result["E03"] = expectation("pass" if stability else "fail", "lint or analyze run" if stability else "only check-level validation")
    else:
        result["E03"] = expectation("na", "scenario has no stability risk")
    last_edit = max((s["t"] for s in snapshots), default=None)
    if last_edit is None:
        result["E04"] = expectation("na", "no edits observed")
    else:
        after = [c for c in calls if command_of(c["argv"]) in VALIDATING and c["t"] + c["seconds"] >= last_edit]
        after += [e for e, request in ((e, p) for e, p, _ in pairs) if request["op"] in VALIDATING and e["t"] >= last_edit]
        matched = [c for c in after if c.get("envelope") and c["envelope"].get("inputIdentity") == final_sha256]
        result["E04"] = expectation("pass" if after else "fail", f"{len(after)} validation(s) after last edit; {len(matched)} match the final content")
    withheld = [c for c in calls if ((c.get("envelope") or {}).get("selection") or {}).get("withheld")]
    flags = sum(1 for c in calls if any(a in ("--severity", "--rule-id", "--file", "--max") for a in c["argv"]))
    result["E06"] = expectation("na" if not withheld and not flags else "info", f"{flags} selection-flag call(s), {len(withheld)} withheld result(s)")
    unsupported = [c for c in calls if c["exit"] >= 3]
    excess = [c for c in unsupported if sum(1 for x in calls if x["argv"] == c["argv"]) > 2]
    refused = [i for i, (_, request, response) in enumerate(pairs) if response and serve_error(response["line"]) == "refused"]
    keys = [request_key(request) for _, request, _ in pairs]
    retry_pairs = sum(1 for i in refused if keys.count(keys[i]) > 2)
    result["E08"] = expectation("na" if not unsupported and not refused else ("fail" if excess or retry_pairs else "pass"),
                                f"{len(unsupported)} exit 3/4 call(s) and {len(refused)} refused request(s), {len(excess) + retry_pairs} retried more than twice")
    if ops:
        malformed = friction(events)["malformedServeRequests"]
        discovery_op = ops[0] == "capabilities" or ":" in ops[0]  # a transport handshake (`mcp:initialize`, ...) is the discovery
        result["E11"] = expectation("pass" if discovery_op and not malformed else "fail", f"first op '{ops[0]}', {malformed} malformed")
    else:
        result["E11"] = expectation("na", "no serve session")
    edit_times = [s["t"] for s in snapshots]
    wasted = 0
    for i, c in enumerate(calls):
        for later in calls[i + 1:]:
            if later["argv"] == c["argv"]:
                wasted += not any(c["t"] <= t <= later["t"] for t in edit_times)
                break
    request_events = [(e, request_key(request)) for e, request, _ in pairs if serve_use(request)]
    for i, (event, key) in enumerate(request_events):
        for later, later_key in request_events[i + 1:]:
            if later_key == key and not any(event["t"] <= t <= later["t"] for t in edit_times):
                wasted += 1
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


if __name__ == "__main__":
    sys.exit(shim_main(sys.argv[2:]))  # the tool shims run this file directly: `bench_trace.py shim <tool> args...`
