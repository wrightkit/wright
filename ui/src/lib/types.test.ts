import { describe, expect, it } from 'vitest'
import type { WorkshopRsRuleContentV1 } from './types/generated/rule-content-v1'
import type {
  CapabilitiesRequest,
  CapabilitiesResult,
  ToolRequest,
  ToolResponse,
} from './types/generated/wright-agent-v1'

// Compile-time check that the generated contract types match the schemas:
// assigning a non-conforming object must be a type error (see svelte-check).
const capabilities: CapabilitiesRequest = { op: 'capabilities' }
const request: ToolRequest = capabilities

const rule: WorkshopRsRuleContentV1 = {
  format: 'workshop-rs/rule-content-v1',
  event: { id: 'global' },
  disabled: false,
  conditions: [],
  actions: [],
}

const capabilitiesResult: CapabilitiesResult = {
  name: 'wright',
  version: '0.0.0',
  contract: 'wright-result/v1',
  agent_contract: 'wright-agent/v1',
  operations: ['capabilities'],
  result_schemas: {},
  languages: [],
  profiles: [],
  athena_version: '0.0.0',
}

const response: ToolResponse = { result: capabilitiesResult }

describe('generated contract types', () => {
  it('accepts a capabilities request as a ToolRequest', () => {
    expect(request.op).toBe('capabilities')
  })

  it('round-trips a minimal rule-content document', () => {
    expect(rule.format).toBe('workshop-rs/rule-content-v1')
    expect(rule.actions).toEqual([])
  })

  it('carries the result envelope alias', () => {
    expect(response).toHaveProperty('result')
  })
})
