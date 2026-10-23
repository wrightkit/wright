// @vitest-environment jsdom
/// <reference types="node" />
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
  conditions: RuleContent['conditions'] = [],
  actions: RuleContent['actions'] = [],
): RuleContent => ({
  format: 'workshop-rs/rule-content-v1',
  event: { id: 'ongoingGlobal' },
  disabled: false,
  conditions,
  actions,
})

const summaryFor = (id: number, name: string, content?: RuleContent): RuleSummary =>
  ({ id, name, span: null, ...(content ? { content } : {}) }) as unknown as RuleSummary

const NAMES: Record<string, { kind: string; name: string }[]> = {
  ongoingGlobal: [{ kind: 'event', name: 'Ongoing - Global' }],
  global: [{ kind: 'structural', name: 'Global' }],
  eventPlayer: [{ kind: 'value', name: 'Event Player' }],
  isAlive: [{ kind: 'value', name: 'Is Alive' }],
  wait: [{ kind: 'action', name: 'Wait' }],
  kill: [{ kind: 'action', name: 'Kill' }],
  setGlobalVariable: [{ kind: 'structural', name: 'Set Global Variable' }],
  setGlobalVariableAtIndex: [{ kind: 'action', name: 'Set Global Variable At Index' }],
}

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
      const entries = (NAMES[request.query] ?? []).map((entry) => ({
        identity: request.query,
        kind: entry.kind,
        spelling: request.query,
        displayName: entry.name,
      }))
      return Promise.resolve({
        result: { language: 'workshop', owner: 'workshop-rs', entries },
      } as ToolResponse)
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
const click = (el: Element) => {
  el.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  flushSync()
}
const openRule = async (id: number) => {
  click(slot(id)!.querySelector<HTMLButtonElement>('.bar')!)
  await settle()
}

const col = (list: 'conditions' | 'actions') =>
  host.querySelector<HTMLElement>(`.col[data-list="${list}"]`)!
const rowsIn = (list: 'conditions' | 'actions') =>
  [...col(list).querySelectorAll<HTMLLIElement>('.statement')]
const filterInput = (list: 'conditions' | 'actions') =>
  col(list).querySelector<HTMLInputElement>('input.col-filter')
const selBar = (list: 'conditions' | 'actions') =>
  col(list).querySelector<HTMLElement>('.column-actions:has(.sel-count)')
const selAll = (list: 'conditions' | 'actions') =>
  col(list).querySelector<HTMLInputElement>('input.sel-all')!
const checkbox = (row: Element) => row.querySelector<HTMLInputElement>('.sel')!

const typeInFilter = (list: 'conditions' | 'actions', query: string) => {
  const input = filterInput(list)!
  input.value = query
  input.dispatchEvent(new Event('input', { bubbles: true }))
  flushSync()
}

const escapeKey = (target: Element) =>
  target.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
const keydown = (target: Element, key: string, init: KeyboardEventInit = {}) =>
  target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, ...init }))

const bigActions = (): RuleContent['actions'] => [
  ...Array.from({ length: 30 }, (_, i) =>
    action('setGlobalVariableAtIndex', [
      { call: 'globalVariable', args: [{ variable: 'arr' }] },
      i,
      { call: 'eventPlayer', args: [] },
    ]),
  ),
  ...Array.from({ length: 5 }, () => action('wait', [0.016])),
]

describe('the column filter field', () => {
  it('stays absent at thirty rows and appears at thirty-one', async () => {
    api.handler = handlerFor([
      summaryFor(1, 'at the limit', contentFor([], Array.from({ length: 30 }, () => action('wait')))),
      summaryFor(2, 'past the limit', contentFor([], Array.from({ length: 31 }, () => action('wait')))),
    ])

    mountApp()
    await settle()
    await openRule(1)
    await openRule(2)

    const at = slot(1)!.querySelector<HTMLElement>('.col[data-list="actions"]')!
    expect(at.querySelector('input.col-filter')).toBeNull()
    expect(at.querySelectorAll('.statement')).toHaveLength(30)

    const past = slot(2)!.querySelector<HTMLElement>('.col[data-list="actions"]')!
    expect(past.querySelector('input.col-filter')).not.toBeNull()
    expect(past.querySelectorAll('.statement')).toHaveLength(31)
  })

  it('appears on the conditions column past thirty rows too', async () => {
    const conditions = Array.from({ length: 31 }, () => ({
      value: { call: 'isAlive', args: [{ call: 'eventPlayer', args: [] }] },
    })) as RuleContent['conditions']
    api.handler = handlerFor([summaryFor(1, 'conditions', contentFor(conditions))])

    mountApp()
    await settle()
    await openRule(1)

    expect(filterInput('conditions')).not.toBeNull()
    expect(filterInput('actions')).toBeNull()
  })
})

