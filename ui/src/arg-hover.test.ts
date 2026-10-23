// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The argument hover card (docs/ui/editor.md § Hover card, #924): an
 * `.arg` token carries its `data-path` into the statement's args; after
 * 450 ms the card names the argument, its slot, and the enclosing call's
 * parameters with the hovered one marked; leaving, clicking, or
 * scrolling hides it.
 */
import { flushSync, mount, unmount } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App.svelte'
import { chromeStrings } from './lib/chrome'
import { enclosingCall, hoverSlot, nodeAtPath, statementRoot } from './lib/hover'
import type { LookupEntry, ToolRequest, ToolResponse } from './lib/types/generated/wright-agent-v1'
import type { RuleContent } from './lib/view'
import type { Node } from './lib/types/generated/rule-content-v1'

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

const condition = (value: Node, extra: Record<string, unknown> = {}): RuleContent['conditions'][number] =>
  ({ value, ...extra }) as RuleContent['conditions'][number]

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

const summaryFor = (id: number, name: string, content?: RuleContent) =>
  ({ id, name, span: null, ...(content ? { content } : {}) }) as never

/**
 * `lookup` names and facts the tests resolve. A call entry carries the
 * `callable.parameters` the card lists; an operator entry carries only
 * the kind so the slot reads "operand".
 */
const ENTRIES: Record<string, LookupEntry[]> = {
  eachPlayer: [
    { identity: 'eachPlayer', kind: 'event', spelling: 'eachPlayer', displayName: 'Ongoing - Each Player' },
  ],
  global: [{ identity: 'global', kind: 'structural', spelling: 'global', displayName: 'global' }],
  setGlobalVariable: [
    {
      identity: 'setGlobalVariable',
      kind: 'structural',
      spelling: 'setGlobalVariable',
      displayName: 'Set Global Variable',
      callable: {
        parameters: [
          { name: 'variable', displayName: 'Variable', type: 'Variable', required: true },
          { name: 'value', displayName: 'Value', type: 'Object|Array', required: true },
        ],
      },
    },
  ],
  '==': [{ identity: '==', kind: 'operator', spelling: '==', displayName: '==' }],
  compare: [
    {
      identity: 'compare',
      kind: 'value',
      spelling: 'compare',
      displayName: 'Compare',
      callable: {
        parameters: [
          { name: 'value', displayName: 'Value', type: 'Any', required: true },
          { name: 'operator', displayName: 'Operator', type: 'Any', required: true },
          { name: 'value', displayName: 'Value', type: 'Any', required: true },
        ],
      },
    },
  ],
  add: [
    {
      identity: 'add',
      kind: 'operator',
      spelling: 'add',
      displayName: 'Add',
      callable: {
        parameters: [
          { name: 'value', displayName: 'Value', type: 'Any', required: true },
          { name: 'value', displayName: 'Value', type: 'Any', required: true },
        ],
      },
    },
  ],
  'Hero.ANA': [{ identity: 'Hero.ANA', kind: 'enumMember', spelling: 'ANA', displayName: 'Ana' }],
}

const lookupResponse = (query: string): ToolResponse =>
  ({ result: { language: 'workshop', owner: 'workshop-rs', entries: ENTRIES[query] ?? [] } }) as ToolResponse

const handlerFor =
  (summaries: ReturnType<typeof summaryFor>[]) =>
  (request: ToolRequest): Promise<ToolResponse> => {
    if (request.op === 'project') {
      return Promise.resolve({ result: { sourceFiles: ['demo.ws'] } } as ToolResponse)
    }
    if (request.op === 'rules' && request.all === true && request.content === true) {
      return Promise.resolve({ result: { rules: summaries } } as unknown as ToolResponse)
    }
    if (request.op === 'lookup' && typeof request.query === 'string') {
      return Promise.resolve(lookupResponse(request.query))
    }
    return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
  }

let host: HTMLDivElement
let app: ReturnType<typeof mount> | undefined

