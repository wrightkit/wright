import argparse
import json
import shutil
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

import agent_bench
import bench_wiki

ARTICLES = {
    "wait-until": b"---\ntitle: Wait Until\nupdated_at: 2026-01-01T00:00:00Z\ncontent_hash: aa\n---\n\n# Wait Until\nbody\n",
    "count-of": b"---\ntitle: Count Of\nupdated_at: 2026-01-02T00:00:00Z\ncontent_hash: bb\n---\n\n# Count Of\nbody\n",
}


class Handler(BaseHTTPRequestHandler):
    categories = {"actions": list(ARTICLES), "values": ["count-of"]}  # count-of appears in two categories

    def do_GET(self):
        path = self.path
        if path.startswith("/wiki/categories/") and path.rsplit("/", 1)[1] in self.categories:
            body = "\n".join(f"- [{s}](https://mirror.example/wiki/articles/{s})" for s in self.categories[path.rsplit("/", 1)[1]]).encode()
        elif path.startswith("/wiki/articles/") and path.rsplit("/", 1)[1] in ARTICLES:
            body = ARTICLES[path.rsplit("/", 1)[1]]
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


class WikiSnapshotTest(unittest.TestCase):
    def setUp(self):
        self.server = HTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.addCleanup(self.server.shutdown)
        self.base = f"http://127.0.0.1:{self.server.server_port}"
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, True)

    def crawl(self, name: str) -> dict:
        return bench_wiki.snapshot(self.base, self.tmp / name, categories=("actions", "values"), delay=0)

    def test_snapshot_crawls_categories_once_per_article_with_a_stable_identity(self):
        first, second = self.crawl("a"), self.crawl("b")
        self.assertEqual((self.tmp / "a/articles/wait-until.md").read_bytes(), ARTICLES["wait-until"])
        self.assertTrue((self.tmp / "a/NOTICE.txt").is_file())
        self.assertEqual([d["slug"] for d in first["documents"]], ["count-of", "wait-until"])
        self.assertEqual(next(d for d in first["documents"] if d["slug"] == "count-of")["categories"], ["actions", "values"])
        self.assertEqual(first["documents"][1]["contentHash"], "aa")
        self.assertEqual(first["snapshotSha256"], second["snapshotSha256"])
        self.assertEqual(bench_wiki.identity(self.tmp / "a")["documents"], 2)

    def test_snapshots_are_pinned_and_slugs_are_checked(self):
        self.crawl("a")
        with self.assertRaises(SystemExit):
            self.crawl("a")
        Handler.categories["evil"] = ["../etc"]
        self.addCleanup(Handler.categories.pop, "evil")
        with self.assertRaises(SystemExit):
            bench_wiki.snapshot(self.base, self.tmp / "c", categories=("evil",), delay=0)
        with self.assertRaises(SystemExit):
            bench_wiki.snapshot(self.base, self.tmp / "d", categories=("missing",), delay=0)

    def test_wiki_level_needs_a_snapshot(self):
        cell = {"wright": "none", "knowledge": "wiki", "network": "off"}
        with self.assertRaises(SystemExit):
            agent_bench.check_cell(cell, argparse.Namespace(skill_dir=None, wiki_dir=self.tmp, wiki_skill_dir=None))
        self.crawl("snap")
        agent_bench.check_cell(cell, argparse.Namespace(skill_dir=None, wiki_dir=self.tmp / "snap", wiki_skill_dir=None))


if __name__ == "__main__":
    unittest.main()
