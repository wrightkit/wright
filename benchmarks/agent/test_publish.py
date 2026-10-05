import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from jsonschema import Draft202012Validator, ValidationError

import bench_publish
import bench_score
from test_score import run


class PublishTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=Path(__file__).resolve().parents[2] / "target", prefix="publish-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.schema = json.loads(bench_publish.SCHEMA.read_text())
        Draft202012Validator.check_schema(self.schema)

    def evaluation(self, name="agent-a", sha="a" * 64, model="provider/model", version="pi 1.0.0 (abcdef0)", mode="official", suite_version="v2", skill_sha="b" * 64):
        directory = self.root / name
        directory.mkdir()
        results = []
        for i, usable in enumerate((True, False)):
            result = run(f"private-secret-task-{i}", 1, usable, sha=sha, model=model, isPrivate=True)
            result["contract"] = "wright-agent-bench/v3"
            result["agentInfo"].update(agent="pi", version=version, transcript="DO NOT PUBLISH", workspace=str(Path.home() / "private-workspace"))
            result["environment"]["skills"]["wright-skill"]["sha256"] = skill_sha
            result["environment"]["suite"] = {"version": suite_version, "publicHash": "c" * 64, "privateHash": "d" * 64 if mode == "official" else None,
                                                "hash": "e" * 64, "mode": mode, "scenarios": 2}
            result["environment"]["skills"]["wright-skill"].update(path=str(Path.home() / "skills"), wikiContent="WIKI DO NOT PUBLISH")
            result["checks"] = [{"id": "private-secret-check", "prompt": "PRIVATE PROMPT", "reference": "PRIVATE REFERENCE"}]
            result["private-secret-task"] = {"transcript": "SECRET"}
            trial = directory / str(i)
            trial.mkdir()
            (trial / "result.json").write_text(json.dumps(result))
            results.append(result)
        card = bench_score.card(results, "opy", [r["scenario"] for r in results] + ["private-secret-missing"])
        (directory / "score.json").write_text(json.dumps({"cards": [card]}))
        return directory

    def test_dry_run_cli_writes_schema_valid_safe_bundle(self):
        directory = self.evaluation()
        out = self.root / "bundle"
        done = subprocess.run([sys.executable, str(Path(bench_publish.__file__).with_name("agent_bench.py")),
                               "publish", str(directory), "--out", str(out), "--dry-run"], capture_output=True, text=True)
        self.assertEqual(done.returncode, 0, done.stderr)
        ident = bench_publish.run_id(directory)
        self.assertIn(f"would upload", done.stdout)
        self.assertIn(f"s3://wrightkit-release/bench/runs/{ident}.json", done.stdout)
        latest = json.loads((out / "bench/latest.json").read_text())
        immutable = json.loads((out / f"bench/runs/{ident}.json").read_text())
        self.assertEqual(latest, immutable)
        Draft202012Validator(self.schema).validate(latest)
        track = latest["entries"][0]["tracks"][0]
        self.assertEqual(track["score"], 50)
        self.assertEqual((track["trialsPerScenario"], track["scenarios"], track["validRuns"]), (1, 2, 2))
        self.assertEqual(track["ci95"], [0, 100])
        self.assertIn("missing held-out scenarios", track["provisional"])
        text = json.dumps(latest)
        for forbidden in (str(Path.home()), "transcript", "workspace", "private-secret", "PRIVATE PROMPT", "PRIVATE REFERENCE", "WIKI DO NOT PUBLISH", "checks", "perScenario"):
            self.assertNotIn(forbidden, text)
        setup = latest["entries"][0]["agent"]
        self.assertEqual(setup["program"], "pi")
        self.assertEqual(setup["version"], "pi 1.0.0 (abcdef0)")
        self.assertEqual(setup["model"], "provider/model")
        self.assertEqual(setup["skills"], [{"name": "wright-skill", "sha256": "b" * 64}])

    def test_schema_rejects_private_fields_and_host_paths(self):
        bundle, _ = bench_publish.build([self.evaluation()])
        for field in ("transcript", "workspace", "private-secret-task"):
            with self.subTest(field=field):
                bad = copy.deepcopy(bundle)
                bad["entries"][0]["agent"][field] = "secret"
                with self.assertRaises(ValidationError):
                    Draft202012Validator(self.schema).validate(bad)
        for path in (str(Path.home() / "private"), "/other/host/path", "~/private", "C:\\Users\\private", "pi (/Users/secret/bin)"):
            with self.subTest(path=path):
                bad = copy.deepcopy(bundle)
                bad["entries"][0]["agent"]["version"] = path
                with self.assertRaises(ValidationError):
                    Draft202012Validator(self.schema).validate(bad)

    def test_publish_refuses_unsafe_agent_metadata_without_echoing_it(self):
        directory = self.evaluation(version=str(Path.home() / "secret"))
        done = subprocess.run([sys.executable, str(Path(bench_publish.__file__).with_name("agent_bench.py")),
                               "publish", str(directory), "--out", str(self.root / "bundle"), "--dry-run"], capture_output=True, text=True)
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("schema", done.stderr)
        self.assertNotIn("Traceback", done.stderr)
        self.assertNotIn(str(Path.home()), done.stderr)
        self.assertFalse((self.root / "bundle").exists())

    def test_mixed_environments_use_largest_group_and_archive_all_runs(self):
        a, b, c = self.evaluation("a"), self.evaluation("b", model="other/model"), self.evaluation("c", sha="f" * 64)
        latest, runs = bench_publish.build([a, b, c])
        self.assertEqual({e["id"] for e in latest["entries"]}, {bench_publish.run_id(a), bench_publish.run_id(b)})
        self.assertEqual(latest["excluded"], [{"id": bench_publish.run_id(c), "reason": "different environment"}])
        self.assertEqual(len(runs), 3)
        for label, options in (("skills", {"skill_sha": "f" * 64}), ("suite", {"suite_version": "v3"})):
            with self.subTest(changed=label):
                other = self.evaluation(label, **options)
                latest, _ = bench_publish.build([a, b, other])
                self.assertEqual(latest["excluded"], [{"id": bench_publish.run_id(other), "reason": "different environment"}])

    def test_stale_cards_missing_program_and_disagreeing_tracks_are_refused(self):
        directory = self.evaluation()
        saved = (directory / "score.json").read_text()
        altered = json.loads(saved)
        altered["cards"][0]["identity"]["wrightSha256"] = "f" * 64
        (directory / "score.json").write_text(json.dumps(altered))
        with self.assertRaisesRegex(ValueError, "stale"):
            bench_publish.build([directory])
        original = json.loads(saved)["cards"][0]
        (directory / "score.json").write_text(json.dumps({"cards": [original, original]}))
        with self.assertRaisesRegex(ValueError, "duplicate language tracks"):
            bench_publish.build([directory])
        (directory / "score.json").write_text(saved)
        track = copy.deepcopy(original)
        track["language"] = "workshop"
        track["identity"]["wrightSha256"] = "f" * 64
        (directory / "score.json").write_text(json.dumps({"cards": [json.loads(saved)["cards"][0], track]}))
        with self.assertRaisesRegex(ValueError, "different environments"):
            bench_publish.build([directory])
        (directory / "score.json").write_text(saved)
        for path in directory.rglob("result.json"):
            data = json.loads(path.read_text())
            data["agentInfo"].pop("agent")
            path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, "program name"):
            bench_publish.build([directory])

    def test_both_language_tracks_are_preserved(self):
        directory = self.evaluation()
        cards = json.loads((directory / "score.json").read_text())["cards"]
        workshop = []
        for path in list(directory.rglob("result.json")):
            data = json.loads(path.read_text())
            data["language"] = "workshop"
            trial = directory / ("workshop-" + path.parent.name)
            trial.mkdir()
            (trial / "result.json").write_text(json.dumps(data))
            workshop.append(data)
        cards.append(bench_score.card(workshop, "workshop", [r["scenario"] for r in workshop]))
        (directory / "score.json").write_text(json.dumps({"cards": cards}))
        latest, _ = bench_publish.build([directory])
        self.assertEqual([t["language"] for t in latest["entries"][0]["tracks"]], ["opy", "workshop"])

    def test_public_sample_is_explicit_and_cannot_claim_official(self):
        latest, _ = bench_publish.build([self.evaluation(mode="public sample")])
        suite = latest["entries"][0]["environment"]["suite"]
        self.assertEqual(suite["mode"], "public sample")
        suite["mode"] = "official"
        with self.assertRaises(ValidationError):
            Draft202012Validator(self.schema).validate(latest)

    def test_local_run_id_cannot_be_overwritten(self):
        directory = self.evaluation()
        out = self.root / "bundle"
        bench_publish.main([directory], out, True, None)
        bench_publish.main([directory], out, True, None)
        before = (out / f"bench/runs/{bench_publish.run_id(directory)}.json").read_bytes()
        cards = json.loads((directory / "score.json").read_text())
        cards["cards"][0]["score"] = 99
        (directory / "score.json").write_text(json.dumps(cards))
        with self.assertRaisesRegex(ValueError, "immutable local bundle"):
            bench_publish.main([directory], out, True, None)
        self.assertEqual((out / f"bench/runs/{bench_publish.run_id(directory)}.json").read_bytes(), before)

    def test_conditional_upload_is_idempotent_and_refuses_different_remote_content(self):
        directory = self.evaluation()
        out = self.root / "bundle"
        bench_publish.main([directory], out, True, None)
        ident = bench_publish.run_id(directory)
        content = (out / f"bench/runs/{ident}.json").read_bytes()
        conflict = subprocess.CompletedProcess([], 1, "", "PreconditionFailed")
        success = subprocess.CompletedProcess([], 0, "", "")
        with patch("bench_publish.subprocess.run", side_effect=[conflict, subprocess.CompletedProcess([], 0, content, b""), success]) as execute:
            bench_publish.upload(out, [ident], "https://example.r2.cloudflarestorage.com")
            self.assertIn("--if-none-match", execute.call_args_list[0].args[0])
            self.assertIn("bench/latest.json", execute.call_args_list[-1].args[0])
        with patch("bench_publish.subprocess.run", side_effect=[conflict, subprocess.CompletedProcess([], 0, b"different", b"")]) as execute:
            with self.assertRaisesRegex(ValueError, "immutable run"):
                bench_publish.upload(out, [ident], "https://example.r2.cloudflarestorage.com")
            self.assertFalse(any("bench/latest.json" in c.args[0] for c in execute.call_args_list))
        with patch("bench_publish.subprocess.run", return_value=subprocess.CompletedProcess([], 1, "", "AccessDenied")) as execute:
            with self.assertRaisesRegex(ValueError, "upload failed"):
                bench_publish.upload(out, [ident], "https://example.r2.cloudflarestorage.com")
            self.assertEqual(execute.call_count, 1)


if __name__ == "__main__":
    unittest.main()
