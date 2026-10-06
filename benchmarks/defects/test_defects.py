import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "agent"))

import agent_bench
import bench_grade
import defects
import generate

WRIGHT = os.environ.get("WRIGHT_BIN", str(agent_bench.ROOT / "target/debug/wright"))
SEED = generate.BENCHMARKS / "defects/seeds/payload-race"
OTHER = generate.BENCHMARKS / "agent/scenarios/understand-opy-project/seed"
CANONICAL = [spec["id"] for spec in generate.INSTANCES]
COMPILED = (
    'rule ("tallyRound") {\n    event {\n        Subroutine;\n        tallyRound;\n    }\n'
    '    actions {\n        Modify Global Variable(round, Add, 1);\n    }\n}\n'
)


class DefectClassTest(unittest.TestCase):
    """SPEC-534: site discovery and injection are deterministic and content-based."""

    def setUp(self):
        self.defect = defects.REGISTRY["renamed-callable"]
        self.files = generate.seed_files(SEED)

    def test_sites_are_stable_and_content_based(self):
        sites = self.defect.sites(self.files)
        self.assertEqual([(s.file, s.symbol, s.calls) for s in sites], [
            ("helpers.opy", "tallyRound", (("main.opy", 21), ("scoring.opy", 6))),
            ("helpers.opy", "clearTags", (("scoring.opy", 10),)),
        ])
        self.assertEqual(sites, self.defect.sites(self.files))

    def test_pick_is_deterministic(self):
        self.assertEqual(generate.pick_site(self.defect, self.files), generate.pick_site(self.defect, self.files))

    def test_two_seeds_give_different_instances(self):
        other = self.defect.sites(generate.seed_files(OTHER))
        self.assertEqual([s.symbol for s in other], ["awardBonus"])
        self.assertNotEqual(generate.pick_site(self.defect, self.files).symbol,
                            generate.pick_site(self.defect, generate.seed_files(OTHER)).symbol)

    def test_apply_injects_only_the_declaration(self):
        applied = self.defect.apply(self.files, defects.Site("helpers.opy", "tallyRound", (("main.opy", 21),)), COMPILED)
        self.assertIn("def tallyRound_v2()", applied.seed_files["helpers.opy"])
        self.assertNotIn("def tallyRound()", applied.seed_files["helpers.opy"])
        self.assertIn("tallyRound()", applied.seed_files["main.opy"])
        self.assertEqual(applied.reference["helpers.opy"], self.files["helpers.opy"])
        self.assertEqual(json.loads(applied.reference["answer.json"]), {"missing": "tallyRound"})

    def test_negative_removes_def_and_calls(self):
        site = defects.Site("helpers.opy", "tallyRound", (("main.opy", 21), ("scoring.opy", 6)))
        applied = self.defect.apply(self.files, site, COMPILED)
        gone = applied.negatives["deleted-def"]["files"]
        self.assertNotIn("def tallyRound", gone["helpers.opy"])
        self.assertNotIn("tallyRound()", gone["main.opy"])
        self.assertIn("def clearTags()", gone["helpers.opy"])


@unittest.skipUnless(Path(WRIGHT).is_file() and bench_grade.oracle_available(), "needs wright and `setup-oracle`")
class GeneratedScenarioTest(unittest.TestCase):
    """SPEC-534 proof: deterministic generation and behavioral grading of the
    committed instances — seed fails, every correct fix passes, the negative fails."""

    def setUp(self):
        (agent_bench.ROOT / "target").mkdir(exist_ok=True)
        self.out = Path(tempfile.mkdtemp(dir=agent_bench.ROOT / "target")).resolve()
        self.addCleanup(shutil.rmtree, self.out, True)

    def grade(self, scenario_id: str, overlay: str | None = None) -> dict:
        scenario = agent_bench.load_scenario(scenario_id)
        workspace = self.out / f"{scenario_id}-{overlay or 'seed'}"
        shutil.rmtree(workspace, ignore_errors=True)
        agent_bench.materialize(scenario, workspace, overlay)
        return bench_grade.grade(scenario, workspace, WRIGHT)

    def test_committed_instances_match_the_generator(self):
        self.assertTrue(generate.check_current(WRIGHT))

    def test_generation_is_deterministic(self):
        spec = generate.INSTANCES[0]
        defect = defects.REGISTRY[spec["defect"]]
        scratch = Path(tempfile.mkdtemp(dir=self.out))
        compiled = generate.compile_seed(WRIGHT, generate.BENCHMARKS / spec["seed"], spec["entry"], defect.language, scratch)
        self.assertEqual(generate.render(defect, spec, compiled), generate.render(defect, spec, compiled))

    def test_seed_fails_and_reference_passes(self):
        for scenario_id in CANONICAL:
            with self.subTest(scenario=scenario_id):
                self.assertFalse(self.grade(scenario_id)["passed"], "defective seed must fail")
                result = self.grade(scenario_id, "reference")
                self.assertEqual([c["id"] for c in result["checks"] if not c["passed"]], [])
                self.assertEqual(result["unsafeEdits"], [])

    def test_retargeted_calls_are_an_accepted_fix(self):
        """Any correct fix passes: renaming every call to the injected def name
        keeps the same checks green as restoring the declaration."""
        for scenario_id in CANONICAL:
            with self.subTest(scenario=scenario_id):
                scenario = agent_bench.load_scenario(scenario_id)
                symbol = scenario["generated"]["site"]["symbol"]
                workspace = self.out / f"{scenario_id}-fixb"
                agent_bench.materialize(scenario, workspace)
                for path in workspace.glob("*.opy"):
                    path.write_text(path.read_text().replace(f"{symbol}(", f"{symbol}_v2("))
                (workspace / "answer.json").write_text(json.dumps({"missing": symbol}))
                result = bench_grade.grade(scenario, workspace, WRIGHT)
                self.assertEqual([c["id"] for c in result["checks"] if not c["passed"]], [])
                self.assertEqual(result["unsafeEdits"], [])

    def test_negative_fails_the_declared_checks(self):
        for scenario_id in CANONICAL:
            with self.subTest(scenario=scenario_id):
                scenario = agent_bench.load_scenario(scenario_id)
                failed = sorted(c["id"] for c in self.grade(scenario_id, "negative/deleted-def")["checks"] if not c["passed"])
                self.assertEqual(failed, sorted(scenario["negatives"]["deleted-def"]["fails"]))


if __name__ == "__main__":
    unittest.main()
