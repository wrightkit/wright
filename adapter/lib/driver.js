/**
 * Frontend driver.
 *
 * Drives the pinned OverPy frontend (overpy@9.7.10) as a parser only: it
 * tokenizes and parses the source into the frontend's parsed AST and the
 * frontend compiler's symbol state (variables, subroutines, constants,
 * macros, initializers, preprocessor defines). It intentionally stops before
 * the frontend's optimization and Workshop lowering passes, which rewrite the
 * program into a target-oriented shape.
 *
 * All OverPy-internal knowledge lives inside this adapter package. The rest
 * of Wright never imports these types; it consumes Opy HIR v1 JSON.
 */

import overpy from "overpy";
import { AdapterError } from "./errors.js";

/**
 * Parse one program and return the frontend state the adapter needs.
 *
 * @param {object} options
 * @param {string} options.content The source text of the main file.
 * @param {string} options.rootPath Directory containing the main file (with a
 *   trailing slash), used to resolve `#!include`.
 * @param {string} options.mainFileName Name of the main file.
 * @returns {Promise<object>} `{ astRules, compiler }`
 */
export async function parse(options) {
  const { content, rootPath, mainFileName } = options;
  await overpy.readyPromise;

  const compiler = new overpy.OverPyCompiler();
  compiler.currentLanguage = "en-US";
  compiler.rootPath = rootPath;
  compiler.mainFileName = mainFileName;
  compiler.importedFiles.push(compiler.rootPath);
  compiler.fileStack = [
    {
      name: compiler.mainFileName || "<main>",
      path: compiler.rootPath + compiler.mainFileName,
      startLine: 1,
      startCol: 1,
      endCol: null,
      endLine: null,
      remainingChars: 99999999999,
      staticMember: true,
      fileStackMemberType: "normal",
    },
  ];
  compiler.macros = [];

  const lines = compiler.tokenize(content);
  const astRules = compiler.parseLines(lines);

  if (compiler.compiledCustomGameSettings !== "") {
    throw new AdapterError(
      "unsupported",
      "custom game settings blocks are outside the Opy HIR v1 corpus boundary"
    );
  }

  return { astRules, compiler };
}
