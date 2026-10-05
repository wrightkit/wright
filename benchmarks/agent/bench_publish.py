"""Allow-listed hosted Wright Agent Score data and immutable R2 publication."""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
from collections import Counter
from pathlib import Path

import bench_report
import bench_score

CONTRACT = "wright-agent-results/v1"
SCHEMA = Path(__file__).resolve().parents[2] / "schemas/wright-agent-results-v1.schema.json"
BUCKET = "wrightkit-release"
PUBLIC_BASE = "https://releases.wrightkit.dev"


def run_id(directory: Path) -> str:
    return "run-" + hashlib.sha256(directory.name.encode()).hexdigest()[:24]


SUITE_KEYS = ("version", "publicHash", "privateHash", "hash", "mode", "scenarios")


def environment(card: dict) -> dict:
    ident = card["identity"]
    suite = ident["suite"]
    if not isinstance(suite, dict):
        raise ValueError("score card predates suite versioning; regenerate score.json")
    missing = [key for key in SUITE_KEYS if key not in suite]
    if missing:
        raise ValueError(f"score card lacks suite identity fields: {', '.join(missing)}")
    protocol = ident.get("protocol") or {}
    return {
        "wright": ident["wright"], "wrightSha256": ident["wrightSha256"],
        "skills": [{"name": name, "sha256": sha} for name, sha in sorted(ident["skills"].items())],
        "suite": {key: suite[key] for key in SUITE_KEYS},
        "protocol": {key: protocol[key] for key in ("timeoutSeconds", "infraRetries")},
        **{key: ident[key] for key in ("fileReadEnforcement", "fileWriteEnforcement", "networkEnforcement")},
    }


def provisional_reasons(card: dict) -> list[str]:
    reasons = []
    for reason in card["provisional"]:
        if reason.startswith("missing held-out scenarios: "):
            reasons.append("missing held-out scenarios")
        elif reason.startswith("unequal valid trials per scenario: "):
            reasons.append("unequal valid trials per scenario")
        elif reason.startswith("fewer than ") and " held-out scenarios in the suite (" in reason:
            reasons.append("too few held-out scenarios")
        else:
            raise ValueError("unrecognized provisional reason; regenerate score.json with this harness")
    return reasons


def entry(directory: Path) -> dict:
    cards = [c for c in json.loads((directory / "score.json").read_text())["cards"] if "refused" not in c]
    if not cards:
        raise ValueError("no scored tracks")
    environments = [environment(c) for c in cards]
    if any(e != environments[0] for e in environments):
        raise ValueError("tracks have different environments")
    identities = [{k: c["identity"][k] for k in ("agent", "model", "effort")} for c in cards]
    if any(i != identities[0] for i in identities):
        raise ValueError("tracks have different agent setups")
    ident = identities[0]
    infos = []
    for result in bench_report.load([directory]):
        if (result.get("condition", {}).get("label") != bench_score.CANONICAL
                or result.get("split") != "test" or result.get("status") not in bench_score.VALID):
            continue
        if bench_score.identity_of(result) != cards[0]["identity"]:
            raise ValueError("score card is stale; regenerate score.json")
        info = result.get("agentInfo") or {}
        infos.append({"program": info.get("agent"), "version": info.get("version"),
                      "model": info.get("model"), "effort": info.get("effort")})
    if len({c["language"] for c in cards}) != len(cards):
        raise ValueError("duplicate language tracks")
    if not infos or any(i != infos[0] for i in infos):
        raise ValueError("missing or inconsistent recorded agent setup")
    setup = infos[0]
    if not setup["program"]:
        raise ValueError("recorded agent setup lacks the program name")
    if (setup["model"], setup["effort"]) != (ident["model"], ident["effort"]):
        raise ValueError("score card has a different model or effort")
    setup["skills"] = environments[0]["skills"]
    tracks = []
    for card in cards:
        tracks.append({
            **{k: card[k] for k in ("language", "score", "ci95", "ciMethod", "trialsPerScenario", "scenarios", "validRuns")},
            "provisional": provisional_reasons(card),
            "exclusions": [{"status": status, "count": count} for status, count in sorted(card["exclusions"].items())],
        })
    return {"id": run_id(directory), "agent": setup, "environment": environments[0],
            "tracks": sorted(tracks, key=lambda t: t["language"])}


