// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The statement editor's argument form (docs/ui/editor.md § Form, #918):
 * one row per `lookup` parameter labelled by the parameter's localized
 * name, the control the slot's type takes (numeric input, text input,
 * boolean-literal picker, nested call rows), the clicked argument's
 * control focused on open, "unset" in `--warn` with OK disabled while a
 * required slot is empty, and OK staging one `replace` edit whose node
 * differs only in the edited literal. The dialog composes no Workshop
 * text.
 */
import { flushSync, mount, unmount } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App.svelte'
import type { RuleContent } from './lib/view'
import type { RuleSummary, ToolRequest, ToolResponse } from './lib/types/generated/wright-agent-v1'

/** The api module the components call; each test installs its own handler. */
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

const contentFor = (
  actions: RuleContent['actions'],
  conditions: RuleContent['conditions'] = [],
): RuleContent => ({
  format: 'workshop-rs/rule-content-v1',
  event: { id: 'ongoingGlobal' },
  disabled: false,
  conditions,
  actions,
})

const summaryFor = (id: number, name: string, content?: RuleContent): RuleSummary =>
  ({ id, name, span: null, ...(content ? { content } : {}) }) as unknown as RuleSummary

/**
 * The fixture `lookup` rows the form's labels come from — `callable
 * .parameters` facts as the service emits them (`displayName` is the
 * localized parameter label the Workshop editor shows).
 */
const LOOKUP: Record<string, object[]> = {
  ongoingGlobal: [{ kind: 'event', displayName: 'Ongoing - Global' }],
  wait: [
    {
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
            enum: { domain: 'Wait', members: ['Ignore Condition', 'Restart'] },
          },
        ],
      },
    },
  ],
  setCrouchEnabled: [
    {
      identity: 'setCrouchEnabled',
      kind: 'action',
      spelling: 'Set Crouch Enabled',
      displayName: 'Set Crouch Enabled',
      callable: {
        parameters: [
          { name: 'player', displayName: 'Player', type: 'Player|Array', required: true },
          { name: 'enabled', displayName: 'Enabled', type: 'Boolean', required: true },
        ],
      },
    },
  ],
  customString: [
    {
      identity: 'customString',
      kind: 'action',
      spelling: 'Custom String',
      displayName: 'Custom String',
      callable: {
        parameters: [
          { name: 'string', displayName: 'String', type: 'String', required: true },
          { name: '{0}', displayName: '{0}', type: 'Object|Array', required: false },
          { name: '{1}', displayName: '{1}', type: 'Object|Array', required: false },
          { name: '{2}', displayName: '{2}', type: 'Object|Array', required: false },
        ],
      },
    },
  ],
  allPlayers: [
    {
      identity: 'allPlayers',
      kind: 'value',
      spelling: 'All Players',
      displayName: 'All Players',
      callable: {
        parameters: [
          {
            name: 'array',
            displayName: 'Team',
            type: 'Team',
            required: false,
            default: 'Team.ALL',
            enum: { domain: 'Team', members: ['All Teams', 'Team 1', 'Team 2'] },
          },
        ],
      },
    },
  ],
  'Team.ALL': [{ identity: 'Team.ALL', kind: 'enumMember', displayName: 'All Teams' }],
  'Wait.IGNORE_CONDITION': [
    { identity: 'Wait.IGNORE_CONDITION', kind: 'enumMember', displayName: 'Ignore Condition' },
  ],
  compare: [
    {
      identity: 'compare',
      kind: 'value',
      spelling: 'Compare',
      displayName: 'Compare',
      callable: {
        parameters: [
          { name: 'value', displayName: 'Value', type: 'Any', required: true },
          { name: 'comparison', displayName: 'Comparison', type: '__Operator__', required: true },
          { name: 'value', displayName: 'Value', type: 'Any', required: true },
        ],
      },
    },
  ],
  '==': [{ identity: '==', kind: 'operator', displayName: '==' }],
  '>=': [{ identity: '>=', kind: 'operator', displayName: '>=' }],
  global: [{ identity: 'global', kind: 'structural', displayName: 'Global' }],
  globalVariable: [],
  playerVariable: [],
  memberAccess: [],
  eventPlayer: [{ identity: 'eventPlayer', kind: 'value', displayName: 'Event Player' }],
}

