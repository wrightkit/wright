// TypeScript declarations for `wright-core.mjs`, the `@wrightkit/core`
// entry (#960). The `wright-agent/v1` types come from the committed
// schema via scripts/generate-core-types.ts.

import type {
  EditTransaction,
  EditValidationPayload,
  Sources,
  ToolRequest,
  ToolResponse,
} from './generated/wright-agent-v1.js'

// The `wright-agent/v1` contract types generated from the committed
// schema — typed requests in, typed responses out.
export * from './generated/wright-agent-v1.js'

/** The packaged `wright-core.wasm` next to this module. */
export declare const wasmUrl: URL

/**
 * Instantiate the module and create the core over `sourceText`, the
 * host's in-memory Workshop source. `name` gives the source its file
 * identity — kind detection and span paths treat it exactly like an
 * on-disk file of that name; relative names resolve against the virtual
 * root `/`.
 */
export declare function createCore(
  wasmBytes: Uint8Array | ArrayBuffer,
  sourceText: string,
  name?: string,
): Promise<WrightCore>

export declare class WrightCore {
  constructor(exports: WebAssembly.Exports, sourceText: string, name: string)

  /**
   * One `wright-agent/v1` request. `body` may be the request object or
   * its serialized form; the answer is the parsed `ToolService` response
   * body — `{result: …}` or `{error: {code, message}}`.
   */
  request(body: ToolRequest | string): ToolResponse

  /**
   * The host write path (ADR-0037 decision 3): the `EditTransaction` an
   * `editStatements` result carried, applied to `sources` — the host's
   * current in-memory text keyed by source name. The answer is the
   * `EditValidation` payload plus the authoritative `sources` map; a
   * refused apply leaves the map unchanged.
   */
  apply(
    transaction: EditTransaction | string,
    sources: Sources | string,
  ): EditValidationPayload & { sources: Sources }
}
