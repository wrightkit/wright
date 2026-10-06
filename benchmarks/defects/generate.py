"""Generate seeded-defect benchmark scenarios (SPEC-534).

`python3 benchmarks/defects/generate.py --all` writes every committed instance
into `benchmarks/agent/scenarios/`; `--check` regenerates them in memory and
reports drift so the committed directories cannot silently diverge from the
generator. A seed plus a defect class maps to exactly one instance: the site
comes from `random.Random(sha256(defect | seed-tree-hash | canary))`, so
regeneration is deterministic and two seeds never produce the same instance.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import random
import shutil
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "agent"))
import agent_bench
import bench_grade
import defects

HERE = Path(__file__).resolve().parent
BENCHMARKS = HERE.parent
SCENARIOS = BENCHMARKS / "agent" / "scenarios"

# Committed public instances. Held-out instances for the private suite are
# generated the same way with private-NNN ids; they are not listed here.
INSTANCES = [
    {"id": "renamed-callable-payload-race", "defect": "renamed-callable", "seed": "defects/seeds/payload-race", "entry": "main.opy"},
    {"id": "renamed-callable-understand-opy", "defect": "renamed-callable", "seed": "agent/scenarios/understand-opy-project/seed", "entry": "mode.opy"},
]


def seed_files(seed: Path) -> dict[str, str]:
    return {str(path.relative_to(seed)): path.read_text() for path in sorted(seed.rglob("*")) if path.is_file()}


def seed_hash(files: dict[str, str]) -> str:
    digest = hashlib.sha256()
    for name, text in sorted(files.items()):
        digest.update(name.encode() + b"\0" + text.encode())
    return digest.hexdigest()


def pick_site(defect: defects.Defect, files: dict[str, str]) -> defects.Site:
    sites = defect.sites(files)
    if not sites:
        raise SystemExit(f"{defect.id}: no injectable site in the seed")
    key = f"{defect.id}:{seed_hash(files)}:{agent_bench.CANARY}"
    return sites[random.Random(int(hashlib.sha256(key.encode()).hexdigest(), 16)).randrange(len(sites))]


def compile_seed(wright: str, seed: Path, entry: str, language: str, scratch: Path) -> str:
    """Compile the pristine seed with the authority whose output the generated
    probes are checked against: the upstream oracle for OPY, Wright otherwise."""
    source = seed / entry
    out = scratch / "compiled.ws"
    if language == "opy":
        status = bench_grade.oracle_compile(source, out)
        if status["status"] != "ok":
            raise SystemExit(f"seed does not compile under the oracle: {status.get('error', 'unavailable')}")
    else:
        status = bench_grade.wright_compile(wright, source, out)
        if status["status"] != "ok":
            raise SystemExit(f"seed does not compile under {wright}: {status.get('error')}")
    return out.read_text()


def render(defect: defects.Defect, spec: dict, compiled: str) -> dict[str, str]:
    """The whole scenario directory as {relative path: content}."""
    files = seed_files(BENCHMARKS / spec["seed"])
    site = pick_site(defect, files)
    applied = defect.apply(files, site, compiled)
    scenario = {
        "id": spec["id"],
        "family": defect.family,
        "language": defect.language,
        "split": "train",
        "entry": spec["entry"],
        "writable": applied.writable,
        "checks": applied.checks,
        "negatives": {name: {"fails": neg["fails"]} for name, neg in applied.negatives.items()},
        "generated": {"defect": defect.id, "site": {"file": site.file, "symbol": site.symbol}, "seedHash": seed_hash(files)},
        "canary": agent_bench.CANARY,
    }
    tree = {"scenario.json": json.dumps(scenario, indent=2) + "\n", "prompt.md": applied.prompt}
    for name, text in applied.seed_files.items():
        tree[f"seed/{name}"] = text
    for name, text in applied.reference.items():
        tree[f"reference/{name}"] = text
    for name, neg in applied.negatives.items():
        for path, text in neg["files"].items():
            tree[f"negative/{name}/{path}"] = text
    return tree


def write_instance(wright: str, spec: dict) -> None:
    defect = defects.REGISTRY[spec["defect"]]
    with tempfile.TemporaryDirectory() as scratch:
        compiled = compile_seed(wright, BENCHMARKS / spec["seed"], spec["entry"], defect.language, Path(scratch))
        tree = render(defect, spec, compiled)
    directory = SCENARIOS / spec["id"]
    shutil.rmtree(directory, ignore_errors=True)
    for name, text in tree.items():
        path = directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    print(f"wrote {directory.relative_to(BENCHMARKS.parent)}")


def check_current(wright: str) -> bool:
    ok = True
    for spec in INSTANCES:
        defect = defects.REGISTRY[spec["defect"]]
        directory = SCENARIOS / spec["id"]
        with tempfile.TemporaryDirectory() as scratch:
            compiled = compile_seed(wright, BENCHMARKS / spec["seed"], spec["entry"], defect.language, Path(scratch))
            want = render(defect, spec, compiled)
        have = {str(p.relative_to(directory)): p.read_text() for p in directory.rglob("*") if p.is_file()} if directory.is_dir() else {}
        if have != want:
            ok = False
            drift = sorted(set(have) ^ set(want)) + [n for n in have.keys() & want.keys() if have[n] != want[n]]
            print(f"drift {spec['id']}: {drift}")
    return ok


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wright", default=str(BENCHMARKS.parent / "target/debug/wright"))
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--all", action="store_true", help="write every committed instance")
    mode.add_argument("--check", action="store_true", help="report drift between the generator and committed instances")
    args = parser.parse_args()
    if args.check:
        return 0 if check_current(args.wright) else 1
    for spec in INSTANCES:
        write_instance(args.wright, spec)
    return 0


if __name__ == "__main__":
    sys.exit(main())
