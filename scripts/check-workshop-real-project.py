#!/usr/bin/env python3
"""Run one owner-pinned Workshop project through Wright's public CLI."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path
from typing import Any


EXPECTED_CONTRACT = "wright-result/v1"
EXPECTED_EXIT_CODES = {0, 1, 3, 4}


def fail(message: str) -> None:
    raise SystemExit(message)


def run_workflow(binary: Path, command: str, project: Path) -> dict[str, Any]:
    process = subprocess.run(
        [
            str(binary),
            command,
            "--kind",
            "workshop",
            str(project),
            "--format",
            "json",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if "panicked at" in process.stderr or "internal failure" in process.stderr.lower():
        fail(f"wright {command} crashed for {project}:\n{process.stderr}")
    try:
        result = json.loads(process.stdout)
    except json.JSONDecodeError as error:
        fail(
            f"wright {command} did not emit JSON for {project}: {error}\n"
            f"stdout: {process.stdout}\nstderr: {process.stderr}"
        )

    if not isinstance(result, dict):
        fail(f"wright {command} result is not an object for {project}: {result!r}")
    metadata = result.get("wright")
    if not isinstance(metadata, dict) or metadata.get("contract") != EXPECTED_CONTRACT:
        fail(f"wright {command} returned an unknown result contract for {project}: {result!r}")
    if result.get("command") != command:
        fail(f"wright returned the wrong command for {project}: {result!r}")
    if not isinstance(result.get("diagnostics"), list):
        fail(f"wright {command} diagnostics are not an array for {project}: {result!r}")
    if not isinstance(result.get("ok"), bool):
        fail(f"wright {command} ok is not boolean for {project}: {result!r}")
    if result.get("exit") != process.returncode:
        fail(
            f"wright {command} exit mismatch for {project}: "
            f"process={process.returncode}, envelope={result.get('exit')}"
        )
    if process.returncode not in EXPECTED_EXIT_CODES:
        fail(f"wright {command} used an unexpected exit code for {project}: {process.returncode}")
    if any(
        diagnostic.get("stage") == "internal"
        for diagnostic in result["diagnostics"]
        if isinstance(diagnostic, dict)
    ):
        fail(f"wright {command} reported an internal diagnostic for {project}: {result!r}")
    for diagnostic in result["diagnostics"]:
        if not isinstance(diagnostic, dict):
            fail(f"wright {command} returned a malformed diagnostic for {project}: {result!r}")
        span = diagnostic.get("span")
        if isinstance(span, dict) and span.get("path") != str(project):
            fail(f"wright {command} returned the wrong diagnostic path for {project}: {result!r}")
    if result["ok"] != (process.returncode == 0):
        fail(f"wright {command} ok does not match its exit status for {project}: {result!r}")
    return result


def main() -> int:
    if len(sys.argv) != 3:
        fail(f"usage: {Path(sys.argv[0]).name} WRIGHT_BINARY PROJECT")
    binary = Path(sys.argv[1])
    project = Path(sys.argv[2])
    if not binary.is_file() or not binary.stat().st_mode & 0o111:
        fail(f"Wright CLI artifact is not executable: {binary}")
    if not project.is_file():
        fail(f"Workshop project is missing: {project}")

    check = run_workflow(binary, "check", project)
    lint = run_workflow(binary, "lint", project)
    if lint["ok"] != check["ok"] or lint["exit"] != check["exit"]:
        fail(f"check and lint disagree on the project verdict: {project}")
    if lint["diagnostics"] != check["diagnostics"]:
        fail(f"check and lint disagree on diagnostics: {project}")
    program_result = lint.get("result")
    program = program_result.get("program", {}) if isinstance(program_result, dict) else {}
    if not isinstance(program, dict) or not isinstance(program.get("rules"), int):
        fail(f"lint did not expose the Workshop program summary: {project}")
    if program["rules"] <= 0:
        fail(f"lint did not reach any Workshop rules: {project}")

    print(f"{project}: public check/lint passed (exit {check['exit']})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
