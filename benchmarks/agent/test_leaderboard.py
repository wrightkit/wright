import json
import shutil
import tempfile
import unittest
from pathlib import Path

import bench_leaderboard
import bench_score
from test_score import SCENARIOS, runs

ROOT = Path(__file__).resolve().parents[2]


class LeaderboardTest(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(dir=ROOT / "target"))
        self.addCleanup(shutil.rmtree, self.root, True)

    def write(self, name, usable, sha="a" * 64, model="m", tracks=("workshop", "opy")):
        directory = self.root / name
        directory.mkdir()
        cards = [bench_score.card([{**r, "language": lang} for r in runs(SCENARIOS[:usable], sha=sha, model=model)], lang, SCENARIOS) for lang in tracks]
        (directory / "score.json").write_text(json.dumps({"contract": bench_score.CONTRACT, "cards": cards}))
        return directory

    def test_entries_are_ranked_and_tied_with_the_top_when_intervals_overlap(self):
        data = bench_leaderboard.build([self.write("low", 1), self.write("high", 8), self.write("close", 7)])
        names = [e["run"] for e in data["entries"]]
        self.assertEqual(names[0], "high")
        standing = {e["run"]: e["standing"] for e in data["entries"]}
        self.assertEqual((standing["high"], standing["close"], standing["low"]), ("top", "tied with top", "below top"))

    def test_runs_against_a_different_wright_are_listed_as_not_comparable(self):
        data = bench_leaderboard.build([self.write("a", 6), self.write("b", 5), self.write("other", 6, sha="d" * 64)])
        self.assertEqual({e["run"] for e in data["entries"]}, {"a", "b"})
        self.assertEqual([o["run"] for o in data["excluded"]], ["other"])
        self.assertIn("Not comparable", bench_leaderboard.markdown(data))

    def test_markdown_and_page_give_the_headline_a_reader_needs(self):
        data = bench_leaderboard.build([self.write("only", 6)])
        text, page = bench_leaderboard.markdown(data), bench_leaderboard.page(data)
        for needed in ("Wright Agent Score", "Workshop score", "OverPy score", "top", "How to read this", "Limits"):
            self.assertIn(needed, text)
        self.assertIn("█", text)
        self.assertIn('class="bar"', page)
        self.assertIn("width:75.0%", page)  # 6 of 8 scenarios
        self.assertNotIn("<script", page)

    def test_the_page_escapes_what_comes_from_results(self):
        data = bench_leaderboard.build([self.write("x", 4, model="<img src=x onerror=alert(1)>")])
        data["entries"][0]["model"] = "<img src=x onerror=alert(1)>"
        self.assertNotIn("<img", bench_leaderboard.page(data))

    def test_main_writes_markdown_html_and_json(self):
        directory = self.write("one", 5)
        out = self.root / "leaderboard"
        self.assertEqual(bench_leaderboard.main([directory], out), 0)
        self.assertTrue(all((out / n).is_file() for n in ("LEADERBOARD.md", "leaderboard.html", "leaderboard.json")))
        self.assertEqual(bench_leaderboard.main([self.root / "missing"], self.root / "none"), 1)


if __name__ == "__main__":
    unittest.main()
