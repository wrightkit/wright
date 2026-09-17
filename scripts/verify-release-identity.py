#!/usr/bin/env python3
"""Verify release revision and workspace version identity."""

from __future__ import annotations

import argparse
import json
import os
import subprocess


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--ref", required=True)
    parser.add_argument(
        "--verify-tag",
        action="store_true",
        help="also verify that the remote tag points to the requested commit",
    )
    args = parser.parse_args()

    head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if head != args.commit or args.ref != args.commit:
        raise SystemExit("release checkout does not match the requested commit")

    if args.verify_tag:
        repository = os.environ["GITHUB_REPOSITORY"]
        tag_commit = subprocess.check_output(
            ["gh", "api", f"repos/{repository}/commits/{args.tag}", "--jq", ".sha"],
            text=True,
        ).strip()
        if tag_commit != args.commit:
            raise SystemExit(f"tag {args.tag} points to {tag_commit}, expected {args.commit}")

    version = args.tag.removeprefix("v")
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
            text=True,
        )
    )
    package_version = next(
        package["version"] for package in metadata["packages"] if package["name"] == "wright-cli"
    )
    if version != package_version:
        raise SystemExit(
            f"tag {args.tag} does not match the workspace implementation version {package_version}"
        )
    print(f"workspace version {package_version} matches tag {args.tag}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
