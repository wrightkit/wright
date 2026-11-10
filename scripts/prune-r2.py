#!/usr/bin/env python3
"""Delete R2 release prefixes beyond the retention window.

Stable keeps the newest versions under wright/releases/<version>/ by semver.
Nightly keeps the newest commits under wright/nightly/<commit>/ by upload time.
Keys outside those prefixes, such as pointers and installer scripts, are never
selected.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import tempfile
from datetime import datetime

STABLE_KEY = re.compile(r"^wright/releases/(\d+)\.(\d+)\.(\d+)/")
NIGHTLY_KEY = re.compile(r"^wright/nightly/([0-9a-f]{7,40})/")
PREFIX = {"stable": "wright/releases/", "nightly": "wright/nightly/"}


def plan(objects: list[dict], channel: str, keep: int) -> list[str]:
    """Return the keys to delete. Each object has `Key` and `LastModified`."""
    groups: dict[object, list[str]] = {}
    newest: dict[object, str] = {}
    for obj in objects:
        key = obj["Key"]
        if channel == "stable":
            match = STABLE_KEY.match(key)
            group = tuple(int(part) for part in match.groups()) if match else None
        else:
            match = NIGHTLY_KEY.match(key)
            group = match.group(1) if match else None
        if group is None:
            continue
        groups.setdefault(group, []).append(key)
        newest[group] = max(newest.get(group, ""), obj["LastModified"])
    if channel == "stable":
        order = sorted(groups, reverse=True)
    else:
        order = sorted(groups, key=lambda g: (newest[g], g), reverse=True)
    return [key for group in order[keep:] for key in groups[group]]


def aws(*args: str) -> str:
    endpoint = os.environ["R2_ENDPOINT"]
    result = subprocess.run(
        ["aws", "s3api", *args, "--endpoint-url", endpoint],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout


def list_objects(bucket: str, prefix: str) -> list[dict]:
    out = aws("list-objects-v2", "--bucket", bucket, "--prefix", prefix, "--output", "json")
    return json.loads(out or "{}").get("Contents") or []


def delete_keys(bucket: str, keys: list[str]) -> None:
    for start in range(0, len(keys), 1000):
        batch = [{"Key": key} for key in keys[start : start + 1000]]
        with tempfile.NamedTemporaryFile("w", suffix=".json") as handle:
            json.dump({"Objects": batch, "Quiet": True}, handle)
            handle.flush()
            out = aws("delete-objects", "--bucket", bucket, "--delete", f"file://{handle.name}")
        if out.strip() and json.loads(out).get("Errors"):
            raise SystemExit(f"error: R2 refused to delete some keys: {out}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--channel", choices=sorted(PREFIX), required=True)
    parser.add_argument("--keep", type=int, required=True)
    args = parser.parse_args()
    if args.keep < 1:
        raise SystemExit("error: --keep must be at least 1")
    bucket = os.environ["R2_BUCKET"]
    keys = plan(list_objects(bucket, PREFIX[args.channel]), args.channel, args.keep)
    print(f"pruning {len(keys)} {args.channel} objects, keeping the newest {args.keep}")
    delete_keys(bucket, keys)


if __name__ == "__main__":
    main()
