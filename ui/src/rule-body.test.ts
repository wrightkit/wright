// @vitest-environment jsdom
/// <reference types="node" />
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import process from 'node:process'
import { flushSync, mount, unmount } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App.svelte'
import type { RuleContent } from './lib/view'
import type { RuleSummary, ToolRequest, ToolResponse } from './lib/types/generated/wright-agent-v1'

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

const summaryFor = (id: number, name: string, content?: RuleContent): RuleSummary =>
  ({ id, name, span: null, ...(content ? { content } : {}) }) as unknown as RuleSummary

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

    const rows = rowsIn(body, 'actions')
    expect(rows).toHaveLength(6)
    const depthOf = (row: HTMLElement) =>
      row.getAttribute('style')?.match(/--depth:\s*(\d+)/)?.[1]
    expect(rows.map((row) => depthOf(row))).toEqual(['0', '1', '0', '1', '2', '0'])
    expect(body.textContent).not.toContain('End')
    expect(body.querySelector('.disabled-badge')).toBeNull()

    const disabledRow = rows[4]
    expect(disabledRow.classList.contains('disabled')).toBe(true)
    expect(disabledRow.querySelector('.disabled-badge')).toBeNull()

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
      expect(children.indexOf(lineNo)).toBe(1)
      expect(row.querySelector('details.debug')).not.toBeNull()
    }
    const css = readFileSync(join(process.cwd(), 'src', 'app.css'), 'utf8')
    const rule = css.match(/\.line-no\s*\{([^}]*)\}/)?.[1] ?? ''
    expect(rule).toContain('var(--dim)')
    expect(rule).toContain('monospace')
  })
})

describe('column selection', () => {
  const actions = [
    action('wait', [0]),
    action('if', [true]),
    action('wait', [1]),
    action('end'),
    action('wait', [2]),
  ]

  async function setup() {
    const content = contentFor(
      { id: 'global' },
      [{ value: true }, { value: false }],
      actions,
    )
    api.handler = handlerFor([summaryFor(1, 'selection', content)])
    mountApp()
    await settle()
    await openRule(1)
  }

  const column = (list: 'conditions' | 'actions' = 'actions') =>
    slot(1)!.querySelector<HTMLElement>(`.col[data-list="${list}"]`)!
  const checks = (list: 'conditions' | 'actions' = 'actions') =>
    [...column(list).querySelectorAll<HTMLInputElement>('input.sel')]
  const selected = () => checks().map((check) => check.checked)
  const key = (name: string, modifiers: KeyboardEventInit = {}) => {
    const event = new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true, ...modifiers })
    column().dispatchEvent(event)
    flushSync()
    return event
  }
  const hover = () => {
    column().dispatchEvent(new MouseEvent('mouseenter'))
    flushSync()
  }

  it.each(['conditions', 'actions'] as const)('replaces + with the count and square batch controls in the %s heading', async (list) => {
    await setup()
    const head = column(list).querySelector('h3')!
    const add = head.querySelector<HTMLButtonElement>('button')!
    expect(add.getAttribute('aria-label')).toBe(`Add ${list === 'actions' ? 'action' : 'condition'}`)
    expect(add.disabled).toBe(true)
    click(checks(list)[0]!)
    expect(head.querySelector('.sel-count')?.textContent).toBe('1 selected')
    expect([...head.querySelectorAll('button')].map((button) => button.getAttribute('aria-label')))
      .toEqual(['Disable', 'Enable', 'Delete', 'Clear'])
    expect(head.querySelectorAll('button.sq')).toHaveLength(4)
    expect(slot(1)!.querySelector('.sel-bar')).toBeNull()
    click(head.querySelector('button[aria-label="Clear"]')! as HTMLElement)
    expect(head.querySelector('.sel-count')).toBeNull()
    expect(head.querySelectorAll('button')).toHaveLength(1)
  })

  it.each([false, true])('shift-click selects only rendered rows across a partially contained block (reverse=%s)', async (reverse) => {
    await setup()
    const first = checks()[reverse ? 3 : 2]!
    const last = checks()[reverse ? 2 : 3]!
    click(first)
    last.dispatchEvent(new MouseEvent('click', { bubbles: true, shiftKey: true }))
    flushSync()
    expect(selected()).toEqual([false, false, true, true])
    expect(column().querySelector('.sel-count')?.textContent).toBe('2 selected')
    click(column().querySelector('button[aria-label="Disable"]')! as HTMLElement)
    expect(column().querySelectorAll('.pending-change')).toHaveLength(2)
    expect(host.querySelector('.toolbar .sq-badge')?.textContent).toBe('2')
  })

  it('extends by visible rows with Shift+Arrow, including across hidden End markers and at column boundaries', async () => {
    await setup()
    hover()
    expect(key('ArrowDown', { shiftKey: true }).defaultPrevented).toBe(false)
    click(checks()[2]!)
    expect(key('ArrowDown', { shiftKey: true }).defaultPrevented).toBe(true)
    expect(selected()).toEqual([false, false, true, true])
    key('ArrowDown', { shiftKey: true })
    expect(selected()).toEqual([false, false, true, true])
    key('ArrowUp', { shiftKey: true })
    key('ArrowUp', { shiftKey: true })
    expect(selected()).toEqual([false, true, true, true])
    key('ArrowUp', { shiftKey: true })
    key('ArrowUp', { shiftKey: true })
    expect(selected()).toEqual([true, true, true, true])
    expect(key('Escape').defaultPrevented).toBe(true)
    expect(selected()).toEqual([false, false, false, false])
    expect(key('ArrowDown', { shiftKey: true }).defaultPrevented).toBe(false)
  })

  it.each(['ctrlKey', 'metaKey'] as const)('%s+A selects visible rows and Delete stages only those rows', async (modifier) => {
    await setup()
    hover()
    expect(key('a', { [modifier]: true }).defaultPrevented).toBe(true)
    expect(selected()).toEqual([true, true, true, true])
    expect(column().querySelector('.sel-count')?.textContent).toBe('4 selected')
    key('Delete')
    expect(column().querySelectorAll('.pending-delete')).toHaveLength(4)
    expect(host.querySelector('.toolbar .sq-badge')?.textContent).toBe('4')
  })
})

