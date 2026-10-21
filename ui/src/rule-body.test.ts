// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The #881 expanded-rule body (docs/ui/design.md § Main screen item 4 and
 * § Statement text): the event rows, the two statement columns, the
 * indented `--bar` statement rows with hidden `End` markers, the spec's
 * syntax-token colours, the `?debug` line numbers, and the in-place
 * refusal that lets the next rule still open.
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import process from 'node:process'
import { flushSync, mount, unmount } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App.svelte'
import type { RuleContent } from './lib/view'
import type { RuleSummary, ToolRequest, ToolResponse } from './lib/types/generated/wright-agent-v1'

/** The api module the components call; each test installs its own handler. */
const api = vi.hoisted(() => ({
  handler: null as null | ((request: ToolRequest) => Promise<ToolResponse>),
}))
vi.mock('./lib/api', () => ({
  sessionToken: 'test-token',
  call: (request: ToolRequest) => {
    if (!api.handler) return Promise.reject(new Error('test installed no api handler'))
    return api.handler(request)
  },
}))

/** Drain pending promise continuations, then flush Svelte's effects and DOM. */
async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flushSync()
  await new Promise((resolve) => setTimeout(resolve, 0))
  flushSync()
}

const action = (
  call: string,
  args: unknown[] = [],
  extra: Record<string, unknown> = {},
): RuleContent['actions'][number] =>
  ({ call, args, span: { start: { line: 5, col: 1 }, end: { line: 5, col: 9 } }, ...extra }) as RuleContent['actions'][number]

const contentFor = (
  event: RuleContent['event'],
  conditions: RuleContent['conditions'] = [],
  actions: RuleContent['actions'] = [],
): RuleContent => ({
  format: 'workshop-rs/rule-content-v1',
  event,
  disabled: false,
  conditions,
  actions,
})

// `content` arrives as the schema's loose `{[k:string]: unknown}` shape, so
// tests build summaries through a record and cast like the loader does.
const summaryFor = (id: number, name: string, content?: RuleContent): RuleSummary =>
  ({ id, name, span: null, ...(content ? { content } : {}) }) as unknown as RuleSummary

/** `lookup` names the tests resolve, keyed by identity. */
const NAMES: Record<string, { kind: string; name: string }[]> = {
  eachPlayer: [{ kind: 'event', name: 'Ongoing - Each Player' }],
  'EventTeam.TEAM_1': [{ kind: 'enumMember', name: 'Team 1' }],
  'Hero.ANA': [{ kind: 'enumMember', name: 'Ana' }],
  subroutine: [{ kind: 'event', name: 'Subroutine' }],
  global: [
    { kind: 'event', name: 'Ongoing - Global' },
    { kind: 'structural', name: 'Global' },
  ],
  eventPlayer: [{ kind: 'value', name: 'Event Player' }],
  if: [{ kind: 'structural', name: 'If' }],
  else: [{ kind: 'structural', name: 'Else' }],
  while: [{ kind: 'structural', name: 'While' }],
  end: [{ kind: 'structural', name: 'End' }],
  wait: [{ kind: 'action', name: 'Wait' }],
  isAlive: [{ kind: 'value', name: 'Is Alive' }],
  setGlobalVariable: [{ kind: 'structural', name: 'Set Global Variable' }],
  setPlayerVariable: [{ kind: 'structural', name: 'Set Player Variable' }],
  '==': [{ kind: 'operator', name: '==' }],
  add: [{ kind: 'operator', name: 'Add' }],
}

const lookupResponse = (query: string): ToolResponse =>
  ({
    result: {
      language: 'workshop',
      owner: 'workshop-rs',
      entries: (NAMES[query] ?? []).map((entry) => ({
        identity: query,
        kind: entry.kind,
        spelling: query,
        displayName: entry.name,
      })),
    },
  }) as ToolResponse

/** `rules`+`project`+`lookup` over the given summaries (bulk content path). */
const handlerFor =
  (summaries: RuleSummary[]) =>
  (request: ToolRequest): Promise<ToolResponse> => {
    if (request.op === 'project') {
      return Promise.resolve({ result: { sourceFiles: ['demo.ws'] } } as ToolResponse)
    }
    if (request.op === 'rules' && request.all === true && request.content === true) {
      return Promise.resolve({ result: { rules: summaries } } as ToolResponse)
    }
    if (request.op === 'lookup' && typeof request.query === 'string') {
      return Promise.resolve(lookupResponse(request.query))
    }
    return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
  }

