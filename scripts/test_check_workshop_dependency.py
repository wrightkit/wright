import unittest
from importlib.util import module_from_spec, spec_from_file_location
from pathlib import Path

spec = spec_from_file_location(
    "check_workshop_dependency", Path(__file__).with_name("check-workshop-dependency.py")
)
assert spec is not None and spec.loader is not None
checker = module_from_spec(spec)
spec.loader.exec_module(checker)
is_pinned_git_candidate = checker.is_pinned_git_candidate


class PinnedGitCandidateTests(unittest.TestCase):
    def test_accepts_exact_revision_pin(self):
        revision = "ac5a6a4cf15bfccc5597cfd6ccb7b5028dfd5053"
        source = (
            "git+https://github.com/wrightkit/workshop-rs.git?rev="
            f"{revision}#{revision}"
        )
        self.assertTrue(is_pinned_git_candidate(source))

    def test_rejects_git_sources_without_an_explicit_full_revision(self):
        resolved = "ac5a6a4cf15bfccc5597cfd6ccb7b5028dfd5053"
        sources = (
            f"git+https://github.com/wrightkit/workshop-rs.git#{resolved}",
            f"git+https://github.com/wrightkit/workshop-rs.git?branch=main#{resolved}",
            "git+https://github.com/wrightkit/workshop-rs.git?rev=v1.0.0#"
            f"{resolved}",
            "git+https://github.com/wrightkit/workshop-rs.git?rev="
            f"{resolved}#0000000000000000000000000000000000000000",
            f"git+https://github.com/other/workshop-rs.git?rev={resolved}#{resolved}",
        )
        for source in sources:
            with self.subTest(source=source):
                self.assertFalse(is_pinned_git_candidate(source))

    def test_rejects_missing_source(self):
        self.assertFalse(is_pinned_git_candidate(None))


if __name__ == "__main__":
    unittest.main()
