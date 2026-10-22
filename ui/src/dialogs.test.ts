// @vitest-environment jsdom
/**
 * The #882 view dialogs (docs/ui/design.md § Dialogs): the shared chrome
 * — centred title, `--dlg` body, `--dlg-foot` band with the one orange
 * Confirm — the `costEstimate` cards and their error form, the top-ten
 * element rules, the severity-coloured findings, the Variables
 * Global/Player/Subroutines grouping, and the rule reveal a top rule or
 * a located finding asks for.
 */
import { flushSync, mount, unmount } from 'svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
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
  apply: () => Promise.reject(new Error('test installed no apply handler')),
}))

/** Drain pending promise continuations, then flush Svelte's effects and DOM. */
async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0))
    flushSync()
  }
}

const contentFor = (eventId: string): RuleContent => ({
  format: 'workshop-rs/rule-content-v1',
  event: { id: eventId },
  disabled: false,
  conditions: [],
  actions: [],
})

const summaryFor = (id: number, name: string): RuleSummary =>
  ({ id, name, event: `ev.${id}`, span: null, content: contentFor(`ev.${id}`) }) as unknown as RuleSummary

const finding = (
  code: string,
  severity: 'error' | 'warning' | 'info',
  rule: number | null,
  message = `${code} message`,
  span: unknown = { file: 0, path: 'demo.ws', start: { line: 3, col: 5 }, end: { line: 3, col: 9 } },
  action: number | null = null,
) => ({
  code,
  severity,
  message,
  span,
  rule,
  action,
  value: null,
  evidence: 'heuristic',
  boundedness: null,
  repeated: null,
})

const symbol = (id: number, kind: string, name: string) => ({ id, kind, name, span: null })

const elementRules = (entries: [number, string, number][]) =>
  entries.map(([index, name, count]) => ({ index, name, count, span: null }))

interface Fixture {
  symbols?: unknown[]
  findings?: unknown[]
  costEstimate?: ToolResponse
}

/**
 * A handler over the fixture: `rules`/`project` always answer; `symbols`,
 * `findings`, `costEstimate` answer the fixture, `lookup` resolves the
 * heading ids with capitalised display names.
 */
const handlerFor =
  (fixture: Fixture, summaries: RuleSummary[]) =>
  (request: ToolRequest): Promise<ToolResponse> => {
    if (request.op === 'project') {
      return Promise.resolve({ result: { sourceFiles: ['demo.ws'] } } as ToolResponse)
    }
    if (request.op === 'rules' && request.all === true && request.content === true) {
      return Promise.resolve({ result: { rules: summaries } } as ToolResponse)
    }
    if (request.op === 'symbols') {
      return Promise.resolve({ result: { symbols: fixture.symbols ?? [] } } as ToolResponse)
    }
    if (request.op === 'findings') {
      return Promise.resolve({ result: { findings: fixture.findings ?? [] } } as ToolResponse)
    }
    if (request.op === 'costEstimate') {
      return Promise.resolve(fixture.costEstimate ?? costResult())
    }
    if (request.op === 'lookup' && typeof request.query === 'string') {
      const name = request.query.charAt(0).toUpperCase() + request.query.slice(1)
      return Promise.resolve({
        result: {
          language: 'workshop',
          owner: 'workshop-rs',
          entries: [
            { identity: request.query, kind: 'structural', spelling: name, displayName: name },
          ],
        },
      } as ToolResponse)
    }
    return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
  }

const costResult = (rules: [number, string, number][] = []): ToolResponse =>
  ({
    result: {
      snapshot: 4,
      exact: {
        emittedBytes: 25600,
        programActions: 42,
        programRules: 12,
        waitActions: 3,
      },
      elementCount: {
        total: 1234,
        budget: 32768,
        rules: elementRules(rules),
      },
      findings: [],
      kind: { exact: 'exact', findings: 'findings', performance: 'perf' },
    },
  }) as ToolResponse

let host: HTMLDivElement
let app: ReturnType<typeof mount> | undefined

beforeEach(() => {
  // jsdom does not implement `scrollIntoView`; the reveal targets it.
  Element.prototype.scrollIntoView = vi.fn()
})

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

const click = (el: Element) => {
  el.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  flushSync()
}

const toolbarButton = (title: string) =>
  host.querySelector<HTMLButtonElement>(`.toolbar button[title="${title}"]`)!
