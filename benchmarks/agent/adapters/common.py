"""Helpers shared by the adapters."""

from __future__ import annotations

import subprocess


def cli_version(binary: str, env: dict | None = None) -> str | None:
    """First line of `<binary> --version`, or None when the CLI does not answer."""
    try:
        done = subprocess.run([binary, "--version"], capture_output=True, text=True, timeout=30, env=env)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return (done.stdout or done.stderr).strip().splitlines()[0] if (done.stdout or done.stderr).strip() else None
