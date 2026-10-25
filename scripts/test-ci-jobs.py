import copy
import importlib.util
from pathlib import Path
import unittest

import yaml


SPEC = importlib.util.spec_from_file_location("ci_jobs", Path(__file__).with_name("ci-jobs.py"))
ci_jobs = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci_jobs)


class JobCouplingTests(unittest.TestCase):
    def setUp(self):
        self.workflow = yaml.safe_load(ci_jobs.WORKFLOW.read_text())

    def test_actual_dependencies_match_gating_jobs(self):
        required = ci_jobs.required_jobs(self.workflow, {})
        self.assertTrue(required["paths"])
        self.assertFalse(required["cli-behavior"])
        self.assertNotIn("benchmark", required)
        self.assertNotIn("metrics-latency", required)

    def test_missing_dependency_fails(self):
        self.workflow["jobs"]["ci-result"]["needs"].remove("cli-behavior")
        with self.assertRaisesRegex(ValueError, "ci-result.needs"):
            ci_jobs.required_jobs(self.workflow, {})

    def test_disabled_job_fails(self):
        self.workflow["jobs"]["cli-behavior"]["if"] = False
        with self.assertRaisesRegex(ValueError, "cli-behavior: expected a path condition"):
            ci_jobs.required_jobs(self.workflow, {})

    def test_required_when_uses_the_actual_job_condition(self):
        self.workflow["jobs"]["cli-behavior"]["if"] = "needs.paths.outputs.cli == 'true'"
        required = ci_jobs.required_jobs(self.workflow, {"cli": "true", "cli-behavior": "false"})
        self.assertTrue(required["cli-behavior"])

    def test_any_build_filter_selects_the_build(self):
        for paths in ({"opy": "true"}, {"cli": "true"}):
            with self.subTest(paths=paths):
                self.assertTrue(ci_jobs.required_jobs(self.workflow, paths)["wright-cli-build"])

    def test_new_gating_job_requires_a_dependency(self):
        self.workflow["jobs"]["new-job"] = copy.deepcopy(self.workflow["jobs"]["cli-behavior"])
        with self.assertRaisesRegex(ValueError, "ci-result.needs"):
            ci_jobs.required_jobs(self.workflow, {})

    def test_skipped_unselected_job_passes(self):
        ci_jobs.check_results({"cli-behavior": False}, {"cli-behavior": {"result": "skipped"}})

    def test_selected_or_failed_jobs_cannot_be_green(self):
        for result in ("skipped", "failure", "cancelled", "missing"):
            with self.subTest(result=result):
                with self.assertRaisesRegex(ValueError, "cli-behavior:"):
                    ci_jobs.check_results({"cli-behavior": True}, {"cli-behavior": {"result": result}})
        for result in ("failure", "cancelled", "missing"):
            with self.subTest(result=result):
                with self.assertRaisesRegex(ValueError, "cli-behavior:"):
                    ci_jobs.check_results({"cli-behavior": False}, {"cli-behavior": {"result": result}})
        ci_jobs.check_results({"cli-behavior": True}, {"cli-behavior": {"result": "success"}})


if __name__ == "__main__":
    unittest.main()