afterEach(() => {
  vi.useRealTimers()
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

async function openRule(id: number) {
  slot(id)!.querySelector<HTMLButtonElement>('.bar')!.click()
  await settle()
  await new Promise((resolve) => setTimeout(resolve, 0))
  flushSync()
}

const argsIn = (el: Element | null) =>
  [...(el?.querySelectorAll<HTMLElement>('.arg[data-path]') ?? [])]

const pointerOver = (el: HTMLElement) => {
  el.dispatchEvent(new PointerEvent('pointerover', { bubbles: true }))
  flushSync()
}
const pointerOut = (el: HTMLElement) => {
  el.dispatchEvent(new PointerEvent('pointerout', { bubbles: true }))
  // pointerleave does not bubble; the row's own listeners use it.
  el.dispatchEvent(new PointerEvent('pointerleave', { bubbles: true }))
  flushSync()
}

const t = chromeStrings('en-US')

/** A fake `Names`-shaped warmed cache over the `ENTRIES` fixture. */
const fakeNames = {
  peek: (identity: string, kinds: string[] = []) => {
    const exact = ENTRIES[identity] ?? []
    const entry =
      kinds.map((kind) => exact.find((item) => item.kind === kind)).find(Boolean) ?? exact[0]
    return entry ? { name: entry.displayName, kind: entry.kind } : null
  },
  peekEntry: (identity: string, kinds: string[] = []) => {
    const exact = ENTRIES[identity] ?? []
    return (
      kinds.map((kind) => exact.find((item) => item.kind === kind)).find(Boolean) ??
      exact[0] ??
      null
    )
  },
}

describe('the hover slot text (#924)', () => {
  it('names a call parameter with its index, label, and call', () => {
    const root = statementRoot({
      call: 'setGlobalVariable',
      args: [{ variable: 'score' }, 1],
    })!
    const enclosing = enclosingCall(root, [1])!
    const slot = hoverSlot(enclosing, fakeNames, t)
    expect(slot.slot).toBe('parameter 2 (Value) of Set Global Variable')
    expect(slot.params.map((p) => [p.label, p.marked])).toEqual([
      ['Variable', false],
      ['Value', true],
    ])
  })

  it('names an operator operand left or right', () => {
    const root = statementRoot({ call: '==', args: [1, 2] })!
    expect(hoverSlot(enclosingCall(root, [0])!, fakeNames, t).slot).toBe('left operand')
    expect(hoverSlot(enclosingCall(root, [1])!, fakeNames, t).slot).toBe('right operand')
  })

  it('names a nested argument against its enclosing call', () => {
    // Compare(1, +, 2): the operator call sits at args[1]; its own
    // operands are path [1,0] and [1,1] inside it.
    const root = statementRoot({
      call: 'compare',
      args: [1, { call: 'add', args: [1, 2] }, 3],
    })!
    const enclosing = enclosingCall(root, [1, 0])!
    expect(enclosing.call).toBe('add')
    expect(enclosing.index).toBe(0)
    expect(enclosing.argCount).toBe(2)
    expect(hoverSlot(enclosing, fakeNames, t).slot).toBe('left operand')
    expect(hoverSlot(enclosingCall(root, [1, 1])!, fakeNames, t).slot).toBe('right operand')
    expect(nodeAtPath(root.args, [1, 0])).toBe(1)
    expect(nodeAtPath(root.args, [1, 1])).toBe(2)
  })
})

describe('the argument hover card (#924)', () => {
  const content = contentFor(
    { id: 'eachPlayer' },
    [condition({ call: '==', args: [{ call: 'add', args: [1, 2] }, 3] })],
    [action('setGlobalVariable', [{ variable: 'score' }, 1])],
  )

  async function setup() {
    api.handler = handlerFor([summaryFor(1, 'hover', content)])
    mountApp()
    await settle()
    await openRule(1)
  }

  it('carries data-path on each argument token', async () => {
    await setup()
    const actionRow = slot(1)!.querySelector('.col[data-list="actions"] .statement')!
    const paths = argsIn(actionRow).map((el) => el.dataset.path)
    expect(paths).toEqual(['0', '1'])
    const condRow = slot(1)!.querySelector('.col[data-list="conditions"] .statement')!
    const condPaths = argsIn(condRow).map((el) => el.dataset.path)
    expect(condPaths).toEqual(['0', '0.0', '0.1', '1'])
  })

  it('shows the card only after the 450 ms delay, and not before', async () => {
    await setup()
    vi.useFakeTimers()
    const arg = slot(1)!.querySelector<HTMLElement>(
      '.col[data-list="actions"] .statement .arg[data-path="1"]',
    )!
    pointerOver(arg)
    expect(arg.classList.contains('arg-hovered')).toBe(true)
    expect(document.querySelector('.arg-card')).toBeNull()
    await vi.advanceTimersByTimeAsync(449)
    expect(document.querySelector('.arg-card')).toBeNull()
    await vi.advanceTimersByTimeAsync(1)
    flushSync()
    const card = document.querySelector('.arg-card')
    expect(card).not.toBeNull()
    expect(card!.textContent).toContain('1')
    expect(card!.textContent).toContain('parameter 2 (Value) of Set Global Variable')
    expect(card!.textContent).toContain('Variable')
    expect(card!.textContent).toContain('Value')
  })

  it('names the operator operands left and right', async () => {
    await setup()
    vi.useFakeTimers()
    const left = slot(1)!.querySelector<HTMLElement>(
      '.col[data-list="conditions"] .statement .arg[data-path="0"]',
    )!
    pointerOver(left)
    await vi.advanceTimersByTimeAsync(450)
    flushSync()
    expect(document.querySelector('.arg-card')!.textContent).toContain('left operand')
  })

  it('hides on pointerleave, on click, and on scroll', async () => {
    await setup()
    vi.useFakeTimers()
    const arg = slot(1)!.querySelector<HTMLElement>(
      '.col[data-list="actions"] .statement .arg[data-path="0"]',
    )!
    pointerOver(arg)
    await vi.advanceTimersByTimeAsync(450)
    flushSync()
    expect(document.querySelector('.arg-card')).not.toBeNull()
    pointerOut(arg)
    expect(document.querySelector('.arg-card')).toBeNull()
    expect(arg.classList.contains('arg-hovered')).toBe(false)

    pointerOver(arg)
    await vi.advanceTimersByTimeAsync(450)
    flushSync()
    expect(document.querySelector('.arg-card')).not.toBeNull()
    arg.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }))
    flushSync()
    expect(document.querySelector('.arg-card')).toBeNull()

    pointerOver(arg)
    await vi.advanceTimersByTimeAsync(450)
    flushSync()
    expect(document.querySelector('.arg-card')).not.toBeNull()
    window.dispatchEvent(new Event('scroll'))
    flushSync()
    expect(document.querySelector('.arg-card')).toBeNull()
  })

  it('outlines the argument under the pointer', async () => {
    await setup()
    const arg = slot(1)!.querySelector<HTMLElement>(
      '.col[data-list="actions"] .statement .arg[data-path="1"]',
    )!
    pointerOver(arg)
    expect(arg.classList.contains('arg-hovered')).toBe(true)
    pointerOut(arg)
    expect(arg.classList.contains('arg-hovered')).toBe(false)
  })
})