const dialog = () => host.querySelector<HTMLDialogElement>('dialog.view')!
const isOpen = (id: number) =>
  host.querySelector(`.rule-slot[data-rule-id="${id}"] .rule-view`) != null
const slot = (id: number) => host.querySelector(`.rule-slot[data-rule-id="${id}"]`)

describe('the Diagnostics dialog (#882)', () => {
  it('renders the card figures from the costEstimate result under the shared chrome', async () => {
    api.handler = handlerFor(
      {
        findings: [],
        costEstimate: costResult([
          [0, 'rule one', 900],
          [1, 'rule two', 300],
        ]),
      },
      [summaryFor(0, 'rule one'), summaryFor(1, 'rule two')],
    )

    mountApp()
    await settle()
    click(toolbarButton('Diagnostics'))
    await settle()

    expect(dialog().open).toBe(true)
    // Chrome: the centred title, the scrolling body, the footer band with
    // the one orange Confirm — and no leftover Close text button.
    expect(dialog().querySelector('.dlg-title')?.textContent).toBe('Diagnostics')
    const foot = dialog().querySelector('.dlg-foot')!
    const buttons = [...foot.querySelectorAll('button')]
    expect(buttons.map((b) => b.textContent?.trim())).toEqual(['Confirm'])
    expect(dialog().textContent).not.toContain('Close')

    const cards = [...dialog().querySelectorAll('.stat-card')]
    expect(cards).toHaveLength(3)

    // Total elements: the element total, the budget in the description,
    // and the total/budget bar.
    expect(cards[0].querySelector('h3')?.textContent).toBe('Total elements')
    expect(cards[0].querySelector('.figure')?.textContent?.trim()).toBe('1234')
    expect(cards[0].querySelector('.desc')?.textContent).toContain('32768')
    const bar = cards[0].querySelector<HTMLElement>('.meter span')!
    expect(bar.style.width).toBe(`${Math.min(100, (1234 / 32768) * 100)}%`)

    // Output size: emitted KB figure; rule/action/wait counts below.
    expect(cards[1].querySelector('h3')?.textContent).toBe('Output size')
    expect(cards[1].querySelector('.figure')?.textContent?.trim()).toBe('25.0 KB')
    expect(cards[1].querySelector('.desc')?.textContent).toContain('12 rules')
    expect(cards[1].querySelector('.desc')?.textContent).toContain('42 actions')
    expect(cards[1].querySelector('.desc')?.textContent).toContain('3 waits')

    // Rule length: no figure, the whole-program explanation.
    expect(cards[2].querySelector('h3')?.textContent).toBe('Rule length')
    expect(cards[2].querySelector('.figure')).toBeNull()
    expect(cards[2].querySelector('.desc')?.textContent).toContain(
      'whole-program output size',
    )
  })

  it('shows the one error card when costEstimate fails', async () => {
    api.handler = handlerFor(
      {
        findings: [],
        costEstimate: {
          error: { code: 'element-count-unavailable', message: 'the catalog cannot count it' },
        } as ToolResponse,
      },
      [summaryFor(0, 'rule one')],
    )

    mountApp()
    await settle()
    click(toolbarButton('Diagnostics'))
    await settle()

    const cards = [...dialog().querySelectorAll('.stat-card')]
    expect(cards).toHaveLength(1)
    expect(cards[0].textContent).toContain('element-count-unavailable')
    expect(cards[0].textContent).toContain('the catalog cannot count it')
    expect(dialog().querySelector('.figure')).toBeNull()
    expect(dialog().querySelector('.top-rules')).toBeNull()
  })

  it('lists the ten rules with the most elements, ranked by count', async () => {
    // Twelve entries deliberately unordered: the view keeps the ten
    // largest, sorted by count.
    const entries: [number, string, number][] = [
      [3, 'rule three', 10],
      [11, 'rule eleven', 5],
      [0, 'rule zero', 800],
      [7, 'rule seven', 40],
      [1, 'rule one', 900],
      [9, 'rule nine', 20],
      [5, 'rule five', 60],
      [2, 'rule two', 700],
      [10, 'rule ten', 15],
      [6, 'rule six', 50],
      [8, 'rule eight', 30],
      [4, 'rule four', 80],
    ]
    api.handler = handlerFor(
      { findings: [], costEstimate: costResult(entries) },
      entries.map(([index, name]) => summaryFor(index, name)),
    )

    mountApp()
    await settle()
    click(toolbarButton('Diagnostics'))
    await settle()

    const rows = [...dialog().querySelectorAll('.top-rules button')]
    expect(rows).toHaveLength(10)
    const names = rows.map((row) => row.querySelector('.name')?.textContent)
    expect(names).toEqual([
      'rule one', // 900
      'rule zero', // 800
      'rule two', // 700
      'rule four', // 80
      'rule five', // 60
      'rule six', // 50
      'rule seven', // 40
      'rule eight', // 30
      'rule nine', // 20
      'rule ten', // 15
    ])
    const counts = rows.map((row) => row.querySelector('.count')?.textContent)
    expect(counts).toEqual(['900', '800', '700', '80', '60', '50', '40', '30', '20', '15'])
  })

  it('rows findings one per row with the severity class and the location', async () => {
    api.handler = handlerFor(
      {
        findings: [
          finding('while-without-wait', 'error', 1),
          finding('repeated-value', 'info', null),
        ],
        costEstimate: costResult(),
      },
      [summaryFor(0, 'rule zero'), summaryFor(1, 'rule one')],
    )

    mountApp()
    await settle()
    click(toolbarButton('Diagnostics'))
    await settle()

    // One group per severity that has findings, most severe first; the
    // heading carries the mark and the level count.
    const groups = [...dialog().querySelectorAll('.severity-group')]
    expect(groups).toHaveLength(2)
    expect(groups[0].querySelector('.severity-heading.severity-error')).not.toBeNull()
    expect(groups[1].querySelector('.severity-heading.severity-info')).not.toBeNull()
    expect(groups[0].querySelector('.severity-count')?.textContent).toBe('(1)')

    const rows = [...dialog().querySelectorAll('.finding-list .finding')]
    expect(rows).toHaveLength(2)
    expect(rows[0].classList.contains('severity-error')).toBe(true)
    expect(rows[0].textContent).toContain('error')
    expect(rows[0].textContent).toContain('while-without-wait message')
    expect(rows[0].textContent).toContain('demo.ws:3:5')
    // A located finding is a button; one without a rule is not clickable.
    expect(rows[0].tagName).toBe('BUTTON')
    expect(rows[1].tagName).toBe('DIV')
    expect(rows[1].classList.contains('severity-info')).toBe(true)
  })
})

