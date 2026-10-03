#!/usr/bin/env python3
"""Validate Wright distribution metadata and release packaging (#108, #338).

Runs in CI and locally. It checks that install.sh covers the declared release
target matrix, verifies the shell syntax of install.sh where Bash is native,
and exercises release packaging on the current host with synthetic binaries.

Usage: python3 scripts/verify-dist.py
"""

import importlib.util
import json
import os
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


def verify_workspace_packages_are_private() -> None:
    metadata = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    public = [
        package["name"]
        for package in json.loads(metadata)["packages"]
        if package.get("publish") != []
    ]
    if public:
        fail(
            "workspace packages must set publish = false; "
            f"public packages: {', '.join(sorted(public))}"
        )
    print("ok: workspace packages explicitly non-publishable")


def load_generator():
    spec = importlib.util.spec_from_file_location(
        "wright_dist", REPO_ROOT / "scripts" / "update-dist-manifests.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def fail(message: str) -> None:
    raise SystemExit(f"dist validation failed: {message}")


def release_promotion_order() -> None:
    """#293: the GitHub Release promotes only after verified R2 staging, and
    wright/latest/version advances only after the canonical release is public.

    Asserts the release workflow's `needs` edges — the mechanism GitHub uses to
    gate promotion — so a failed R2 publish/verify cannot leave a public release,
    and a missing canonical release cannot leave the pointer advanced."""
    jobs_text = (REPO_ROOT / ".github/workflows/release.yml").read_text().split("\njobs:", 1)[1]
    bodies: dict[str, str] = {}
    for match in re.finditer(r"(?m)^  ([a-zA-Z0-9_-]+):\n", jobs_text):
        bodies[match.group(1)] = match.start()
    job_lines: dict[str, list[str]] = {}
    markers = sorted(bodies.items(), key=lambda kv: kv[1])
    for i, (job, start) in enumerate(markers):
        end = markers[i + 1][1] if i + 1 < len(markers) else len(jobs_text)
        job_lines[job] = jobs_text[start:end].splitlines()

    def needs_of(job: str) -> list[str]:
        needs, collecting = [], False
        for line in job_lines[job]:
            head = re.match(r"^    needs:\s*(.*)$", line)
            if head:
                needs.extend(re.findall(r"[a-zA-Z0-9_-]+", head.group(1)))
                collecting = not head.group(1).strip()
                continue
            item = re.match(r"^      -\s*([a-zA-Z0-9_-]+)\s*$", line)
            if collecting and item:
                needs.append(item.group(1))
            elif line.strip():
                collecting = False
        return needs

    for job in ("publish-r2", "publish-release", "advance-latest", "publish-tap"):
        if job not in job_lines:
            fail(f"release.yml is missing the {job} job")
    if "publish-r2" not in needs_of("publish-release"):
        fail("publish-release does not need publish-r2: the GitHub Release can promote without verified R2 staging")
    if "publish-release" in needs_of("publish-r2"):
        fail("publish-r2 still waits on the GitHub Release: R2 staging is not before promotion")
    if "publish-release" not in needs_of("advance-latest"):
        fail("advance-latest does not need publish-release: wright/latest/version can advance before the release is public")
    r2_body = "\n".join(job_lines["publish-r2"])
    if "publish-r2.sh objects" not in r2_body:
        fail("publish-r2 does not run `publish-r2.sh objects`: the immutable upload/verify phase is missing")
    latest_body = "\n".join(job_lines["advance-latest"])
    if "publish-r2.sh pointer" not in latest_body:
        fail("advance-latest does not run `publish-r2.sh pointer`: wright/latest/version is advanced elsewhere")
    for step in re.split(r"(?m)^      - ", r2_body):
        if "publish-r2.sh pointer" in step and "nightly" not in step:
            fail("publish-r2 advances a pointer outside the nightly-gated step: stable's pointer must wait for advance-latest")
    if "publish-release" not in needs_of("publish-tap"):
        fail("publish-tap does not need publish-release: the formula can publish ahead of the canonical release")
    print("ok: release promotion orders R2 staging before the public GitHub Release and the pointer last")


def main() -> None:
    gen = load_generator()
    verify_workspace_packages_are_private()
    release_promotion_order()

    install_sh = (REPO_ROOT / "install.sh").read_text()
    unix_triples = {t for k, t in gen.TARGETS.items() if k != "windows-x64"}
    for triple in sorted(unix_triples):
        if triple not in install_sh:
            fail(f"install.sh does not cover target triple {triple}")
    if gen.TARGETS["windows-x64"] in install_sh:
        fail("install.sh is a Unix-only installer and must not claim the Windows triple")
    print("ok: install.sh covers the declared Unix target matrix")

    if os.name != "nt":
        subprocess.run(["bash", "-n", str(REPO_ROOT / "install.sh")], check=True)
        print("ok: install.sh shell syntax valid")
    else:
        # install.sh is a Unix-only installer (checked above); on Windows the
        # `bash` on PATH is the WSL launcher, which fails without a distro.
        print("skip: install.sh shell syntax check (Unix-only, no real bash on Windows)")

    subprocess.run(
        [sys.executable, str(REPO_ROOT / "scripts" / "test-package-release.py")],
        cwd=REPO_ROOT,
        check=True,
    )
    print("ok: release packaging smoke passed on this host")
    print("dist validation passed")


if __name__ == "__main__":
    main()
