// The wasm-core acceptance test (#959): drives the WebAssembly module
// and the native parity driver through identical wright-agent/v1
// request sequences and asserts the response bodies agree — plus the
// ADR-0037 apply round-trip, stale-transaction refusal, capability
// refusals, malformed-input handling, and the gzipped transfer-size
// budget.
//
//   node crates/wright-wasm/js/wright-core.test.mjs
//
// Env overrides:
//   WRIGHT_WASM              wasm artifact (default release build)
//   WRIGHT_PARITY            native parity binary (default release)
//   WRIGHT_WASM_BUDGET_KIB   gzipped-size budget in KiB (default 2048 —
//                            the release artifact measures ~1407 KiB)

import { spawn } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { createInterface } from 'node:readline'
import { gzipSync } from 'node:zlib'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import assert from 'node:assert/strict'
import { createCore } from './wright-core.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..')
const WASM = path.resolve(process.env.WRIGHT_WASM
  ?? path.join(root, 'target/wasm32-unknown-unknown/release/wright_wasm.wasm'))
const PARITY = path.resolve(process.env.WRIGHT_PARITY
  ?? path.join(root, 'target/release/wright-core-parity'))
const BUDGET_KIB = Number(process.env.WRIGHT_WASM_BUDGET_KIB ?? 2048)

const NAME = 'practice-range.ow'
const SOURCE = readFileSync(path.join(root, 'tests/fixtures/athena/practice-range.ow'), 'utf8')
const SCENARIO = JSON.parse(
  readFileSync(path.join(root, 'tests/fixtures/athena/scenarios/practice-range.json'), 'utf8'),
)

let passed = 0
const check = (label, fn) => {
  try {
    fn()
    passed += 1
    console.log(`ok   ${label}`)
  } catch (error) {
    console.error(`FAIL ${label}\n     ${error.message}`)
    process.exitCode = 1
  }
}

// The native side resolves its Text input against the process cwd, and
// its `same_source` match uses filesystem canonicalization — so the
// fixture text is staged under target/ (gitignored) and the driver runs
// there. The wasm build's virtual root `/` produces the same display,
// span, and identity fields.
const STAGE = path.join(root, 'target/wasm-parity-src')
mkdirSync(STAGE, { recursive: true })
writeFileSync(path.join(STAGE, NAME), SOURCE)

/** The native parity driver: one JSONL envelope in, one body out. */
class NativeCore {
  constructor(bin, sourceFile, name, cwd) {
    this.proc = spawn(bin, [sourceFile, name], { cwd, stdio: ['pipe', 'pipe', 'inherit'] })
    this.rl = createInterface({ input: this.proc.stdout })
    this.queue = []
    this.waiting = []
    this.rl.on('line', (line) => {
      const resolve = this.waiting.shift()
      if (resolve) resolve(JSON.parse(line))
      else this.queue.push(JSON.parse(line))
    })
  }
  send(envelope) {
    const line = new Promise((resolve) => {
      if (this.queue.length) resolve(this.queue.shift())
      else this.waiting.push(resolve)
    })
    this.proc.stdin.write(JSON.stringify(envelope) + '\n')
    return line
  }
  request(body) { return this.send({ request: body }) }
  apply(transaction, sources) { return this.send({ apply: { transaction, sources } }) }
  close() { this.proc.stdin.end(); this.proc.kill() }
}

// ---------------------------------------------------------------- //
// Section 1 — response-body parity over identical request sequences  //
// ---------------------------------------------------------------- //

const native = new NativeCore(PARITY, path.join(STAGE, NAME), NAME, STAGE)
const wasm = await createCore(readFileSync(WASM), SOURCE, NAME)

const parity = async (label, request) => {
  const [n, w] = await Promise.all([native.request(request), Promise.resolve(wasm.request(request))])
  check(label, () => assert.deepStrictEqual(w, n))
  return w
}

const project = await parity('project', { op: 'project' })
const rulesBody = await parity('rules', { op: 'rules' })
const generation = rulesBody.result.snapshot
await parity('symbols', { op: 'symbols' })
await parity('findings', { op: 'findings' })
await parity('costEstimate', { op: 'costEstimate' })
await parity('targetMetadata', { op: 'targetMetadata' })
await parity('lookup (workshop)', { op: 'lookup', language: 'workshop', query: 'wait' })
await parity('listEntries (actions)', { op: 'listEntries', language: 'workshop', kind: 'action', pageSize: 25 })
// `lookup` for `opy` goes through the LPP provider transport on native —
// a host without one returns `unavailable` there; the wasm build answers
// the same structured refusal naming the owner, never silently
// substitutes (ADR-0037 decision 5). Asserted as a wasm refusal, not
// byte-parity, because a host *with* a provider configured answers
// entries natively.
check('lookup (opy) → unavailable naming opy-rs', () => {
  const response = wasm.request({ op: 'lookup', language: 'opy', query: 'wait' })
  assert.equal(response.result?.owner, 'opy-rs')
  assert.equal(response.result?.unavailable?.capability, 'lookup')
})
await parity('capabilities', { op: 'capabilities' })

