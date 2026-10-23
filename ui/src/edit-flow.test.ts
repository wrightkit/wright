// @vitest-environment jsdom
import { flushSync, mount, unmount } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App.svelte'
import type { RuleContent } from './lib/view'
import type { RuleSummary, ToolRequest, ToolResponse } from './lib/types/generated/wright-agent-v1'

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

const WAIT_ENTRY = {
  identity: 'wait',
  kind: 'action',
  spelling: 'Wait',
  displayName: 'Wait',
  callable: {
    parameters: [
      { name: 'time', displayName: 'Duration', type: 'Any', required: true },
      {
        name: 'waitBehavior',
        displayName: 'Wait Behavior',
        type: 'Wait',
        required: false,
        default: 'Wait.IGNORE_CONDITION',
        enum: { domain: 'Wait' },
      },
    ],
  },
}

const MODIFY_ENTRY = {
  identity: 'modifyGlobalVariable',
  kind: 'action',
  spelling: 'Modify Global Variable',
  displayName: 'Modify Global Variable',
  callable: {
    parameters: [
      { name: 'variable', displayName: 'Variable', type: 'Variable', required: true },
      {
        name: 'operation',
        displayName: 'Operation',
        type: 'Operation',
        required: true,
        content: { operations: ['add', 'subtract'] },
      },
      { name: 'value', displayName: 'Value', type: 'Any', required: true },
    ],
  },
}

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

const pointerDown = (el: Element) => {
  el.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }))
  flushSync()
}
const pick = (el: Element) => {
  el.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }))
  el.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
  flushSync()
}
const type = (el: HTMLInputElement, value: string) => {
  el.value = value
  el.dispatchEvent(new Event('input', { bubbles: true }))
  flushSync()
}

const bar = (id: number) =>
  host.querySelector<HTMLButtonElement>(`.rule-slot[data-rule-id="${id}"] .bar`)!
const openRule = async (id: number) => {
  click(bar(id))
  await settle()
}

const pendingButton = () =>
  host.querySelector<HTMLButtonElement>('.toolbar button[aria-label="Pending changes"]')!
const pendingBadge = () => pendingButton().querySelector('.sq-badge')?.textContent ?? null

