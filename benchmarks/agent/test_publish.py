import contextlib
import io
import json
import shutil
import tempfile
import unittest
from pathlib import Path

import bench_publish
import bench_score
from test_score import SCENARIOS, run, runs

ROOT = Path(__file__).resolve().parents[2]

SUITE = {"version": "v2", "publicHash": "c" * 64, "privateHash": "d" * 64,
         "hash": "e" * 64, "mode": "official", "scenarios": 8}
AGENT_INFO = {"agent": "codex", "version": "codex 1.0", "model": "m", "effort": "high"}


def results(usable=set(SCENARIOS), sha="a" * 64, language="opy", agentInfo=None, **kw):
    data = runs(usable, language=language, sha=sha, agentInfo=dict(agentInfo or AGENT_INFO), **kw)
    for r in data:
        r["environment"]["suite"] = dict(SUITE)
    return data


class PublishTest(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(dir=ROOT / "target"))
        self.addCleanup(shutil.rmtree, self.root, True)
        self.out = self.root / "bundle"

    def write(self, name, data, languages=("opy",)):
        """An evaluation directory: score.json plus the result.json files the cards were scored from."""
        directory = self.root / name
        directory.mkdir(parents=True)
        cards = []
        for language in languages:
            track = [r for r in data if r["language"] == language]
            for i, r in enumerate(track):
                trial = directory / f"{language}-{i}-{r['_trial']}"
                trial.mkdir()
                (trial / "result.json").write_text(json.dumps({"contract": "wright-agent-bench/v3", **r}))
            cards.append(bench_score.card(track, language, SCENARIOS))
        (directory / "score.json").write_text(json.dumps({"contract": bench_score.CONTRACT, "cards": cards}))
        return directory

    def test_dry_run_writes_the_bundle_and_prints_the_uploads(self):
        directory = self.write("eval", results())
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(bench_publish.main([directory], self.out, True, None), 0)
        ident = bench_publish.run_id(directory)
        self.assertTrue((self.out / "bench/latest.json").is_file())
        self.assertTrue((self.out / f"bench/runs/{ident}.json").is_file())
        text = output.getvalue()
        self.assertIn("would upload", text)
        self.assertIn("s3://wrightkit-release/bench/latest.json", text)
        self.assertIn(f"releases.wrightkit.dev/bench/runs/{ident}.json", text)

    def test_bundles_validate_against_the_committed_schema(self):
        directory = self.write("eval", results(language="opy") + results(language="workshop"))
        latest, runs_bundles = bench_publish.build([directory])
        bench_publish.validate(latest)
        for bundle in runs_bundles.values():
            bench_publish.validate(bundle)
        broken = json.loads(json.dumps(latest))
        broken["entries"][0]["transcript"] = "leaked"
        from jsonschema.exceptions import ValidationError
        self.assertRaises(ValidationError, bench_publish.validate, broken)

    def test_bundle_never_contains_a_host_path_transcript_or_private_scenario_name(self):
        marker = "secret-elimination-task"
        data = results(agent={"id": "codex", "command": str(Path.home() / "bin" / "codex"), "seconds": 1.0},
                       transcript="the agent's full transcript", workspacePath=str(Path.home() / "workspaces" / "w1"))
        data.append(run(marker, 1, True, isPrivate=True, language="opy", sha="a" * 64,
                        agentInfo=dict(AGENT_INFO), prompt=marker, checks=[{"id": marker}]))
        data[-1]["environment"]["suite"] = dict(SUITE)
        directory = self.write("eval", data)
        latest, runs_bundles = bench_publish.build([directory])
        published = json.dumps(latest) + json.dumps(runs_bundles)
        self.assertNotIn(str(Path.home()), published)
        self.assertNotIn("transcript", published)
        self.assertNotIn(marker, published)

    def test_re_publishing_a_run_id_with_different_content_is_refused(self):
        first = self.write("one/eval", results())
        second = self.write("two/eval", results(sha="d" * 64))
        bench_publish.main([first], self.out, True, None)
        bench_publish.main([first], self.out, True, None)  # identical content is idempotent
        self.assertRaisesRegex(ValueError, "immutable", bench_publish.main, [second], self.out, True, None)

    def test_runs_from_mixed_environments_produce_one_latest_and_an_exclusion_list(self):
        same_a = self.write("same-a", results())
        same_b = self.write("same-b", results())
        other = self.write("other", results(sha="d" * 64))
        latest, runs_bundles = bench_publish.build([same_a, same_b, other])
        self.assertEqual({e["id"] for e in latest["entries"]}, {bench_publish.run_id(same_a), bench_publish.run_id(same_b)})
        self.assertEqual(latest["excluded"], [{"id": bench_publish.run_id(other), "reason": "different environment"}])
        self.assertEqual(len(runs_bundles), 3)  # excluded from latest is still published as an immutable run
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            bench_publish.main([same_a, same_b, other], self.out, True, None)
        self.assertIn(f"excluded from latest: {bench_publish.run_id(other)}", output.getvalue())

    def test_a_run_whose_cards_disagree_or_lack_the_agent_program_is_refused(self):
        no_program = self.write("no-program", results(agentInfo={"model": "m", "effort": "high"}))
        self.assertRaisesRegex(ValueError, "cannot publish", bench_publish.build, [no_program])


if __name__ == "__main__":
    unittest.main()
