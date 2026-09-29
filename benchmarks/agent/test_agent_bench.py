import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import agent_bench

WRIGHT = os.environ.get("WRIGHT_BIN", str(agent_bench.ROOT / "target/debug/wright"))


@unittest.skipUnless(Path(WRIGHT).is_file(), "build wright first or set WRIGHT_BIN")
class AgentBenchTest(unittest.TestCase):
    def setUp(self):
        (agent_bench.ROOT / "target").mkdir(exist_ok=True)
        self.out = Path(tempfile.mkdtemp(dir=agent_bench.ROOT / "target"))
        self.addCleanup(shutil.rmtree, self.out, True)

    def test_scenarios_are_solvable_and_not_vacuous(self):
        self.assertTrue(agent_bench.validate(WRIGHT, self.out))

    def test_conditions_differ_only_in_wright_availability(self):
        scenario = "repair-runaway-loop"
        reference = agent_bench.SCENARIOS / scenario / "reference"
        agent = f"cp {reference}/* . && (wright check mode.ws >/dev/null 2>&1 || echo no-wright > missing-wright.txt)"
        subprocess.run(
            [sys.executable, agent_bench.__file__, "run", scenario, "--wright", WRIGHT, "--out", str(self.out), "--agent-id", "fake", "--agent-cmd", agent],
            check=False,
            capture_output=True,
        )
        baseline = json.loads((self.out / scenario / "baseline-1/result.json").read_text())
        assisted = json.loads((self.out / scenario / "wright-1/result.json").read_text())
        self.assertEqual(baseline["wrightUse"]["invocations"], 0)
        self.assertIn("missing-wright.txt", baseline["unsafeEdits"])
        self.assertEqual(assisted["wrightUse"]["byCommand"], {"check": 1})
        self.assertTrue(assisted["passed"])
        self.assertEqual(assisted["unsafeEdits"], [])


if __name__ == "__main__":
    unittest.main()