let host: HTMLDivElement
let app: ReturnType<typeof mount> | undefined

afterEach(() => {
  if (app) void unmount(app)
  app = undefined
  api.handler = null
  document.body.innerHTML = ''
  window.history.replaceState({}, '', '/')
})

function mountApp(): void {
  host = document.createElement('div')
  document.body.appendChild(host)
  app = mount(App, { target: host })
}

const slot = (id: number) => host.querySelector(`.rule-slot[data-rule-id="${id}"]`)
const barOf = (id: number) => slot(id)!.querySelector<HTMLButtonElement>('.bar')!

const click = (el: HTMLElement) => {
  el.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  flushSync()
}

const openRule = async (id: number) => {
  click(barOf(id))
  await settle()
}

const rowsIn = (root: Element, list: 'conditions' | 'actions') =>
  [...root.querySelectorAll<HTMLLIElement>(`.col[data-list="${list}"] .statement`)]

describe('the statement-text colour table (#881)', () => {
  const css = readFileSync(join(process.cwd(), 'src', 'app.css'), 'utf8')
  const root = css.match(/:root\s*\{([^}]*)\}/)?.[1] ?? ''

  // docs/ui/design.md § Statement text: every spec class resolves through
  // a `--tok-*` custom property holding its spec colour.
  it.each([
    ['action', '#ffffff'],
    ['value', '#8fd0ff'],
    ['enum', '#f2c86b'],
    ['number', '#b7e28a'],
    ['string', '#f6a77c'],
    ['variable', '#d1a6ff'],
    ['operator', '#ff9e64'],
    ['literal', '#6fe0e6'],
    ['punctuation', '#a9b0c2'],
  ])('maps the .tok.%s class to its spec token', (cls, colour) => {
    expect(root, `--tok-${cls}`).toContain(`--tok-${cls}: ${colour}`)
    const rule = css.match(new RegExp(`\\.tok\\.${cls}\\s*\\{([^}]*)\\}`))?.[1] ?? ''
    expect(rule, `.tok.${cls}`).toContain(`var(--tok-${cls})`)
  })

  it('takes no literal colours in the components', () => {
    for (const file of [
      'src/App.svelte',
      'src/lib/Expr.svelte',
      'src/lib/RuleView.svelte',
      'src/lib/RuleBar.svelte',
      'src/lib/SqButton.svelte',
      'src/lib/StatementEditor.svelte',
      'src/lib/PendingChanges.svelte',
      'src/lib/Dialogs.svelte',
    ]) {
      const source = readFileSync(join(process.cwd(), file), 'utf8').replace(
        /\/\*[\s\S]*?\*\//g,
        '',
      )
      // A colour literal is a 6- or 8-digit hex; `#123` issue refs are not.
      expect(source, file).not.toMatch(/#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{8}\b/)
    }
  })
})

describe('the event rows (#881)', () => {
  it('renders Event, Team, and Player rows with lookup names when the event carries filters', async () => {
    const content = contentFor({ id: 'eachPlayer', team: 'TEAM_1', player: 'ANA' })
    api.handler = handlerFor([summaryFor(1, 'damage track', content)])

    mountApp()
    await settle()
    await openRule(1)

    const rows = [...slot(1)!.querySelectorAll<HTMLElement>('.event-row')]
    expect(rows).toHaveLength(3)
    expect(rows.map((row) => row.querySelector('.event-label')?.textContent)).toEqual([
      'Event',
      'Team',
      'Player',
    ])
    expect(rows.map((row) => row.querySelector('.event-value')?.textContent)).toEqual([
      'Ongoing - Each Player',
      'Team 1',
      'Ana',
    ])
    // Each white value cell carries the dropdown caret affordance.
    for (const row of rows) {
      expect(row.querySelector('.event-value svg')).not.toBeNull()
    }
  })

  it('renders only Event for a bare global event and adds a Subroutine row for a subroutine rule', async () => {
    const globalRule = contentFor({ id: 'global' })
    const subRule = contentFor({ id: 'subroutine', subroutine: 'tick' })
    api.handler = handlerFor([
      summaryFor(1, 'plain', globalRule),
      summaryFor(2, 'worker', subRule),
    ])

    mountApp()
    await settle()
    await openRule(1)
    await openRule(2)

    const plainRows = [...slot(1)!.querySelectorAll<HTMLElement>('.event-row')]
    expect(plainRows.map((row) => row.querySelector('.event-label')?.textContent)).toEqual([
      'Event',
    ])
    expect(plainRows[0].querySelector('.event-value')?.textContent).toBe('Ongoing - Global')

    const subRows = [...slot(2)!.querySelectorAll<HTMLElement>('.event-row')]
    expect(subRows.map((row) => row.querySelector('.event-label')?.textContent)).toEqual([
      'Event',
      'Subroutine',
    ])
    expect(subRows.map((row) => row.querySelector('.event-value')?.textContent)).toEqual([
      'Subroutine',
      'tick',
    ])
  })
})

