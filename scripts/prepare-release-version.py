#!/usr/bin/env python3
"""Select and synchronize the version for an explicit stable release."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SEMVER = re.compile(
    r"^(0|[1-9][0-9]*)\."
    r"(0|[1-9][0-9]*)\."
    r"(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$"
)


@dataclass(frozen=True)
class Version:
    major: int
    minor: int
    patch: int
    prerelease: tuple[str, ...] = ()
    build: tuple[str, ...] = ()

    @classmethod
    def parse(cls, value: str) -> "Version":
        match = SEMVER.fullmatch(value)
        if match is None:
            raise ValueError(f"invalid SemVer: {value!r}")
        prerelease = tuple((match.group(4) or "").split(".")) if match.group(4) else ()
        build = tuple((match.group(5) or "").split(".")) if match.group(5) else ()
        for identifier in prerelease:
            if identifier.isdigit() and len(identifier) > 1 and identifier.startswith("0"):
                raise ValueError(f"invalid SemVer numeric prerelease identifier: {value!r}")
        return cls(
            int(match.group(1)),
            int(match.group(2)),
            int(match.group(3)),
            prerelease,
            build,
        )

    def __str__(self) -> str:
        value = f"{self.major}.{self.minor}.{self.patch}"
        if self.prerelease:
            value += "-" + ".".join(self.prerelease)
        if self.build:
            value += "+" + ".".join(self.build)
        return value


def compare(left: Version, right: Version) -> int:
    core = (left.major, left.minor, left.patch)
    other_core = (right.major, right.minor, right.patch)
    if core != other_core:
        return (core > other_core) - (core < other_core)
    if not left.prerelease or not right.prerelease:
        return (not left.prerelease) - (not right.prerelease)
    for left_id, right_id in zip(left.prerelease, right.prerelease):
        if left_id == right_id:
            continue
        left_numeric = left_id.isdigit()
        right_numeric = right_id.isdigit()
        if left_numeric and right_numeric:
            return (int(left_id) > int(right_id)) - (int(left_id) < int(right_id))
        if left_numeric != right_numeric:
            return -1 if left_numeric else 1
        return (left_id > right_id) - (left_id < right_id)
    return (len(left.prerelease) > len(right.prerelease)) - (
        len(left.prerelease) < len(right.prerelease)
    )


def workspace_version(root: Path) -> str:
    cargo = (root / "Cargo.toml").read_text()
    section = re.search(
        r"(?ms)^\[workspace\.package\]\s*\n(.*?)(?=^\[|\Z)", cargo
    )
    if section is None:
        raise SystemExit("Cargo.toml is missing [workspace.package]")
    version = re.search(r'(?m)^version\s*=\s*"([^"]+)"\s*$', section.group(1))
    if version is None:
        raise SystemExit("Cargo.toml is missing [workspace.package].version")
    Version.parse(version.group(1))
    return version.group(1)


def validate_current_state(root: Path) -> str:
    current = workspace_version(root)
    stamp = (root / "version.txt").read_text().strip()
    if stamp != current:
        raise SystemExit(
            f"version.txt {stamp!r} does not match workspace version {current!r}"
        )
    return current


def select_version(current_text: str, requested: str | None) -> str:
    current = Version.parse(current_text)
    if requested is None:
        return str(Version(current.major, current.minor, current.patch + 1))
    candidate = Version.parse(requested)
    if compare(candidate, current) <= 0:
        raise ValueError(
            f"stable release version {requested!r} must be newer than {current_text!r}"
        )
    return str(candidate)


def synchronize_version(root: Path, version: str) -> None:
    current = validate_current_state(root)
    selected = select_version(current, version)
    cargo_path = root / "Cargo.toml"
    cargo = cargo_path.read_text()
    section = re.search(
        r"(?ms)^(\[workspace\.package\]\s*\n)(.*?)(?=^\[|\Z)", cargo
    )
    if section is None:
        raise SystemExit("Cargo.toml is missing [workspace.package]")
    body, count = re.subn(
        r'(?m)^(version\s*=\s*")[^"]+("\s*)$',
        rf"\g<1>{selected}\g<2>",
        section.group(2),
        count=1,
    )
    if count != 1:
        raise SystemExit("Cargo.toml workspace version could not be updated")
    cargo_path.write_text(cargo[: section.start(2)] + body + cargo[section.end(2) :])
    (root / "version.txt").write_text(selected + "\n")
    subprocess.run(
        [
            sys.executable,
            str(root / "scripts" / "update-dist-manifests.py"),
            "--version",
            selected,
        ],
        cwd=root,
        check=True,
        stdout=subprocess.DEVNULL,
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--version", help="explicit newer SemVer; omit for next patch")
    args = parser.parse_args()
    root = args.root.resolve()
    current = validate_current_state(root)
    try:
        selected = select_version(current, args.version)
        synchronize_version(root, selected)
    except ValueError as error:
        raise SystemExit(str(error)) from error
    print(selected)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
