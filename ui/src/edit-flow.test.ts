// @vitest-environment jsdom
/**
 * The editing layer's submit path (ADR-0032 layer 2): staged statement
 * edits go out as one `editStatements` request, the returned transaction
 * posts to the host `/apply` route, and refusals — including a stale
 * identity — surface on the page without a write.
 */
import { flushSync, mount, unmount } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App.svelte'
import type { RuleContent } from './lib/view'
import type { RuleSummary, ToolRequest, ToolResponse } from './lib/types/generated/wright-agent-v1'

/** The api module the components call; each test installs its own handlers. */
const api = vi.hoisted(() => ({
  handler: null as null | ((request: ToolRequest) => Promise<ToolResponse>),
  apply: null as null | ((transaction: unknown) => Promise<unknown>),
}))
vi.mock('./lib/api', () => ({
  sessionToken: 'test-token',
  call: (request: ToolRequest) => {
    if (!api.handler) return Promise.reject(new Error('test installed no api handler'))
    return api.handler(request)
  },
  apply: (transaction: unknown) => {
    if (!api.apply) return Promise.reject(new Error('test installed no apply handler'))
    return api.apply(transaction)
  },
}))

/** Drain pending promise continuations, then flush Svelte's effects and DOM. */
async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0))
    flushSync()
  }
}

const action = (
  call: string,
  args: unknown[] = [],
  extra: Record<string, unknown> = {},
): RuleContent['actions'][number] =>
  ({
    call,
    args,
    span: { start: { line: 1, col: 5 }, end: { line: 1, col: 20 } },
    ...extra,
  }) as RuleContent['actions'][number]

const contentFor = (actions: RuleContent['actions']): RuleContent => ({
  format: 'workshop-rs/rule-content-v1',
  event: { id: 'ongoingGlobal' },
  disabled: false,
  conditions: [],
  actions,
})

const summaryFor = (id: number, name: string, content?: RuleContent): RuleSummary =>
  ({ id, name, span: null, ...(content ? { content } : {}) }) as unknown as RuleSummary

const statements = (): ToolResponse => ({
  result: {
    ok: true,
    exit: 0,
    diagnostics: [],
    transaction: {
      edits: [
        {
          kind: 'editStatements',
          source: 'project.ws',
          source_identity: 'id-1',
          range: { start_line: 1, start_col: 5, end_line: 1, end_col: 20 },
          new_text: 'edited',
        },
      ],
    },
    preview: [{ source: 'project.ws', source_identity: 'id-2', new_text: 'edited' }],
  },
}) as ToolResponse

let host: HTMLDivElement
let app: ReturnType<typeof mount> | undefined

afterEach(() => {
  if (app) void unmount(app)
  app = undefined
  api.handler = null
  api.apply = null
  document.body.innerHTML = ''
})

function mountApp(): void {
  host = document.createElement('div')
  document.body.appendChild(host)
  app = mount(App, { target: host })
}

const click = (el: Element) => {
  el.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  flushSync()
}

// The editing layer lives in an open rule's body (#880); the bar opens it.
const bar = (id: number) =>
  host.querySelector<HTMLButtonElement>(`.rule-slot[data-rule-id="${id}"] .bar`)!
const openRule = async (id: number) => {
  click(bar(id))
  await settle()
}

// The shell's pending-changes view button is the icon button labelled
// "Pending changes"; its spec count badge shows the staged-edit count.
const pendingButton = () =>
  host.querySelector<HTMLButtonElement>('.toolbar button[aria-label="Pending changes"]')!
const pendingBadge = () => pendingButton().querySelector('.sq-badge')?.textContent ?? null

const rows = () => [...host.querySelectorAll<HTMLLIElement>('.statement')]
const checkbox = (row: Element) => row.querySelector<HTMLInputElement>('.sel')!
// Bits UI portals the dialogs into `document.body`; mounted content is open.
const dialog = () => document.body.querySelector<HTMLElement>('.dialog.pending')!
const buttonByText = (root: Element, text: string) =>
  [...root.querySelectorAll('button')].find((button) => button.textContent === text)!

