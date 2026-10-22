// @vitest-environment jsdom
/**
 * The simulation panel's run path (#836): Start sends `simulate` with the
 * project identity and the room's scenario, a trace renders its outcome
 * and globals, and a refusal shows its message while the previous result
 * is kept.
 */
import { flushSync, mount, unmount } from 'svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import SimulatePanel from './lib/SimulatePanel.svelte'
import { chromeStrings } from './lib/chrome'
import type {
  ProjectResult,
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

const t = chromeStrings('en-US')
const project = {
  inputIdentity: 'identity-1',
  origin: { kind: 'workshop', locale: 'en-US' },
  settings: null,
} as unknown as ProjectResult

const trace = {
  outcome: { status: 'complete' },
  logs: [{ tick: 4, text: 'ran' }],
  globals: { score: { type: 'number', value: 3 } },
  players: {},
  hud: [],
  missingInputs: [],
  expectations: [],
}

let host: HTMLDivElement
let app: ReturnType<typeof mount> | undefined

beforeEach(() => {
  host = document.createElement('div')
  document.body.append(host)
})

afterEach(() => {
  if (app) unmount(app)
  host.remove()
  document.body.innerHTML = ''
  api.handler = null
})

// Bits UI portals the dialog into `document.body`.
function panel() {
  app = mount(SimulatePanel, {
    target: host,
    props: { open: true, project, heroes: [], t, onclose: () => {} },
  })
  flushSync()
  return document.body.querySelector<HTMLElement>('.dialog')!
}

function start() {
  document.body.querySelector<HTMLButtonElement>('.sim-run .primary')!.click()
}

describe('the simulation panel run path (#836)', () => {
  it('shows the trace outcome, globals, and logs after Start', async () => {
    api.handler = async (request) => {
      expect(request).toMatchObject({
        op: 'simulate',
        inputIdentity: 'identity-1',
        scenario: { scenarioVersion: 1, seconds: 60, players: [] },
      })
      return { result: { trace } } as unknown as ToolResponse
    }
    const dialog = panel()
    await settle()
    start()
    await settle()
    expect(dialog.querySelector('.outcome strong')?.textContent).toBe('complete')
    expect(dialog.textContent).toContain('score')
    expect(dialog.textContent).toContain('3')
    expect(dialog.textContent).toContain('ran')
  })

  it('shows a refusal and keeps the previous result', async () => {
    let calls = 0
    api.handler = async () => {
      calls += 1
      if (calls === 1) return { result: { trace } } as unknown as ToolResponse
      return {
        error: { code: 'edit-stale-source', message: 'the source changed' },
      } as unknown as ToolResponse
    }
    const dialog = panel()
    await settle()
    start()
    await settle()
    expect(dialog.querySelector('.outcome strong')?.textContent).toBe('complete')

    start()
    await settle()
    expect(dialog.querySelector('.refusal')?.textContent).toBe('the source changed')
    // The earlier trace is still rendered — a refusal changes nothing else.
    expect(dialog.querySelector('.outcome strong')?.textContent).toBe('complete')
    expect(dialog.textContent).toContain('ran')
  })
})
