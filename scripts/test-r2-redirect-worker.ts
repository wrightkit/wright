import assert from "node:assert/strict";
import { test } from "node:test";

import worker, { type Env } from "./r2-redirect-worker.ts";

const stored = new Map([
  ["wright/releases/1.2.3/wright-1.2.3-x86_64-apple-darwin.tar.gz", "archive"],
  ["wright/latest/version", "1.2.3\n"],
]);

const env: Env = {
  BUCKET: {
    async get(key) {
      const value = stored.get(key);
      if (value === undefined) return null;
      return {
        body: new Response(value).body!,
        httpEtag: '"etag"',
        writeHttpMetadata(headers) {
          headers.set("cache-control", "public, max-age=31536000, immutable");
        },
      };
    },
  },
};

const call = (path: string, method = "GET") =>
  worker.fetch(new Request(`https://install.wrightkit.dev/${path}`, { method }), env);

test("serves an existing object unchanged", async () => {
  const res = await call("wright/releases/1.2.3/wright-1.2.3-x86_64-apple-darwin.tar.gz");
  assert.equal(res.status, 200);
  assert.equal(await res.text(), "archive");
  assert.equal(res.headers.get("cache-control"), "public, max-age=31536000, immutable");
  assert.equal(res.headers.get("etag"), '"etag"');
});

test("HEAD returns headers without a body", async () => {
  const res = await call("wright/latest/version", "HEAD");
  assert.equal(res.status, 200);
  assert.equal(await res.text(), "");
});

for (const file of [
  "wright-0.9.0-x86_64-unknown-linux-gnu.tar.gz",
  "wright-0.9.0-x86_64-unknown-linux-gnu.tar.gz.sha256",
  "wright-0.9.0-x86_64-pc-windows-msvc.zip",
]) {
  test(`redirects pruned ${file} to GitHub`, async () => {
    const res = await call(`wright/releases/0.9.0/${file}`);
    assert.equal(res.status, 302);
    assert.equal(
      res.headers.get("location"),
      `https://github.com/wrightkit/wright/releases/download/v0.9.0/${file}`,
    );
  });
}

for (const path of [
  "wright/nightly/abcdef123456/wright-0.9.0-x86_64-unknown-linux-gnu.tar.gz",
  "wright/nightly/version",
  "wright/latest/missing",
  "wright/releases/0.9.0/wright-0.8.0-x86_64-unknown-linux-gnu.tar.gz",
  "wright/releases/0.9.0/other.txt",
]) {
  test(`returns 404 for missing ${path}`, async () => {
    assert.equal((await call(path)).status, 404);
  });
}