describe('the editing layer', () => {
  it('stages a batch disable, submits the transaction, and re-reads the program', async () => {
    const summaries = [
      summaryFor(
        1,
        'setup',
        contentFor([action('wait', [0.016]), action('setGlobalVariable', [{ variable: 'score' }, 1])]),
      ),
    ]
    let rulesCalls = 0
    let editRequest: ToolRequest | null = null
    api.handler = (request) => {
      if (request.op === 'project') {
        return Promise.resolve({
          result: { inputIdentity: 'id-1', snapshot: 1 },
        } as ToolResponse)
      }
      if (request.op === 'rules' && request.all === true && request.content === true) {
        rulesCalls += 1
        return Promise.resolve({ result: { rules: summaries, snapshot: 1 } } as ToolResponse)
      }
      if (request.op === 'lookup') {
        return Promise.resolve({
          result: { language: 'workshop', owner: 'workshop-rs', entries: [] },
        } as ToolResponse)
      }
      if (request.op === 'editStatements') {
        editRequest = request
        return Promise.resolve(statements())
      }
      return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
    }
    let applied: unknown = null
    api.apply = (transaction) => {
      applied = transaction
      return Promise.resolve({ ok: true, exit: 0, diagnostics: [] })
    }

    mountApp()
    await settle()
    await openRule(1)
    expect(rows()).toHaveLength(2)

    // Two statement checkboxes, then the heading's Disable button.
    click(checkbox(rows()[0]))
    click(checkbox(rows()[1]))
    expect(host.querySelector('.col[data-list="actions"] .sel-count')?.textContent).toBe('2 selected')
    click(host.querySelector('.column-actions button[aria-label="Disable"]')!)
    expect(pendingBadge()).toBe('2')
    expect(rows()[0].classList.contains('pending-change')).toBe(true)

    // The pending dialog lists the changes; Submit sends the request and
    // posts the returned transaction to the host apply path.
    click(pendingButton())
    await settle()
    expect(dialog(), 'the pending dialog is open').not.toBeNull()
    expect(dialog().textContent).toContain('actions[0]')
    click(buttonByText(dialog(), 'Submit'))
    await settle()

    const request = editRequest as unknown as {
      op: string
      inputIdentity: string
      snapshot: number
      edits: Record<string, unknown>[]
    }
    expect(request.op).toBe('editStatements')
    expect(request.inputIdentity).toBe('id-1')
    expect(request.snapshot).toBe(1)
    expect(request.edits).toHaveLength(2)
    for (const [i, edit] of request.edits.entries()) {
      expect(edit).toMatchObject({ rule: 1, list: 'actions', index: i, change: 'replace' })
      expect(edit.statement).toMatchObject({ disabled: true })
      expect(edit.statement).not.toHaveProperty('span')
      expect(edit.statement).not.toHaveProperty('spans')
    }
    const transaction = (statements().result as { transaction: unknown }).transaction
    expect(applied).toEqual(transaction)
    // The page re-reads the program after a write; the pending map cleared.
    expect(rulesCalls).toBe(2)
    expect(pendingBadge()).toBeNull()
  })

  it('reports a stale identity and never posts the transaction', async () => {
    const summaries = [summaryFor(1, 'setup', contentFor([action('wait')]))]
    api.handler = (request) => {
      if (request.op === 'project') {
        return Promise.resolve({ result: { inputIdentity: 'id-1' } } as ToolResponse)
      }
      if (request.op === 'rules') {
        return Promise.resolve({ result: { rules: summaries, snapshot: 1 } } as ToolResponse)
      }
      if (request.op === 'lookup') {
        return Promise.resolve({
          result: { language: 'workshop', owner: 'workshop-rs', entries: [] },
        } as ToolResponse)
      }
      if (request.op === 'editStatements') {
        return Promise.resolve({
          result: {
            ok: false,
            exit: 2,
            diagnostics: [
              {
                code: 'edit-stale-source',
                stage: 'discovery',
                severity: 'error',
                message: "the file changed since the page read it",
              },
            ],
          },
        } as ToolResponse)
      }
      return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
    }
    const applySpy = vi.fn()
    api.apply = applySpy

    mountApp()
    await settle()
    await openRule(1)
    click(checkbox(rows()[0]))
    click(host.querySelector('.column-actions button[aria-label="Delete"]')!)
    expect(rows()[0].classList.contains('pending-delete')).toBe(true)

    click(pendingButton())
    await settle()
    click(buttonByText(dialog(), 'Submit'))
    await settle()

    // The refusal renders in the dialog; the apply route was never hit.
    expect(applySpy).not.toHaveBeenCalled()
    expect(document.body.querySelector('[data-testid="pending-refusal"]')?.textContent).toContain(
      'edit-stale-source',
    )
    // A stale refusal drops the staging: the marks addressed a program the
    // page no longer displays, and it re-reads.
    expect(pendingBadge()).toBeNull()
    expect(rows()[0].classList.contains('pending-delete')).toBe(false)
  })

  it('reports a stale apply refusal and writes nothing through the page', async () => {
    const summaries = [summaryFor(1, 'setup', contentFor([action('wait')]))]
    api.handler = (request) => {
      if (request.op === 'project') {
        return Promise.resolve({ result: { inputIdentity: 'id-1' } } as ToolResponse)
      }
      if (request.op === 'rules') {
        return Promise.resolve({ result: { rules: summaries, snapshot: 1 } } as ToolResponse)
      }
      if (request.op === 'lookup') {
        return Promise.resolve({
          result: { language: 'workshop', owner: 'workshop-rs', entries: [] },
        } as ToolResponse)
      }
      if (request.op === 'editStatements') {
        return Promise.resolve(statements())
      }
      return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
    }
    api.apply = () =>
      Promise.resolve({
        ok: false,
        exit: 2,
        diagnostics: [
          {
            code: 'edit-stale-source',
            stage: 'discovery',
            severity: 'error',
            message: 'the file changed since the transaction was validated',
          },
        ],
      })

    mountApp()
    await settle()
    await openRule(1)
    click(checkbox(rows()[0]))
    click(host.querySelector('.column-actions button[aria-label="Disable"]')!)
    click(pendingButton())
    await settle()
    click(buttonByText(dialog(), 'Submit'))
    await settle()

    expect(document.body.querySelector('[data-testid="pending-refusal"]')?.textContent).toContain(
      'edit-stale-source',
    )
    expect(pendingBadge()).toBeNull()
  })

  it('opens the statement editor on a row click and stages the toggle', async () => {
    const summaries = [summaryFor(1, 'setup', contentFor([action('wait')]))]
    api.handler = (request) => {
      if (request.op === 'project') {
        return Promise.resolve({ result: { inputIdentity: 'id-1' } } as ToolResponse)
      }
      if (request.op === 'rules') {
        return Promise.resolve({ result: { rules: summaries, snapshot: 1 } } as ToolResponse)
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
    await openRule(1)
    click(rows()[0])
    await settle()

    const editor = document.body.querySelector<HTMLElement>('.dialog.editor')!
    expect(editor, 'the editor dialog is open').not.toBeNull()
    const toggle = editor.querySelector<HTMLInputElement>('[data-testid="editor-disable"]')!
    click(toggle)
    click(buttonByText(editor, 'OK'))
    await settle()

    expect(document.body.querySelector('.dialog.editor'), 'the editor closed').toBeNull()
    expect(pendingBadge()).toBe('1')
    expect(rows()[0].classList.contains('pending-change')).toBe(true)
  })
})
