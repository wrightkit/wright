#!/usr/bin/env python3
"""Verify that the release-please GitHub Release is still a draft."""

from __future__ import annotations

import argparse
import json
import os
import subprocess


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    release = json.loads(
        subprocess.check_output(
            [
                "gh",
                "release",
                "view",
                args.tag,
                "--repo",
                os.environ["GITHUB_REPOSITORY"],
                "--json",
                "tagName,isDraft,isImmutable,url",
            ],
            text=True,
        )
    )
    print(json.dumps(release, indent=2))
    if release.get("tagName") != args.tag or release.get("isDraft") is not True:
        raise SystemExit("release-please Release is not the expected draft")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
