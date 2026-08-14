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
  "synthetic/declarations-numbers",
  "synthetic/declarations-rules",
  "synthetic/expressions-values",
  "synthetic/preprocessing",
  "synthetic/settings",
  "real-world/overpy-cake",
  "real-world/overpy-pixelart",
  "real-world/overpy-client-to-server",
];

const FAILURE_FIXTURES = [
  { id: "synthetic/diagnostics", code: "parse" },
  { id: "real-world/overpy-santa", code: "unsupported" },
  { id: "real-world/overpy-meipocalypse", code: "parse" },
  { id: "real-world/overpy-zencopter", code: "parse" },
  { id: "real-world/overpy-cronch", code: "unsupported" },
  { id: "real-world/overpy-broken-weapons", code: "unsupported" },
  { id: "real-world/ow1-emulator", code: "parse" },
  { id: "real-world/6v6-adjustments", code: "parse" },
];

// Recorded reference failures (code and message verbatim, recorded from the
// pinned adapter at M11 acquisition). The meipocalypse ENOENT message embeds
// the fixture directory path, so its expected message is constructed from the
// same root the adapter receives; every other message is machine-stable.
const FAILURE_MESSAGES = {
  "real-world/overpy-santa":
    "construct '__doWhile__' is outside the Opy HIR v1 corpus boundary",
  "real-world/overpy-zencopter":
    "Invalid content before string: 'arena'\n    | line 38, col 17, at heli.opy",
  "real-world/overpy-cronch":
    "construct '@Name' is outside the Opy HIR v1 corpus boundary",
  "real-world/overpy-broken-weapons":
    "construct '__doWhile__' is outside the Opy HIR v1 corpus boundary",
  "real-world/ow1-emulator":
    "Found 'if', but no 'else'\n    | line 94, col 14, at arena.opy\n    | line 86, col 1, at 1v1_main.opy",
  "real-world/6v6-adjustments":
    "Unknown member '_hp_reset' of 'eventPlayer'\n    | line 31, col 17, at custom_hp.opy\n    | line 14, col 1, at main.opy",
};

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
    assert.equal(program.protocol.version, "1.1.0");
    assertSnapshot(fixtureId, program, snapshotPath(fixtureId));
  });
}

for (const failure of FAILURE_FIXTURES) {
  test(`fails explicitly on corpus fixture ${failure.id}`, async () => {
    const { source, root, mainFile } = corpusSource(failure.id);
    const content = readFileSync(source, "utf8");
    await assert.rejects(
      convert({ content, rootPath: root, mainFileName: mainFile }),
      (error) => {
        assert.ok(error instanceof AdapterError, "expected an AdapterError");
        assert.equal(error.code, failure.code);
        assert.ok(error.message.length > 0);
        if (failure.id === "real-world/overpy-meipocalypse") {
          assert.equal(
            error.message,
            `ENOENT: no such file or directory, lstat '${join(root, "generateWalls.js")}'`
          );
        } else if (FAILURE_MESSAGES[failure.id] !== undefined) {
          assert.equal(error.message, FAILURE_MESSAGES[failure.id]);
        } else {
          assert.ok(error.span, "parse error must carry a source span");
        }
        return true;
      }
    );
  });
}

const MINI_SNAPSHOTS = ["constants", "macros", "settings"];
const MINI_UNSUPPORTED = ["unsupported-goto", "unsupported-annotation"];

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
