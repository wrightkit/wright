// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The #880 rule-bar coverage — one bar per rule in `rules` order, per-rule
 * open state, the event cell, the disabled name, and the debug element
 * count — plus the #879 page-shell coverage: the spec tokens on `:root`,
 * the title row, the icon toolbar, and the single-column layout.
 *
 * The async-overwrite defect class (#868) — an async read must not
 * overwrite state a newer action produced during its await:
 *
 * - `load()` used to assign `selected = rules[0]` after the per-rule reads
 *   landed, overwriting a rule the user clicked while they were pending
 *   (#829/#863). The test below opens a bar while the reads are still out.
 * - `RuleView`'s name warm used to write `namesReady` unconditionally, so a
 *   superseded warm resolving last could mark a newer rule's body ready
 *   before its own lookups landed (#863). The test below lets the old
 *   rule's lookup resolve after the newer rule's was issued.
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

interface Deferred<T> {
  promise: Promise<T>
  resolve: (value: T) => void
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((r) => {
    resolve = r
  })
  return { promise, resolve }
}

/** Drain pending promise continuations, then flush Svelte's effects and DOM. */
async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flushSync()
  await new Promise((resolve) => setTimeout(resolve, 0))
  flushSync()
}

const contentFor = (eventId: string, disabled = false): RuleContent => ({
  format: 'workshop-rs/rule-content-v1',
  event: { id: eventId },
  disabled,
  conditions: [],
  actions: [],
})

// `content` arrives as the schema's loose `{[k:string]: unknown}` shape, so
// tests build summaries through a record and cast like the loader does.
const summaryFor = (
  id: number,
  name: string,
  content?: RuleContent,
  event?: string,
): RuleSummary =>
  ({ id, name, event, span: null, ...(content ? { content } : {}) }) as unknown as RuleSummary

const lookupResult = (identity: string, displayName: string): ToolResponse =>
  ({
    result: {
      language: 'workshop',
      owner: 'workshop-rs',
      entries: [{ identity, kind: 'event', spelling: identity, displayName }],
    },
  }) as ToolResponse

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
const chevOf = (id: number) =>
  slot(id)!.querySelector<HTMLButtonElement>('.chev .sq')!
const isOpen = (id: number) => slot(id)?.querySelector('.rule-view') != null
const openBodies = () => host.querySelectorAll('.rule-view')

const click = (el: HTMLElement) => {
  el.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  flushSync()
}

describe('the page shell (#879)', () => {
  it('defines every spec token on :root with its spec value and drops the removed tokens', () => {
    const css = readFileSync(join(process.cwd(), 'src', 'app.css'), 'utf8')
    const root = css.match(/:root\s*\{([^}]*)\}/)?.[1] ?? ''
    // docs/ui/design.md § Tokens, including the "white control" row
    // (`--white`/`--on-white` carry `#f4f5f8` on `#111`).
    const tokens: Record<string, string> = {
      '--bar': 'rgb(44 48 68 / 0.72)',
      '--bar-hover': 'rgb(44 48 68 / 0.86)',
      '--dlg': '#262c40',
      '--dlg-foot': '#171b26',
      '--card': '#1d2131',
      '--lbl': '#3a4566',
      '--lbl-focus': '#9c9fa8',
      '--ctl': '#2c5383',
      '--ctl-hi': '#3a6db0',
      '--num': '#343d58',
      '--opt-hi': '#4a8fcb',
      '--text': '#f2f4f8',
      '--muted': '#b9bfce',
      '--dim': '#8a91a6',
      '--orange': '#cf6a26',
      '--gold': '#d9a03f',
      '--blue': '#3d9be9',
      '--warn': '#f0b04a',
      '--error': '#ff7a70',
      '--white': '#f4f5f8',
      '--on-white': '#111',
    }
    for (const [token, value] of Object.entries(tokens)) {
      expect(root, token).toContain(`${token}: ${value}`)
    }
    for (const removed of ['--bg', '--panel', '--panel-2', '--line', '--accent']) {
      expect(root, removed).not.toMatch(new RegExp(`${removed}\\s*:`))
    }
  })

  it('draws the spec backdrop: the fixed triangle pattern over the three-stop gradient', () => {
    const css = readFileSync(join(process.cwd(), 'src', 'app.css'), 'utf8')
    const body = css.match(/(?:^|\n)\s*body\s*\{([^}]*)\}/)?.[1] ?? ''
    expect(body).toContain('data:image/svg+xml')
    expect(body).toContain('#c3c9e6')
    expect(body).toContain('#98a6cc')
    expect(body).toContain('#6f80aa')
    expect(body).toContain('fixed')
  })

  it('renders the title row and the toolbar, with a disabled Add rule and no sidebar', async () => {
    const summaries = [
      summaryFor(1, 'rule one', contentFor('ev.one')),
      summaryFor(2, 'rule two', contentFor('ev.two')),
    ]
    api.handler = (request) => {
      if (request.op === 'project') {
        return Promise.resolve({
          result: { files: 1, sourceFiles: ['maps/demo-project.ws'] },
        } as ToolResponse)
      }
      if (request.op === 'rules' && request.all === true && request.content === true) {
        return Promise.resolve({ result: { rules: summaries } } as ToolResponse)
      }
      if (request.op === 'lookup') {
        return Promise.resolve({
          result: { language: 'workshop', owner: 'workshop-rs', entries: [] },
        } as ToolResponse)
      }
      return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
    }

    mountApp()
    await settle()

    // Title row: "Workshop" large italic, then the entry file's base name.
    expect(host.querySelector('.title-row h1')?.textContent).toBe('Workshop')
    expect(host.querySelector('.file-name')?.textContent).toBe('demo-project.ws')

    const toolbar = host.querySelector('.toolbar')!
    expect(toolbar.querySelector('.rule-count')?.textContent?.trim()).toBe('Rules (2)')

    // One square icon button per toolbar action, each with a `title`
    // tooltip and an inline SVG: Diagnostics on the left; Variables, the
    // pending-changes view button, and a disabled Add rule on the right.
    const squares = [...toolbar.querySelectorAll<HTMLButtonElement>('.sq')]
    expect(squares).toHaveLength(4)
    for (const button of squares) {
      expect(button.title).not.toBe('')
      expect(button.querySelector('svg')).not.toBeNull()
    }
    const [diagnostics, variables, pending, addRule] = squares
    expect(diagnostics.title).toBe('Diagnostics')
    expect(variables.title).toBe('Variables')
    expect(pending.title).toBe('Pending changes')
    // The count badge only renders once edits are staged.
    expect(pending.querySelector('.sq-badge')).toBeNull()
    expect(addRule.disabled).toBe(true)
    expect(addRule.title).toContain('rule-level edits are not available')

    expect(toolbar.querySelector<HTMLInputElement>('input.filter')?.type).toBe('search')
    expect(toolbar.querySelectorAll('.sep')).toHaveLength(2)

    // One column at any width: no sidebar grid and no <nav> pane; the rule
    // list sits under the toolbar inside the single column.
    expect(host.querySelector('.layout')).toBeNull()
    expect(host.querySelector('nav')).toBeNull()
    expect(host.querySelector('.page > .rule-list')).not.toBeNull()
  })
})

