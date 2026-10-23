// The JavaScript-callable entry to the WebAssembly Wright core (#959,
// ADR-0037 decision 2): `wright-agent/v1` request bodies in, the
// `ToolService` response bodies out. The host owns the source text; the
// core owns the session. No bundler, no wasm-bindgen — five exports over
// a `(ptr, len)` UTF-8 ABI.
//
//   import { createCore } from './wright-core.mjs'
//   const core = await createCore(wasmBytes, sourceText, 'main.ws')
//   const project = core.request({ op: 'project' })
//   const edits = core.request({ op: 'editStatements', inputIdentity, edits })
//   const applied = core.apply(edits.result.transaction, sources)

const encoder = new TextEncoder()
const decoder = new TextDecoder()

// The packaged `wright-core.wasm` next to this module — the asset URL
// a Node or bundler host reads the core bytes from (#960).
export const wasmUrl = new URL('./wright-core.wasm', import.meta.url)

/**
 * Instantiate the module and create the core over `sourceText`, the
 * host's in-memory Workshop source. `name` gives the source its file
 * identity — kind detection and span paths treat it exactly like an
 * on-disk file of that name; relative names resolve against the virtual
 * root `/`.
 */
export async function createCore(wasmBytes, sourceText, name = 'project.ws') {
  const { instance } = await WebAssembly.instantiate(wasmBytes, {})
  return new WrightCore(instance.exports, sourceText, name)
}

export class WrightCore {
  #exports

  constructor(exports, sourceText, name) {
    this.#exports = exports
    const init = this.#call(
      (source, sourceLength, sourceName, nameLength) =>
        exports.wright_new(source, sourceLength, sourceName, nameLength),
      sourceText,
      name,
    )
    if (init.error) {
      throw new Error(`wright core init refused: ${init.error.message}`)
    }
  }

  /**
   * One `wright-agent/v1` request. `body` may be the request object or
   * its serialized form; the answer is the parsed `ToolService` response
   * body — `{result: …}` or `{error: {code, message}}`.
   */
  request(body) {
    return this.#call((ptr, len) => this.#exports.wright_request(ptr, len), this.#body(body))
  }

  /**
   * The host write path (ADR-0037 decision 3): the `EditTransaction` an
   * `editStatements` result carried, applied to `sources` — the host's
   * current in-memory text keyed by source name. The answer is the
   * `EditValidation` payload plus the authoritative `sources` map; a
   * refused apply leaves the map unchanged.
   */
  apply(transaction, sources) {
    return this.#call(
      (transactionPtr, transactionLength, sourcesPtr, sourcesLength) =>
        this.#exports.wright_apply(
          transactionPtr,
          transactionLength,
          sourcesPtr,
          sourcesLength,
        ),
      this.#body(transaction),
      this.#body(sources),
    )
  }

  #body(value) {
    return typeof value === 'string' ? value : JSON.stringify(value)
  }

  // Write every argument into wasm memory, invoke `entry` with the
  // (ptr, len) pairs, free the arguments, and return the decoded,
  // freed response body.
  #call(entry, ...texts) {
    const allocations = texts.map((text) => {
      const bytes = encoder.encode(text)
      const ptr = this.#exports.wright_alloc(bytes.length)
      new Uint8Array(this.#exports.memory.buffer, ptr, bytes.length).set(bytes)
      return [ptr, bytes.length]
    })
    const packed = entry(...allocations.flat())
    for (const [ptr, len] of allocations) {
      this.#exports.wright_dealloc(ptr, len)
    }
    const ptr = Number(packed >> 32n)
    const len = Number(packed & 0xffffffffn)
    const body = decoder.decode(new Uint8Array(this.#exports.memory.buffer, ptr, len))
    this.#exports.wright_dealloc(ptr, len)
    return JSON.parse(body)
  }
}