const ENTRIES: Record<string, object[]> = {
  Team: [
    { identity: 'Team.ALL', kind: 'enumMember', displayName: 'All Teams' },
    { identity: 'Team.TEAM_1', kind: 'enumMember', displayName: 'Team 1' },
    { identity: 'Team.TEAM_2', kind: 'enumMember', displayName: 'Team 2' },
  ],
  Wait: [
    { identity: 'Wait.IGNORE_CONDITION', kind: 'enumMember', displayName: 'Ignore Condition' },
    { identity: 'Wait.RESTART', kind: 'enumMember', displayName: 'Restart' },
  ],
}

function handlerFor(summaries: RuleSummary[]) {
  return (request: ToolRequest): Promise<ToolResponse> => {
    if (request.op === 'project') {
      return Promise.resolve({
        result: { inputIdentity: 'id-1', snapshot: 1, sourceFiles: ['demo.ws'] },
      } as ToolResponse)
    }
    if (request.op === 'rules' && request.all === true && request.content === true) {
      return Promise.resolve({ result: { rules: summaries, snapshot: 1 } } as ToolResponse)
    }
    if (request.op === 'lookup' && typeof request.query === 'string') {
      const entries = LOOKUP[request.query] ?? []
      return Promise.resolve({
        result: {
          language: 'workshop',
          owner: 'workshop-rs',
          entries: entries.map((entry) => ({
            spelling: request.query,
            ...entry,
          })),
        },
      } as ToolResponse)
    }
    if (request.op === 'listEntries') {
      return Promise.resolve({
        result: {
          language: 'workshop',
          owner: 'workshop-rs',
          entries: ENTRIES[request.within ?? ''] ?? [],
          total: (ENTRIES[request.within ?? ''] ?? []).length,
        },
      } as ToolResponse)
    }
    return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
  }
}

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

/** Bits UI opens its popover on pointerdown and selects on pointerup. */
const pointerDown = (el: Element) => {
  el.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }))
  flushSync()
}
const pick = (el: Element) => {
  el.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }))
  el.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
  flushSync()
}

const bar = (id: number) =>
  host.querySelector<HTMLButtonElement>(`.rule-slot[data-rule-id="${id}"] .bar`)!
const openRule = async (id: number) => {
  click(bar(id))
  await settle()
}

const rows = () => [...host.querySelectorAll<HTMLLIElement>('.statement')]
const editor = () => document.body.querySelector<HTMLElement>('.dialog.editor')!
const editorRows = () => [...editor().querySelectorAll<HTMLElement>('.editor-arg')]
const editorLabels = () => editorRows().map((row) => row.querySelector('.editor-arg-label')!.textContent)
const buttonByText = (root: Element, text: string) =>
  [...root.querySelectorAll('button')].find((button) => button.textContent === text)!
const ok = () => buttonByText(editor(), 'OK') as HTMLButtonElement

const type = (el: HTMLInputElement, value: string) => {
  el.value = value
  el.dispatchEvent(new Event('input', { bubbles: true }))
  flushSync()
}

