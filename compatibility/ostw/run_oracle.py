#!/usr/bin/env python3
"""Pinned OSTW v3.4.0 language-server oracle (reference infrastructure only).

Every recorded observation identifies an explicit compile/document root:
the corpus runner opens exactly one document per LSP session, so the
deterministic final compile triple can only be that document's compile.
Results never derive meaning from didOpen ordering.
"""
from __future__ import annotations

import argparse, hashlib, json, os, re, shutil, subprocess, sys, tempfile, time, urllib.request, zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
REFERENCE = HERE / "reference.json"
CORPUS = HERE / "corpus.json"
RESULTS = HERE / "results.json"
PROBES = HERE / "probes"
PROBE_RESULTS = PROBES / "results.json"
SOURCE_SUFFIXES = (".ostw", ".del")
# After the last didOpen, the langserver's ~50ms debounce fires one final
# compile. Any gap of this many seconds means the final compile has fully
# published; earlier (interleaved) compile triples are timing-dependent and
# are never recorded.
QUIET_SECONDS = 3.0

class OracleError(RuntimeError): pass

def load(path): return json.loads(path.read_text(encoding="utf-8"))
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def dump(path, value): path.write_text(json.dumps(value, indent=2, sort_keys=True)+"\n", encoding="utf-8")

def acquire(reference, root):
    asset = reference["asset"]; archive = root / asset["name"]; executable = root / asset["executable"]
    root.mkdir(parents=True, exist_ok=True)
    if not archive.is_file():
        with urllib.request.urlopen(asset["url"], timeout=120) as response: archive.write_bytes(response.read())
    if archive.stat().st_size != asset["size"] or sha(archive) != asset["sha256"]:
        raise OracleError("OSTW_REFERENCE_INCOMPATIBLE: pinned asset size or SHA-256 differs")
    if not executable.is_file():
        with zipfile.ZipFile(archive) as bundle:
            member = next((name for name in bundle.namelist() if name.replace("\\", "/").endswith("/Deltinteger")), None)
            if member is None: raise OracleError("OSTW_REFERENCE_INCOMPATIBLE: Deltinteger is absent from asset")
            for item in bundle.infolist():
                item.filename = item.filename.replace("\\", "/")
                bundle.extract(item, root)
        nested = next(root.glob("*/Deltinteger"), None)
        if nested is None: raise OracleError("OSTW_REFERENCE_INCOMPATIBLE: extracted executable missing")
        for item in nested.parent.iterdir(): shutil.move(str(item), root / item.name)
        nested.parent.rmdir()
        executable.chmod(0o755)
    return executable

def command(executable):
    if shutil.which("docker"):
        return ["docker", "run", "--rm", "-i", "--platform", "linux/amd64", "-v", f"{executable.parent}:/ostw:ro", "-v", f"{ROOT}:/workspace:ro", "-w", "/workspace", "mcr.microsoft.com/dotnet/runtime:8.0", "/ostw/Deltinteger"]
    if sys.platform == "linux": return [str(executable)]
    raise OracleError("OSTW_REFERENCE_MISSING: linux-x64 oracle needs Docker on this host")

def ping(executable, reference):
    result = subprocess.run(command(executable)+["--ping"], capture_output=True, text=True, encoding="utf-8")
    if result.returncode or result.stdout.strip() != reference["ping"]:
        raise OracleError("OSTW_REFERENCE_INCOMPATIBLE: --ping did not return the pinned response")

def send(process, payload):
    body = json.dumps(payload, separators=(",", ":")).encode(); process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode()+body); process.stdin.flush()

_BUFFER = bytearray()

