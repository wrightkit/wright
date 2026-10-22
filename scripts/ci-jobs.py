import argparse
import json
import os
from pathlib import Path
import re

import yaml


WORKFLOW = Path(__file__).resolve().parents[1] / ".github/workflows/ci.yml"
PATH_CONDITION = re.compile(r"needs\.paths\.outputs\.([a-z_]+) == 'true'")


def required_jobs(workflow, paths):
    jobs = workflow["jobs"]
    required = {}
    for name, job in jobs.items():
        if name == "ci-result" or job.get("continue-on-error") is True:
            continue
        condition = job.get("if")
        if name == "paths" and condition is None:
            required[name] = True
            continue
        if not isinstance(condition, str):
            raise ValueError(f"{name}: expected a path condition, got {condition!r}")
        terms = condition.removeprefix("${{").removesuffix("}}").strip().split(" || ")
        filters = [PATH_CONDITION.fullmatch(term) for term in terms]
        if not all(filters):
            raise ValueError(f"{name}: unsupported path condition {condition!r}")
        required[name] = any(paths.get(term[1]) == "true" for term in filters)
    dependencies = jobs["ci-result"]["needs"]
    if not isinstance(dependencies, list) or len(dependencies) != len(set(dependencies)):
        raise ValueError("ci-result.needs must list each required job once")
    if set(dependencies) != set(required):
        raise ValueError(f"ci-result.needs must contain exactly {sorted(required)}")
    return required


def check_results(required, needs):
    failures = []
    for name, selected in required.items():
        result = needs.get(name, {}).get("result", "missing")
        if result in ("failure", "cancelled", "missing") or (selected and result != "success"):
            failures.append(f"{name}: {result} (required={selected})")
    if failures:
        raise ValueError("CI jobs did not succeed: " + "; ".join(failures))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("check", "result"))
    args = parser.parse_args()
    workflow = yaml.safe_load(WORKFLOW.read_text())
    required = required_jobs(workflow, json.loads(os.environ.get("PATHS", "{}")))
    if args.mode == "result":
        check_results(required, json.loads(os.environ["NEEDS"]))
    print(f"CI job coupling: {args.mode} passed")


if __name__ == "__main__":
    main()
