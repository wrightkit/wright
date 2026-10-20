// @vitest-environment jsdom
/**
 * Regression coverage for the async-overwrite defect class (#868): an async
 * read must not overwrite state a newer action produced during its await.
 *
 * - `load()` used to assign `selected = rules[0]` after the per-rule reads
 *   landed, overwriting a rule the user clicked while they were pending
 *   (#829/#863). The test below clicks while the reads are still out.
 * - `RuleView`'s name warm used to write `namesReady` unconditionally, so a
 *   superseded warm resolving last could mark a newer rule's body ready
 *   before its own lookups landed (#863). The test below lets the old
 *   rule's lookup resolve after the newer rule's was issued.
 */
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