def receive(process, deadline):
    """Read one Content-Length framed JSON-RPC message. The langserver
    occasionally emits stray non-protocol bytes (bare CRLFCRLF) on stdout
    between frames; bytes before a valid frame header are skipped rather than
    treated as a malformed frame, and bytes after a frame's body are retained
    for the next message."""
    import select
    global _BUFFER
    while True:
        match = re.search(rb"Content-Length: (\d+)\r\n\r\n", bytes(_BUFFER))
        if match:
            length = int(match.group(1))
            body_start = match.end()
            if len(_BUFFER) >= body_start + length:
                frame = bytes(_BUFFER[body_start:body_start+length])
                del _BUFFER[:body_start+length]
                return json.loads(frame)
            chunk = read_chunk(process, time.monotonic()+10)
            if chunk: _BUFFER += chunk
            continue
        if time.monotonic() > deadline: raise TimeoutError
        chunk = read_chunk(process, deadline)
        if chunk: _BUFFER += chunk
        if len(_BUFFER) > 1 << 20:
            # Never saw a valid frame header; treat as a lost session rather
            # than buffering unbounded noise.
            raise OracleError("OSTW_ORACLE_SESSION_CLOSED: no valid langserver frame")

def read_chunk(process, deadline):
    import select
    if not select.select([process.stdout], [], [], .1)[0]:
        if time.monotonic() > deadline: raise TimeoutError
        return b""
    data = os.read(process.stdout.fileno(), 65536)
    if not data: raise OracleError("OSTW_ORACLE_SESSION_CLOSED: langserver stdout closed")
    return data

def lsp_session(executable, root, open_paths, attempts=3):
    """One LSP session that opens exactly open_paths under root and returns the
    full message stream. open_paths are sorted for determinism. A session that
    drops mid-stream (transient container failure) is retried; the final
    compile triple is deterministic given the pinned binary."""
    for attempt in range(attempts):
        try:
            return _lsp_session(executable, root, open_paths)
        except OracleError as error:
            if not str(error).startswith("OSTW_ORACLE_SESSION_CLOSED"): raise
            if attempt + 1 == attempts:
                raise OracleError(f"OSTW_ORACLE_SESSION_CLOSED: {open_paths} after {attempts} attempts") from error