def validate(bundle: dict) -> None:
    try:
        from jsonschema import Draft202012Validator
    except ImportError:
        raise ValueError("publish needs jsonschema: pip install -r benchmarks/agent/requirements-publish.txt") from None
    from jsonschema.exceptions import ValidationError
    try:
        Draft202012Validator(json.loads(SCHEMA.read_text())).validate(bundle)
    except ValidationError as error:
        raise ValueError("bundle violates the public results schema at " + ".".join(map(str, error.absolute_path))) from None


def build(dirs: list[Path]) -> tuple[dict, dict[str, dict]]:
    entries, excluded, runs = [], [], {}
    for directory in dirs:
        ident = run_id(directory)
        if ident in runs:
            raise ValueError("duplicate run id; evaluation directory names must be unique")
        try:
            item = entry(directory)
            bundle = {"contract": CONTRACT, "entries": [item], "excluded": []}
            validate(bundle)
        except (ValueError, KeyError, TypeError, AttributeError, OSError) as error:
            raise ValueError(f"cannot publish {directory.name}: {error}") from error
        runs[ident] = bundle
        entries.append(item)
    if not entries:
        raise ValueError("no evaluation runs supplied")
    key = lambda e: json.dumps(e["environment"], sort_keys=True)
    selected = Counter(key(e) for e in entries).most_common(1)[0][0]
    included = []
    for item in entries:
        if key(item) == selected:
            included.append(item)
        else:
            excluded.append({"id": item["id"], "reason": "different environment"})
    latest = {"contract": CONTRACT, "entries": sorted(included, key=lambda e: e["id"]),
              "excluded": sorted(excluded, key=lambda e: e["id"])}
    validate(latest)
    return latest, runs


def aws(endpoint: str, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["aws", "s3api", *args, "--bucket", BUCKET, "--endpoint-url", endpoint],
                          capture_output=True, text=True)


def upload(out: Path, ids: list[str], endpoint: str) -> None:
    for ident in ids:
        key = f"bench/runs/{ident}.json"
        source = out / key
        # Conditional writes enforce immutability even when two publishers race.
        put = aws(endpoint, "put-object", "--key", key, "--body", str(source), "--if-none-match", "*",
                  "--content-type", "application/json", "--cache-control", "public, max-age=31536000, immutable")
        if put.returncode:
            if "PreconditionFailed" not in put.stderr and "ConditionalRequestConflict" not in put.stderr:
                raise ValueError(f"R2 upload failed for {key}: {put.stderr.strip()}")
            get = subprocess.run(["aws", "s3", "cp", f"s3://{BUCKET}/{key}", "-", "--endpoint-url", endpoint],
                                 capture_output=True)
            if get.returncode:
                raise ValueError(f"cannot verify existing R2 run {ident}: {get.stderr.decode().strip()}")
            if get.stdout != source.read_bytes():
                raise ValueError(f"immutable run {ident} already exists with different content")
    put = aws(endpoint, "put-object", "--key", "bench/latest.json", "--body", str(out / "bench/latest.json"),
              "--content-type", "application/json", "--cache-control", "no-store, max-age=0")
    if put.returncode:
        raise ValueError(f"R2 latest upload failed: {put.stderr.strip()}")


def main(dirs: list[Path], out: Path, dry_run: bool, endpoint: str | None) -> int:
    if not dry_run:
        endpoint = endpoint or os.environ.get("R2_ENDPOINT")
        if not endpoint:
            raise ValueError("set R2_ENDPOINT to the existing release bucket's S3 endpoint")
    latest, runs = build(dirs)
    objects = {"bench/latest.json": latest, **{f"bench/runs/{ident}.json": bundle for ident, bundle in sorted(runs.items())}}
    for key, bundle in objects.items():
        path = out / key
        path.parent.mkdir(parents=True, exist_ok=True)
        content = json.dumps(bundle, sort_keys=True, indent=2, ensure_ascii=True) + "\n"
        if key != "bench/latest.json" and path.exists() and path.read_text() != content:
            raise ValueError(f"immutable local bundle {key} already has different content")
        path.write_text(content)
        print(f"{'would upload' if dry_run else 'upload'} {path} -> s3://{BUCKET}/{key} ({PUBLIC_BASE}/{key})")
    for item in latest["excluded"]:
        print(f"excluded from latest: {item['id']}: {item['reason']}")
    if not dry_run:
        upload(out, sorted(runs), endpoint)
    return 0