describe('the statement editor form (#918)', () => {
  it('renders one labelled row per lookup parameter and no statement text', async () => {
    api.handler = handlerFor([
      summaryFor(
        1,
        'setup',
        contentFor([action('wait', [0.25, { enum: 'Wait', member: 'IGNORE_CONDITION' }])]),
      ),
    ])
    mountApp()
    await settle()
    await openRule(1)

    click(rows()[0])
    await settle()

    expect(editor(), 'the editor dialog is open').not.toBeNull()
    // The title is the session-locale call name; the read-only `Expr`
    // statement view is gone — the form rows carry the parameters.
    expect(editor().querySelector('.editor-title')!.textContent).toBe('Wait')
    expect(editor().querySelector('.editor-statement')).toBeNull()
    expect(editorLabels()).toEqual(['Duration', 'Wait Behavior'])

    const number = editor().querySelector<HTMLInputElement>('[data-arg-control="0"] input')!
    expect(number, 'the Duration slot is a numeric input').not.toBeNull()
    expect(number.type).toBe('number')
    expect(number.value).toBe('0.25')

    const picker = editor().querySelector<HTMLElement>(
      '[data-arg-control="1"] .picker-ctl',
    )!
    expect(picker, 'the enum slot is a picker').not.toBeNull()
    expect(picker.textContent).toContain('Ignore Condition')
  })

  it('opens with the clicked argument focused', async () => {
    api.handler = handlerFor([
      summaryFor(
        1,
        'setup',
        contentFor([
          action('wait', [0.25, { enum: 'Wait', member: 'IGNORE_CONDITION' }]),
          action('setCrouchEnabled', [{ call: 'eventPlayer', args: [] }, true]),
        ]),
      ),
    ])
    mountApp()
    await settle()
    await openRule(1)

    // The statement text's second argument span opens the editor on it.
    const arg = rows()[0].querySelector<HTMLElement>('[data-arg-path="1"]')!
    expect(arg, 'the Wait Behavior argument carries its path').not.toBeNull()
    click(arg)
    await settle()

    const row = editor().querySelector<HTMLElement>('[data-arg-control="1"]')!
    expect(row, 'the clicked slot has a row').not.toBeNull()
    const focused = document.activeElement as HTMLElement
    expect(
      row.contains(focused) || row === focused,
      `focus lands on the Wait Behavior control, got ${focused?.tagName}.${focused?.className}`,
    ).toBe(true)
  })

  it('stages a replace differing only in the edited number literal', async () => {
    const summaries = [
      summaryFor(
        1,
        'setup',
        contentFor([action('wait', [0.25, { enum: 'Wait', member: 'IGNORE_CONDITION' }])]),
      ),
    ]
    let editRequest: ToolRequest | null = null
    const base = handlerFor(summaries)
    api.handler = (request) => {
      if (request.op === 'editStatements') {
        editRequest = request
        return Promise.resolve({
          result: {
            ok: true,
            exit: 0,
            diagnostics: [],
            transaction: { edits: [{ kind: 'editStatements', source: 'demo.ws', source_identity: 'id-1', range: {}, new_text: 'x' }] },
          },
        } as ToolResponse)
      }
      return base(request)
    }
    api.apply = () => Promise.resolve({ ok: true, exit: 0, diagnostics: [] })

    mountApp()
    await settle()
    await openRule(1)
    click(rows()[0])
    await settle()

    const number = editor().querySelector<HTMLInputElement>('[data-arg-control="0"] input')!
    type(number, '2')
    click(ok())
    await settle()

    expect(
      document.body.querySelector('.dialog.editor'),
      'the editor closed',
    ).toBeNull()
    // Submit the staged edit; the request carries one `replace` whose
    // statement differs from the original only in the duration literal.
    click(
      host.querySelector<HTMLButtonElement>(
        '.toolbar button[aria-label="Pending changes"]',
      )!,
    )
    await settle()
    click(buttonByText(document.body.querySelector('.dialog.pending')!, 'Submit'))
    await settle()

    const request = editRequest as unknown as { edits: Record<string, unknown>[] }
    expect(request.edits).toHaveLength(1)
    expect(request.edits[0]).toMatchObject({ rule: 1, list: 'actions', index: 0, change: 'replace' })
    expect(request.edits[0].statement).toEqual({
      call: 'wait',
      args: [2, { enum: 'Wait', member: 'IGNORE_CONDITION' }],
    })
  })

  it('disables OK while a required slot is unset and shows "unset" in --warn', async () => {
    api.handler = handlerFor([
      summaryFor(1, 'setup', contentFor([action('setCrouchEnabled', [{ call: 'eventPlayer', args: [] }])])),
    ])
    mountApp()
    await settle()
    await openRule(1)
    click(rows()[0])
    await settle()

    // `enabled` is a required Boolean parameter with no argument in the
    // slot: the row reads unset and OK stays disabled until a literal is
    // picked.
    const slot = editor().querySelector<HTMLElement>('[data-arg-control="1"]')!
    expect(slot.classList.contains('unset')).toBe(true)
    expect(slot.querySelector('.picker-ctl')!.textContent).toContain('unset')
    expect(ok().disabled).toBe(true)

    // Choose True in the boolean picker; the staged arg fills the slot.
    pointerDown(slot.querySelector('.picker-ctl')!)
    await settle()
    const all = [...document.body.querySelectorAll<HTMLElement>('.picker-option')]
    expect(all.map((o) => o.textContent?.trim()), 'the two literals are listed').toEqual([
      'True',
      'False',
    ])
    pick(all.find((option) => option.textContent?.trim() === 'True')!)
    await settle()
    // The staged node is boolean `true`; the slot is set and OK frees.
    expect(editor().querySelector('.unset'), 'no required slot stays unset').toBeNull()
    expect(ok().disabled).toBe(false)
  })

  it('renders a call argument as its call name with nested parameter rows', async () => {
    api.handler = handlerFor([
      summaryFor(
        1,
        'setup',
        contentFor([
          action('createHudText', [
            { call: 'allPlayers', args: [{ enum: 'Team', member: 'ALL' }] },
          ]),
        ]),
      ),
    ])
    // `createHudText` needs a lookup row too — 11 parameters, arg 0
    // VisibleTo: Player|Array.
    LOOKUP.createHudText = [
      {
        identity: 'createHudText',
        kind: 'action',
        spelling: 'Create HUD Text',
        displayName: 'Create HUD Text',
        callable: {
          parameters: [
            { name: 'visibleTo', displayName: 'Visible To', type: 'Player|Array', required: true },
            { name: 'header', displayName: 'Header', type: 'Object|String', required: true },
          ],
        },
      },
    ]
    mountApp()
    await settle()
    await openRule(1)
    click(rows()[0])
    await settle()

    // The first argument row shows the nested call's name as plain text;
    // once the call's own `lookup` lands its parameters indent beneath.
    const first = editor().querySelector<HTMLElement>('[data-arg-control="0"]')!
    expect(first.querySelector('.editor-arg-value')!.textContent).toBe('All Players')
    const nested = editor().querySelector<HTMLElement>(
      '.editor-nested [data-arg-control="0.0"]',
    )
    expect(nested, 'allPlayers parameter rows nest under the call row').not.toBeNull()
    expect(nested!.querySelector('.editor-arg-label')!.textContent).toBe('Team')
  })

  it('renders a condition as left value, operator name, right value', async () => {
    api.handler = handlerFor([
      summaryFor(
        1,
        'setup',
        contentFor(
          [],
          [
            {
              value: {
                call: '>=',
                args: [
                  { call: 'globalVariable', args: [{ variable: 'score' }] },
                  3,
                ],
              },
            },
          ],
        ),
      ),
    ])
    mountApp()
    await settle()
    await openRule(1)
    click(rows()[0])
    await settle()

    expect(editor().querySelector('.editor-title')!.textContent).toBe('Condition')
    expect(editorLabels()).toEqual(['Value', 'Comparison', 'Value'])
    // The operator row shows the call name as plain text — #919 turns it
    // into a picker.
    const middle = editor().querySelector<HTMLElement>('[data-arg-control=""]')!
    expect(middle.querySelector('.editor-arg-value')!.textContent).toBe('>=')
    const right = editor().querySelector<HTMLInputElement>('[data-arg-control="1"] input')!
    expect(right.value).toBe('3')
  })

  it('renders a bare variable-access condition as its variable text', async () => {
    api.handler = handlerFor([
      summaryFor(
        1,
        'setup',
        contentFor(
          [],
          [{ value: { call: 'globalVariable', args: [{ variable: 'score' }] } }],
        ),
      ),
    ])
    mountApp()
    await settle()
    await openRule(1)
    click(rows()[0])
    await settle()

    // The single call row shows the variable-access text, not the raw id.
    const row = editor().querySelector<HTMLElement>('[data-arg-control=""]')!
    expect(row.querySelector('.editor-arg-value')!.textContent).toBe('Global.score')
  })

  it('renders a two-arg reserved call condition as its statement text', async () => {
    // `playerVariable`/`memberAccess` take the compare-shaped three-row
    // form; the operator row still shows the statement text, not the id.
    api.handler = handlerFor([
      summaryFor(
        1,
        'setup',
        contentFor(
          [],
          [
            {
              value: {
                call: 'playerVariable',
                args: [{ call: 'eventPlayer', args: [] }, { variable: 'aim' }],
              },
            },
            {
              value: {
                call: 'memberAccess',
                args: [
                  { call: 'globalVariable', args: [{ variable: 'm' }] },
                  { string: 'x' },
                ],
              },
            },
          ],
        ),
      ),
    ])
    mountApp()
    await settle()
    await openRule(1)

    click(rows()[0])
    await settle()
    let middle = editor().querySelector<HTMLElement>('[data-arg-control=""]')!
    expect(middle.querySelector('.editor-arg-value')!.textContent).toBe('Event Player.aim')
    click(editor().querySelector<HTMLElement>('.cancel')!)
    await settle()

    click(rows()[1])
    await settle()
    middle = editor().querySelector<HTMLElement>('[data-arg-control=""]')!
    expect(middle.querySelector('.editor-arg-value')!.textContent).toBe('Global.m.x')
  })
})