describe('the rule bars (#880)', () => {
  /** A `rules`+`project`+`lookup` handler over the given summaries. */
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
        return Promise.resolve(lookupResult(request.query, `Name ${request.query}`))
      }
      return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
    }

  it('renders one closed bar per rule in `rules` order, chevron outside the bar', async () => {
    api.handler = handlerFor([
      summaryFor(5, 'first', contentFor('ev.a')),
      summaryFor(9, 'second', contentFor('ev.b')),
      summaryFor(2, 'third', contentFor('ev.c')),
    ])

    mountApp()
    await settle()

    const bars = [...host.querySelectorAll<HTMLButtonElement>('.rule-list .bar')]
    expect(bars.map((bar) => bar.querySelector('.bar-name')?.textContent)).toEqual([
      'first',
      'second',
      'third',
    ])
    // Every bar starts closed; its chevron is the white square button
    // outside the bar on the left.
    expect(openBodies()).toHaveLength(0)
    for (const bar of bars) {
      expect(bar.getAttribute('aria-expanded')).toBe('false')
      const row = bar.parentElement!
      const chev = row.querySelector<HTMLButtonElement>('.chev .sq')!
      expect(chev).not.toBeNull()
      expect(row.firstElementChild).toBe(chev.closest('.chev'))
    }
  })

  it('toggles a rule open under its bar from the bar or the chevron, others untouched', async () => {
    api.handler = handlerFor([
      summaryFor(1, 'first', contentFor('ev.a')),
      summaryFor(2, 'second', contentFor('ev.b')),
      summaryFor(3, 'third', contentFor('ev.c')),
    ])

    mountApp()
    await settle()

    // Any number open at once: bar click, then chevron click.
    click(barOf(1))
    click(chevOf(3))
    click(barOf(2))
    await settle()
    expect([isOpen(1), isOpen(2), isOpen(3)]).toEqual([true, true, true])
    expect(openBodies()).toHaveLength(3)
    expect(barOf(1).getAttribute('aria-expanded')).toBe('true')
    expect(slot(2)!.querySelector('.chev')!.classList.contains('open')).toBe(true)

    // Closing one leaves the others open — and its chevron rotates back.
    click(chevOf(1))
    await settle()
    expect([isOpen(1), isOpen(2), isOpen(3)]).toEqual([false, true, true])
    expect(slot(1)!.querySelector('.chev')!.classList.contains('open')).toBe(false)
  })

  it('hides non-matching bars on the filter and keeps open rules open', async () => {
    api.handler = handlerFor([
      summaryFor(1, 'setup mode', contentFor('ev.a')),
      summaryFor(2, 'spawn handler', contentFor('ev.b')),
    ])

    mountApp()
    await settle()
    click(barOf(1))
    click(barOf(2))
    await settle()

    const filter = host.querySelector<HTMLInputElement>('input.filter')!
    filter.value = 'SETUP'
    filter.dispatchEvent(new Event('input', { bubbles: true }))
    flushSync()

    // Case-insensitive name match: 'spawn handler' is hidden, 'setup mode'
    // stays and stays open.
    expect(slot(2)).toBeNull()
    expect(isOpen(1)).toBe(true)
    expect(host.querySelectorAll('.rule-slot')).toHaveLength(1)

    // Clearing the filter restores the hidden bar with its open state intact.
    filter.value = ''
    filter.dispatchEvent(new Event('input', { bubbles: true }))
    flushSync()
    expect([isOpen(1), isOpen(2)]).toEqual([true, true])
  })

  it('dims and strikes the name of a disabled rule and carries no badge', async () => {
    api.handler = handlerFor([
      summaryFor(1, 'enabled rule', contentFor('ev.a')),
      summaryFor(2, 'disabled rule', contentFor('ev.b', true)),
    ])

    mountApp()
    await settle()

    const disabled = host.querySelector('.bar.disabled')!
    expect(disabled.querySelector('.bar-name')?.textContent).toBe('disabled rule')
    expect(host.querySelectorAll('.bar .disabled-badge')).toHaveLength(0)
  })

  it('shows the lookup name, the subroutine name, and the raw id on a failed lookup', async () => {
    const queries: string[] = []
    const summaries = [
      summaryFor(1, 'global', contentFor('ev.a'), 'ongoingGlobal'),
      summaryFor(2, 'worker', contentFor('ev.b'), 'subroutine:worker'),
      summaryFor(3, 'odd', contentFor('ev.c'), 'oddEvent'),
    ]
    api.handler = (request) => {
      if (request.op === 'rules' && request.all === true && request.content === true) {
        return Promise.resolve({ result: { rules: summaries } } as ToolResponse)
      }
      if (request.op === 'lookup' && typeof request.query === 'string') {
        queries.push(request.query)
        if (request.query === 'ongoingGlobal') {
          return Promise.resolve(lookupResult('ongoingGlobal', 'Ongoing - Global'))
        }
        return Promise.resolve({
          result: { language: 'workshop', owner: 'workshop-rs', entries: [] },
        } as ToolResponse)
      }
      return Promise.resolve({ result: {} } as ToolResponse)
    }

    mountApp()
    await settle()

    const eventCell = (id: number) => barOf(id).querySelector('.bar-event')!.textContent
    expect(eventCell(1)).toBe('Ongoing - Global')
    expect(eventCell(2)).toBe('worker')
    expect(eventCell(3)).toBe('oddEvent')
    // A subroutine id is not a catalog identity — it never hits `lookup`.
    expect(queries).not.toContain('subroutine:worker')
  })

  it('shows the element count on each bar only with ?debug', async () => {
    const summaries = [
      summaryFor(1, 'first', contentFor('ev.a')),
      summaryFor(2, 'second', contentFor('ev.b')),
    ]
    let costCalls = 0
    api.handler = (request) => {
      if (request.op === 'rules' && request.all === true && request.content === true) {
        return Promise.resolve({ result: { rules: summaries } } as ToolResponse)
      }
      if (request.op === 'costEstimate') {
        costCalls += 1
        return Promise.resolve({
          result: {
            elementCount: {
              total: 42,
              budget: 10000,
              // Ranked by count, not `rules` order — `index` names the rule.
              rules: [
                { index: 2, name: 'second', count: 30 },
                { index: 1, name: 'first', count: 12 },
              ],
            },
          },
        } as ToolResponse)
      }
      if (request.op === 'lookup') {
        return Promise.resolve({
          result: { language: 'workshop', owner: 'workshop-rs', entries: [] },
        } as ToolResponse)
      }
      return Promise.resolve({ result: {} } as ToolResponse)
    }

    // Without `?debug` the count is absent from the DOM and never fetched.
    mountApp()
    await settle()
    expect(host.querySelectorAll('.bar-count, .rule-bar [data-debug]')).toHaveLength(0)
    expect(costCalls).toBe(0)
    void unmount(app!)
    app = undefined
    document.body.innerHTML = ''

    window.history.pushState({}, '', '/?debug')
    mountApp()
    await settle()
    expect(costCalls).toBe(1)
    expect(barOf(2).querySelector('.bar-count')?.textContent).toBe('30')
    expect(barOf(1).querySelector('.bar-count')?.textContent).toBe('12')
  })
})