describe('filtering rows', () => {
  it('keeps rows whose session-locale text contains the query, case-insensitively', async () => {
    api.handler = handlerFor([summaryFor(1, 'init', contentFor([], bigActions()))])

    mountApp()
    await settle()
    await openRule(1)

    typeInFilter('actions', 'WAIT')
    const rows = rowsIn('actions')
    expect(rows).toHaveLength(5)
    for (const row of rows) {
      expect(row.querySelector('.statement-text')!.textContent).toContain('Wait')
    }
    typeInFilter('actions', 'event player')
    expect(rowsIn('actions').length).toBeGreaterThan(0)
    for (const row of rowsIn('actions')) {
      expect(row.querySelector('.statement-text')!.textContent).toContain('Event Player')
    }
  })

  it('also matches a call-name prefix the rendered text does not contain', async () => {
    const actions = [
      action('setGlobalVariable', [{ variable: 'score' }, 0]),
      ...bigActions(),
    ]
    api.handler = handlerFor([summaryFor(1, 'init', contentFor([], actions))])

    mountApp()
    await settle()
    await openRule(1)

    typeInFilter('actions', 'setglobalvariableati')
    const rows = rowsIn('actions')
    expect(rows).toHaveLength(30)
    for (const row of rows) {
      expect(row.querySelector('.statement-text')!.textContent).toContain(
        'Set Global Variable At Index',
      )
    }
  })
})

