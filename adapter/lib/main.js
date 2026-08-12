/**
 * Adapter entry logic shared by the CLI and the test runner.
 */

import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { parse } from "./driver.js";
import { convertProgram } from "./adapter.js";
import { SpanBuilder } from "./span.js";
import { AdapterError, parseFailure } from "./errors.js";

const require = createRequire(import.meta.url);
const packageJson = require("../package.json");

/**
 * Deterministically render a HIR program as JSON with stable key order and
 * indentation, plus a trailing newline.
 *
 * @param {object} program
 * @returns {string}
 */
export function stringify(program) {
  return JSON.stringify(program, null, 2) + "\n";
}

/**
 * The `generator` envelope field, derived from this package's manifest.
 *
 * @returns {{ name: string, version: string, frontend: string }}
 */
export function generatorIdentity() {
  const overpySpec = packageJson.dependencies?.overpy ?? "unknown";
  return {
    name: packageJson.name ?? "wright-overpy-adapter",
    version: packageJson.version ?? "0.0.0",
    frontend: `overpy@${overpySpec}`,
  };
}

/**
 * Convert one `.opy` program into Opy HIR v1.
 *
 * @param {object} options
 * @param {string} options.content Source text of the main file.
 * @param {string} options.rootPath Directory containing the main file, used
 *   to resolve includes.
 * @param {string} options.mainFileName Name of the main file.
 * @returns {Promise<object>} The HIR program object.
 */
export async function convert({ content, rootPath, mainFileName }) {
  const normalizedRoot = rootPath.trim().replaceAll("\\", "/");
  const root = normalizedRoot.endsWith("/") ? normalizedRoot : `${normalizedRoot}/`;
  let parsed;
  try {
    parsed = await parse({ content, rootPath: root, mainFileName });
  } catch (error) {
    if (error instanceof AdapterError) {
      throw error;
    }
    throw parseFailure(error);
  }
  return convertProgram({
    astRules: parsed.astRules,
    compiler: parsed.compiler,
    spans: new SpanBuilder(),
    generator: generatorIdentity(),
  });
}

/**
 * CLI driver.
 *
 * @param {object} options
 * @param {string} options.input Path to the main `.opy` file.
 * @param {string} options.root Directory containing the main file.
 * @param {string} options.mainFile Name of the main file.
 * @param {string | null} options.output Where to write HIR JSON; stdout when null.
 * @returns {Promise<number>} Process exit code.
 */
export async function run({ input, root, mainFile, output }) {
  let content;
  try {
    content = readFileSync(input, "utf8");
  } catch (error) {
    const message = error && error.message ? error.message : String(error);
    process.stderr.write(`${JSON.stringify({ code: "io", message })}\n`);
    return 1;
  }

  let program;
  try {
    program = await convert({ content, rootPath: root, mainFileName: mainFile });
  } catch (error) {
    if (error instanceof AdapterError) {
      process.stderr.write(`${JSON.stringify(error.toJSON())}\n`);
      return 1;
    }
    throw error;
  }

  const payload = stringify(program);
  if (output) {
    const { writeFileSync, mkdirSync } = await import("node:fs");
    mkdirSync(require("node:path").dirname(output), { recursive: true });
    writeFileSync(output, payload, "utf8");
  } else {
    process.stdout.write(payload);
  }
  return 0;
}
