"""Helpers shared by the adapters."""

from __future__ import annotations

import subprocess

INFRA_EXIT = 75  # EX_TEMPFAIL: a provider or infrastructure failure, not an agent failure; the harness retries the trial later
# Error text that means the provider, not the agent, failed. Shared so the classification cannot drift between adapters.
TRANSIENT = ("rate limit", "overloaded", "429", "502", "503", "529", "bad gateway", "timed out", "timeout", "temporarily",
             "usage limit", "quota", "credits", "fetch failed", "websocket error", "connection error", "econnreset", "unavailable")


def as_dict(value) -> dict:
    """value when it is a dict, else {} — `or {}` alone does not guard a truthy non-dict from a malformed stream line."""
    return value if isinstance(value, dict) else {}


def cli_version(binary: str, env: dict | None = None) -> str | None:
    """First line of `<binary> --version`, or None when the CLI does not answer."""
    try:
        done = subprocess.run([binary, "--version"], capture_output=True, text=True, timeout=30, env=env)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return (done.stdout or done.stderr).strip().splitlines()[0] if (done.stdout or done.stderr).strip() else None


EFFORTS = ("low", "medium", "high", "xhigh", "max")


def split_effort(model_id: str) -> tuple[str, str | None]:
    """Some CLIs name the effort in the model id (`swe-2-max`, `gemini-3.8-flash-high`) and never report it separately."""
    for effort in EFFORTS:
        if model_id.endswith(f"-{effort}"):
            return model_id[: -len(effort) - 1], effort
    return model_id, None
