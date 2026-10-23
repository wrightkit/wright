// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest'
import {
  api,
  action,
  buttonByText,
  click,
  contentFor,
  editor,
  handlerFor,
  mountApp,
  ok,
  openRule,
  pick,
  pointerDown,
  root,
  settle,
  summaryFor,
} from './lib/editor-harness'
import type { ToolRequest, ToolResponse } from './lib/types/generated/wright-agent-v1'

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

describe('inserting a block opener from the column heading', () => {
  it('stages the opener and its matching end in the same insert edit', async () => {
    // `editStatements` refuses an action stream with an unclosed opener
    // (`unbalanced-block`), so the `+` form stages `If` and `End`
    // together — the request carries one insert edit of two statements.
    let editRequest: ToolRequest | null = null
    const base = handlerFor([summaryFor(1, 'setup', contentFor([action('wait', [0.016])]))])
    api.handler = (request) => {
      if (request.op === 'editStatements') {
        editRequest = request
        return Promise.resolve(statements())
      }
      return base(request)
    }
    api.apply = () => Promise.resolve({ ok: true, exit: 0, diagnostics: [] })

    mountApp()
    await settle()
    await openRule(1)

    click(root().querySelector('.col[data-list="actions"] .column-actions button')!)
    await settle()

    pointerDown(editor().querySelector('[data-arg-control="action"] .picker-ctl')!)
    await settle()
    pick(
      [...document.body.querySelectorAll<HTMLElement>('.picker-option')].find(
        (el) => el.textContent?.trim() === 'If',
      )!,
    )
    await settle()

    expect(editor().textContent).toContain('Condition')
    pointerDown(editor().querySelector('[data-arg-control="0"] .picker-ctl')!)
    await settle()
    pick(
      [...document.body.querySelectorAll<HTMLElement>('.picker-option')].find(
        (el) => el.textContent?.trim() === 'True',
      )!,
    )
    await settle()
    expect(ok().disabled).toBe(false)
    click(ok())
    await settle()

    click(root().querySelector('.toolbar button[aria-label="Pending changes"]')!)
    await settle()
    const dialog = document.body.querySelector<HTMLElement>('.dialog.pending')!
    click(buttonByText(dialog, 'Submit'))
    await settle()

    const request = editRequest as unknown as { edits: Record<string, unknown>[] }
    expect(request.edits).toEqual([
      {
        rule: 1,
        list: 'actions',
        index: 1,
        change: 'insert',
        statements: [
          { call: 'if', args: [true] },
          { call: 'end', args: [] },
        ],
      },
    ])
  })
})
