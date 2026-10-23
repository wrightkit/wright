import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import process from 'node:process'
import { describe, expect, it } from 'vitest'

const root = process.cwd()
const spec = readFileSync(join(root, '..', 'docs', 'ui', 'visual-system.md'), 'utf8')
const css = readFileSync(join(root, 'src', 'app.css'), 'utf8')

function specRows(): Map<string, number[]> {
  const section = spec.split('## Spacing and sizes')[1]
  expect(section, 'the spec keeps a Spacing and sizes section').toBeTruthy()
  const rows = new Map<string, number[]>()
  for (const line of section.split('\n')) {
    const cells = line.split('|').map((c) => c.trim())
    if (cells.length < 4 || cells[1] === '' || cells[1] === 'Element' || cells[1].startsWith('-')) {
      continue
    }
    const values = [...cells[2].matchAll(/(\d+(?:\s+\d+)*)\s*px/g)].flatMap((m) =>
      m[1].split(/\s+/).map(Number)
    )
    if (values.length > 0) rows.set(cells[1], values)
  }
  return rows
}

function rootProps(): Map<string, string> {
  const props = new Map<string, string>()
  const block = css.match(/:root\s*\{([\s\S]*?)\n\}/)?.[1]
  expect(block, ':root block parses').toBeTruthy()
  for (const m of block!.matchAll(/(--[a-z-]+)\s*:\s*([^;]+);/g)) {
    props.set(m[1], m[2].trim())
  }
  return props
}

const ROW_PROPS: Record<string, string[]> = {
  'Page padding': ['--page-pad'],
  Title: ['--title-size', '--title-track', '--file-name-size'],
  Toolbar: ['--toolbar-gap', '--toolbar-pad-below', '--toolbar-line'],
  'Rule list': ['--rule-list-gap', '--rule-list-below'],
  'Rule bar': [
    '--rule-bar-height',
    '--rule-bar-pad',
    '--rule-bar-gap',
    '--chev-size',
    '--chev-gap',
  ],
  'Rule body': ['--rule-body-indent', '--rule-body-pad'],
  'Event row': [
    '--event-row-height',
    '--event-label-pad',
    '--event-row-gap',
    '--event-rows-below',
  ],
  Columns: ['--cols-gap', '--col-head-size', '--col-head-below', '--col-head-line'],
  'Statement row': [
    '--stmt-min-height',
    '--stmt-pad-block',
    '--stmt-pad-inline',
    '--stmt-gap',
    '--stmt-text',
    '--stmt-indent',
  ],
  Dialog: [
    '--dialog-body-pad',
    '--dialog-title-size',
    '--dialog-foot-pad',
    '--dialog-foot-gap',
  ],
  'Editor form row': [
    '--form-ctl-height',
    '--form-label-gap',
    '--form-row-gap',
    '--form-nest-indent',
    '--form-nest-below',
    '--form-nest-line',
  ],
}

describe('spacing custom properties (#914)', () => {
  it('every spec-table row is covered and every px value appears in its properties', () => {
    const props = rootProps()
    const rows = specRows()
    expect(rows.size, 'the spec table parses all rows').toBe(11)
    for (const [row, expected] of rows) {
      const names = ROW_PROPS[row]
      expect(names, `spec row "${row}" has mapped properties`).toBeTruthy()
      const declared = names
        .map((name) => {
          const value = props.get(name)
          expect(value, `${name} is declared on :root for "${row}"`).toBeTruthy()
          return value!
        })
        .join(' ')
      const actual = [...declared.matchAll(/(\d+(?:\s+\d+)*)\s*px/g)].flatMap((m) =>
        m[1].split(/\s+/).map(Number)
      )
      expect(
        actual,
        `"${row}": ${names.join(', ')} carry the spec's ${expected.join(', ')} px`
      ).toEqual(expected)
    }
  })

  it('the covered selectors consume the properties, not literals or rems', () => {
    for (const [selector, needle] of [
      ['.page', 'padding: var(--page-pad)'],
      ['.title-row h1', 'font-size: var(--title-size)'],
      ['.file-name', 'font-size: var(--file-name-size)'],
      ['.chev', 'width: var(--chev-size)'],
      ['.rule-body', 'var(--rule-body-indent)'],
      ['.event-row', 'var(--event-row-gap)'],
      ['.event-label', 'var(--event-row-height)'],
      ['.event-label', 'var(--event-label-pad)'],
      ['.rule-cols', 'var(--cols-gap)'],
      ['.rule-view h3', 'var(--col-head-size)'],
      ['.statement', 'var(--stmt-min-height)'],
      ['.statement', 'var(--stmt-pad-block)'],
      ['.statement', 'var(--stmt-pad-inline)'],
      ['.statement', 'var(--stmt-indent)'],
      ['.statement-text', 'var(--stmt-text)'],
      ['.dlg-body', 'var(--dialog-body-pad)'],
      ['.dlg-foot', 'var(--dialog-foot-pad)'],
    ] as const) {
      const rule = css.match(
        new RegExp(`${selector.replaceAll('.', '\\.')}\\b\\s*\\{([^}]*)\\}`)
      )
      expect(rule, `${selector} rule exists`).toBeTruthy()
      expect(rule![1], `${selector} consumes ${needle}`).toContain(needle)
    }
    for (const selector of [
      '.page',
      '.title-row h1',
      '.toolbar',
      '.rule-list',
      '.rule-bar',
      '.bar',
      '.chev',
      '.rule-body',
      '.event-row',
      '.event-label',
      '.event-value',
      '.rule-cols',
      '.rule-view h3',
      '.statement',
      '.dialog h2',
      '.dlg-title',
      '.dlg-body',
      '.dlg-foot',
      '.editor-arg',
    ]) {
      const rule = css.match(
        new RegExp(`${selector.replaceAll('.', '\\.')}\\b\\s*\\{([^}]*)\\}`)
      )
      expect(rule, `${selector} rule exists`).toBeTruthy()
      expect(rule![1], `${selector} holds no rem multiple`).not.toMatch(/\drem/)
    }
  })
})
