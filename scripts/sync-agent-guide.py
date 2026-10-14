#!/usr/bin/env python3
"""Sync or verify the vendored Wright agent guide.

`agent-guide/wright/` is a verbatim copy of `skills/wright/` from the
wrightkit/skills repository, embedded in the binary for `wright agent install`.
The commit it was taken from is recorded in two places, `agent-guide/UPSTREAM.md`
and `UPSTREAM` in `crates/wright-cli/src/agent.rs`. This script keeps all three
in step.

Usage:
  python3 scripts/sync-agent-guide.py --skills-dir DIR [--ref REF]
      Replace the vendored copy with `skills/wright/` at REF (default
      origin/main) of the skills clone in DIR and rewrite both pins.
  python3 scripts/sync-agent-guide.py --skills-dir DIR --check
      Verify the copy equals `skills/wright/` at the recorded pin, both pins
      agree, and every vendored file is embedded by agent.rs. DIR needs full
      history so the pinned commit is reachable.
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
GUIDE_DIR = REPO_ROOT / "agent-guide" / "wright"
UPSTREAM_MD = REPO_ROOT / "agent-guide" / "UPSTREAM.md"
AGENT_RS = REPO_ROOT / "crates" / "wright-cli" / "src" / "agent.rs"
SKILL_PREFIX = "skills/wright/"

MD_PIN = re.compile(r"at commit `([0-9a-f]+)`")
RS_PIN = re.compile(r'"wrightkit/skills@([0-9a-f]+)"')


def git(skills_dir: Path, *args: str) -> str:
    return subprocess.run(
        ["git", "-C", str(skills_dir), *args],
        check=True,
        capture_output=True,
        text=True,
    ).stdout


def skill_files(skills_dir: Path, rev: str) -> dict[str, str]:
    names = git(skills_dir, "ls-tree", "-r", "--name-only", rev, SKILL_PREFIX).split()
    return {n[len(SKILL_PREFIX) :]: git(skills_dir, "show", f"{rev}:{n}") for n in names}


def vendored_files() -> dict[str, str]:
    return {
        p.relative_to(GUIDE_DIR).as_posix(): p.read_text()
        for p in sorted(GUIDE_DIR.rglob("*"))
        if p.is_file()
    }


def read_pin(path: Path, pattern: re.Pattern[str]) -> str:
    match = pattern.search(path.read_text())
    if match is None:
        sys.exit(f"{path.relative_to(REPO_ROOT)}: no pin found")
    return match.group(1)


def write_pin(path: Path, pattern: re.Pattern[str], pin: str) -> None:
    text = path.read_text()
    path.write_text(pattern.sub(lambda m: m.group(0).replace(m.group(1), pin), text))


def sync(skills_dir: Path, ref: str) -> None:
    pin = git(skills_dir, "rev-parse", "--short=7", ref).strip()
    files = skill_files(skills_dir, pin)
    for name in vendored_files().keys() - files.keys():
        (GUIDE_DIR / name).unlink()
    for name, content in files.items():
        target = GUIDE_DIR / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content)
    write_pin(UPSTREAM_MD, MD_PIN, pin)
    write_pin(AGENT_RS, RS_PIN, pin)
    print(f"synced agent-guide/wright to wrightkit/skills@{pin}")
    for name in files.keys() - set(re.findall(r"agent-guide/wright/([^\"]+)", AGENT_RS.read_text())):
        print(f"warning: {name} is not embedded by agent.rs; add it to FILES", file=sys.stderr)


def check(skills_dir: Path) -> int:
    errors = []
    md_pin, rs_pin = read_pin(UPSTREAM_MD, MD_PIN), read_pin(AGENT_RS, RS_PIN)
    if md_pin != rs_pin:
        errors.append(f"pins disagree: UPSTREAM.md has {md_pin}, agent.rs has {rs_pin}")
    expected, actual = skill_files(skills_dir, md_pin), vendored_files()
    for name in sorted(expected.keys() | actual.keys()):
        if expected.get(name) != actual.get(name):
            errors.append(f"agent-guide/wright/{name} differs from skills@{md_pin}")
    embedded = set(re.findall(r"agent-guide/wright/([^\"]+)", AGENT_RS.read_text()))
    for name in sorted(actual.keys() - embedded):
        errors.append(f"agent-guide/wright/{name} is not embedded by agent.rs")
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if errors:
        print("run scripts/sync-agent-guide.py to re-sync; do not edit the copy", file=sys.stderr)
        return 1
    print(f"agent-guide/wright matches wrightkit/skills@{md_pin}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--skills-dir", type=Path, required=True)
    parser.add_argument("--ref", default="origin/main")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.check:
        return check(args.skills_dir)
    sync(args.skills_dir, args.ref)
    return 0


if __name__ == "__main__":
    sys.exit(main())