describe('the statement columns (#881)', () => {
  const nested = [
    action('if', [true]),
    action('setPlayerVariable', [
      { call: 'eventPlayer', args: [] },
      { variable: 'streak' },
      { call: 'add', args: [{ call: 'playerVariable', args: [{ call: 'eventPlayer', args: [] }, { variable: 'streak' }] }, 1] },
    ]),
    action('else'),
    action('while', [true]),
    action('wait', [0.016], { disabled: true }),
    action('end'),
    action('end'),
    action('setGlobalVariable', [
      { variable: 'score' },
      { call: 'globalVariable', args: [{ variable: 'score' }] },
    ]),
  ]

  it('shows both heads with counts, statement rows with per-level indentation, and no End', async () => {
    const condition = {
      value: { call: '==', args: [{ call: 'isAlive', args: [{ call: 'eventPlayer', args: [] }] }, true] },
      span: { start: { line: 4, col: 1 }, end: { line: 4, col: 9 } },
    }
    const content = contentFor(
      { id: 'eachPlayer', team: 'TEAM_1', player: 'ANA' },
      [condition as RuleContent['conditions'][number]],
      nested,
    )
    api.handler = handlerFor([summaryFor(1, 'streak', content)])

    mountApp()
    await settle()
    await openRule(1)

    const body = slot(1)!.querySelector('.rule-view')!
    const heads = [...body.querySelectorAll<HTMLElement>('.col h3')]
    expect(heads.map((head) => head.textContent?.replace(/\s+/g, ' ').trim())).toEqual([
      'Conditions (1)',
      'Actions (6)',
    ])

    // One --bar row per statement; the two `end` markers render nothing.
    const rows = rowsIn(body, 'actions')
    expect(rows).toHaveLength(6)
    const depthOf = (row: HTMLElement) =>
      row.getAttribute('style')?.match(/--depth:\s*(\d+)/)?.[1]
    expect(rows.map((row) => depthOf(row))).toEqual(['0', '1', '0', '1', '2', '0'])
    expect(body.textContent).not.toContain('End')
    expect(body.querySelector('.disabled-badge')).toBeNull()

    // The disabled statement is struck through at half opacity — the
    // row carries `.disabled` and no marker badge.
    const disabledRow = rows[4]
    expect(disabledRow.classList.contains('disabled')).toBe(true)
    expect(disabledRow.querySelector('.disabled-badge')).toBeNull()

    // Statement text: `Global.name` and `owner.name` forms, punctuation tokens.
    const last = rows[5].querySelector('.statement-text')!
    expect(last.textContent).toContain('Set Global Variable')
    expect(last.textContent).toContain('Global.score')
    const playerVar = rows[1].querySelector('.statement-text')!
    expect(playerVar.textContent).toContain('Event Player.streak')
    expect(last.querySelector('.tok.punctuation')).not.toBeNull()
    expect(last.querySelector('.tok.variable')?.textContent).toBe('score')
    expect(last.querySelector('.tok.action')).not.toBeNull()
  })

  it('shows a muted none row in an empty column', async () => {
    const content = contentFor({ id: 'global' }, [], [action('wait', [1])])
    api.handler = handlerFor([summaryFor(1, 'plain', content)])

    mountApp()
    await settle()
    await openRule(1)

    const body = slot(1)!.querySelector('.rule-view')!
    const heads = [...body.querySelectorAll<HTMLElement>('.col h3')]
    expect(heads[0].textContent).toContain('Conditions (0)')
    expect(heads[1].textContent).toContain('Actions (1)')
    const none = body.querySelector('.col .none')!
    expect(none.textContent).toBe('none')
    const css = readFileSync(join(process.cwd(), 'src', 'app.css'), 'utf8')
    expect(css.match(/\.none\s*\{([^}]*)\}/)?.[1] ?? '').toContain('var(--muted)')
  })
})

