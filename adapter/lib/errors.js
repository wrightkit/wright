/**
 * Structured adapter errors.
 *
 * The adapter fails loudly: a parse failure from the frontend, or a construct
 * the adapter cannot map to Opy HIR v1, produces an `AdapterError` with a
 * stable code, a message, and an optional span. These are reported as JSON on
 * stderr and as a non-zero exit code, so tests and later tooling can treat
 * unsupported input as explicit rather than silently degraded.
 */

export class AdapterError extends Error {
  /** @type {"parse" | "unsupported" | "io"} */
  code;
  /** @type {object | null} */
  span;

  /**
   * @param {"parse" | "unsupported" | "io"} code
   * @param {string} message
   * @param {object | null} [span]
   */
  constructor(code, message, span = null) {
    super(message);
    this.name = "AdapterError";
    this.code = code;
    this.span = span;
  }

  /** Render the error as the stable JSON record written to stderr. */
  toJSON() {
    const record = { code: this.code, message: this.message };
    if (this.span) {
      record.span = this.span;
    }
    return record;
  }
}

/**
 * Build an `AdapterError` for a construct the adapter cannot map.
 *
 * @param {string} message
 * @param {object | null} [span] A built HIR span (see lib/span.js).
 * @returns {AdapterError}
 */
export function unsupported(message, span) {
  return new AdapterError("unsupported", message, span ?? null);
}

/**
 * Build an `AdapterError` for a frontend parse failure, preserving the
 * frontend's source location when it reported one.
 *
 * @param {Error} error
 * @returns {AdapterError}
 */
export function parseFailure(error) {
  const fileStack = error && error.fileStack && error.fileStack[0];
  const span =
    fileStack && fileStack.name
      ? {
          file: fileStack.name, // source file name; error records are adapter diagnostics, not HIR payloads
          start: { line: fileStack.startLine ?? 1, col: fileStack.startCol ?? 1 },
          end: { line: fileStack.endLine ?? fileStack.startLine ?? 1, col: (fileStack.endCol ?? fileStack.startCol ?? 1) + 1 },
        }
      : null;
  return new AdapterError("parse", error.message || String(error), span);
}
