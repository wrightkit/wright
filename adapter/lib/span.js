/**
 * Span and file-registry helpers for Opy HIR v1.
 *
 * Every source-originated HIR node carries a span referencing the `files`
 * registry (see docs/hir/opy-hir-v1.md §3). This module owns the mapping from
 * a frontend `fileStack` entry to a HIR span, including the 1-based,
 * half-open interval normalization and the file-id table.
 */

/**
 * @typedef {object} HirSpan
 * @property {number} file
 * @property {{ line: number, col: number }} start
 * @property {{ line: number, col: number }} end
 */

/**
 * Builds HIR spans and the `files` registry for one program.
 */
export class SpanBuilder {
  constructor() {
    /** @type {string[]} file names in first-encounter order */
    this._names = [];
    /** @type {Map<string, number>} file name -> registry id */
    this._ids = new Map();
  }

  /**
   * Resolve a frontend `fileStack[0]` entry to a HIR span, registering the
   * file as needed.
   *
   * @param {object | null | undefined} member
   * @returns {HirSpan | null}
   */
  fromFileStackMember(member) {
    if (!member || typeof member.name !== "string") {
      return null;
    }
    const startLine = Number.isInteger(member.startLine) ? member.startLine : 1;
    const startCol = Number.isInteger(member.startCol) ? member.startCol : 1;
    // The frontend reports inclusive end columns; the protocol uses an
    // exclusive end. Fall back to one-past-start when no end is available.
    const endLine = Number.isInteger(member.endLine) ? member.endLine : startLine;
    const endCol = Number.isInteger(member.endCol) ? member.endCol + 1 : startCol + 1;
    return {
      file: this.fileId(member.name),
      start: { line: startLine, col: startCol },
      end: { line: endLine, col: endCol },
    };
  }

  /**
   * Register a file name and return its id. Ids are assigned in first-call
   * order.
   *
   * @param {string} name
   * @returns {number}
   */
  fileId(name) {
    let id = this._ids.get(name);
    if (id === undefined) {
      id = this._names.length;
      this._names.push(name);
      this._ids.set(name, id);
    }
    return id;
  }

  /**
   * The `files` array for the protocol envelope, in registry order.
   *
   * @returns {{ id: number, path: string }[]}
   */
  files() {
    return this._names.map((name, id) => ({ id, path: name }));
  }
}
