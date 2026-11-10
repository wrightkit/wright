#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("prune_r2", Path(__file__).with_name("prune-r2.py"))
prune = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prune)


def obj(key: str, modified: str = "2026-01-01T00:00:00Z") -> dict:
    return {"Key": key, "LastModified": modified}


def release(version: str) -> list[dict]:
    return [
        obj(f"wright/releases/{version}/wright-{version}-x86_64-unknown-linux-gnu.tar.gz"),
        obj(f"wright/releases/{version}/wright-{version}-x86_64-unknown-linux-gnu.tar.gz.sha256"),
    ]


class StablePlan(unittest.TestCase):
    def test_keeps_newest_by_semver_not_lexical_order(self) -> None:
        objects = [o for v in ("0.9.0", "0.10.0", "0.41.7", "0.41.10", "0.2.0") for o in release(v)]
        deleted = prune.plan(objects, "stable", 3)
        self.assertEqual({k.split("/")[2] for k in deleted}, {"0.9.0", "0.2.0"})
        self.assertEqual(len(deleted), 4)

    def test_never_selects_pointers_scripts_or_other_keys(self) -> None:
        objects = release("1.0.0") + release("1.1.0") + [
            obj("wright/latest/version"),
            obj("wright/install.sh"),
            obj("wright/install.ps1"),
            obj("wright/nightly/version"),
            obj("other/releases/0.0.1/x"),
        ]
        self.assertEqual(prune.plan(objects, "stable", 1), [o["Key"] for o in release("1.0.0")])

    def test_nothing_deleted_within_window(self) -> None:
        objects = release("1.0.0") + release("1.1.0")
        self.assertEqual(prune.plan(objects, "stable", 2), [])


class NightlyPlan(unittest.TestCase):
    def test_keeps_newest_by_upload_time(self) -> None:
        commits = ["a" * 12, "b" * 12, "c" * 12, "d" * 12]
        objects = []
        for day, commit in enumerate(commits, start=1):
            for name in ("wright-x.tar.gz", "wright-x.tar.gz.sha256"):
                objects.append(obj(f"wright/nightly/{commit}/{name}", f"2026-01-0{day}T00:00:00Z"))
        deleted = prune.plan(objects, "nightly", 2)
        self.assertEqual({k.split("/")[2] for k in deleted}, {"a" * 12, "b" * 12})

    def test_never_selects_pointer(self) -> None:
        objects = [obj("wright/nightly/version"), obj(f"wright/nightly/{'a' * 12}/x"), obj(f"wright/nightly/{'b' * 12}/x", "2026-02-01T00:00:00Z")]
        self.assertEqual(prune.plan(objects, "nightly", 1), [f"wright/nightly/{'a' * 12}/x"])


if __name__ == "__main__":
    unittest.main()