describe('async results must not overwrite newer state', () => {
  it('keeps the rule the user opened while the initial per-rule reads were pending', async () => {
    const summaries = [summaryFor(1, 'rule one'), summaryFor(2, 'rule two')]
    const reads = new Map<number, Deferred<ToolResponse>>()
    api.handler = (request) => {
      if (request.op === 'lookup') {
        // Name lookups for an open rule resolve on their own.
        return Promise.resolve({
          result: { language: 'workshop', owner: 'workshop-rs', entries: [] },
        } as ToolResponse)
      }
      if (request.op !== 'rules') {
        return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
      }
      if (typeof request.rule === 'number') {
        const read = deferred<ToolResponse>()
        reads.set(request.rule, read)
        return read.promise
      }
      if (request.content === true) {
        // Bulk `rules?all&content` refuses, forcing the per-rule path.
        return Promise.resolve({
          error: { code: 'rule-content-unavailable', message: 'opaque node' },
        } as ToolResponse)
      }
      return Promise.resolve({ result: { rules: summaries, snapshot: 3 } } as ToolResponse)
    }

    mountApp()
    await settle()
    // The bars are already clickable while the per-rule reads are still out.
    expect(reads.size).toBe(2)

    // The user opens rule 2 before either read lands.
    click(barOf(2))
    expect(isOpen(2)).toBe(true)
    expect(isOpen(1)).toBe(false)

    reads.get(1)!.resolve({
      result: { rules: [summaryFor(1, 'rule one', contentFor('ev.one'))] },
    } as ToolResponse)
    reads.get(2)!.resolve({
      result: { rules: [summaryFor(2, 'rule two', contentFor('ev.two'))] },
    } as ToolResponse)
    await settle()

    expect(isOpen(2)).toBe(true)
    expect(isOpen(1)).toBe(false)
    expect(slot(2)!.querySelector('.event-value')?.textContent).toContain('ev.two')
  })

  it('keeps a newer rule loading until its own lookups land when an old read resolves late', async () => {
    const summaries = [
      summaryFor(1, 'rule one', contentFor('ev.one')),
      summaryFor(2, 'rule two', contentFor('ev.two')),
    ]
    const lookups = new Map<string, Deferred<ToolResponse>>()
    api.handler = (request) => {
      if (request.op === 'rules' && request.all === true && request.content === true) {
        return Promise.resolve({ result: { rules: summaries } } as ToolResponse)
      }
      if (request.op === 'lookup' && typeof request.query === 'string') {
        const read = deferred<ToolResponse>()
        lookups.set(request.query, read)
        return read.promise
      }
      return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
    }

    mountApp()
    await settle()

    // The user opens rule 1, then rule 2; both bodies' lookups are out.
    click(barOf(1))
    await settle()
    expect(lookups.has('ev.one')).toBe(true)
    click(barOf(2))
    await settle()
    expect(lookups.has('ev.two')).toBe(true)
    expect(openBodies()).toHaveLength(2)

    // The older read resolves after the newer one was issued: it must not
    // mark the newer rule's body ready.
    lookups.get('ev.one')!.resolve(lookupResult('ev.one', 'Event One'))
    await settle()
    expect(slot(1)!.querySelector('.event-row .event-value')?.textContent).toContain('Event One')
    expect(slot(2)!.querySelector('.event-row')).toBeNull()
    expect(slot(2)!.textContent).toContain('Loading rule…')

    // Once the newer rule's own lookups land, its body renders — resolved
    // names, never the stale read's.
    lookups.get('ev.two')!.resolve(lookupResult('ev.two', 'Event Two'))
    await settle()
    expect(slot(2)!.querySelector('.event-row .event-value')?.textContent).toContain('Event Two')
  })
})
