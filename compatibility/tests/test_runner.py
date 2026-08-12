import json
import sys
import unittest
from pathlib import Path


COMPATIBILITY_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(COMPATIBILITY_DIR))

import run_oracle  # noqa: E402


class RunnerTests(unittest.TestCase):
    def test_normalize_text_keeps_content_and_removes_presentation_noise(self):
        self.assertEqual(
            run_oracle.normalize_text("one  \r\ntwo\r\n\r\n"),
            "one\ntwo\n",
        )

    def test_normalize_diagnostics_preserves_error_and_location_text(self):
        diagnostics = run_oracle.normalize_diagnostics(
            "Error: broken\n    | line 1, col 1\n\nWarning: check this\n"
        )
        self.assertEqual(
            diagnostics,
            [
                {
                    "severity": "error",
                    "text": "Error: broken\n    | line 1, col 1",
                },
                {"severity": "warning", "text": "Warning: check this"},
            ],
        )

    def test_repository_fixture_metadata_is_valid(self):
        fixtures = run_oracle.discover_fixtures(
            COMPATIBILITY_DIR / "fixtures"
        )
        self.assertEqual(
            [fixture["id"] for _, fixture in fixtures],
            ["synthetic/basic-rule"],
        )

    def test_snapshot_is_valid_json_when_present(self):
        snapshot = (
            COMPATIBILITY_DIR
            / "fixtures"
            / "synthetic"
            / "basic-rule"
            / "oracle.json"
        )
        if snapshot.exists():
            self.assertIsInstance(json.loads(snapshot.read_text(encoding="utf-8")), dict)


if __name__ == "__main__":
    unittest.main()
