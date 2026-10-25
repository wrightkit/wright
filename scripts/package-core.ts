// Packages `@wrightkit/core` — the WebAssembly Wright core — for npm
// publication (#960, ADR-0037 decision 2). The package version is the
// `wright` release version; the shipped `wright-agent/v1` types are
// regenerated from the committed schemas here, and packing refuses to
// proceed when they differ from the committed generated file.
//
//   node scripts/package-core.ts \
//     --version 0.36.0 --commit <sha> [--tag v0.36.0] \
//     [--wasm target/wasm32-unknown-unknown/release/wright_wasm.wasm] \
//     [--out-dir dist]
//
// Produces dist/wrightkit-core-<version>.tgz plus its `.sha256` checksum
// and `.build.json` identity, matching the native archive contract.

import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import {
  copyFileSync,
  cpSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { generateCoreTypes } from './generate-core-types.ts'

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const jsDir = join(repoRoot, 'crates/wright-wasm/js')
const schemaFile = 'wright-agent-v1.schema.json'

function arg(name: string, fallback: string): string
function arg(name: string, fallback?: string): string | undefined
function arg(name: string, fallback?: string): string | undefined {
  const index = process.argv.indexOf(`--${name}`)
  return index === -1 ? fallback : process.argv[index + 1]
}

const version = arg('version')?.replace(/^v/, '')
const commit = arg('commit')
const tag = arg('tag', version ? `v${version}` : undefined)
const wasm = arg(
  'wasm',
  join(repoRoot, 'target/wasm32-unknown-unknown/release/wright_wasm.wasm'),
)
const outDir = resolve(arg('out-dir', join(repoRoot, 'dist')))

if (!version || !commit) {
  console.error('usage: package-core.ts --version X.Y.Z --commit <sha> [--tag <tag>]')
  process.exit(2)
}
if (!/^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`--version must be MAJOR.MINOR.PATCH(-prerelease)(+build), got '${version}'`)
  process.exit(2)
}

// The contract name is the schema file's identity:
// `wright-agent-v1.schema.json` → `wright-agent/v1`.
const contract = basename(schemaFile, '.schema.json').replace(/-([^-]+)$/, '/$1')

// Regenerate the shipped types from the committed schema and refuse to
// pack when the committed generated file has drifted from them.
const generated = await generateCoreTypes()
const committed = readFileSync(
  join(jsDir, 'generated/wright-agent-v1.d.ts'),
  'utf8',
)
if (committed !== generated) {
  console.error(
    'crates/wright-wasm/js/generated/wright-agent-v1.d.ts is stale;' +
      ' run `node scripts/generate-core-types.ts`',
  )
  process.exit(1)
}

const stage = join(repoRoot, 'target/core-package/package')
rmSync(join(repoRoot, 'target/core-package'), { recursive: true, force: true })
mkdirSync(join(stage, 'schemas'), { recursive: true })
mkdirSync(join(stage, 'generated'), { recursive: true })

copyFileSync(join(jsDir, 'wright-core.mjs'), join(stage, 'wright-core.mjs'))
copyFileSync(join(jsDir, 'wright-core.d.ts'), join(stage, 'wright-core.d.ts'))
writeFileSync(join(stage, 'generated/wright-agent-v1.d.ts'), generated)
copyFileSync(wasm, join(stage, 'wright-core.wasm'))
copyFileSync(
  join(repoRoot, 'schemas', schemaFile),
  join(stage, 'schemas', schemaFile),
)
copyFileSync(join(repoRoot, 'LICENSE'), join(stage, 'LICENSE'))

writeFileSync(
  join(stage, 'package.json'),
  `${JSON.stringify(
    {
      name: '@wrightkit/core',
      version,
      description:
        'WebAssembly build of the Wright core: wright-agent/v1 over in-memory Workshop text.',
      license: 'AGPL-3.0-or-later',
      type: 'module',
      repository: {
        type: 'git',
        url: 'https://github.com/wrightkit/wright.git',
        directory: 'crates/wright-wasm',
      },
      sideEffects: false,
      exports: {
        '.': { types: './wright-core.d.ts', default: './wright-core.mjs' },
        './wright-core.wasm': './wright-core.wasm',
        [`./schemas/${schemaFile}`]: `./schemas/${schemaFile}`,
        './package.json': './package.json',
      },
      publishConfig: { access: 'public' },
      agent_contract: contract,
    },
    null,
    2,
  )}\n`,
)

mkdirSync(outDir, { recursive: true })
const pack = spawnSync('npm', ['pack', '--pack-destination', outDir], {
  cwd: stage,
  stdio: 'inherit',
})
if (pack.status !== 0) process.exit(pack.status ?? 1)

const tarball = join(outDir, `wrightkit-core-${version}.tgz`)
const hash = createHash('sha256').update(readFileSync(tarball)).digest('hex')
writeFileSync(`${tarball}.sha256`, `${hash}  ${basename(tarball)}\n`)
const toolchain = readFileSync(join(repoRoot, 'rust-toolchain.toml'), 'utf8').match(
  /^channel\s*=\s*"([^"]+)"/m,
)?.[1]
if (!toolchain) {
  console.error('rust-toolchain.toml has no channel pin to record')
  process.exit(2)
}
writeFileSync(
  `${tarball}.build.json`,
  `${JSON.stringify({
    revision: commit,
    tag,
    version,
    agent_contract: contract,
    target: 'wasm32-unknown-unknown',
    toolchain,
    profile: 'release',
    packages: ['wright-wasm'],
  })}\n`,
)
console.log(tarball)