def _lsp_session(executable, root, open_paths):
    """One LSP session that opens exactly open_paths under root and returns the
    full message stream. open_paths are sorted for determinism."""
    proc = subprocess.Popen(command(executable)+["--langserver"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    root_uri = f"file:///workspace/{root.relative_to(ROOT).as_posix()}"
    uri_for = lambda path: f"file:///workspace/{path.relative_to(ROOT).as_posix()}"
    send(proc, {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":None,"rootUri":root_uri,"capabilities":{}}})
    deadline=time.monotonic()+30; receive(proc, deadline); send(proc,{"jsonrpc":"2.0","method":"initialized","params":{}})
    for path in sorted(open_paths):
        send(proc,{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri_for(path),"languageId":"ostw","version":1,"text":path.read_text(encoding="utf-8")}}})
    # The langserver debounces compiles ~50ms after the last didOpen and
    # publishes one coherent workshopCode/elementCount/diagnostics triple per
    # compile. Compiles that fire between didOpen batches are timing-dependent;
    # only the LAST triple, after every file is open, is deterministic
    # evidence. Drain until the server is quiet, then take the last triple.
    messages=[]
    last_activity=time.monotonic()
    drain_deadline=time.monotonic()+120
    try:
        while time.monotonic() < drain_deadline:
            try: message=receive(proc, time.monotonic()+1.0)
            except TimeoutError:
                if time.monotonic()-last_activity >= QUIET_SECONDS: break
                continue
            last_activity=time.monotonic()
            messages.append(message)
    finally:
        proc.terminate(); proc.wait(timeout=5)
    return messages

def last_compile_triple(messages):
    """Extract the deterministic LAST workshopCode/elementCount/diagnostics
    triple from a message stream; diagnostics published after the final
    elementCount belong to the final compile. Returns
    (workshop, elements, diagnostics, final_elements_index)."""
    workshop=None; elements=None; final_elements_index=-1
    for index,message in enumerate(messages):
        method=message.get("method"); params=message.get("params")
        if method == "workshopCode":
            workshop = params if isinstance(params,str) else params.get("workshopCode")
        elif method == "elementCount":
            elements = params
            final_elements_index = index
    diagnostics=[]
    for message in messages[final_elements_index+1:]:
        if message.get("method") == "textDocument/publishDiagnostics":
            diagnostics.extend(message.get("params", {}).get("diagnostics", []))
    return workshop, elements, diagnostics, final_elements_index

def located_diagnostics(messages, root_dir, final_elements_index):
    """Flatten the diagnostics of the deterministic final compile, attaching
    the document uri carried by each publishDiagnostics notification."""
    prefix = f"file:///workspace/{root_dir.relative_to(ROOT).as_posix()}/"
    located = []
    for message in messages[final_elements_index+1:]:
        if message.get("method") != "textDocument/publishDiagnostics":
            continue
        uri = (message.get("params") or {}).get("uri") or ""
        if uri.startswith(prefix): uri = uri[len(prefix):]
        for diagnostic in (message.get("params") or {}).get("diagnostics", []):
            entry = {"uri": uri, "severity": diagnostic.get("severity"), "range": diagnostic.get("range"), "message": diagnostic.get("message")}
            if diagnostic.get("code") is not None: entry["code"] = diagnostic["code"]
            if diagnostic.get("source") is not None: entry["source"] = diagnostic["source"]
            located.append(entry)
    return located

def strip_comments(text):
    """Remove // line and /* */ block comments so active-import scanning is
    comment-insensitive (commented imports are not part of the graph)."""
    out=[]; i=0; n=len(text)
    while i < n:
        if text[i:i+2] == "//":
            j = text.find("\n", i)
            if j == -1: break
            i = j
        elif text[i:i+2] == "/*":
            j = text.find("*/", i+2)
            if j == -1: break
            i = j+2
        else:
            out.append(text[i]); i += 1
    return "".join(out)

IMPORT_RE = re.compile(r'\bimport\s*"([^"]+)"\s*;?')

def import_closure(root, entry):
    """Deterministic BFS of the active quoted-import graph of `entry` under
    `root`. Import paths resolve relative to the importing file (pinned P2
    evidence). Returns (sorted reachable source files relative to root,
    sorted missing imports as {"imported": ..., "from": ...})."""
    root = root.resolve()
    reachable=set(); seen=set(); missing=[]
    pending=[root / entry]
    while pending:
        current = pending.pop().resolve()
        if current in seen: continue
        seen.add(current)
        if not (current.is_file() and current.suffix.lower() in SOURCE_SUFFIXES): continue
        reachable.add(current)
        for imported in find_imports(current):
            resolved = (current.parent / imported).resolve()
            if resolved.is_file() and resolved.suffix.lower() in SOURCE_SUFFIXES:
                pending.append(resolved)
            else:
                missing.append({"imported": imported, "from": str(current.relative_to(root))})
    closure = sorted(str(path.relative_to(root)) for path in reachable)
    missing = sorted(missing, key=lambda item: (item["from"], item["imported"]))
    return closure, missing

def find_imports(path):
    return IMPORT_RE.findall(strip_comments(path.read_text(encoding="utf-8")))

def run_observation(executable, project, root_spec):
    """Run the pinned reference with exactly one document open: the explicit
    observation root. The recorded compile can only be that root's compile;
    didOpen ordering cannot change its meaning."""
    root_dir = ROOT / project["path"]
    root = root_spec["root"]
    doc = root_dir / root
    if not doc.is_file(): raise OracleError(f"missing corpus root: {project['id']} {root}")
    closure, missing = import_closure(root_dir, root)
    messages = lsp_session(executable, root_dir, [doc])
    workshop, elements, _diagnostics, final_elements_index = last_compile_triple(messages)
    compiled = elements is not None and elements >= 0
    return {
        "root": root,
        "role": root_spec.get("role", "document-root"),
        "openMode": "single",
        "opened": [root],
        "importClosure": closure,
        "missingImports": missing,
        "accept": "accept" if compiled else "reject",
        "elementCount": elements,
        "diagnostics": located_diagnostics(messages, root_dir, final_elements_index),
        "workshopAvailable": bool(workshop) and compiled,
        "workshopCodeSha256": hashlib.sha256(workshop.encode()).hexdigest() if workshop is not None else None,
    }

def run_probe(executable, reference, probe_dir):
    """Run one probe project: one LSP session per manifest run (openMode entry
    or all). Writes result.<runId>.json and workshop.<runId>.txt into the probe
    directory and returns the per-run result records."""
    manifest = load(probe_dir / "probe.json")
    if manifest.get("schemaVersion") != 1: raise OracleError(f"unsupported probe schema: {probe_dir}")
    results=[]
    for run in manifest["runs"]:
        run_id = run["runId"]; mode = run["openMode"]
        if mode == "all":
            open_paths = sorted(path for path in probe_dir.rglob("*") if path.suffix.lower() in SOURCE_SUFFIXES)
        elif mode == "entry":
            open_paths = [probe_dir / manifest["entry"]]
        else:
            raise OracleError(f"unknown probe openMode {mode!r} in {probe_dir.name}")
        messages = lsp_session(executable, probe_dir, open_paths)
        workshop, elements, _diagnostics, final_elements_index = last_compile_triple(messages)
        compiled = elements is not None and elements >= 0
        result = {
            "runId": run_id,
            "openMode": mode,
            "opened": [str(path.relative_to(probe_dir)) for path in open_paths],
            "accept": "accept" if compiled else "reject",
            "elementCount": elements,
            "diagnostics": located_diagnostics(messages, probe_dir, final_elements_index),
            "messageCount": len(messages),
            "workshopAvailable": bool(workshop) and compiled,
            "workshopCodeSha256": hashlib.sha256(workshop.encode()).hexdigest() if workshop is not None else None,
        }
        dump(probe_dir / f"result.{run_id}.json", result)
        if workshop is not None:
            (probe_dir / f"workshop.{run_id}.txt").write_text(workshop, encoding="utf-8")
        results.append(result)
    return results

def main():
    parser=argparse.ArgumentParser(); parser.add_argument("--reference-root", type=Path, default=ROOT/"target/ostw-reference"); parser.add_argument("--acquire",action="store_true"); parser.add_argument("--ping",action="store_true"); parser.add_argument("--update",action="store_true"); parser.add_argument("--probes",action="store_true"); args=parser.parse_args()
    try:
        reference=load(REFERENCE); executable = acquire(reference,args.reference_root) if args.acquire else args.reference_root/reference["asset"]["executable"]
        if not executable.is_file(): raise OracleError("OSTW_REFERENCE_MISSING: run with --acquire or provide --reference-root")
        ping(executable,reference)
        if args.ping: return 0
        if args.probes:
            probes=[]
            for probe_dir in sorted(PROBES.iterdir()):
                if not probe_dir.is_dir() or not (probe_dir/"probe.json").is_file(): continue
                manifest = load(probe_dir/"probe.json")
                record = {"id": manifest["id"], "intent": manifest.get("intent"), "entry": manifest.get("entry"), "runs": run_probe(executable, reference, probe_dir)}
                if manifest.get("role") is not None: record["role"] = manifest["role"]
                probes.append(record)
            for probe in probes:
                if probe.get("role") == "differential-target" and not any(run["accept"] == "accept" for run in probe["runs"]):
                    raise OracleError(f"OSTW_ORACLE_DRIFT: differential target {probe['id']} is reference-rejected")
            targets = sorted(probe["id"] for probe in probes if probe.get("role") == "differential-target")
            dump(PROBE_RESULTS, {"schemaVersion":2,"reference":reference,"differentialTargets":targets,"probes":probes})
            return 0
        projects=[]
        for project in load(CORPUS)["projects"]:
            observations=[run_observation(executable, project, root_spec) for root_spec in project["roots"]]
            projects.append({"id": project["id"], "entry": project["entry"], "observations": observations})
        result={"schemaVersion":2,"reference":reference,"projects":projects}
        if args.update: dump(RESULTS,result)
        elif load(RESULTS)!=result: raise OracleError("OSTW_ORACLE_DRIFT: results.json differs; review with --update")
    except (OracleError, OSError, subprocess.SubprocessError) as error:
        print(str(error),file=sys.stderr); return 2
    return 0
if __name__ == "__main__": raise SystemExit(main())
