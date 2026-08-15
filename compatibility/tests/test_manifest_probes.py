import subprocess
import sys
import unittest
from pathlib import Path


COMPATIBILITY_DIR = Path(__file__).resolve().parents[1]
WORKSPACE = COMPATIBILITY_DIR.parent
VALIDATOR = (
    WORKSPACE
    / "crates/wright-opy/src/manifest/probes/validate.py"
)


@unittest.skipUnless(
    (WORKSPACE / "compatibility/oracle/node_modules/overpy/cli.js").is_file(),
    "the pinned OverPy oracle is not installed (pnpm install --dir compatibility/oracle)",
)
class ManifestProbeTests(unittest.TestCase):
    def test_manifest_probes_match_the_pinned_oracle(self):
        """The OPY semantic manifest's probe evidence must match the pinned
        oracle (S/D level: accept/reject, emission hash, diagnostic
        category). This is the reference validation behind the Wright-owned
        manifest data (issue #109)."""
        result = subprocess.run(
            [sys.executable, str(VALIDATOR)],
            capture_output=True,
            text=True,
        )
        self.assertEqual(
            result.returncode,
            0,
            msg=result.stdout + result.stderr,
        )
        self.assertIn("all 44 probes match", result.stdout)


if __name__ == "__main__":
    unittest.main()
