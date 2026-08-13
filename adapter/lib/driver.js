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
 * Extract the custom-game-settings block from the main source text, mirroring
 * the native frontend's `find_blocks` (m11-settings-design-constraints §5):
 * a logical-line scan that skips blank lines and `#`/block-comment lines,
 * requiring the block to be the first construct, to open with `{` on the
 * keyword line, and to be the only block; the block region is found by brace
 * matching that respects quoted strings and nesting. OverPy consumes the
 * settings inside `parseLines` without pushing a node into the rule list, so
 * the adapter must extract the block up front to map it onto the Opy HIR v1
 * settings node.
 *
 * Positions are 1-based and the block `end` is exclusive (one past the
 * closing brace), matching the HIR span convention.
 *
 * @param {string} content Source text of the main file.
 * @param {string} mainFileName Name of the main file (span file registry).
 * @returns {object | null} `{ file, text, keyword, open, end }` where `text`
 *   is the raw JSONC between the braces, or `null` when no block exists.
 */
export function extractSettingsBlock(content, mainFileName) {
  const lineStarts = [0];
  for (let i = 0; i < content.length; i++) {
    if (content[i] === "\n") {
      lineStarts.push(i + 1);
    }
  }
  const positionAt = (offset) => {
    let low = 0;
    let high = lineStarts.length - 1;
    while (low < high) {
      const mid = (low + high + 1) >> 1;
      if (lineStarts[mid] <= offset) {
        low = mid;
      } else {
        high = mid - 1;
      }
    }
    return { line: low + 1, col: offset - lineStarts[low] + 1 };
  };

  const lineOf = (offset) => {
    let low = 0;
    let high = lineStarts.length - 1;
    while (low < high) {
      const mid = (low + high + 1) >> 1;
      if (lineStarts[mid] <= offset) {
        low = mid;
      } else {
        high = mid - 1;
      }
    }
    return low;
  };

  // Phase 1: find the first non-comment construct line and the first
  // `settings` keyword line anywhere in the file.
  let firstConstruct = null;
  let settingsOffset = null;
  let inBlockComment = false;
  for (let i = 0; i < lineStarts.length; i++) {
    let offset = lineStarts[i];
    let line = content.slice(offset, i + 1 < lineStarts.length ? lineStarts[i + 1] - 1 : content.length);
    let scanned = 0;
    for (;;) {
      if (inBlockComment) {
        const close = line.indexOf("*/", scanned);
        if (close === -1) {
          break;
        }
        inBlockComment = false;
        scanned = close + 2;
      }
      const rest = line.slice(scanned).trimStart();
      if (rest === "") {
        break;
      }
      if (rest.startsWith("#")) {
        break; // comment or directive line
      }
      if (rest.startsWith("/*")) {
        const close = line.indexOf("*/", scanned + 2);
        if (close === -1) {
          inBlockComment = true;
          break;
        }
        scanned = close + 2;
        continue;
      }
      const tokenOffset = offset + scanned + line.slice(scanned).length - rest.length;
      const token = rest.split(/\s+/)[0];
      if (firstConstruct === null) {
        firstConstruct = { token, offset: tokenOffset };
      }
      if (token === "settings") {
        settingsOffset = tokenOffset;
      }
      break;
    }
    if (settingsOffset !== null) {
      break;
    }
  }

  if (settingsOffset === null) {
    return null;
  }
  if (firstConstruct === null || firstConstruct.token !== "settings") {
    throw settingsError(
      "settings block must be the first construct in the file",
      positionAt(settingsOffset),
      positionAt(settingsOffset + "settings".length),
      mainFileName,
    );
  }

  // Phase 2: the block must open with `{` on the keyword line.
  let cursor = settingsOffset + "settings".length;
  while (cursor < content.length && /\s/.test(content[cursor])) {
    cursor++;
  }
  if (content[cursor] !== "{") {
    throw settingsError(
      "settings block must open with '{'",
      positionAt(cursor),
      positionAt(cursor + 1),
      mainFileName,
    );
  }
  const openOffset = cursor;

  // Phase 3: brace match to the closing `}`, respecting quoted strings.
  let depth = 0;
  cursor = openOffset;
  while (cursor < content.length) {
    const char = content[cursor];
    if (char === '"' || char === "'") {
      const quote = char;
      cursor++;
      let closed = false;
      while (cursor < content.length) {
        const inner = content[cursor];
        if (inner === "\\") {
          cursor += 2;
          continue;
        }
        if (inner === quote) {
          cursor++;
          closed = true;
          break;
        }
        if (inner === "\n") {
          break;
        }
        cursor++;
      }
      if (!closed) {
        throw settingsError(
          "unterminated string in settings block",
          positionAt(cursor - 1),
          positionAt(cursor),
          mainFileName,
        );
      }
      continue;
    }
    if (char === "{") {
      depth++;
    } else if (char === "}") {
      depth--;
      if (depth === 0) {
        break;
      }
    }
    cursor++;
  }
  if (depth !== 0) {
    throw settingsError(
      "unterminated settings block",
      positionAt(settingsOffset),
      positionAt(settingsOffset + "settings".length),
      mainFileName,
    );
  }
  const closeOffset = cursor;
  const blockEnd = closeOffset + 1;

  // Phase 4: no second settings block may follow.
  for (let i = lineOf(blockEnd); i < lineStarts.length; i++) {
    const start = lineStarts[i];
    const line = content.slice(start, i + 1 < lineStarts.length ? lineStarts[i + 1] - 1 : content.length);
    let scanned = 0;
    let lineInBlockComment = inBlockComment;
    for (;;) {
      if (lineInBlockComment) {
        const close = line.indexOf("*/", scanned);
        if (close === -1) {
          break;
        }
        lineInBlockComment = false;
        scanned = close + 2;
      }
      const rest = line.slice(scanned).trimStart();
      if (rest === "") {
        break;
      }
      if (rest.startsWith("#")) {
        break;
      }
      if (rest.startsWith("/*")) {
        const close = line.indexOf("*/", scanned + 2);
        if (close === -1) {
          lineInBlockComment = true;
          break;
        }
        scanned = close + 2;
        continue;
      }
      if (rest.split(/\s+/)[0] === "settings") {
        const tokenOffset = start + scanned + line.slice(scanned).length - rest.length;
        throw settingsError(
          "custom game settings have already been declared",
          positionAt(tokenOffset),
          positionAt(tokenOffset + "settings".length),
          mainFileName,
        );
      }
      break;
    }
  }

  return {
    file: mainFileName,
    text: content.slice(openOffset + 1, closeOffset),
    keyword: positionAt(settingsOffset),
    open: positionAt(openOffset),
    end: positionAt(blockEnd),
  };
}

function settingsError(message, start, end, fileName) {
  return new AdapterError("parse", message, { file: fileName, start, end });
}

/**
 * Parse one program and return the frontend state the adapter needs.
 *
 * @param {object} options
 * @param {string} options.content The source text of the main file.
 * @param {string} options.rootPath Directory containing the main file (with a
 *   trailing slash), used to resolve `#!include`.
 * @param {string} options.mainFileName Name of the main file.
 * @returns {Promise<object>} `{ astRules, compiler, settings }`
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

  const settings = extractSettingsBlock(content, mainFileName);

  const lines = compiler.tokenize(content);
  const astRules = compiler.parseLines(lines);

  return { astRules, compiler, settings };
}
