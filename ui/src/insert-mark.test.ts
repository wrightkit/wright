// @vitest-environment jsdom
import { flushSync, mount, unmount } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App.svelte'
import type { RuleContent } from './lib/view'
import type { RuleSummary, ToolRequest, ToolResponse } from './lib/types/generated/wright-agent-v1'

const api = vi.hoisted(() => ({
  handler: null as null | ((request: ToolRequest) => Promise<ToolResponse>),
}))
vi.mock('./lib/api', () => ({
  sessionToken: 'test-token',
  call: (request: ToolRequest) => {
    if (!api.handler) return Promise.reject(new Error('test installed no api handler'))
    return api.handler(request)
  },
  apply: () => Promise.resolve({ ok: true, exit: 0, diagnostics: [] }),
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
): RuleContent['actions'][number] =>
  ({
    call,
    args,
    span: { start: { line: 1, col: 5 }, end: { line: 1, col: 20 } },
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

const WAIT_ENTRY = {
  identity: 'wait',
  kind: 'action',
  spelling: 'Wait',
  displayName: 'Wait',
  callable: {
    parameters: [{ name: 'time', displayName: 'Duration', type: 'Any', required: true }],
  },
}

/** A zero-argument `Number` value the `Any` Duration slot can pick. */
const GAME_TIME_ENTRY = {
  identity: 'gameTime',
  kind: 'value',
  spelling: 'Game Time',
  displayName: 'Game Time',
  callable: { returns: 'Number' },
}

let host: HTMLDivElement
let app: ReturnType<typeof mount> | undefined
let editRequest: ToolRequest | null

afterEach(() => {
  if (app) void unmount(app)
  app = undefined
  api.handler = null
  editRequest = null
  document.body.innerHTML = ''
})

const handlerFor =
  (summaries: RuleSummary[]) =>
  (request: ToolRequest): Promise<ToolResponse> => {
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
              : request.kind === 'value'
                ? [GAME_TIME_ENTRY]
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

const click = (el: Element) => {
  el.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  flushSync()
}
const pick = (el: Element) => {
  el.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }))
  el.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
  flushSync()
}

const col = () => host.querySelector<HTMLElement>('.col[data-list="actions"]')!
const mark = () => col().querySelector('.statement.pending-insert')
const pendingBadge = () =>
  host.querySelector('.toolbar button[aria-label="Pending changes"] .sq-badge')?.textContent ?? null
const buttonByText = (root: Element, text: string) =>
  [...root.querySelectorAll('button')].find((button) => button.textContent === text)!

/** Drive the `+` → pick `Wait` → fill Duration → OK staging gesture. */
async function stageWaitInsert(): Promise<void> {
  click(col().querySelector('.column-actions button[aria-label="Add action"]')!)
  await settle()
  const editor = document.body.querySelector<HTMLElement>('.dialog.editor')!
  editor.querySelector('[data-arg-control="action"] .picker-ctl')!.dispatchEvent(
    new MouseEvent('pointerdown', { bubbles: true }),
  )
  await settle()
  pick(
    [...document.body.querySelectorAll<HTMLElement>('.picker-option')].find(
      (el) => el.textContent?.trim() === 'Wait',
    )!,
  )
  await settle()
  // Duration is a value picker over `Any`; pick the zero-arg Number
  // call the fixture lists.
  editor.querySelector('[data-arg-control="0"] .picker-ctl')!.dispatchEvent(
    new MouseEvent('pointerdown', { bubbles: true }),
  )
  await settle()
  pick(
    [...document.body.querySelectorAll<HTMLElement>('.picker-option')].find(
      (el) => el.textContent?.trim() === 'Game Time',
    )!,
  )
  await settle()
  click(buttonByText(editor, 'OK'))
  await settle()
}

const mountRule = async (actions: RuleContent['actions']): Promise<void> => {
  api.handler = handlerFor([summaryFor(1, 'setup', contentFor(actions))])
  host = document.createElement('div')
  document.body.appendChild(host)
  app = mount(App, { target: host })
  await settle()
  click(host.querySelector('.rule-slot[data-rule-id="1"] .bar')!)
  await settle()
}

const submitStaged = async (): Promise<Record<string, unknown>[]> => {
  click(host.querySelector('.toolbar button[aria-label="Pending changes"]')!)
  await settle()
  click(buttonByText(document.body.querySelector<HTMLElement>('.dialog.pending')!, 'Submit'))
  await settle()
  return (editRequest as unknown as { edits: Record<string, unknown>[] }).edits
}

describe('the staged-insert mark when the anchor cannot host it', () => {
  it('renders after a hidden end tail', async () => {
    // [if, wait, end]: `end` is hidden, so a `+` at the column end
    // (index 3, the flat length) has no shown row with `i + 1 === 3` to
    // host the mark — it must still render, after the last shown row.
    await mountRule([action('if', [true]), action('wait', [0.016]), action('end')])
    await stageWaitInsert()

    const items = [...col().querySelectorAll('.statement')]
    const markAt = items.findIndex((el) => el.classList.contains('pending-insert'))
    expect(markAt, 'the placeholder rendered after the hidden end tail').toBe(2)
    expect(
      items[markAt]!.getAttribute('style'),
      'the new row lands at rule depth, past the end dedent',
    ).toContain('--depth: 0')
    expect(pendingBadge()).toBe('1')

    expect((await submitStaged())[0]).toMatchObject({
      rule: 1,
      list: 'actions',
      index: 3,
      change: 'insert',
    })
  })

  it('renders although the column filter hides the anchor', async () => {
    // Row 0 is selected, then the filter hides every shown row — the
    // mark at index 1 has no shown anchor and must still render.
    await mountRule(Array.from({ length: 35 }, () => action('kill')))

    click(col().querySelector<HTMLInputElement>('.statement .sel')!)
    await settle()
    const input = col().querySelector<HTMLInputElement>('input.col-filter')!
    input.value = 'zzz-no-match'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    flushSync()
    expect(
      col().querySelectorAll('.statement .sel'),
      'the filter hid every row',
    ).toHaveLength(0)

    await stageWaitInsert()

    expect(mark(), 'the placeholder rendered although the anchor is filter-hidden')
      .not.toBeNull()
    expect(pendingBadge()).toBe('1')

    expect((await submitStaged())[0]).toMatchObject({
      rule: 1,
      list: 'actions',
      index: 1,
      change: 'insert',
    })
  })
})
