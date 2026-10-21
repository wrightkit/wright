// @vitest-environment jsdom
/// <reference types="node" />
import { flushSync, mount, unmount } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App.svelte'
import type { RuleContent } from './lib/view'
import type {
  Finding,
  RuleSummary,
  SourceSpan,
  ToolRequest,
  ToolResponse,
} from './lib/types/generated/wright-agent-v1'

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

async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0))
    flushSync()
  }
}

const span = (startCol: number, endCol: number, line = 10): SourceSpan => ({
  file: 0,
  path: 'demo.ws',
  start: { line, col: startCol },
  end: { line, col: endCol },
})

const action = (call: string): RuleContent['actions'][number] =>
  ({ call, args: [], span: span(1, 20, 20) }) as RuleContent['actions'][number]

const condition = (at: SourceSpan): RuleContent['conditions'][number] =>
  ({
    value: { call: 'compare', args: [1, { call: '==', args: [] }, 2] },
    span: at,
  }) as RuleContent['conditions'][number]

const contentFor = (
  conditions: RuleContent['conditions'],
  actions: RuleContent['actions'],
): RuleContent => ({
  format: 'workshop-rs/rule-content-v1',
  event: { id: 'ev.a' },
  disabled: false,
  conditions,
  actions,
})

const summaryFor = (id: number, name: string, content: RuleContent): RuleSummary =>
  ({ id, name, event: `ev.${id}`, span: null, content }) as unknown as RuleSummary

const finding = (extra: Partial<Finding>): Finding =>
  ({
    code: 'a-finding',
    severity: 'info',
    message: 'a finding',
    span: null,
    rule: null,
    ruleAddress: 'demo',
    action: null,
    value: null,
    evidence: 'heuristic',
    boundedness: null,
    repeated: null,
    ...extra,
  }) as Finding

let host: HTMLDivElement
let app: ReturnType<typeof mount> | undefined

