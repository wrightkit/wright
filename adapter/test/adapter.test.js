/**
 * Adapter tests.
 *
 * Covers:
 *  - conversion of every success fixture in the compatibility corpus to a
 *    checked-in Opy HIR v1 snapshot (determinism evidence);
 *  - explicit, structured failure for the corpus's expected-failure fixture;
 *  - mapping edges exercised by mini-fixtures (constants, macros, and
 *    unsupported constructs).
 *
 * Regenerate snapshots after an intentional adapter change with:
 *   UPDATE_FIXTURES=1 node --test test/
 */

import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { convert, stringify } from "../lib/main.js";
import { AdapterError } from "../lib/adapter.js";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const REPO = resolve(ROOT, "..");
const CORPUS = join(REPO, "compatibility", "fixtures");
const SNAPSHOTS = join(ROOT, "fixtures");
const MINI = join(ROOT, "test", "fixtures");
const UPDATE = process.env.UPDATE_FIXTURES === "1";

const SUCCESS_FIXTURES = [
  "synthetic/basic-rule",
  "synthetic/control-flow",
  "synthetic/declarations-rules",
  "synthetic/expressions-values",
  "synthetic/preprocessing",
  "real-world/overpy-cake",
];

const FAILURE_FIXTURES = ["synthetic/diagnostics"];

function corpusSource(fixtureId) {
  const dir = join(CORPUS, fixtureId);
  const metadata = JSON.parse(readFileSync(join(dir, "fixture.json"), "utf8"));
  return { source: join(dir, metadata.source), root: dir, mainFile: metadata.source };
}

function snapshotPath(fixtureId) {
  return join(SNAPSHOTS, `${fixtureId}.json`);
}

function assertSnapshot(relativeName, actual, snapshotFile) {
  const payload = stringify(actual);
  if (UPDATE) {
    mkdirSync(dirname(snapshotFile), { recursive: true });
    writeFileSync(snapshotFile, payload, "utf8");
    return;
  }
  if (!existsSync(snapshotFile)) {
    assert.fail(
      `missing snapshot ${snapshotFile}; regenerate with UPDATE_FIXTURES=1 after review`
    );
  }
  assert.equal(
    payload,
    readFileSync(snapshotFile, "utf8"),
    `snapshot mismatch for ${relativeName}`
  );
}

for (const fixtureId of SUCCESS_FIXTURES) {
  test(`converts corpus fixture ${fixtureId}`, async () => {
    const { source, root, mainFile } = corpusSource(fixtureId);
    const content = readFileSync(source, "utf8");
    const program = await convert({ content, rootPath: root, mainFileName: mainFile });
    assert.equal(program.protocol.name, "wright/opy-hir");
    assert.equal(program.protocol.version, "1.0.0");
    assertSnapshot(fixtureId, program, snapshotPath(fixtureId));
  });
}

for (const fixtureId of FAILURE_FIXTURES) {
  test(`fails explicitly on corpus fixture ${fixtureId}`, async () => {
    const { source, root, mainFile } = corpusSource(fixtureId);
    const content = readFileSync(source, "utf8");
    await assert.rejects(
      convert({ content, rootPath: root, mainFileName: mainFile }),
      (error) => {
        assert.ok(error instanceof AdapterError, "expected an AdapterError");
        assert.equal(error.code, "parse");
        assert.ok(error.message.length > 0);
        assert.ok(error.span, "parse error must carry a source span");
        return true;
      }
    );
  });
}

const MINI_SNAPSHOTS = ["constants", "macros"];
const MINI_UNSUPPORTED = ["unsupported-goto", "unsupported-annotation", "unsupported-settings"];

for (const name of MINI_SNAPSHOTS) {
  test(`converts mini-fixture ${name}`, async () => {
    const content = readFileSync(join(MINI, `${name}.opy`), "utf8");
    const program = await convert({ content, rootPath: MINI, mainFileName: `${name}.opy` });
    assertSnapshot(`${name}`, program, join(MINI, `${name}.json`));
  });
}

for (const name of MINI_UNSUPPORTED) {
  test(`rejects mini-fixture ${name} explicitly`, async () => {
    const content = readFileSync(join(MINI, `${name}.opy`), "utf8");
    await assert.rejects(
      convert({ content, rootPath: MINI, mainFileName: `${name}.opy` }),
      (error) => {
        assert.ok(error instanceof AdapterError, "expected an AdapterError");
        assert.ok(["parse", "unsupported"].includes(error.code));
        return true;
      }
    );
  });
}
