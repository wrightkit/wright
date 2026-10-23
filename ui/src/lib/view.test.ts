import { describe, expect, it } from 'vitest'
import {
  debugEnabled,
  ruleStateFromError,
  ruleStateFromResponse,
  ruleStateFromSummary,
  type RuleContent,
} from './view'
import type { RuleSummary, ToolResponse } from './types/generated/wright-agent-v1'

const content: RuleContent = {
  format: 'workshop-rs/rule-content-v1',
  event: { id: 'global' },
  disabled: false,
  conditions: [],
  actions: [],
}

const summary = (extra: Record<string, unknown> = {}): RuleSummary =>
  ({ id: 0, name: 'rule', ...extra }) as unknown as RuleSummary

describe('debugEnabled', () => {
  it('is off without the flag', () => {
    expect(debugEnabled('')).toBe(false)
    expect(debugEnabled('?other=1')).toBe(false)
  })

  it('is on for ?debug in any position or form', () => {
    expect(debugEnabled('?debug')).toBe(true)
    expect(debugEnabled('?debug=1')).toBe(true)
    expect(debugEnabled('?x=1&debug')).toBe(true)
  })
})

describe('ruleStateFromSummary', () => {
  it('maps carried content to ready', () => {
    expect(ruleStateFromSummary(summary({ content }))).toEqual({
      phase: 'ready',
      content,
    })
  })

  it('degrades missing content to the rule-content refusal', () => {
    const state = ruleStateFromSummary(summary())
    expect(state.phase).toBe('refused')
    if (state.phase === 'refused') {
      expect(state.refusal.code).toBe('rule-content-unavailable')
    }
  })
})

describe('ruleStateFromResponse', () => {
  it('keeps a service refusal verbatim so the pane renders it in place', () => {
    const response = {
      error: { code: 'rule-content-unavailable', message: 'opaque node' },
    } as ToolResponse
    expect(ruleStateFromResponse(response)).toEqual({
      phase: 'refused',
      refusal: { code: 'rule-content-unavailable', message: 'opaque node' },
    })
  })

  it('maps a rule+copy result to ready', () => {
    const response = { result: { rules: [summary({ content })] } } as ToolResponse
    expect(ruleStateFromResponse(response)).toEqual({ phase: 'ready', content })
  })

  it('degrades a missing rule to the same refusal shape', () => {
    const response = { result: [] } as ToolResponse
    const state = ruleStateFromResponse(response)
    expect(state.phase).toBe('refused')
    if (state.phase === 'refused') {
      expect(state.refusal.code).toBe('rule-content-unavailable')
    }
  })
})

describe('ruleStateFromError', () => {
  it('maps an endpoint failure to a refused state, never a throw', () => {
    const state = ruleStateFromError(new Error('HTTP 403'))
    expect(state).toEqual({
      phase: 'refused',
      refusal: { code: 'endpoint-unreachable', message: 'HTTP 403' },
    })
    expect(ruleStateFromError('string failure').phase).toBe('refused')
  })
})
