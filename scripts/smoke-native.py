#!/usr/bin/env python3
"""Run Wright's small post-install native runtime smoke contract (#255)."""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import NoReturn


ROOT = Path(__file__).resolve().parent.parent


def fail(message: str) -> NoReturn:
    raise SystemExit(f"native runtime smoke failed: {message}")


def run(label: str, command: list[str], env: dict[str, str] | None = None) -> str:
    print(f"==> native runtime: {label}")
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            env=env,
            check=True,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError as error:
        fail(f"{label}: missing executable {error.filename}")
    except subprocess.CalledProcessError as error:
        output = "\n".join(part for part in (error.stdout, error.stderr) if part)
        if output:
            print(output, file=sys.stderr, end="" if output.endswith("\n") else "\n")
        fail(f"{label}: command exited with status {error.returncode}")
    return result.stdout


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wright", type=Path, required=True)
    parser.add_argument("--wright-lsp", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument(
        "--compile",
        type=Path,
        default=Path("compatibility/fixtures/synthetic/basic-rule/source.opy"),
    )
    parser.add_argument("--check", type=Path, default=Path("scenarios/loops.opy"))
    parser.add_argument(
        "--provider-bootstrap",
        action="store_true",
        help="bootstrap the first-party OPY provider from an empty provider store",
    )
    args = parser.parse_args()

    wright = args.wright.resolve()
    lsp = args.wright_lsp.resolve()
    for name, path in (("wright", wright), ("wright-lsp", lsp)):
        if not path.is_file():
            fail(f"{name} binary is missing: {path}")

    for name, path in (("compile input", args.compile), ("check input", args.check)):
        if not (ROOT / path).is_file():
            fail(f"{name} is missing: {ROOT / path}")

    for name, binary in (("wright version", wright), ("wright-lsp version", lsp)):
        output = run(name, [str(binary), "--version"])
        if args.version not in output:
            fail(f"{name} did not report version {args.version}: {output.strip()}")

    run(
        "compile",
        [str(wright), "compile", str(args.compile), "--profile", "compat"],
    )
    run("check", [str(wright), "check", str(args.check), "--profile", "compat"])

    if args.provider_bootstrap:
        with tempfile.TemporaryDirectory(prefix="wright-provider-smoke-") as store:
            env = os.environ.copy()
            env["WRIGHT_PROVIDER_DATA_DIR"] = store
            run(
                "first-party OPY provider bootstrap from clean state",
                [str(wright), "provider", "update", "opy"],
                env,
            )

    print("native runtime smoke passed")


if __name__ == "__main__":
    main()
