import hashlib
import json
import shutil
import tempfile
import unittest
from pathlib import Path

import wiki_skill

CATALOG = {
    "actions": [{"id": "playEffect", "aliases": {"en-US": "Play Effect"}}, {"id": "evaluateOnce", "aliases": {"en-US": "Evaluate Once"}},
                {"id": "createHudText", "aliases": {"en-US": "Create HUD Text"}}],
    "values": [], "events": [], "enums": [{"domain": "HudPosition"}],
}
MANIFEST = {"aliases": [{"source": "evalOnce", "target": "evaluateOnce"}], "functions": [{"id": "playEffect"}, {"id": "evaluateOnce"}, {"id": "hudText", "catalogId": "createHudText"}]}
UPSTREAM = "playEffect evalOnce HudPosition hudText"


def article(title: str, slug: str, body: str) -> str:
    return f"---\ntitle: {title}\ndescription: Workshop.code wiki article\nslug: {slug}\ntags: []\nupdated_at: 2026-07-10T18:15:21.022Z\ncontent_hash: abc\n---\n\n# {title}\n\n{body}\n"


class WikiSkillTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, True)
        snap = self.tmp / "snap"
        (snap / "articles").mkdir(parents=True)
        docs = [
            ("Play Effect", "play-effect", ["actions"], "Plays an effect. It stops when the player dies."),
            ("Evaluate Once", "evaluate-once", ["actions"], "Evaluates a value once."),
            ("HUD Position", "hud-position", ["constants"], "Where a HUD text appears on screen."),
            ("Unknown Thing", "unknown-thing", ["references"], "A reference table that has no catalog entry."),
            ("Create HUD Text", "create-hud-text", ["actions"], "Create HUD Text"),
        ]
        records = []
        for title, slug, cats, body in docs:
            raw = article(title, slug, body).encode()
            (snap / "articles" / f"{slug}.md").write_bytes(raw)
            records.append({"slug": slug, "categories": cats, "title": title, "updatedAt": "2026-07-10T18:15:21.022Z", "contentHash": "abc", "sha256": hashlib.sha256(raw).hexdigest()})
        identity_text = "\n".join(f"{d['slug']} {d['sha256']}" for d in sorted(records, key=lambda d: d["slug"]))
        (snap / "SNAPSHOT.json").write_text(json.dumps({"snapshotSha256": hashlib.sha256(identity_text.encode()).hexdigest(), "documents": records}))
        self.snap, self.out = snap, self.tmp / "workshop-wiki"

    def build(self, upstream: str = UPSTREAM) -> dict:
        return wiki_skill.build(self.snap, self.out, CATALOG, MANIFEST, upstream)

    def test_layout_and_progressive_disclosure(self):
        stats = self.build()
        skill = (self.out / "SKILL.md").read_text()
        self.assertIn("name: workshop-wiki", skill)
        self.assertLess(len(skill.splitlines()), 40)
        self.assertEqual(stats["articles"], 5)
        categories = (self.out / "references/categories.md").read_text()
        self.assertIn("actions.md", categories)
        index = (self.out / "references/actions.md").read_text()
        self.assertIn("[Play Effect](articles/play-effect.md)", index)
        note = (self.out / "references/articles/play-effect.md").read_text()
        self.assertNotIn("Workshop.code wiki article", note)
        self.assertIn("overpy: playEffect", note)
        self.assertIn("It stops when the player dies.", note)

    def test_overpy_spelling_is_upstream_name_and_only_when_verified(self):
        stats = self.build()
        index = (self.out / "references/actions.md").read_text()
        self.assertIn("overpy `playEffect`", index)
        self.assertIn("overpy `evalOnce`", index)  # the upstream spelling, not the canonical id
        self.assertNotIn("evaluateOnce", index.replace("Evaluate Once", ""))
        self.assertIn("overpy `HudPosition`", (self.out / "references/constants.md").read_text())
        self.assertIn("overpy `hudText`", index)  # through the manifest's catalogId, not the Workshop name
        self.assertEqual(stats["overpy"], 4)
        self.assertEqual(stats["catalogMatched"], 4)

    def test_unverified_spellings_are_dropped_not_guessed(self):
        stats = self.build(upstream="playEffect")
        self.assertEqual(stats["overpy"], 1)
        self.assertEqual(stats["overpyUnverified"], 3)
        self.assertNotIn("overpy `evalOnce`", (self.out / "references/actions.md").read_text())

    def test_identity_is_stable_and_output_is_not_overwritten(self):
        first = self.build()
        with self.assertRaises(SystemExit):
            self.build()
        shutil.rmtree(self.out)
        self.assertEqual(self.build()["skillSha256"], first["skillSha256"])
        with self.assertRaises(SystemExit):
            wiki_skill.build(self.snap, self.tmp / "other-name", CATALOG, MANIFEST, UPSTREAM)

    def test_changed_snapshot_is_refused_before_writing_a_skill(self):
        (self.snap / "articles/play-effect.md").write_text("changed")
        with self.assertRaisesRegex(SystemExit, "snapshot content mismatch"):
            self.build()
        self.assertFalse(self.out.exists())

    def test_skill_identity_refuses_changed_or_added_content(self):
        record = self.build()
        self.assertEqual(wiki_skill.identity(self.out), record)
        note = self.out / "references/articles/play-effect.md"
        original = note.read_text()
        note.write_text("changed")
        with self.assertRaisesRegex(SystemExit, "wiki skill content mismatch"):
            wiki_skill.identity(self.out)
        note.write_text(original)
        (self.out / "references/extra.md").write_text("extra context")
        with self.assertRaisesRegex(SystemExit, "wiki skill content mismatch"):
            wiki_skill.identity(self.out)

    def test_summary_takes_the_first_sentence_without_markup(self):
        self.assertEqual(wiki_skill.summary("> Source: https://x\n\nPlays an **effect**. It stops early."), "Plays an effect")
        self.assertEqual(wiki_skill.summary("# T\n\nshort"), "")
        self.assertEqual(wiki_skill.summary("<big>Waits until the condition is true.</big>"), "Waits until the condition is true")

    def test_stub_articles_fall_back_to_their_syntax_line(self):
        body = "# Create HUD Text\n\nCreate HUD Text\n\n```\nCreate HUD Text(All Players(All Teams), Custom String(\"x\"));\n```\n"
        self.assertTrue(wiki_skill.summary(body, "Create HUD Text").startswith("syntax `Create HUD Text("))


if __name__ == "__main__":
    unittest.main()
