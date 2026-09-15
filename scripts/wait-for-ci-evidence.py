#!/usr/bin/env python3
"""Wait for a successful push CI run at an exact commit."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import time


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--attempts", type=int, default=60)
    parser.add_argument("--interval", type=int, default=30)
    args = parser.parse_args()

    for attempt in range(1, args.attempts + 1):
        print(f"Polling exact-SHA CI evidence (attempt {attempt}/{args.attempts})")
        runs = json.loads(
            subprocess.check_output(
                [
                    "gh",
                    "api",
                    f"repos/{os.environ['GITHUB_REPOSITORY']}/actions/runs?head_sha={args.commit}&event=push&per_page=100",
                ],
                text=True,
            )
        )
        candidates = [
            run
            for run in runs.get("workflow_runs", [])
            if run.get("name") == "CI"
            and run.get("event") == "push"
            and run.get("head_sha") == args.commit
        ]
        if candidates:
            run = sorted(candidates, key=lambda item: item.get("created_at", ""))[-1]
            status = run.get("status")
            conclusion = run.get("conclusion") or ""
            run_id = run.get("id")
            print(f"CI run {run_id}: {status}/{conclusion}")
            if status == "completed":
                if conclusion != "success":
                    raise SystemExit(f"CI run {run_id} did not provide successful release evidence")
                print(f"Reusing successful CI run {run_id} for {args.commit}")
                return 0
        else:
            print(f"No push CI run found yet for {args.commit}")
        if attempt < args.attempts:
            time.sleep(args.interval)

    raise SystemExit(f"Timed out waiting for CI evidence for {args.commit}")


if __name__ == "__main__":
    raise SystemExit(main())
