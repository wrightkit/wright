/**
 * ADR-0036 decision 6: every runtime dependency in `package.json` appears
 * in `docs/ui/dependencies.md`. CI enforces the same invariant through
 * `scripts/check-deps.mjs`; this keeps it inside `npm test` so a missing
 * row fails before push.
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import process from 'node:process'
import { describe, expect, it } from 'vitest'

const root = process.cwd()
const doc = readFileSync(join(root, '..', 'docs', 'ui', 'dependencies.md'), 'utf8')
const { dependencies = {} } = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'))

const escapeRegExp = (name: string) => name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

describe('the runtime dependency registry (ADR-0036 decision 6)', () => {
  it('documents every package.json dependency in docs/ui/dependencies.md', () => {
    const names = Object.keys(dependencies)
    expect(names.length, 'package.json declares runtime dependencies').toBeGreaterThan(0)
    for (const name of names) {
      expect(
        new RegExp(`\\b${escapeRegExp(name)}\\b`, 'i').test(doc),
        `${name} is registered in docs/ui/dependencies.md`,
      ).toBe(true)
    }
  })
})
