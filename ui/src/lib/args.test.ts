import { describe, expect, it } from 'vitest'
import {
  type ParamFact,
  argAt,
  argFields,
  currentId,
  enumNode,
  operationNode,
  paramFacts,
  variableNode,
  withArg,
} from './args'
import {
  buildRequest,
  keyFor,
  setDisabled,
  stageStatement,
  type PendingChange,
} from './edit'
import type { LookupEntry } from './types/generated/wright-agent-v1'
import type { Action } from './types/generated/rule-content-v1'

const entry = (parameters: ParamFact[]): LookupEntry =>
  ({ identity: 'c', kind: 'action', spelling: 'c', displayName: 'c', callable: { parameters } }) as LookupEntry

const modifyParams: ParamFact[] = [
  { name: 'variable', type: 'Variable' },
  {
    name: 'operation',
    type: 'Operation',
    content: { operations: ['add', 'subtract', 'removeFromArrayByIndex'] },
  },
  { name: 'value', type: 'Any' },
]

const modify: Action = {
  call: 'modifyGlobalVariable',
  args: [{ variable: 'score' }, { call: 'add', args: [] }, 5],
}

describe('argFields', () => {
  it('marks a Variable arg as a variable field scoped by the call', () => {
    const fields = argFields(
      [{ name: 'variable' }, { name: 'value' }],
      { call: 'setPlayerVariable', args: [{ variable: 'kills' }, 0] },
    )
    expect(fields[0]).toMatchObject({ kind: 'variable', scope: 'playerVariable', path: [0] })
    expect(fields[1]).toMatchObject({ kind: 'other', path: [1] })
  })

  it('offers exactly the operations the parameter declares', () => {
    const fields = argFields(modifyParams, modify)
    expect(fields[1].kind).toBe('operation')
    expect(fields[1].operations).toEqual(['add', 'subtract', 'removeFromArrayByIndex'])
  })

  it('maps fold members inside the folded target call', () => {
    const fields = argFields(
      [
        { name: 'variable', content: { fold: { call: 'globalVariable', arg: 0 } } },
        { name: 'destination' },
        { name: 'duration' },
      ],
      {
        call: 'chaseOverTime',
        args: [{ call: 'globalVariable', args: [{ variable: 'souls' }] }, 100, 10],
      },
    )
    expect(fields[0]).toMatchObject({ kind: 'variable', scope: 'globalVariable', path: [0, 0] })
    expect(fields[1].path).toEqual([1])
    expect(fields[2].path).toEqual([2])
  })

  it('marks an enum arg with the domain the parameter declares', () => {
    const fields = argFields(
      [{ name: 'behavior', enum: { domain: 'Wait' } }],
      { call: 'wait', args: [{ enum: 'Wait', member: 'IGNORE_CONDITION' }] },
    )
    expect(fields[0]).toMatchObject({ kind: 'enum', domain: 'Wait' })
  })

  it('reads the node inside a folded call through argAt', () => {
    expect(argAt(modify.args, [1])).toEqual({ call: 'add', args: [] })
    expect(argAt([{ call: 'globalVariable', args: [{ variable: 'x' }] }], [0, 0])).toEqual({
      variable: 'x',
    })
  })
})

describe('withArg', () => {
  it('replaces a top-level argument without mutating the action', () => {
    const next = withArg(modify, [2], 9)
    expect(next.args[2]).toBe(9)
    expect(modify.args[2]).toBe(5)
  })

  it('replaces inside a folded call', () => {
    const chase: Action = {
      call: 'chaseOverTime',
      args: [{ call: 'globalVariable', args: [{ variable: 'souls' }] }, 100, 10],
    }
    const next = withArg(chase, [0, 0], variableNode('kills'))
    expect(argAt(next.args, [0, 0])).toEqual({ variable: 'kills' })
    expect(argAt(chase.args, [0, 0])).toEqual({ variable: 'souls' })
  })
})

describe('node constructors', () => {
  it('produce the nodes editStatements accepts', () => {
    expect(variableNode('kills')).toEqual({ variable: 'kills' })
    expect(operationNode('removeFromArrayByIndex')).toEqual({
      call: 'removeFromArrayByIndex',
      args: [],
    })
    expect(enumNode('Wait.RESTART')).toEqual({ enum: 'Wait', member: 'RESTART' })
    expect(currentId({ enum: 'Wait', member: 'RESTART' })).toBe('Wait.RESTART')
  })
})

describe('paramFacts', () => {
  it('reads callable.parameters from a lookup entry', () => {
    expect(paramFacts(entry(modifyParams))).toHaveLength(3)
    expect(paramFacts(null)).toEqual([])
  })
})

describe('stageStatement', () => {
  it('stages a replace whose statement carries the edited args verbatim', () => {
    const map = new Map<string, PendingChange>()
    const original = { ...modify, span: { start: {}, end: {} } }
    stageStatement(
      map,
      0,
      'setup',
      'actions',
      0,
      original as never,
      { ...modify, args: [{ variable: 'kills' }, { call: 'add', args: [] }, 5] } as never,
    )
    const request = buildRequest(map, 'digest', 3)
    expect(request.edits).toHaveLength(1)
    const edit = request.edits[0]
    expect(edit.change).toBe('replace')
    expect(edit.statement).toEqual({
      call: 'modifyGlobalVariable',
      args: [{ variable: 'kills' }, { call: 'add', args: [] }, 5],
    })
  })

  it('unstages when the rebuilt statement equals the original', () => {
    const map = new Map<string, PendingChange>()
    stageStatement(map, 0, 'r', 'actions', 0, modify as never, modify as never)
    expect(map.has(keyFor(0, 'actions', 0))).toBe(false)
  })

  it('re-saving the staged statement keeps the pending change', () => {
    // A reopened editor seeds its args from the staged change; an
    // unchanged save must re-stage it, not drop it.
    const map = new Map<string, PendingChange>()
    const staged = {
      ...modify,
      args: [{ variable: 'kills' }, { call: 'subtract', args: [] }, 5],
    }
    stageStatement(map, 0, 'r', 'actions', 0, modify as never, staged as never)
    stageStatement(map, 0, 'r', 'actions', 0, modify as never, staged as never)
    expect(map.get(keyFor(0, 'actions', 0))?.statement).toEqual(staged)
  })

  it('a batch enable toggles disabled on the staged change, args kept', () => {
    const map = new Map<string, PendingChange>()
    const staged = {
      ...modify,
      args: [{ variable: 'kills' }, { call: 'subtract', args: [] }, 5],
      disabled: true,
    }
    stageStatement(map, 0, 'r', 'actions', 0, modify as never, staged as never)
    setDisabled(map, 0, 'r', 'actions', 0, modify as never, false)
    expect(map.get(keyFor(0, 'actions', 0))?.statement).toEqual({
      call: 'modifyGlobalVariable',
      args: [{ variable: 'kills' }, { call: 'subtract', args: [] }, 5],
    })
  })
})
