/**
 * The statement editor test harness: the jsdom `App` mount and the
 * fixture `lookup`/`listEntries`/`symbols` rows the editor's form tests
 * share (`statement-editor.test.ts`,
 * `statement-editor-pickers.test.ts`). Each test file keeps its own
 * `api` mock — `vi.hoisted` must live in the test file itself. Each
 * importing test file gets its own copy of the module state — vitest
 * isolates files.
 */
import { flushSync, mount, unmount } from 'svelte'
import { afterEach } from 'vitest'
import App from '../App.svelte'
import type { RuleContent } from './view'
import type { RuleSummary, ToolRequest, ToolResponse } from './types/generated/wright-agent-v1'

/**
 * The test file's own `api` mock installs itself here so the harness's
 * `afterEach` can reset it.
 */
export const api: {
  handler: null | ((request: ToolRequest) => Promise<ToolResponse>)
  apply: null | ((transaction: unknown) => Promise<unknown>)
} = { handler: null, apply: null }

/** Drain pending promise continuations, then flush Svelte's effects and DOM. */
export async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0))
    flushSync()
  }
}

export const action = (
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

export const contentFor = (
  actions: RuleContent['actions'],
  conditions: RuleContent['conditions'] = [],
): RuleContent => ({
  format: 'workshop-rs/rule-content-v1',
  event: { id: 'ongoingGlobal' },
  disabled: false,
  conditions,
  actions,
})

export const summaryFor = (id: number, name: string, content?: RuleContent): RuleSummary =>
  ({ id, name, span: null, ...(content ? { content } : {}) }) as unknown as RuleSummary

/**
 * The fixture `lookup` rows the form's labels come from — `callable
 * .parameters` facts as the service emits them (`displayName` is the
 * localized parameter label the Workshop editor shows).
 */
export const LOOKUP: Record<string, object[]> = {
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
  setGlobalVariable: [
    {
      identity: 'setGlobalVariable',
      kind: 'structural',
      displayName: 'Set Global Variable',
      callable: {
        parameters: [
          { name: 'variable', displayName: 'Variable', type: 'Variable', required: true },
          { name: 'value', displayName: 'Value', type: 'Any', required: true },
        ],
      },
    },
  ],
  setPlayerVariable: [
    {
      identity: 'setPlayerVariable',
      kind: 'structural',
      displayName: 'Set Player Variable',
      callable: {
        parameters: [
          { name: 'player', displayName: 'Player', type: 'Player|Array', required: true },
          { name: 'variable', displayName: 'Variable', type: 'Variable', required: true },
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

export const ENTRIES: Record<string, object[]> = {
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

/** The `listEntries` catalogs #919's pickers list, keyed by `kind`. */
export const LISTINGS: Record<string, object[]> = {
  action: [
    {
      identity: 'wait',
      kind: 'action',
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
    {
      identity: 'setGlobalVariable',
      kind: 'action',
      displayName: 'Set Global Variable',
      callable: {
        parameters: [
          { name: 'variable', displayName: 'Variable', type: 'Variable', required: true },
          { name: 'value', displayName: 'Value', type: 'Any', required: true },
        ],
      },
    },
    {
      identity: 'setCrouchEnabled',
      kind: 'action',
      displayName: 'Set Crouch Enabled',
      callable: {
        parameters: [
          { name: 'player', displayName: 'Player', type: 'Player|Array', required: true },
          { name: 'enabled', displayName: 'Enabled', type: 'Boolean', required: true },
        ],
      },
    },
  ],
  value: [
    { identity: 'eventPlayer', kind: 'value', displayName: 'Event Player', callable: { returns: 'Player' } },
    { identity: 'allPlayers', kind: 'value', displayName: 'All Players', callable: { returns: 'Array', parameters: [{ name: 'array', displayName: 'Team', type: 'Team', required: false, default: 'Team.ALL' }] } },
    { identity: 'getMaxHealth', kind: 'value', displayName: 'Max Health', callable: { returns: 'Number', parameters: [{ name: 'player', displayName: 'Player', type: 'Player|Array', required: true }] } },
    { identity: 'true', kind: 'value', displayName: 'True', callable: { returns: 'Boolean' } },
    { identity: 'compare', kind: 'value', displayName: 'Compare' },
    { identity: 'null', kind: 'value', displayName: 'Null', callable: { returns: 'Null' } },
  ],
}

export function handlerFor(summaries: RuleSummary[], symbols: object[] = []) {
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
      const entries =
        typeof request.within === 'string'
          ? (ENTRIES[request.within] ?? [])
          : (LISTINGS[request.kind as string] ?? [])
      return Promise.resolve({
        result: {
          language: 'workshop',
          owner: 'workshop-rs',
          entries,
          total: entries.length,
        },
      } as ToolResponse)
    }
    if (request.op === 'symbols') {
      return Promise.resolve({ result: { symbols } } as ToolResponse)
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

export function mountApp(): void {
  host = document.createElement('div')
  document.body.appendChild(host)
  app = mount(App, { target: host })
}

export const click = (el: Element) => {
  el.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  flushSync()
}

/** Bits UI opens its popover on pointerdown and selects on pointerup. */
export const pointerDown = (el: Element) => {
  el.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }))
  flushSync()
}
export const pick = (el: Element) => {
  el.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }))
  el.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
  flushSync()
}

export const bar = (id: number) =>
  host.querySelector<HTMLButtonElement>(`.rule-slot[data-rule-id="${id}"] .bar`)!
export const openRule = async (id: number) => {
  click(bar(id))
  await settle()
}

export const rows = () => [...host.querySelectorAll<HTMLLIElement>('.statement')]
export const root = () => host
export const editor = () => document.body.querySelector<HTMLElement>('.dialog.editor')!
export const editorRows = () => [...editor().querySelectorAll<HTMLElement>('.editor-arg')]
export const editorLabels = () =>
  editorRows().map((row) => row.querySelector('.editor-arg-label')!.textContent)
export const buttonByText = (root: Element, text: string) =>
  [...root.querySelectorAll('button')].find((button) => button.textContent === text)!
export const ok = () => buttonByText(editor(), 'OK') as HTMLButtonElement

export const type = (el: HTMLInputElement, value: string) => {
  el.value = value
  el.dispatchEvent(new Event('input', { bubbles: true }))
  flushSync()
}

/** Open a picker's option list; options render under `.picker-option`. */
export const pickerOptions = async (row: HTMLElement): Promise<HTMLElement[]> => {
  pointerDown(row.querySelector<HTMLElement>('.picker-ctl')!)
  await settle()
  return [...document.body.querySelectorAll<HTMLElement>('.picker-option')]
}
