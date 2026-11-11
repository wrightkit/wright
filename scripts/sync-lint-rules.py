#!/usr/bin/env python3
"""Sync or verify the embedded first-party lint rules.

`lint-rules/rules/` is a verbatim copy of `rules/` from the wrightkit/lint-rules
repository, embedded in the binary by `crates/wright-analyzer/src/first_party.rs`
as the default first-party lint package. The full commit it was taken from is
recorded in `lint-rules/UPSTREAM.md`.

Usage:
  python3 scripts/sync-lint-rules.py --lint-rules-dir DIR [--ref REF]
      Replace the embedded copy with `rules/` at REF (default origin/main) of
      the lint-rules clone in DIR and rewrite the recorded commit.
  python3 scripts/sync-lint-rules.py --lint-rules-dir DIR --check
      Verify the copy equals `rules/` at the recorded commit and every rule
      file is embedded by first_party.rs. DIR needs full history so the
      recorded commit is reachable.
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
RULES_DIR = REPO_ROOT / "lint-rules" / "rules"
UPSTREAM_MD = REPO_ROOT / "lint-rules" / "UPSTREAM.md"
FIRST_PARTY_RS = REPO_ROOT / "crates" / "wright-analyzer" / "src" / "first_party.rs"
RULES_PREFIX = "rules/"

PIN = re.compile(r"at commit\s+`([0-9a-f]{40})`")
EMBEDDED = re.compile(r"lint-rules/rules/([^\"]+)")


def git(clone: Path, *args: str) -> str:
    return subprocess.run(
        ["git", "-C", str(clone), *args],
        check=True,
        capture_output=True,
        text=True,
    ).stdout


def upstream_files(clone: Path, rev: str) -> dict[str, str]:
    names = git(clone, "ls-tree", "--name-only", rev, RULES_PREFIX).split()
    return {
        n[len(RULES_PREFIX) :]: git(clone, "show", f"{rev}:{n}")
        for n in names
        if n.endswith((".yaml", ".yml"))
    }


def embedded_copy() -> dict[str, str]:
    return {p.name: p.read_text() for p in sorted(RULES_DIR.iterdir()) if p.is_file()}


def read_pin() -> str:
    match = PIN.search(UPSTREAM_MD.read_text())
    if match is None:
        sys.exit("lint-rules/UPSTREAM.md: no commit pin found")
    return match.group(1)


def sync(clone: Path, ref: str) -> None:
    pin = git(clone, "rev-parse", f"{ref}^{{commit}}").strip()
    files = upstream_files(clone, pin)
    for name in embedded_copy().keys() - files.keys():
        (RULES_DIR / name).unlink()
    for name, content in files.items():
        (RULES_DIR / name).write_text(content)
    text = UPSTREAM_MD.read_text()
    UPSTREAM_MD.write_text(PIN.sub(lambda m: m.group(0).replace(m.group(1), pin), text))
    print(f"synced lint-rules/rules to wrightkit/lint-rules@{pin}")
    embedded = set(EMBEDDED.findall(FIRST_PARTY_RS.read_text()))
    for name in sorted(files.keys() ^ embedded):
        print(f"warning: update RULES in first_party.rs for {name}", file=sys.stderr)


def check(clone: Path) -> int:
    errors = []
    pin = read_pin()
    expected, actual = upstream_files(clone, pin), embedded_copy()
    if not expected:
        errors.append(f"wrightkit/lint-rules@{pin} has no rule files under rules/")
    for name in sorted(expected.keys() | actual.keys()):
        if expected.get(name) != actual.get(name):
            errors.append(f"lint-rules/rules/{name} differs from wrightkit/lint-rules@{pin}")
    embedded = set(EMBEDDED.findall(FIRST_PARTY_RS.read_text()))
    for name in sorted(actual.keys() ^ embedded):
        errors.append(f"lint-rules/rules/{name}: the copy and first_party.rs RULES disagree")
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if errors:
        print("run scripts/sync-lint-rules.py to re-sync; do not edit the copy", file=sys.stderr)
        return 1
    print(f"lint-rules/rules matches wrightkit/lint-rules@{pin}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--lint-rules-dir", type=Path, required=True)
    parser.add_argument("--ref", default="origin/main")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.check:
        return check(args.lint_rules_dir)
    sync(args.lint_rules_dir, args.ref)
    return 0


if __name__ == "__main__":
    sys.exit(main())