const rows = () => [...host.querySelectorAll<HTMLLIElement>('.statement')]
const checkbox = (row: Element) => row.querySelector<HTMLInputElement>('.sel')!
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

    click(checkbox(rows()[0]))
    click(checkbox(rows()[1]))
    expect(host.querySelector('.col[data-list="actions"] .sel-count')?.textContent).toBe('2 selected')
    click(host.querySelector('.column-actions button[aria-label="Disable"]')!)
    expect(pendingBadge()).toBe('2')
    expect(rows()[0].classList.contains('pending-change')).toBe(true)

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

    expect(applySpy).not.toHaveBeenCalled()
    expect(document.body.querySelector('[data-testid="pending-refusal"]')?.textContent).toContain(
      'edit-stale-source',
    )
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

  it('stages an insert at the column end from the heading +', async () => {
    const summaries = [
      summaryFor(1, 'setup', contentFor([action('wait', [0.016]), action('kill', [])])),
    ]
    let editRequest: ToolRequest | null = null
    api.handler = (request) => {
      if (request.op === 'project') {
        return Promise.resolve({ result: { inputIdentity: 'id-1' } } as ToolResponse)
      }
      if (request.op === 'rules') {
        return Promise.resolve({ result: { rules: summaries, snapshot: 1 } } as ToolResponse)
      }
      if (request.op === 'lookup') {
        return Promise.resolve({
          result: {
            language: 'workshop',
            owner: 'workshop-rs',
            entries: request.query === 'wait' ? [WAIT_ENTRY] : [],
          },
        } as ToolResponse)
      }
      if (request.op === 'listEntries') {
        return Promise.resolve({
          result: {
            language: 'workshop',
            owner: 'workshop-rs',
            entries:
              request.kind === 'action'
                ? [{ identity: 'wait', kind: 'action', spelling: 'Wait', displayName: 'Wait' }]
                : [],
            total: 1,
          },
        } as ToolResponse)
      }
      if (request.op === 'editStatements') {
        editRequest = request
        return Promise.resolve(statements())
      }
      return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
    }
    api.apply = () => Promise.resolve({ ok: true, exit: 0, diagnostics: [] })

    mountApp()
    await settle()
    await openRule(1)

    click(host.querySelector('.col[data-list="actions"] .column-actions button')!)
    await settle()
    const editor = document.body.querySelector<HTMLElement>('.dialog.editor')!
    expect(editor, 'the editor dialog is open').not.toBeNull()
    const ok = buttonByText(editor, 'OK') as HTMLButtonElement
    expect(ok.disabled, 'OK is disabled before an action is picked').toBe(true)

    pointerDown(editor.querySelector('[data-arg-control="action"] .picker-ctl')!)
    await settle()
    const option = [...document.body.querySelectorAll<HTMLElement>('.picker-option')].find(
      (el) => el.textContent?.trim() === 'Wait',
    )!
    pick(option)
    await settle()

    expect(editor.textContent).toContain('Duration')
    type(
      editor.querySelector<HTMLInputElement>('[data-arg-control="0"] input')!,
      '0.25',
    )
    await settle()
    expect(ok.disabled).toBe(false)
    click(ok)
    await settle()

    const mark = host.querySelector('.col[data-list="actions"] .statement.pending-insert')
    expect(mark, 'the insert placeholder rendered').not.toBeNull()
    expect(mark!.querySelector('.pending-tag')!.textContent).toBe('insert')
    expect(pendingBadge()).toBe('1')

    click(pendingButton())
    await settle()
    click(buttonByText(dialog(), 'Submit'))
    await settle()

    const request = editRequest as unknown as { edits: Record<string, unknown>[] }
    expect(request.edits).toEqual([
      {
        rule: 1,
        list: 'actions',
        index: 2,
        change: 'insert',
        statements: [
          { call: 'wait', args: [0.25, { enum: 'Wait', member: 'IGNORE_CONDITION' }] },
        ],
      },
    ])
  })

  it('stages an insert after the one selected row', async () => {
    const summaries = [
      summaryFor(1, 'setup', contentFor([action('wait', [0.016]), action('kill', [])])),
    ]
    let editRequest: ToolRequest | null = null
    api.handler = (request) => {
      if (request.op === 'project') {
        return Promise.resolve({ result: { inputIdentity: 'id-1' } } as ToolResponse)
      }
      if (request.op === 'rules') {
        return Promise.resolve({ result: { rules: summaries, snapshot: 1 } } as ToolResponse)
      }
      if (request.op === 'lookup') {
        return Promise.resolve({
          result: {
            language: 'workshop',
            owner: 'workshop-rs',
            entries: request.query === 'kill' ? [] : [WAIT_ENTRY],
          },
        } as ToolResponse)
      }
      if (request.op === 'listEntries') {
        return Promise.resolve({
          result: {
            language: 'workshop',
            owner: 'workshop-rs',
            entries:
              request.kind === 'action'
                ? [{ identity: 'wait', kind: 'action', spelling: 'Wait', displayName: 'Wait' }]
                : [],
            total: 1,
          },
        } as ToolResponse)
      }
      if (request.op === 'editStatements') {
        editRequest = request
        return Promise.resolve(statements())
      }
      return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
    }
    api.apply = () => Promise.resolve({ ok: true, exit: 0, diagnostics: [] })

    mountApp()
    await settle()
    await openRule(1)

    click(checkbox(rows()[0]))
    await settle()
    click(host.querySelector('.col[data-list="actions"] .column-actions button[aria-label="Add action"]')!)
    await settle()
    const editor = document.body.querySelector<HTMLElement>('.dialog.editor')!
    expect(editor, 'the editor dialog is open').not.toBeNull()

    pointerDown(editor.querySelector('[data-arg-control="action"] .picker-ctl')!)
    await settle()
    const option = [...document.body.querySelectorAll<HTMLElement>('.picker-option')].find(
      (el) => el.textContent?.trim() === 'Wait',
    )!
    pick(option)
    await settle()
    type(
      editor.querySelector<HTMLInputElement>('[data-arg-control="0"] input')!,
      '0.25',
    )
    click(buttonByText(editor, 'OK'))
    await settle()

    const items = [
      ...host.querySelectorAll('.col[data-list="actions"] .statement'),
    ]
    const markAt = items.findIndex((el) => el.classList.contains('pending-insert'))
    expect(markAt, 'the insert placeholder rendered between the rows').toBe(1)

    click(pendingButton())
    await settle()
    click(buttonByText(dialog(), 'Submit'))
    await settle()

    const request = editRequest as unknown as { edits: Record<string, unknown>[] }
    expect(request.edits).toEqual([
      {
        rule: 1,
        list: 'actions',
        index: 1,
        change: 'insert',
        statements: [
          { call: 'wait', args: [0.25, { enum: 'Wait', member: 'IGNORE_CONDITION' }] },
        ],
      },
    ])
  })

  it('keeps OK disabled for a call whose slots need #919\'s pickers', async () => {
    const summaries = [
      summaryFor(1, 'setup', contentFor([action('wait', [0.016]), action('kill', [])])),
    ]
    let editRequest: ToolRequest | null = null
    api.handler = (request) => {
      if (request.op === 'project') {
        return Promise.resolve({ result: { inputIdentity: 'id-1' } } as ToolResponse)
      }
      if (request.op === 'rules') {
        return Promise.resolve({ result: { rules: summaries, snapshot: 1 } } as ToolResponse)
      }
      if (request.op === 'lookup') {
        return Promise.resolve({
          result: {
            language: 'workshop',
            owner: 'workshop-rs',
            entries: request.query === 'modifyGlobalVariable' ? [MODIFY_ENTRY] : [],
          },
        } as ToolResponse)
      }
      if (request.op === 'listEntries') {
        return Promise.resolve({
          result: {
            language: 'workshop',
            owner: 'workshop-rs',
            entries:
              request.kind === 'action'
                ? [
                    {
                      identity: 'modifyGlobalVariable',
                      kind: 'action',
                      spelling: 'Modify Global Variable',
                      displayName: 'Modify Global Variable',
                    },
                  ]
                : [],
            total: 1,
          },
        } as ToolResponse)
      }
      if (request.op === 'symbols') {
        return Promise.resolve({ result: { symbols: [] } } as unknown as ToolResponse)
      }
      if (request.op === 'editStatements') {
        editRequest = request
        return Promise.resolve(statements())
      }
      return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
    }
    api.apply = () => Promise.resolve({ ok: true, exit: 0, diagnostics: [] })

    mountApp()
    await settle()
    await openRule(1)

    click(host.querySelector('.col[data-list="actions"] .column-actions button')!)
    await settle()
    const editor = document.body.querySelector<HTMLElement>('.dialog.editor')!
    const ok = buttonByText(editor, 'OK') as HTMLButtonElement

    pointerDown(editor.querySelector('[data-arg-control="action"] .picker-ctl')!)
    await settle()
    pick(
      [...document.body.querySelectorAll<HTMLElement>('.picker-option')].find(
        (el) => el.textContent?.trim() === 'Modify Global Variable',
      )!,
    )
    await settle()

    type(editor.querySelector<HTMLInputElement>('[data-arg-control="2"] input')!, '1')
    await settle()
    expect(ok.disabled, 'OK is disabled on a picker-bound call').toBe(true)
    click(buttonByText(editor, 'Cancel'))
    await settle()
    expect(pendingBadge(), 'no insert was staged').toBeNull()
  })
})