afterEach(() => {
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
const click = (el: HTMLElement) => {
  el.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  flushSync()
}

const handlerFor =
  (
    summaries: RuleSummary[],
    findings: Finding[] = [],
    findingsResponse?: ToolResponse,
  ) =>
  (request: ToolRequest): Promise<ToolResponse> => {
    if (request.op === 'project') {
      return Promise.resolve({ result: { sourceFiles: ['demo.ws'] } } as ToolResponse)
    }
    if (request.op === 'rules' && request.all === true && request.content === true) {
      return Promise.resolve({ result: { rules: summaries } } as ToolResponse)
    }
    if (request.op === 'findings') {
      return Promise.resolve(
        findingsResponse ?? ({ result: { findings } } as ToolResponse),
      )
    }
    if (request.op === 'lookup') {
      return Promise.resolve({
        result: { language: 'workshop', owner: 'workshop-rs', entries: [] },
      } as ToolResponse)
    }
    return Promise.reject(new Error(`unexpected request: ${JSON.stringify(request)}`))
  }

describe('findings in place (#921)', () => {
  it('marks the rule bar with the most severe mark and the total count', async () => {
    const content = contentFor(
      [condition(span(5, 40))],
      [action('setGlobalVariable'), action('wait')],
    )
    api.handler = handlerFor(
      [summaryFor(7, 'scored rule', content), summaryFor(9, 'clean rule', content)],
      [
        finding({ rule: 7, action: 0, severity: 'info', code: 'a-info' }),
        finding({ rule: 7, action: 1, severity: 'warning', code: 'a-warning' }),
        finding({ rule: 7, span: span(9, 20), severity: 'error', code: 'an-error' }),
      ],
    )

    mountApp()
    await settle()

    const bar = slot(7)!.querySelector('.bar')!
    const mark = bar.querySelector<HTMLElement>('.finding-mark')!
    expect(mark.classList.contains('severity-error')).toBe(true)
    expect(mark.querySelector('.finding-count')?.textContent).toBe('1')
    expect(slot(9)!.querySelector('.finding-mark')).toBeNull()
    expect(mark.tagName).toBe('SPAN')
    expect(bar.querySelector('button')).toBeNull()
  })

  it('opens a hover card on the bar mark listing the rule\'s findings by severity', async () => {
    const content = contentFor([condition(span(5, 40))], [action('wait')])
    api.handler = handlerFor(
      [summaryFor(7, 'scored rule', content)],
      [
        finding({ rule: 7, action: 0, severity: 'info', code: 'the-info', message: 'info one' }),
        finding({ rule: 7, span: span(9, 20), severity: 'error', code: 'the-error', message: 'error one' }),
      ],
    )

    mountApp()
    await settle()

    const card = slot(7)!.querySelector('.finding-card')!
    const rows = [...card.querySelectorAll<HTMLElement>('.card-row')]
    expect(rows.map((row) => row.className)).toEqual([
      'card-row severity-error',
      'card-row severity-info',
    ])
    expect(rows[0].querySelector('.card-code')?.textContent).toBe('the-error')
    expect(rows[0].querySelector('.card-msg')?.textContent).toBe('error one')
    expect(rows[1].querySelector('.card-code')?.textContent).toBe('the-info')
  })

  it('marks a statement row: 3 px severity bar and a mark at its right end', async () => {
    const content = contentFor(
      [condition(span(5, 40)), condition(span(50, 80))],
      [action('setGlobalVariable'), action('wait'), action('end')],
    )
    api.handler = handlerFor(
      [summaryFor(7, 'scored rule', content)],
      [
        finding({ rule: 7, action: 0, severity: 'warning', code: 'act-find' }),
        finding({ rule: 7, span: span(55, 70), severity: 'error', code: 'cond-find' }),
        finding({ rule: 7, span: span(5, 9, 999), severity: 'info', code: 'bar-find' }),
      ],
    )

    mountApp()
    await settle()
    click(slot(7)!.querySelector<HTMLButtonElement>('.bar')!)
    await settle()

    const statements = [...slot(7)!.querySelectorAll<HTMLElement>('.statement')]
    expect(statements).toHaveLength(4)

    const [cond0, cond1, act0, act1] = statements
    expect(cond1!.classList.contains('has-findings')).toBe(true)
    expect(cond1!.querySelector('.finding-mark.severity-error')).not.toBeNull()
    expect(cond0!.querySelector('.finding-mark')).toBeNull()

    expect(act0!.classList.contains('has-findings')).toBe(true)
    expect(act0!.querySelector('.finding-mark.severity-warning')).not.toBeNull()
    expect(act1!.querySelector('.finding-mark')).toBeNull()

    const barMark = slot(7)!.querySelector('.bar .finding-mark')!
    expect(barMark.querySelector('.finding-count')?.textContent).toBe('1')

    const rows = act0!.querySelectorAll('.finding-card .card-row')
    expect(rows).toHaveLength(1)
    expect(rows[0]!.querySelector('.card-code')?.textContent).toBe('act-find')
  })

  it('sets the row severity bar to the severity token, not the severity name', async () => {
    const content = contentFor(
      [condition(span(5, 40))],
      [action('setGlobalVariable'), action('wait')],
    )
    api.handler = handlerFor(
      [summaryFor(7, 'scored rule', content)],
      [
        finding({ rule: 7, action: 0, severity: 'warning', code: 'a-warning' }),
        finding({ rule: 7, action: 1, severity: 'info', code: 'an-info' }),
      ],
    )

    mountApp()
    await settle()
    click(slot(7)!.querySelector<HTMLButtonElement>('.bar')!)
    await settle()

    const rows = [...slot(7)!.querySelectorAll<HTMLElement>('.statement.has-findings')]
    expect(rows).toHaveLength(2)
    expect(rows[0]!.style.getPropertyValue('--finding').trim()).toBe('var(--warn)')
    expect(rows[1]!.style.getPropertyValue('--finding').trim()).toBe('var(--muted)')
  })

  it('shows a findings refusal on the page instead of silent unmarked rules', async () => {
    const content = contentFor([condition(span(5, 40))], [action('wait')])
    api.handler = handlerFor([summaryFor(7, 'scored rule', content)], [], {
      error: { code: 'findings-unavailable', message: 'the findings read failed' },
    } as ToolResponse)

    mountApp()
    await settle()

    const banner = host.querySelector<HTMLElement>('.findings-refusal')
    expect(banner).not.toBeNull()
    expect(banner!.textContent).toContain('findings-unavailable')
    expect(banner!.textContent).toContain('the findings read failed')
    expect(slot(7)!.querySelector('.finding-mark')).toBeNull()
  })
})
