
import { readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const uiDir = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const doc = readFileSync(join(uiDir, '..', 'docs', 'ui', 'dependencies.md'), 'utf8')
const { dependencies = {} } = JSON.parse(readFileSync(join(uiDir, 'package.json'), 'utf8'))

const escapeRegExp = (name) => name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

const missing = Object.keys(dependencies).filter(
  (name) => !new RegExp(`(?<![\\w@/.-])${escapeRegExp(name)}(?![\\w/.-])`, 'i').test(doc),
)

if (missing.length > 0) {
  console.error(
    `docs/ui/dependencies.md is missing runtime dependencies: ${missing.join(', ')}\n` +
      'Document each entry in the same change that adds it (ADR-0036 decision 6).',
  )
  process.exit(1)
}
console.log('all runtime dependencies are documented in docs/ui/dependencies.md')