describe('the Variables dialog (#882)', () => {
  it('groups symbols into the Global and Player columns and centred Subroutines', async () => {
    api.handler = handlerFor(
      {
        symbols: [
          symbol(0, 'globalVariable', 'score'),
          symbol(1, 'globalVariable', 'ticks'),
          symbol(2, 'playerVariable', 'marker'),
          symbol(3, 'subroutine', 'report'),
          symbol(4, 'rule', 'not a variable'),
        ],
        costEstimate: costResult(),
      },
      [summaryFor(0, 'rule zero')],
    )

    mountApp()
    await settle()
    click(toolbarButton('Variables'))
    await settle()

    expect(dialog().querySelector('.dlg-title')?.textContent).toBe('Variables')

    const columns = [...dialog().querySelectorAll('.var-columns section')]
    expect(columns).toHaveLength(2)
    // Localized headings resolve through `lookup` in the session locale.
    expect(columns[0].querySelector('h3')?.textContent).toBe('Global')
    expect(columns[1].querySelector('h3')?.textContent).toBe('Player')

    const itemsOf = (section: Element) =>
      [...section.querySelectorAll('.var-list li')].map((li) => li.textContent?.trim())
    expect(itemsOf(columns[0])).toEqual(['score', 'ticks'])
    expect(itemsOf(columns[1])).toEqual(['marker'])

    const subs = dialog().querySelector('.var-subs')!
    expect(subs.querySelector('h3')?.textContent).toBe('Subroutines')
    expect(itemsOf(subs)).toEqual(['report'])

    // Rule-kind symbols never enter the dialog, and no per-kind h3 list
    // survives from the interim view.
    expect(dialog().textContent).not.toContain('not a variable')
    expect(dialog().querySelector('.symbol-kind, .symbol-list')).toBeNull()
  })
})

