// Smoke check for the packed `@wrightkit/core` tarball (#960 acceptance):
// installs it in a clean directory, type-checks a small TypeScript call
// to `request` against the shipped types, and runs it in Node.
//
//   node scripts/smoke-core-package.mjs --tarball dist/wrightkit-core-0.36.0.tgz

import { spawnSync } from 'node:child_process'
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const index = process.argv.indexOf('--tarball')
const tarball = index === -1 ? undefined : process.argv[index + 1]
if (!tarball) {
  console.error('usage: smoke-core-package.mjs --tarball <path>')
  process.exit(2)
}

// A clean consumer directory: only the packed tarball plus the pinned
// toolchain a TypeScript consumer carries.
const dir = join(repoRoot, 'target/core-smoke')
rmSync(dir, { recursive: true, force: true })
mkdirSync(dir, { recursive: true })
writeFileSync(
  join(dir, 'package.json'),
  `${JSON.stringify(
    {
      name: 'wrightkit-core-smoke',
      private: true,
      version: '0.0.0',
      type: 'module',
      devDependencies: {
        typescript: '~6.0.3',
        '@types/node': '^26.6.5',
      },
    },
    null,
    2,
  )}\n`,
)
writeFileSync(
  join(dir, 'tsconfig.json'),
  `${JSON.stringify(
    {
      compilerOptions: {
        strict: true,
        noEmit: true,
        module: 'nodenext',
        moduleResolution: 'nodenext',
        target: 'es2022',
        types: ['node'],
      },
      include: ['smoke.ts'],
    },
    null,
    2,
  )}\n`,
)

// The consumer's typed call: `request` takes a `ToolRequest` and returns
// a `ToolResponse`; `wasmUrl` locates the packaged core next to the
// installed entry. The source text is the consumer's own input.
const source = readFileSync(
  join(repoRoot, 'tests/fixtures/athena/practice-range.ow'),
  'utf8',
)
writeFileSync(
  join(dir, 'smoke.ts'),
  `import { createCore, wasmUrl } from '@wrightkit/core'
import type { CapabilitiesResult, ToolRequest, ToolResponse } from '@wrightkit/core'
import { readFileSync } from 'node:fs'

const core = await createCore(readFileSync(wasmUrl), ${JSON.stringify(source)}, 'practice-range.ow')
const request: ToolRequest = { op: 'capabilities' }
const response: ToolResponse = core.request(request)
if (!('result' in response)) throw new Error(\`request refused: \${JSON.stringify(response)}\`)
const capabilities = response.result as CapabilitiesResult
if (capabilities.agent_contract !== 'wright-agent/v1') {
  throw new Error(\`unexpected agent_contract: \${capabilities.agent_contract}\`)
}
console.log(\`smoke ok: \${capabilities.agent_contract} from \${capabilities.name} \${capabilities.version}\`)
`,
)

const run = (command, args) => {
  const result = spawnSync(command, args, { cwd: dir, stdio: 'inherit' })
  if (result.status !== 0) process.exit(result.status ?? 1)
}

run('npm', ['install', '--no-audit', '--no-fund', resolve(tarball)])
run('npm', ['exec', 'tsc', '--', '-p', 'tsconfig.json'])
run('node', ['smoke.ts'])
