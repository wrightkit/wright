#!/usr/bin/env python3
"""Validate Wright's single released or candidate workshop-rs dependency contract."""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
CANDIDATE_SOURCE = re.compile(
    r"^git\+https://github\.com/wrightkit/workshop-rs\.git\?rev=([0-9a-f]{40})#([0-9a-f]{40})$"
)


def is_pinned_git_candidate(source: str | None) -> bool:
    if source is None:
        return False
    match = CANDIDATE_SOURCE.fullmatch(source)
    return match is not None and match.group(1) == match.group(2)


def metadata() -> dict:
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise SystemExit(
            "workshop dependency validation failed: cargo metadata failed\n"
            + result.stderr
        )
    return json.loads(result.stdout)


def main() -> int:
    data = metadata()
    packages_by_id = {package["id"]: package for package in data["packages"]}
    workspace_packages = [
        packages_by_id[package_id] for package_id in data["workspace_members"]
    ]
    workshop_packages = [
        package for package in data["packages"] if package["name"] == "workshop-rs"
    ]

    if len(workshop_packages) != 1:
        versions = ", ".join(
            f"{package['version']} ({package['source'] or 'unpublished'})"
            for package in workshop_packages
        )
        raise SystemExit(
            "workshop dependency validation failed: expected exactly one resolved "
            f"workshop-rs package, found {len(workshop_packages)}: {versions or 'none'}"
        )

    workshop = workshop_packages[0]
    source = workshop.get("source")
    is_registry = bool(source and source.startswith("registry+"))
    is_pinned_git = is_pinned_git_candidate(source)

    if not (is_registry or is_pinned_git):
        raise SystemExit(
            "workshop dependency validation failed: workshop-rs must come from a "
            f"released registry or pinned git candidate, got {source or 'unpublished'}"
        )

    direct = []
    for package in workspace_packages:
        for dependency in package["dependencies"]:
            if dependency["name"] == "workshop-rs":
                direct.append((package["name"], dependency))

    if not direct:
        raise SystemExit(
            "workshop dependency validation failed: no workspace package directly "
            "consumes workshop-rs"
        )

    aliases = [
        f"{package}: {dependency['rename']}"
        for package, dependency in direct
        if dependency.get("rename")
    ]
    if aliases:
        raise SystemExit(
            "workshop dependency validation failed: renamed workshop-rs "
            f"dependencies are not allowed ({', '.join(aliases)})"
        )

    requirements = {dependency["req"] for _, dependency in direct}
    if is_registry:
        if len(requirements) != 1 or not re.fullmatch(
            r"\^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", next(iter(requirements), "")
        ):
            consumers = ", ".join(
                f"{package} ({dependency['req']})" for package, dependency in direct
            )
            raise SystemExit(
                "workshop dependency validation failed: direct consumers must use "
                f"one ordinary compatible SemVer requirement, found {consumers}"
            )
    else:
        if requirements != {"*"}:
            consumers = ", ".join(
                f"{package} ({dependency['req']})" for package, dependency in direct
            )
            raise SystemExit(
                "workshop dependency validation failed: git candidate direct consumers "
                f"must use '*', found {consumers}"
            )

    consumers = ", ".join(sorted(package for package, _ in direct))
    print(
        f"workshop-rs contract: {workshop['version']} from {source} "
        f"({next(iter(requirements))}; {len(direct)} direct consumers: {consumers})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
