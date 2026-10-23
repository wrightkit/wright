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

const STARTRULE_ENTRY = {
  identity: 'startRule',
  kind: 'action',
  spelling: 'Start Rule',
  displayName: 'Start Rule',
  callable: {
    parameters: [
      { name: 'subroutine', displayName: 'Subroutine', type: 'Subroutine', required: true },
      {
        name: 'ifAlreadyExecuting',
        displayName: 'If Already Executing',
        type: 'StartRuleBehavior',
        required: true,
        enum: { domain: 'StartRuleBehavior' },
      },
    ],
  },
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
// Statement rows with a selection checkbox — the `.pending-insert`
// placeholder is also a `.statement` li but carries no checkbox.
const selectable = () => rows().filter((row) => row.querySelector('.sel') !== null)
const dialog = () => document.body.querySelector<HTMLElement>('.dialog.pending')!
const buttonByText = (root: Element, text: string) =>
  [...root.querySelectorAll('button')].find((button) => button.textContent === text)!

describe('inserting a statement from the column heading', () => {
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
    type(editor.querySelector<HTMLInputElement>('[data-arg-control="0"] input')!, '0.25')
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
    type(editor.querySelector<HTMLInputElement>('[data-arg-control="0"] input')!, '0.25')
    click(buttonByText(editor, 'OK'))
    await settle()

    const items = [...host.querySelectorAll('.col[data-list="actions"] .statement')]
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
      return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
    }

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

  it('inserts a startRule through its declared-subroutine picker', async () => {
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
            entries:
              request.query === 'startRule'
                ? [STARTRULE_ENTRY]
                : request.query === 'StartRuleBehavior.RESTART'
                  ? [
                      {
                        identity: 'StartRuleBehavior.RESTART',
                        kind: 'enumMember',
                        spelling: 'Restart Rule',
                        displayName: 'Restart Rule',
                      },
                    ]
                  : [],
          },
        } as ToolResponse)
      }
      if (request.op === 'listEntries') {
        const within = request.within
        return Promise.resolve({
          result: {
            language: 'workshop',
            owner: 'workshop-rs',
            entries:
              request.kind === 'action'
                ? [
                    {
                      identity: 'startRule',
                      kind: 'action',
                      spelling: 'Start Rule',
                      displayName: 'Start Rule',
                    },
                  ]
                : within === 'StartRuleBehavior'
                  ? [
                      {
                        identity: 'StartRuleBehavior.RESTART',
                        kind: 'enumMember',
                        spelling: 'Restart Rule',
                        displayName: 'Restart Rule',
                      },
                      {
                        identity: 'StartRuleBehavior.DO_NOTHING',
                        kind: 'enumMember',
                        spelling: 'Do Nothing',
                        displayName: 'Do Nothing',
                      },
                    ]
                  : [],
            total: 1,
          },
        } as ToolResponse)
      }
      if (request.op === 'symbols') {
        return Promise.resolve({
          result: {
            symbols: [{ id: 3, kind: 'subroutine', name: 'addScore', span: null }],
          },
        } as unknown as ToolResponse)
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
        (el) => el.textContent?.trim() === 'Start Rule',
      )!,
    )
    await settle()

    // The Subroutine slot is a declared-name picker (a literal
    // `{"string"}` node would submit an insert `editStatements` must
    // refuse). Pick `addScore`.
    pointerDown(editor.querySelector('[data-arg-control="0"] .picker-ctl')!)
    await settle()
    pick(
      [...document.body.querySelectorAll<HTMLElement>('.picker-option')].find(
        (el) => el.textContent?.trim() === 'addScore',
      )!,
    )
    await settle()

    // The StartRuleBehavior enum slot: pick a member.
    pointerDown(editor.querySelector('[data-arg-control="1"] .picker-ctl')!)
    await settle()
    pick(
      [...document.body.querySelectorAll<HTMLElement>('.picker-option')].find(
        (el) => el.textContent?.trim() === 'Restart Rule',
      )!,
    )
    await settle()

    expect(ok.disabled).toBe(false)
    click(ok)
    await settle()

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
          {
            call: 'startRule',
            args: [
              { subroutine: 'addScore' },
              { enum: 'StartRuleBehavior', member: 'RESTART' },
            ],
          },
        ],
      },
    ])
  })

  it('supersedes a staged insert at the index a delete takes', async () => {
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

    // Stage a `+` insert at index 1 (after selected row 0)…
    click(checkbox(rows()[0]))
    await settle()
    click(host.querySelector('.col[data-list="actions"] .column-actions button[aria-label="Add action"]')!)
    await settle()
    const editor = document.body.querySelector<HTMLElement>('.dialog.editor')!
    pointerDown(editor.querySelector('[data-arg-control="action"] .picker-ctl')!)
    await settle()
    pick(
      [...document.body.querySelectorAll<HTMLElement>('.picker-option')].find(
        (el) => el.textContent?.trim() === 'Wait',
      )!,
    )
    await settle()
    type(editor.querySelector<HTMLInputElement>('[data-arg-control="0"] input')!, '0.25')
    click(buttonByText(editor, 'OK'))
    await settle()

    // …then delete the rows that index addresses. `editStatements`
    // refuses two edits at one address (`edit-duplicate-address`), so
    // the later delete supersedes the insert — the placeholder clears
    // and the dialog lists one edit, not a duplicate-key crash.
    click(checkbox(selectable()[0]))
    click(checkbox(selectable()[1]))
    await settle()
    click(host.querySelector('.col[data-list="actions"] .column-actions button[aria-label="Delete"]')!)
    await settle()
    expect(host.querySelector('.statement.pending-insert')).toBeNull()

    click(pendingButton())
    await settle()
    expect(dialog(), 'the pending dialog opened').not.toBeNull()
    const items = [...dialog().querySelectorAll('.pending-list li')]
    expect(items.map((li) => li.querySelector('.pending-tag')?.textContent)).toEqual(['delete'])

    click(buttonByText(dialog(), 'Submit'))
    await settle()
    const request = editRequest as unknown as { edits: Record<string, unknown>[] }
    expect(request.edits).toEqual([{ rule: 1, list: 'actions', index: 1, change: 'delete' }])
  })
})
