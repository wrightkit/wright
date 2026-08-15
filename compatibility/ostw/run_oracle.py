#!/usr/bin/env python3
"""Pinned OSTW v3.4.0 language-server oracle (reference infrastructure only)."""
from __future__ import annotations

import argparse, hashlib, json, os, shutil, subprocess, sys, tempfile, time, urllib.request, zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
REFERENCE = HERE / "reference.json"
CORPUS = HERE / "corpus.json"
RESULTS = HERE / "results.json"

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
    # Linux self-contained asset needs an amd64 Linux executor on macOS/arm64.
    if sys.platform == "linux": return [str(executable)]
    if shutil.which("docker"):
        return ["docker", "run", "--rm", "-i", "--platform", "linux/amd64", "-v", f"{executable.parent}:/ostw:ro", "-v", f"{ROOT}:/workspace:ro", "-w", "/workspace", "mcr.microsoft.com/dotnet/runtime:8.0", "/ostw/Deltinteger"]
    raise OracleError("OSTW_REFERENCE_MISSING: linux-x64 oracle needs Docker on this host")

def ping(executable, reference):
    result = subprocess.run(command(executable)+["--ping"], capture_output=True, text=True, encoding="utf-8")
    if result.returncode or result.stdout.strip() != reference["ping"]:
        raise OracleError("OSTW_REFERENCE_INCOMPATIBLE: --ping did not return the pinned response")

def send(process, payload):
    body = json.dumps(payload, separators=(",", ":")).encode(); process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode()+body); process.stdin.flush()
def receive(process, deadline):
    import select
    headers = b""
    while b"\r\n\r\n" not in headers:
        if time.monotonic() > deadline: raise TimeoutError
        if not select.select([process.stdout], [], [], .1)[0]: continue
        headers += os.read(process.stdout.fileno(), 1)
    length = int(next(line.split(b":",1)[1] for line in headers.split(b"\r\n") if line.lower().startswith(b"content-length:")))
    body = b""
    while len(body) < length:
        if time.monotonic() > deadline: raise TimeoutError
        if select.select([process.stdout], [], [], .1)[0]: body += os.read(process.stdout.fileno(), length-len(body))
    return json.loads(body)

def run_project(executable, reference, project):
    root = ROOT / project["path"]; entry = root / project["entry"]
    if not entry.is_file(): raise OracleError(f"missing corpus entry: {project['id']}")
    proc = subprocess.Popen(command(executable)+["--langserver"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if sys.platform == "linux":
        root_uri = root.resolve().as_uri()
        uri_for = lambda path: path.resolve().as_uri()
    else:
        root_uri = f"file:///workspace/{root.relative_to(ROOT).as_posix()}"
        uri_for = lambda path: f"file:///workspace/{path.relative_to(ROOT).as_posix()}"
    send(proc, {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":None,"rootUri":root_uri,"capabilities":{}}})
    deadline=time.monotonic()+30; receive(proc, deadline); send(proc,{"jsonrpc":"2.0","method":"initialized","params":{}})
    for path in sorted(root.rglob("*")):
        if path.suffix.lower() in (".ostw", ".del"):
            send(proc,{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri_for(path),"languageId":"ostw","version":1,"text":path.read_text(encoding="utf-8")}}})
    diagnostics=[]; workshop=None; elements=None
    while time.monotonic() < deadline:
        try: message=receive(proc, deadline)
        except TimeoutError: break
        method=message.get("method"); params=message.get("params")
        if method == "textDocument/publishDiagnostics": diagnostics.extend(params.get("diagnostics", []))
        elif method == "workshopCode": workshop = params if isinstance(params,str) else params.get("workshopCode")
        elif method == "elementCount": elements = params
        if workshop is not None: break
    proc.terminate(); proc.wait(timeout=5)
    return {"id":project["id"],"entry":project["entry"],"accept":"reject" if diagnostics else "accept","firstDiagnostic":diagnostics[0] if diagnostics else None,"workshopCodeSha256":hashlib.sha256(workshop.encode()).hexdigest() if workshop else None,"workshopAvailable":bool(workshop),"elementCount":elements,"files":project["files"]}

def main():
    parser=argparse.ArgumentParser(); parser.add_argument("--reference-root", type=Path, default=ROOT/"target/ostw-reference"); parser.add_argument("--acquire",action="store_true"); parser.add_argument("--ping",action="store_true"); parser.add_argument("--update",action="store_true"); args=parser.parse_args()
    try:
        reference=load(REFERENCE); executable = acquire(reference,args.reference_root) if args.acquire else args.reference_root/reference["asset"]["executable"]
        if not executable.is_file(): raise OracleError("OSTW_REFERENCE_MISSING: run with --acquire or provide --reference-root")
        ping(executable,reference)
        if args.ping: return 0
        result={"schemaVersion":1,"reference":reference,"projects":[run_project(executable,reference,p) for p in load(CORPUS)["projects"]]}
        if args.update: dump(RESULTS,result)
        elif load(RESULTS)!=result: raise OracleError("OSTW_ORACLE_DRIFT: results.json differs; review with --update")
    except (OracleError, OSError, subprocess.SubprocessError) as error:
        print(str(error),file=sys.stderr); return 2
    return 0
if __name__ == "__main__": raise SystemExit(main())