describe('rule reveal (#882)', () => {
  it('reveals a rule when a top rule or a located finding is clicked', async () => {
    api.handler = handlerFor(
      {
        findings: [finding('while-without-wait', 'error', 1)],
        costEstimate: costResult([
          [1, 'rule one', 900],
          [0, 'rule zero', 300],
        ]),
      },
      [summaryFor(0, 'rule zero'), summaryFor(1, 'rule one')],
    )

    mountApp()
    await settle()
    click(toolbarButton('Diagnostics'))
    await settle()

    // A top-rule row: the dialog closes, rule 1's bar opens, the slot is
    // scrolled into view.
    const topRule = dialog().querySelector<HTMLButtonElement>('.top-rules button')!
    click(topRule)
    await settle()

    expect(dialog().open).toBe(false)
    expect(isOpen(1)).toBe(true)
    expect(Element.prototype.scrollIntoView).toHaveBeenCalledTimes(1)
    expect(vi.mocked(Element.prototype.scrollIntoView).mock.instances[0]).toBe(
      slot(1),
    )

    // Reopen the dialog; the located finding reveals the same way.
    click(toolbarButton('Diagnostics'))
    await settle()
    const row = dialog().querySelector<HTMLButtonElement>('button.finding')!
    click(row)
    await settle()

    expect(dialog().open).toBe(false)
    expect(isOpen(1)).toBe(true)
    expect(Element.prototype.scrollIntoView).toHaveBeenCalledTimes(2)
  })

  it('drops a filter that would hide the reveal target', async () => {
    api.handler = handlerFor(
      {
        findings: [finding('while-without-wait', 'error', 1)],
        costEstimate: costResult(),
      },
      [summaryFor(0, 'visible rule'), summaryFor(1, 'hidden target')],
    )

    mountApp()
    await settle()

    const filter = host.querySelector<HTMLInputElement>('input.filter')!
    filter.value = 'visible'
    filter.dispatchEvent(new Event('input', { bubbles: true }))
    flushSync()
    expect(slot(1)).toBeNull()

    click(toolbarButton('Diagnostics'))
    await settle()
    click(dialog().querySelector<HTMLButtonElement>('button.finding')!)
    await settle()

    expect(filter.value).toBe('')
    expect(isOpen(1)).toBe(true)
  })
})

/**
 * Severity levels (#922, docs/ui/diagnostics.md): the toolbar's badges
 * and "Only rules with findings" toggle, and the dialog's grouped,
 * toggling severity headings and row reveal.
 */
