// @vitest-environment jsdom
/// <reference types="node" />
/**
 * Regression coverage for the async-overwrite defect class (#868): an async
 * read must not overwrite state a newer action produced during its await,
 * plus the #879 page-shell coverage: the spec tokens on `:root`, the
 * title row, the icon toolbar, and the single-column layout.
 *
 * - `load()` used to assign `selected = rules[0]` after the per-rule reads
 *   landed, overwriting a rule the user clicked while they were pending
 *   (#829/#863). The test below clicks while the reads are still out.
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

const contentFor = (eventId: string): RuleContent => ({
  format: 'workshop-rs/rule-content-v1',
  event: { id: eventId },
  disabled: false,
  conditions: [],
  actions: [],
})

// `content` arrives as the schema's loose `{[k:string]: unknown}` shape, so
// tests build summaries through a record and cast like the loader does.
const summaryFor = (id: number, name: string, content?: RuleContent): RuleSummary =>
  ({ id, name, span: null, ...(content ? { content } : {}) }) as unknown as RuleSummary

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
})

function mountApp(): void {
  host = document.createElement('div')
  document.body.appendChild(host)
  app = mount(App, { target: host })
}

const selectedId = () =>
  host.querySelector('.rule-item.selected')?.getAttribute('data-rule-id') ?? null

const ruleButton = (id: number) =>
  host.querySelector<HTMLButtonElement>(`.rule-item[data-rule-id="${id}"]`)!

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

describe('async results must not overwrite newer state', () => {
  it('keeps the rule the user clicked while the initial per-rule reads were pending', async () => {
    const summaries = [summaryFor(1, 'rule one'), summaryFor(2, 'rule two')]
    const reads = new Map<number, Deferred<ToolResponse>>()
    api.handler = (request) => {
      if (request.op === 'lookup') {
        // Name lookups for the selected rule resolve on their own.
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
    // The list is already clickable while the per-rule reads are still out.
    expect(reads.size).toBe(2)

    // The user picks rule 2 before either read lands.
    click(ruleButton(2))
    expect(selectedId()).toBe('2')

    reads.get(1)!.resolve({
      result: { rules: [summaryFor(1, 'rule one', contentFor('ev.one'))] },
    } as ToolResponse)
    reads.get(2)!.resolve({
      result: { rules: [summaryFor(2, 'rule two', contentFor('ev.two'))] },
    } as ToolResponse)
    await settle()

    expect(selectedId()).toBe('2')
    expect(host.querySelector('.rule-title')?.textContent).toContain('rule two')
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
    // Rule 1 is selected; its name lookups are still in flight.
    expect(lookups.has('ev.one')).toBe(true)
    expect(host.textContent).toContain('Loading rule…')

    // The user switches to rule 2; its lookups go out while rule 1's pend.
    click(ruleButton(2))
    await settle()
    expect(lookups.has('ev.two')).toBe(true)

    // The superseded read resolves after the newer one was issued: it must
    // not mark the newer rule's body ready.
    lookups.get('ev.one')!.resolve(lookupResult('ev.one', 'Event One'))
    await settle()
    expect(host.querySelector('.rule-title')?.textContent).toContain('rule two')
    expect(host.querySelector('header.event')).toBeNull()
    expect(host.textContent).toContain('Loading rule…')

    // Once the newer rule's own lookups land, its body renders — resolved
    // names, never the stale read's.
    lookups.get('ev.two')!.resolve(lookupResult('ev.two', 'Event Two'))
    await settle()
    expect(host.querySelector('header.event .tok.value')?.textContent).toBe('Event Two')
  })
})
