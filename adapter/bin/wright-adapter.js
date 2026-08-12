#!/usr/bin/env node
/**
 * Wright OverPy compatibility adapter CLI.
 *
 * Converts one `.opy` program through the pinned OverPy frontend into an
 * Opy HIR v1 payload (docs/hir/opy-hir-v1.md).
 *
 * Usage:
 *   wright-adapter --input source.opy --root <dir> [--main-file source.opy] [--output out.json]
 *
 * On success the HIR payload is written to `--output` (or stdout) and the
 * process exits 0. On a frontend parse failure or an unsupported construct a
 * structured error record is written to stderr and the process exits 1.
 */

import { basename } from "node:path";
import { run } from "../lib/main.js";

function usage() {
  return [
    "usage: wright-adapter --input <file.opy> --root <dir> [--main-file <name>] [--output <path>]",
    "",
    "options:",
    "  --input <path>      main .opy file to convert",
    "  --root <dir>        directory containing the main file (include base)",
    "  --main-file <name>  name of the main file (default: basename of --input)",
    "  --output <path>     write HIR JSON here instead of stdout",
    "  -h, --help          show this help",
  ].join("\n");
}

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(2);
}

function parseArgs(argv) {
  const args = { input: null, root: null, mainFile: null, output: null };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "-h" || arg === "--help") {
      process.stdout.write(`${usage()}\n`);
      process.exit(0);
    } else if (arg === "--input" || arg === "--root" || arg === "--main-file" || arg === "--output") {
      const value = argv[++i];
      if (value === undefined) {
        fail(`missing value for ${arg}`);
      }
      args[arg.slice(2).replaceAll("-", "_")] = value;
    } else {
      fail(`unknown argument: ${arg}\n\n${usage()}`);
    }
  }
  if (!args.input) {
    fail(`--input is required\n\n${usage()}`);
  }
  if (!args.root) {
    fail(`--root is required\n\n${usage()}`);
  }
  if (!args.mainFile) {
    args.mainFile = basename(args.input);
  }
  return args;
}

const args = parseArgs(process.argv.slice(2));
const exitCode = await run({
  input: args.input,
  root: args.root,
  mainFile: args.mainFile,
  output: args.output,
});
process.exit(exitCode);