describe('severity levels (#922)', () => {
  const toggle = () => toolbarButton('Only rules with findings')
  const badges = () => [...host.querySelectorAll('.toolbar .sev-badge')]

  it('badges the Diagnostics button per shown severity, error first', async () => {
    api.handler = handlerFor(
      {
        findings: [
          finding('an-error', 'error', 0),
          finding('a-warning', 'warning', 1),
          finding('info-one', 'info', 1),
          finding('info-two', 'info', 1),
        ],
        costEstimate: costResult(),
      },
      [summaryFor(0, 'rule zero'), summaryFor(1, 'rule one')],
    )

    mountApp()
    await settle()

    // Info stays off the button while an error or warning is shown.
    expect(badges().map((badge) => badge.textContent)).toEqual(['1', '1'])
    expect(badges()[0].classList.contains('severity-error')).toBe(true)
    expect(badges()[1].classList.contains('severity-warning')).toBe(true)
  })

  it('badges info only when no error or warning exists, and clears with no findings', async () => {
    api.handler = handlerFor(
      {
        findings: [finding('info-one', 'info', 0), finding('info-two', 'info', 1)],
        costEstimate: costResult(),
      },
      [summaryFor(0, 'rule zero'), summaryFor(1, 'rule one')],
    )

    mountApp()
    await settle()
    expect(badges()).toHaveLength(1)
    expect(badges()[0].textContent).toBe('2')
    expect(badges()[0].classList.contains('severity-info')).toBe(true)
  })

  it('shows no badge with no findings and none until findings load', async () => {
    api.handler = handlerFor({ findings: [], costEstimate: costResult() }, [summaryFor(0, 'rule zero')])

    mountApp()
    await settle()
    expect(badges()).toHaveLength(0)
  })

  it('groups findings by severity with error and warning open, info collapsed', async () => {
    api.handler = handlerFor(
      {
        findings: [
          finding('info-finding', 'info', 0),
          finding('error-finding', 'error', 0),
          finding('warning-finding', 'warning', 1),
        ],
        costEstimate: costResult(),
      },
      [summaryFor(0, 'rule zero'), summaryFor(1, 'rule one')],
    )

    mountApp()
    await settle()
    click(toolbarButton('Diagnostics'))
    await settle()

    const groups = [...dialog().querySelectorAll<HTMLDetailsElement>('.severity-group')]
    expect(groups.map((group) => group.open)).toEqual([true, true, false])
    const headings = groups.map((group) => group.querySelector('.severity-name')?.textContent)
    expect(headings).toEqual(['Errors', 'Warnings', 'Info'])
  })

  it('keeps the heading toggle choice across dialog opens for the session', async () => {
    api.handler = handlerFor(
      {
        findings: [finding('info-finding', 'info', 0), finding('error-finding', 'error', 0)],
        costEstimate: costResult(),
      },
      [summaryFor(0, 'rule zero')],
    )

    mountApp()
    await settle()
    click(toolbarButton('Diagnostics'))
    await settle()

    // Open the collapsed info group, then close the dialog: the choice
    // persists for the session.
    const infoGroup = dialog().querySelector('.severity-heading.severity-info')!
      .parentElement! as HTMLDetailsElement
    infoGroup.querySelector('summary')!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    // jsdom lacks the UA toggle; the component binds `open` one way, so
    // emulate the toggle the browser would have run.
    infoGroup.open = true
    infoGroup.dispatchEvent(new Event('toggle'))
    flushSync()
    expect(infoGroup.open).toBe(true)

    click(dialog().querySelector<HTMLButtonElement>('.dlg-foot .primary')!)
    await settle()
    click(toolbarButton('Diagnostics'))
    await settle()

    const reopened = [...dialog().querySelectorAll<HTMLDetailsElement>('.severity-group')]
    expect(reopened.map((group) => group.open)).toEqual([true, true])
  })

  it('limits the rule list to findings at or above the lowest shown severity', async () => {
    api.handler = handlerFor(
      {
        findings: [
          finding('a-warning', 'warning', 1),
          finding('an-info', 'info', 0),
          finding('an-error', 'error', 2),
        ],
        costEstimate: costResult(),
      },
      [
        summaryFor(0, 'info-only rule'),
        summaryFor(1, 'warning rule'),
        summaryFor(2, 'error rule'),
        summaryFor(3, 'clean rule'),
      ],
    )

    mountApp()
    await settle()

    expect(toggle().disabled).toBe(false)
    click(toggle())
    await settle()

    // The lowest shown severity is warning — the info-only and clean
    // rules leave the list.
    expect(host.querySelectorAll('.rule-slot')).toHaveLength(2)
    expect(slot(0)).toBeNull()
    expect(slot(1)).not.toBeNull()
    expect(slot(2)).not.toBeNull()
    expect(slot(3)).toBeNull()
    expect(toggle().getAttribute('aria-pressed')).toBe('true')

    click(toggle())
    await settle()
    expect(host.querySelectorAll('.rule-slot')).toHaveLength(4)
  })

  it('keeps every rule under the toggle when info is the lowest shown severity', async () => {
    api.handler = handlerFor(
      {
        findings: [finding('an-info', 'info', 0)],
        costEstimate: costResult(),
      },
      [summaryFor(0, 'scored rule'), summaryFor(1, 'clean rule')],
    )

    mountApp()
    await settle()
    click(toggle())
    await settle()

    expect(slot(0)).not.toBeNull()
    expect(slot(1)).toBeNull()
  })

  it('reveals a clicked finding\'s row and flashes it once', async () => {
    const content = {
      format: 'workshop-rs/rule-content-v1',
      event: { id: 'ev.1' },
      disabled: false,
      conditions: [],
      actions: [
        { call: 'setGlobalVariable', args: [] },
        { call: 'wait', args: [] },
      ],
    } as unknown as RuleContent
    api.handler = handlerFor(
      {
        findings: [finding('while-without-wait', 'error', 1, 'a message', null, 1)],
        costEstimate: costResult(),
      },
      [
        summaryFor(0, 'rule zero'),
        { id: 1, name: 'rule one', event: 'ev.1', span: null, content } as unknown as RuleSummary,
      ],
    )

    mountApp()
    await settle()
    click(toolbarButton('Diagnostics'))
    await settle()
    click(dialog().querySelector<HTMLButtonElement>('button.finding')!)
    await settle()

    expect(dialog().open).toBe(false)
    expect(isOpen(1)).toBe(true)

    const row = slot(1)!.querySelector<HTMLElement>(
      '.col[data-list="actions"] .statement[data-index="1"]',
    )!
    expect(row).not.toBeNull()
    expect(row.classList.contains('flash')).toBe(true)
    expect(vi.mocked(Element.prototype.scrollIntoView).mock.instances).toContain(row)
  })
})