describe('the debug layer (#881)', () => {
  const content = () =>
    contentFor({ id: 'global' }, [], [action('wait', [1]), action('wait', [2])])

  it('keeps line numbers and node JSON out of the DOM without ?debug', async () => {
    api.handler = handlerFor([summaryFor(1, 'plain', content())])

    mountApp()
    await settle()
    await openRule(1)

    const body = slot(1)!.querySelector('.rule-view')!
    expect(body.querySelectorAll('[data-debug]')).toHaveLength(0)
    expect(body.querySelectorAll('.line-no')).toHaveLength(0)
    expect(body.querySelectorAll('details')).toHaveLength(0)
    expect(body.textContent).not.toContain('"call"')
  })

  it('puts line numbers at the row start in --dim monospace with ?debug', async () => {
    api.handler = handlerFor([summaryFor(1, 'plain', content())])
    window.history.pushState({}, '', '/?debug')

    mountApp()
    await settle()
    await openRule(1)

    const rows = rowsIn(slot(1)!, 'actions')
    expect(rows).toHaveLength(2)
    for (const row of rows) {
      const children = [...row.children]
      const lineNo = row.querySelector('.line-no')!
      expect(lineNo.textContent).toBe('5')
      // Row start: the line number follows only the selection checkbox.
      expect(children.indexOf(lineNo)).toBe(1)
      expect(row.querySelector('details.debug')).not.toBeNull()
    }
    const css = readFileSync(join(process.cwd(), 'src', 'app.css'), 'utf8')
    const rule = css.match(/\.line-no\s*\{([^}]*)\}/)?.[1] ?? ''
    expect(rule).toContain('var(--dim)')
    expect(rule).toContain('monospace')
  })
})

describe('a refused rule (#881)', () => {
  it('renders the refusal code and message in the body while the next rule opens', async () => {
    const good = summaryFor(
      2,
      'fine',
      contentFor({ id: 'global' }, [], [action('wait', [1])]),
    )
    api.handler = (request) => {
      if (request.op === 'rules' && request.all === true && request.content === true) {
        // The bulk read refuses wholesale, so the page reads per rule.
        return Promise.resolve({
          error: { code: 'rule-content-unavailable', message: 'opaque node' },
        } as ToolResponse)
      }
      if (request.op === 'rules' && request.all === true) {
        return Promise.resolve({
          result: { rules: [summaryFor(1, 'broken'), good], snapshot: 1 },
        } as ToolResponse)
      }
      if (request.op === 'rules' && request.rule === 1) {
        return Promise.resolve({
          error: { code: 'rule-content-unavailable', message: 'opaque node in rule one' },
        } as ToolResponse)
      }
      if (request.op === 'rules' && request.rule === 2) {
        return Promise.resolve({ result: { rules: [good] } } as ToolResponse)
      }
      if (request.op === 'lookup' && typeof request.query === 'string') {
        return Promise.resolve(lookupResponse(request.query))
      }
      return Promise.resolve({ result: {} } as ToolResponse)
    }

    mountApp()
    await settle()
    await openRule(1)
    await openRule(2)

    const refusal = slot(1)!.querySelector('.refusal')!
    expect(refusal.textContent).toContain('rule-content-unavailable')
    expect(refusal.textContent).toContain('opaque node in rule one')

    // The following rule still opens and renders its statement rows.
    const rows = rowsIn(slot(2)!, 'actions')
    expect(rows).toHaveLength(1)
    expect(rows[0].querySelector('.statement-text')?.textContent).toContain('Wait')
  })
})
