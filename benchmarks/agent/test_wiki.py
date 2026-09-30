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

ARTICLES = {"wait-until": b"# Wait Until\nbody\n", "count-of": b"# Count Of\nbody\n"}


class Handler(BaseHTTPRequestHandler):
    manifest = {"schemaVersion": 1, "documents": [{"slug": s, "title": s, "updatedAt": "2026-01-01T00:00:00Z", "sourceUrl": f"https://workshop.codes/wiki/articles/{s}"} for s in ARTICLES]}

    def do_GET(self):
        if self.path == "/manifest.json":
            body = json.dumps(self.manifest).encode()
        elif self.path == "/wiki/articles":
            body = b"# index\n"
        elif self.path.startswith("/wiki/articles/") and self.path.rsplit("/", 1)[1] in ARTICLES:
            body = ARTICLES[self.path.rsplit("/", 1)[1]]
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

    def test_snapshot_writes_documents_and_a_stable_identity(self):
        first = bench_wiki.snapshot(self.base, self.tmp / "a", delay=0)
        second = bench_wiki.snapshot(self.base, self.tmp / "b", delay=0)
        self.assertEqual((self.tmp / "a/articles/wait-until.md").read_bytes(), ARTICLES["wait-until"])
        self.assertTrue((self.tmp / "a/index.md").is_file() and (self.tmp / "a/NOTICE.txt").is_file())
        self.assertEqual(first["snapshotSha256"], second["snapshotSha256"])
        self.assertEqual(bench_wiki.identity(self.tmp / "a")["documents"], 2)

    def test_snapshots_are_pinned_and_slugs_are_checked(self):
        bench_wiki.snapshot(self.base, self.tmp / "a", delay=0)
        with self.assertRaises(SystemExit):
            bench_wiki.snapshot(self.base, self.tmp / "a", delay=0)
        Handler.manifest = {"schemaVersion": 1, "documents": [{"slug": "../evil"}]}
        self.addCleanup(lambda: Handler.manifest.update(documents=[{"slug": s} for s in ARTICLES]))
        with self.assertRaises(SystemExit):
            bench_wiki.snapshot(self.base, self.tmp / "c", delay=0)

    def test_wiki_level_needs_a_snapshot(self):
        cell = {"wright": "none", "knowledge": "wiki", "network": "off"}
        with self.assertRaises(SystemExit):
            agent_bench.check_cell(cell, argparse.Namespace(skill_dir=None, wiki_dir=self.tmp))
        bench_wiki.snapshot(self.base, self.tmp / "snap", delay=0)
        agent_bench.check_cell(cell, argparse.Namespace(skill_dir=None, wiki_dir=self.tmp / "snap"))


if __name__ == "__main__":
    unittest.main()
