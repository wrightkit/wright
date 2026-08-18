"""Integrity checks for Wright's committed compatibility regression evidence.

Wright consumes these snapshots as migration/product regression inputs. The
source-language repositories own live upstream oracle acquisition and refresh.
These tests deliberately require no Node, .NET, or upstream compiler runtime.
"""

import hashlib
import json
import unittest
from pathlib import Path


COMPATIBILITY_DIR = Path(__file__).resolve().parents[1]
FIXTURES_DIR = COMPATIBILITY_DIR / "fixtures"


def load_json(path: Path) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise AssertionError(f"JSON root must be an object: {path}")
    return value


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class CompatibilityEvidenceTests(unittest.TestCase):
    def test_recorded_fixture_snapshots_are_self_consistent(self):
        metadata_paths = sorted(FIXTURES_DIR.glob("**/fixture.json"))
        self.assertTrue(metadata_paths, "compatibility regression fixtures are missing")

        ids = set()
        for metadata_path in metadata_paths:
            fixture_dir = metadata_path.parent
            metadata = load_json(metadata_path)
            fixture_id = metadata.get("id")
            self.assertIsInstance(fixture_id, str, metadata_path)
            self.assertNotIn(fixture_id, ids, f"duplicate fixture id: {fixture_id}")
            ids.add(fixture_id)

            relative_id = fixture_dir.relative_to(FIXTURES_DIR).as_posix()
            self.assertEqual(fixture_id, relative_id)
            self.assertEqual(metadata.get("schemaVersion"), 1, fixture_id)
            self.assertIn(metadata.get("expectedStatus"), {"success", "failure"}, fixture_id)

            source_rel = metadata.get("source")
            self.assertIsInstance(source_rel, str, fixture_id)
            source_path = fixture_dir / source_rel
            self.assertTrue(source_path.is_file(), f"{fixture_id}: missing {source_rel}")

            provenance = metadata.get("provenance")
            self.assertIsInstance(provenance, dict, fixture_id)
            self.assertIn("origin", provenance, fixture_id)
            self.assertIn("license", provenance, fixture_id)
            self.assertIs(provenance.get("redistributable"), True, fixture_id)

            snapshot_path = fixture_dir / "oracle.json"
            self.assertTrue(snapshot_path.is_file(), f"{fixture_id}: missing recorded snapshot")
            snapshot = load_json(snapshot_path)
            self.assertEqual(snapshot.get("schemaVersion"), 1, fixture_id)
            self.assertEqual(snapshot.get("fixture"), fixture_id)

            input_record = snapshot.get("input")
            self.assertIsInstance(input_record, dict, fixture_id)
            self.assertEqual(input_record.get("source"), source_rel, fixture_id)
            self.assertEqual(input_record.get("sha256"), sha256(source_path), fixture_id)

            compile_record = snapshot.get("compile")
            self.assertIsInstance(compile_record, dict, fixture_id)
            self.assertEqual(
                compile_record.get("status"),
                metadata.get("expectedStatus"),
                fixture_id,
            )
            self.assertIsInstance(compile_record.get("diagnostics"), list, fixture_id)
            self.assertIsInstance(compile_record.get("workshop"), str, fixture_id)

            workshop_hash = compile_record.get("workshopSha256")
            if workshop_hash is not None:
                self.assertEqual(
                    workshop_hash,
                    hashlib.sha256(compile_record["workshop"].encode("utf-8")).hexdigest(),
                    fixture_id,
                )

    def test_imported_fixture_provenance_is_pinned(self):
        for metadata_path in sorted(FIXTURES_DIR.glob("**/fixture.json")):
            metadata = load_json(metadata_path)
            provenance = metadata.get("provenance", {})
            if provenance.get("kind") not in {"imported-example", "imported-project"}:
                continue
            fixture_id = metadata["id"]
            commit = provenance.get("sourceCommit")
            self.assertIsInstance(commit, str, fixture_id)
            self.assertEqual(len(commit), 40, fixture_id)
            self.assertEqual(provenance.get("modifications"), "none", fixture_id)


if __name__ == "__main__":
    unittest.main()
