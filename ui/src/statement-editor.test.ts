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
import { describe, expect, it, vi } from 'vitest'
import {
  api,
  action,
  buttonByText,
  click,
  contentFor,
  editor,
  editorLabels,
  handlerFor,
  LOOKUP,
  mountApp,
  ok,
  openRule,
  pick,
  pointerDown,
  root,
  rows,
  settle,
  summaryFor,
  type,
} from './lib/editor-harness'
import type { ToolRequest, ToolResponse } from './lib/types/generated/wright-agent-v1'

/** Wire the shared harness `api` object into the mocked `./lib/api`. */
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
    expect(editorLabels()).toEqual(['Action', 'Duration', 'Wait Behavior'])

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
      root().querySelector<HTMLButtonElement>(
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

    // The first argument row shows the nested call's name in its picker;
    // once the call's own `lookup` lands its parameters indent beneath.
    const first = editor().querySelector<HTMLElement>('[data-arg-control="0"]')!
    expect(first.querySelector('.picker-value')!.textContent).toBe('All Players')
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
    // The operator row is the comparison picker (#919) showing the call
    // name.
    const middle = editor().querySelector<HTMLElement>('[data-arg-control=""]')!
    expect(middle.querySelector('.picker-value')!.textContent).toBe('>=')
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

    // The single call row's picker shows the variable-access text, not
    // the raw id.
    const row = editor().querySelector<HTMLElement>('[data-arg-control=""]')!
    expect(row.querySelector('.picker-value')!.textContent).toBe('Global.score')
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
    expect(middle.querySelector('.picker-value')!.textContent).toBe('Event Player.aim')
    click(editor().querySelector<HTMLElement>('.cancel')!)
    await settle()

    click(rows()[1])
    await settle()
    middle = editor().querySelector<HTMLElement>('[data-arg-control=""]')!
    expect(middle.querySelector('.picker-value')!.textContent).toBe('Global.m.x')
  })
})