// simulate — bare seconds and the practice-range scenario fixture
const identity = project.result.inputIdentity
await parity('simulate (seconds)', { op: 'simulate', inputIdentity: identity, seconds: 1 })
await parity('simulate (scenario)', { op: 'simulate', inputIdentity: identity, scenario: SCENARIO })

// ---------------------------------------------------------------- //
// Section 2 — ADR-0037 host write path: editStatements → apply →     //
// rules, then the stale-transaction refusal                         //
// ---------------------------------------------------------------- //

const edits = {
  op: 'editStatements',
  inputIdentity: identity,
  snapshot: generation,
  edits: [{ rule: 0, list: 'actions', index: 0, change: 'delete' }],
}
const produced = await parity('editStatements (delete)', edits)
check('editStatements produced a transaction', () => assert.ok(produced.result.transaction))

const sources = { [NAME]: SOURCE }
const [appliedN, appliedW] = await Promise.all([
  native.apply(produced.result.transaction, { ...sources }),
  Promise.resolve(wasm.apply(produced.result.transaction, { ...sources })),
])
check('apply round-trip body', () => assert.deepStrictEqual(appliedW, appliedN))
check('apply accepted', () => assert.equal(appliedW.ok, true))
check('apply rewrote the held text', () =>
  assert.notEqual(appliedW.sources[NAME], SOURCE))

const after = await parity('rules after apply', { op: 'rules' })
check('edited program served', () => {
  const rule = after.result.rules.find((r) => r.id === 0 || r.address?.includes('addScore'))
  assert.ok(rule, 'rule 0 still listed')
})

// The same transaction applied again is stale: the recheck fails and
// nothing changes.
const sourcesBefore = JSON.stringify(appliedW.sources)
const [staleN, staleW] = await Promise.all([
  native.apply(produced.result.transaction, { ...appliedW.sources }),
  Promise.resolve(wasm.apply(produced.result.transaction, { ...appliedW.sources })),
])
check('stale apply body', () => assert.deepStrictEqual(staleW, staleN))
check('stale apply refused', () => {
  assert.equal(staleW.ok, false)
  assert.ok(
    staleW.diagnostics.some((d) => d.code === 'edit-stale-source'),
    `expected edit-stale-source, got ${JSON.stringify(staleW.diagnostics)}`)
})
check('stale apply left sources unchanged', () =>
  assert.equal(JSON.stringify(staleW.sources), sourcesBefore))

// ---------------------------------------------------------------- //
// Section 3 — wasm refusals and malformed inputs                    //
// ---------------------------------------------------------------- //

const refuse = (label, body, code) => check(label, () => {
  const response = wasm.request(body)
  assert.equal(response.error?.code, code, JSON.stringify(response).slice(0, 200))
})

refuse('providerValidateEdit → LPP refusal', {
  op: 'providerValidateEdit', language_id: 'opy', transaction: { edits: [] },
}, 'source-provider-unavailable')
refuse('compile → no output file', { op: 'compile' }, 'output-unavailable')
check('malformed JSON → malformed-request', () => {
  const response = wasm.request('this is not json')
  assert.equal(response.error?.code, 'malformed-request')
})
refuse('unknown op → malformed-request', { op: 'bogusOp' }, 'malformed-request')
check('lookup (ostw) → invalid-language', () => {
  const response = wasm.request({ op: 'lookup', language: 'ostw', query: 'wait' })
  assert.equal(response.error?.code, 'invalid-language')
})

// OPY / DEL(OSTW) in-memory input: structured refusal, no panic.
for (const name of ['main.opy', 'project.ostw']) {
  const c = await createCore(readFileSync(WASM), SOURCE, name)
  check(`${name} input → source-provider-unavailable`, () => {
    const response = c.request({ op: 'project' })
    assert.equal(response.error?.code, 'source-provider-unavailable')
  })
}

// ---------------------------------------------------------------- //
// Section 4 — gzipped transfer-size budget                          //
// ---------------------------------------------------------------- //

const wasmBytes = readFileSync(WASM)
const gz = gzipSync(wasmBytes, { level: 9 }).length
const kib = gz / 1024
console.log(`wasm artifact: ${wasmBytes.length} B; gzipped: ${gz} B (${kib.toFixed(1)} KiB); budget: ${BUDGET_KIB} KiB`)
check('gzipped size within budget', () => assert.ok(kib <= BUDGET_KIB))

native.close()
console.log(`\n${passed} checks passed${process.exitCode ? ' (with failures)' : ''}`)