describe('selection under a filter', () => {
  it.each(['conditions', 'actions'] as const)('select-all preserves hidden selections when checked and unchecked in %s', async (list) => {
    const actions = bigActions()
    const content = list === 'actions'
      ? contentFor([], actions)
      : contentFor(actions.map((value) => ({ value })) as RuleContent['conditions'])
    api.handler = handlerFor([summaryFor(1, 'init', content)])

    mountApp()
    await settle()
    await openRule(1)

    click(checkbox(rowsIn(list)[0]))
    typeInFilter(list, 'wait')
    click(selAll(list))
    expect(selBar(list)!.textContent).toContain('6 selected (1 hidden)')
    click(selAll(list))
    expect(selBar(list)!.textContent).toContain('1 selected (1 hidden)')
    expect(rowsIn(list).some((row) => checkbox(row).checked)).toBe(false)

    for (const modifier of ['ctrlKey', 'metaKey']) {
      checkbox(rowsIn(list)[0]).dispatchEvent(new KeyboardEvent('keydown', {
        key: 'a', [modifier]: true, bubbles: true,
      }))
      flushSync()
      expect(selBar(list)!.textContent).toContain('6 selected (1 hidden)')
      click(selAll(list))
    }
    typeInFilter(list, '')
    expect(rowsIn(list).filter((row) => checkbox(row).checked)).toEqual([rowsIn(list)[0]])
  })

  it.each(['conditions', 'actions'] as const)('extends click and keyboard ranges only over filtered rows in %s', async (list) => {
    const actions = [
      action('wait', [1]),
      action('if', [true]),
      action('wait', [2]),
      action('end'),
      action('kill'),
      action('wait', [3]),
      ...Array.from({ length: 30 }, () => action('kill')),
    ]
    const content = list === 'actions'
      ? contentFor([], actions)
      : contentFor(actions.map((value) => ({ value })) as RuleContent['conditions'])
    api.handler = handlerFor([summaryFor(1, 'init', content)])

    mountApp()
    await settle()
    await openRule(1)
    typeInFilter(list, 'wait')
    click(checkbox(rowsIn(list)[0]))
    const key = (key: string) => {
      checkbox(rowsIn(list)[0]).dispatchEvent(new KeyboardEvent('keydown', {
        key, shiftKey: true, bubbles: true,
      }))
      flushSync()
    }
    key('ArrowDown')
    expect(rowsIn(list).map((row) => checkbox(row).checked)).toEqual([true, true, false])
    key('ArrowDown')
    expect(selBar(list)!.textContent).toContain('3 selected')
    expect(selBar(list)!.textContent).not.toContain('hidden')
    key('Escape')
    expect(selBar(list)!.textContent).toContain('3 selected')
    expect(rowsIn(list).filter((row) => checkbox(row).checked)).toHaveLength(3)
    key('Escape')

    typeInFilter(list, 'wait')
    click(checkbox(rowsIn(list)[0]))
    checkbox(rowsIn(list)[2]).dispatchEvent(new MouseEvent('click', {
      shiftKey: true, bubbles: true,
    }))
    flushSync()
    expect(selBar(list)!.textContent).toContain('3 selected')
    typeInFilter(list, '')
    expect(rowsIn(list).filter((row) => checkbox(row).checked)).toHaveLength(3)
  })
  it('select-all selects the visible matches only', async () => {
    api.handler = handlerFor([summaryFor(1, 'init', contentFor([], bigActions()))])

    mountApp()
    await settle()
    await openRule(1)

    typeInFilter('actions', 'wait')
    click(selAll('actions'))

    expect(selBar('actions')!.textContent).toContain('5 selected')
    const selected = rowsIn('actions').filter((row) => row.classList.contains('selected'))
    expect(selected).toHaveLength(5)
    typeInFilter('actions', '')
    expect(rowsIn('actions')).toHaveLength(35)
    expect(rowsIn('actions').filter((row) => row.classList.contains('selected'))).toHaveLength(5)
  })

  it('keeps hidden rows selected and counts them as hidden', async () => {
    api.handler = handlerFor([summaryFor(1, 'init', contentFor([], bigActions()))])

    mountApp()
    await settle()
    await openRule(1)

    click(checkbox(rowsIn('actions')[0]))
    click(checkbox(rowsIn('actions')[30]))
    expect(selBar('actions')!.textContent).toContain('2 selected')

    typeInFilter('actions', 'wait')
    expect(rowsIn('actions')).toHaveLength(5)
    expect(selBar('actions')!.textContent).toContain('2 selected (1 hidden)')
    expect(
      rowsIn('actions').filter((row) => row.classList.contains('selected')),
    ).toHaveLength(1)
  })

  it('Esc clears the filter before it clears the selection', async () => {
    api.handler = handlerFor([summaryFor(1, 'init', contentFor([], bigActions()))])

    mountApp()
    await settle()
    await openRule(1)

    click(checkbox(rowsIn('actions')[0]))
    typeInFilter('actions', 'wait')
    expect(rowsIn('actions')).toHaveLength(5)
    expect(selBar('actions')!.textContent).toContain('1 selected (1 hidden)')

    escapeKey(filterInput('actions')!)
    flushSync()
    expect(filterInput('actions')!.value).toBe('')
    expect(rowsIn('actions')).toHaveLength(35)
    expect(selBar('actions')!.textContent).toContain('1 selected')

    escapeKey(filterInput('actions')!)
    flushSync()
    expect(selBar('actions')).toBeNull()
  })

  it('scopes keyboard gestures to the rule whose DOM owns the target', async () => {
    api.handler = handlerFor([
      summaryFor(1, 'rule a', contentFor([], bigActions())),
      summaryFor(2, 'rule b', contentFor([], [action('wait', [1])])),
    ])

    mountApp()
    await settle()
    await openRule(1)
    await openRule(2)

    const slotA = slot(1)!
    const slotB = slot(2)!
    const rowA = slotA.querySelector<HTMLLIElement>('.statement')!
    const rowB = slotB.querySelector<HTMLLIElement>('.statement')!
    click(checkbox(rowA))
    click(checkbox(rowB))
    expect(slotB.querySelector('.column-actions .sel-count')!.textContent).toContain('1 selected')

    const inputA = slotA.querySelector<HTMLInputElement>('input.col-filter')!
    inputA.value = 'wait'
    inputA.dispatchEvent(new Event('input', { bubbles: true }))
    flushSync()
    escapeKey(inputA)
    flushSync()
    expect(inputA.value).toBe('')
    expect(slotB.querySelector('.column-actions .sel-count')!.textContent).toContain('1 selected')

    escapeKey(rowB)
    flushSync()
    expect(slotB.querySelector('.column-actions .sel-count')).toBeNull()
  })

  it('leaves gestures to a sibling rule while the pointer is over this column', async () => {
    api.handler = handlerFor([
      summaryFor(1, 'rule a', contentFor([], bigActions())),
      summaryFor(2, 'rule b', contentFor([], bigActions())),
    ])

    mountApp()
    await settle()
    await openRule(1)
    await openRule(2)

    const colA = slot(1)!.querySelector<HTMLElement>('.col[data-list="actions"]')!
    const colB = slot(2)!.querySelector<HTMLElement>('.col[data-list="actions"]')!
    const rowB = colB.querySelector<HTMLLIElement>('.statement')!
    const boxB = checkbox(rowB)
    click(checkbox(colA.querySelector<HTMLLIElement>('.statement')!))
    click(boxB)

    colA.dispatchEvent(new MouseEvent('mouseenter', { bubbles: false }))
    keydown(boxB, 'a', { ctrlKey: true })
    flushSync()
    const count = (col: HTMLElement) =>
      col.querySelector('.column-actions .sel-count')!.textContent
    expect(count(colA)).toContain('1 selected')
    expect(count(colB)).toContain('35 selected')

    keydown(boxB, 'Delete')
    flushSync()
    expect(colA.querySelectorAll('.pending-delete')).toHaveLength(0)
    expect(colB.querySelectorAll('.pending-delete')).toHaveLength(35)

    const filterA = colA.querySelector<HTMLInputElement>('input.col-filter')!
    const filterB = colB.querySelector<HTMLInputElement>('input.col-filter')!
    typeInFilter('actions', 'wait')
    filterB.value = 'wait'
    filterB.dispatchEvent(new Event('input', { bubbles: true }))
    flushSync()
    escapeKey(filterB)
    flushSync()
    expect(filterA.value).toBe('wait')
    expect(filterB.value).toBe('')
  })
})