describe('the statement-row context menu (#917)', () => {
  const actions = [
    action('wait', [0]),
    action('if', [true]),
    action('wait', [1]),
    action('while', [true]),
    action('wait', [2]),
    action('end'),
    action('else'),
    action('wait', [3]),
    action('end'),
    action('wait', [4]),
  ]
  const conditions = [
    { value: { call: 'isAlive', args: [{ call: 'eventPlayer', args: [] }] } },
    { value: true },
    { value: { call: 'isAlive', args: [{ call: 'eventPlayer', args: [] }] } },
  ]

  async function setup() {
    const content = contentFor(
      { id: 'global' },
      conditions as RuleContent['conditions'],
      actions,
    )
    api.handler = handlerFor([summaryFor(1, 'menu', content)])
    mountApp()
    await settle()
    await openRule(1)
  }

  const column = (list: 'conditions' | 'actions' = 'actions') =>
    slot(1)!.querySelector<HTMLElement>(`.col[data-list="${list}"]`)!
  const rows = (list: 'conditions' | 'actions' = 'actions') =>
    [...column(list).querySelectorAll<HTMLLIElement>('.statement')]
  const selected = (list: 'conditions' | 'actions' = 'actions') =>
    new Map(
      rows(list).map((row) => [
        Number(row.getAttribute('data-index')),
        row.classList.contains('selected'),
      ]),
    )
  const menu = () => document.body.querySelector<HTMLElement>('[role="menu"]')
  const menuItems = () => [...menu()!.querySelectorAll<HTMLElement>('[role="menuitem"]')]
  const itemNamed = (label: string) =>
    menuItems().find((item) => item.textContent === label)!

  const contextmenu = async (row: HTMLElement) => {
    row.dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }),
    )
    await settle()
  }
  const choose = async (item: HTMLElement) => {
    item.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await settle()
  }

  it('selects every same-call row across nested blocks', async () => {
    await setup()
    await contextmenu(rows()[2]!)
    expect(menu(), 'the menu opened in the top layer').not.toBeNull()
    await choose(itemNamed('Select same action'))
    expect(selected()).toEqual(
      new Map([
        [0, true],
        [1, false],
        [2, true],
        [3, false],
        [4, true],
        [6, false],
        [7, true],
        [9, true],
      ]),
    )
    expect(menu(), 'choosing closed the menu').toBeNull()
  })

  it('selects a block through its End and no row past it', async () => {
    await setup()
    await contextmenu(rows()[1]!)
    await choose(itemNamed('Select block'))
    expect(selected()).toEqual(
      new Map([
        [0, false],
        [1, true],
        [2, true],
        [3, true],
        [4, true],
        [6, true],
        [7, true],
        [9, false],
      ]),
    )
    expect(column().querySelector('.sel-count')?.textContent).toBe('8 selected')
  })

  it('selects a nested block without its enclosing block', async () => {
    await setup()
    await contextmenu(rows()[3]!)
    await choose(itemNamed('Select block'))
    expect(selected()).toEqual(
      new Map([
        [0, false],
        [1, false],
        [2, false],
        [3, true],
        [4, true],
        [6, false],
        [7, false],
        [9, false],
      ]),
    )
    expect(column().querySelector('.sel-count')?.textContent).toBe('3 selected')
  })

  it('disables Select block on a non-opener', async () => {
    await setup()
    await contextmenu(rows()[0]!)
    const item = itemNamed('Select block')
    expect(item.getAttribute('aria-disabled'), 'aria-disabled').toBe('true')
    await choose(item)
    expect([...selected().values()], 'a disabled item selects nothing').toEqual(
      rows().map(() => false),
    )
    expect(itemNamed('Select same action').getAttribute('aria-disabled')).toBe('false')
  })

  it('selects same-call conditions and disables Select block on a condition row', async () => {
    await setup()
    await contextmenu(rows('conditions')[0]!)
    await choose(itemNamed('Select same action'))
    expect(selected('conditions')).toEqual(
      new Map([
        [0, true],
        [1, false],
        [2, true],
      ]),
    )

    await contextmenu(rows('conditions')[0]!)
    expect(itemNamed('Select block').getAttribute('aria-disabled')).toBe('true')
  })

  it('renders the menu in the popover style of editor.md § Pickers', async () => {
    await setup()
    await contextmenu(rows()[0]!)
    const pop = menu()!
    expect(pop.classList.contains('picker-pop'), 'the popover chrome').toBe(true)
    expect(pop.getAttribute('aria-label'), 'the menu is labelled').toBe('action 1 menu')
    for (const item of menuItems()) {
      expect(item.classList.contains('picker-option')).toBe(true)
    }
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

    const rows = rowsIn(slot(2)!, 'actions')
    expect(rows).toHaveLength(1)
    expect(rows[0].querySelector('.statement-text')?.textContent).toContain('Wait')
  })
})
