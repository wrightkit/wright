import { describe, expect, it } from 'vitest'
import {
  type ParamFact,
  admitsCall,
  argAt,
  argFields,
  callNode,
  conditionFields,
  currentId,
  defaultArgs,
  defaultNode,
  enumNode,
  fillGaps,
  operationNode,
  paramFacts,
  slotSet,
  stringNode,
  subroutineNode,
  unsetRequired,
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
    expect(fields[1]).toMatchObject({ kind: 'number', path: [1] })
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

  it('flags the slots only #919\'s pickers can fill', () => {
    const params: ParamFact[] = [
      { name: 'variable', type: 'Variable' },
      {
        name: 'operation',
        type: 'Operation',
        content: { operations: ['add', 'subtract'] },
      },
      { name: 'value', type: 'Any' },
    ]
    const empty = argFields(params, { call: 'modifyGlobalVariable', args: [] })
    expect(empty.map((field) => field.insertable)).toEqual([false, false, undefined])
    const filled = argFields(params, {
      call: 'modifyGlobalVariable',
      args: [{ variable: 'x' }, { call: 'add', args: [] }, 1],
    })
    expect(filled.map((field) => field.insertable)).toEqual([undefined, undefined, undefined])
    const literal = argFields(params, {
      call: 'modifyGlobalVariable',
      args: [{ string: 'x' }, { call: 'drop', args: [] }, 1],
    })
    expect(literal.map((field) => field.insertable)).toEqual([false, false, undefined])
    const folded = argFields(
      [
        { name: 'player', type: 'Player|Array', content: { fold: { call: 'playerVariable', arg: 0 } } },
        { name: 'variable', type: 'Variable', content: { fold: { call: 'playerVariable', arg: 1 } } },
        { name: 'value', type: 'Any' },
      ],
      { call: 'setPlayerVariableAtIndex', args: [] },
    )
    expect(folded.map((field) => field.insertable)).toEqual([undefined, false, undefined])
  })

  it('flags a Subroutine slot unless the program declares subroutines', () => {
    const params: ParamFact[] = [{ name: 'subroutine', type: 'Subroutine' }]
    const noSubs = argFields(params, { call: 'startRule', args: [] }, undefined, false)
    expect(noSubs[0].insertable).toBe(false)
    const withSubs = argFields(params, { call: 'startRule', args: [] }, undefined, true)
    expect(withSubs[0].insertable).toBeUndefined()
    const filled = argFields(params, { call: 'startRule', args: [{ subroutine: 'addScore' }] })
    expect(filled[0].insertable).toBeUndefined()
  })

  it('flags a bare Variable and a folded global variable position alike', () => {
    const params: ParamFact[] = [
      { name: 'variable', type: 'Variable' },
      { name: 'globalVariable', type: 'Global Variable' },
      { name: 'playerVariable', type: 'Player Variable' },
    ]
    const fields = argFields(params, { call: 'demo', args: [] })
    expect(fields.map((field) => field.insertable)).toEqual([false, false, false])
    const folded = argFields(
      [
        { name: 'variable', type: 'Variable', content: { fold: { call: 'globalVariable', arg: 0 } } },
      ],
      { call: 'chaseAtRate', args: [{ call: 'globalVariable', args: [] }] },
    )
    expect(folded[0].insertable).toBe(false)
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
    expect(stringNode('hello')).toEqual({ string: 'hello' })
    expect(defaultNode('Wait.IGNORE_CONDITION')).toEqual({
      enum: 'Wait',
      member: 'IGNORE_CONDITION',
    })
    expect(defaultNode('0.5')).toBe(0.5)
    expect(defaultNode('null')).toBeNull()
    expect(defaultNode('Free-form')).toEqual({ string: 'Free-form' })
    expect(stringNode('x')).toEqual({ string: 'x' })
    expect(subroutineNode('addScore')).toEqual({ subroutine: 'addScore' })
  })

  it('reads a call-id default on a non-String parameter as a call node', () => {
    expect(defaultNode('eventPlayer', { type: 'Player' })).toEqual({
      call: 'eventPlayer',
      args: [],
    })
    expect(defaultNode('allPlayers', { type: 'Player|Array' })).toEqual({
      call: 'allPlayers',
      args: [],
    })
    expect(defaultNode('Hello', { type: 'String' })).toEqual({ string: 'Hello' })
  })
})

describe('defaultArgs', () => {
  it('seeds lookup defaults and pads a crossed required slot with null', () => {
    const args = defaultArgs([
      { name: 'time', type: 'Any', required: true },
      {
        name: 'waitBehavior',
        type: 'Wait',
        required: false,
        default: 'Wait.IGNORE_CONDITION',
        enum: { domain: 'Wait' },
      },
    ])
    expect(args).toHaveLength(2)
    expect(args[0]).toBeNull()
    expect(args[1]).toEqual({ enum: 'Wait', member: 'IGNORE_CONDITION' })
  })

  it('omits a trailing run of unset slots', () => {
    const args = defaultArgs([
      { name: 'a', type: 'Any', required: true },
      { name: 'b', type: 'Any', required: false },
      { name: 'c', type: 'Any', required: false },
    ])
    expect(args).toHaveLength(0)
  })

  it('materializes the folded call a variable-target action seeds', () => {
    const args = defaultArgs([
      { name: 'player', type: 'Player|Array', content: { fold: { call: 'playerVariable', arg: 0 } } },
      { name: 'variable', type: 'Variable', content: { fold: { call: 'playerVariable', arg: 1 } } },
      { name: 'index', type: 'Number|Boolean' },
      { name: 'value', type: 'Object|Array' },
    ])
    expect(args).toHaveLength(1)
    expect(args[0]).toEqual({ call: 'playerVariable', args: [] })
  })

  it('omits positions past the last declared default', () => {
    // `setGlobalVariable`: a required Variable and a required Value,
    // neither declaring a default — the fresh call stays fully unset.
    expect(
      defaultArgs([
        { name: 'variable', type: 'Variable', required: true },
        { name: 'value', type: 'Any', required: true },
      ]),
    ).toEqual([])
  })

  it('materializes a fold group so member writes have a call', () => {
    expect(
      defaultArgs([
        {
          name: 'player',
          type: 'Player',
          required: true,
          content: { fold: { call: 'playerVariable', arg: 0 } },
        },
        {
          name: 'variable',
          type: 'Variable',
          required: true,
          content: { fold: { call: 'playerVariable', arg: 1 } },
        },
      ]),
    ).toEqual([{ call: 'playerVariable', args: [] }])
    expect(
      defaultArgs([
        {
          name: 'player',
          type: 'Player',
          required: true,
          content: { fold: { call: 'playerVariable', arg: 0 } },
        },
        {
          name: 'variable',
          type: 'Variable',
          required: true,
          default: 'aim',
          content: { fold: { call: 'playerVariable', arg: 1 } },
        },
      ]),
    ).toEqual([{ call: 'playerVariable', args: [null, { variable: 'aim' }] }])
  })

  it('callNode builds the fresh call with its default args', () => {
    expect(
      callNode('allPlayers', [
        {
          name: 'array',
          type: 'Team',
          required: false,
          default: 'Team.ALL',
          enum: { domain: 'Team' },
        },
      ]),
    ).toEqual({ call: 'allPlayers', args: [{ enum: 'Team', member: 'ALL' }] })
  })
})

describe('literal controls', () => {
  it('classifies a number, a string, and a boolean node to their inputs', () => {
    const fields = argFields(
      [
        { name: 'time', type: 'Any', displayName: 'Duration' },
        { name: 'text', type: 'String', displayName: 'Text' },
        { name: 'enabled', type: 'Boolean', displayName: 'Enabled' },
      ],
      { call: 'demo', args: [0.25, { string: 'hi' }, false] },
    )
    expect(fields.map((field) => field.kind)).toEqual(['number', 'string', 'boolean'])
  })

  it('marks a call argument a `call` row and nests its lookup params', () => {
    const nested: Record<string, ParamFact[]> = {
      allPlayers: [
        {
          name: 'array',
          displayName: 'Team',
          type: 'Team',
          required: false,
          default: 'Team.ALL',
          enum: { domain: 'Team' },
        },
      ],
    }
    const fields = argFields(
      [{ name: 'visibleTo', type: 'Player|Array', displayName: 'Visible To', required: true }],
      { call: 'createHudText', args: [{ call: 'allPlayers', args: [{ enum: 'Team', member: 'ALL' }] }] },
      (call) => nested[call] ?? null,
    )
    expect(fields[0].kind).toBe('call')
    expect(fields[0].nested?.[0]).toMatchObject({
      kind: 'enum',
      label: 'Team',
      path: [0, 0],
    })
  })

  it('fills a gap before the edited slot with the parameter default', () => {
    const fields = argFields(
      [
        { name: 'time', type: 'Any', displayName: 'Duration', required: true },
        {
          name: 'waitBehavior',
          displayName: 'Wait Behavior',
          required: false,
          default: 'Wait.IGNORE_CONDITION',
          enum: { domain: 'Wait' },
        },
        { name: 'note', displayName: 'Note', required: true, type: 'Number' },
      ],
      { call: 'demo', args: [] },
    )
    const filled = fillGaps({ call: 'demo', args: [] }, fields, [2])
    expect(filled.args).toEqual([null, { enum: 'Wait', member: 'IGNORE_CONDITION' }])
    expect(fillGaps({ call: 'demo', args: [] }, fields, [0]).args).toEqual([])
  })

  it('fills a crossed gap with the call node a call-id default declares', () => {
    const fields = argFields(
      [
        { name: 'player', displayName: 'Player', type: 'Player', default: 'eventPlayer' },
        { name: 'button', displayName: 'Button', type: 'Button', enum: { domain: 'Button' } },
      ],
      { call: 'isButtonHeld', args: [] },
    )
    const filled = fillGaps({ call: 'isButtonHeld', args: [] }, fields, [1])
    expect(filled.args).toEqual([{ call: 'eventPlayer', args: [] }])
  })

  it('flags a required empty slot while a defaulted one stays set', () => {
    const fields = argFields(
      [
        { name: 'time', type: 'Any', displayName: 'Duration', required: true },
        {
          name: 'waitBehavior',
          displayName: 'Wait Behavior',
          required: false,
          default: 'Wait.IGNORE_CONDITION',
          enum: { domain: 'Wait' },
        },
      ],
      { call: 'wait', args: [] },
    )
    expect(unsetRequired({ call: 'wait', args: [] }, fields)).toHaveLength(1)
    expect(slotSet({ call: 'wait', args: [1] }, fields[0])).toBe(true)
    expect(slotSet({ call: 'wait', args: [1] }, fields[1])).toBe(false)
    expect(unsetRequired({ call: 'wait', args: [1] }, fields)).toHaveLength(0)
  })
})

describe('conditionFields', () => {
  const compare: ParamFact[] = [
    { name: 'value', displayName: 'Value', type: 'Any', required: true },
    { name: 'comparison', displayName: 'Comparison', type: '__Operator__', required: true },
    { name: 'value', displayName: 'Value', type: 'Any', required: true },
  ]

  it('splits a two-arg operator call into left, operator, right rows', () => {
    const fields = conditionFields([], compare, {
      call: '>=',
      args: [{ call: 'globalVariable', args: [{ variable: 'score' }] }, 3],
    })
    expect(fields.map((field) => field.label)).toEqual(['Value', 'Comparison', 'Value'])
    expect(fields[0].path).toEqual([0])
    expect(fields[0].kind).toBe('call')
    expect(fields[1]).toMatchObject({ kind: 'comparison', call: '>=', path: [] })
    expect(fields[2]).toMatchObject({ kind: 'number', path: [1] })
  })

  it('renders a non-comparison value as a single row', () => {
    const fields = conditionFields([], compare, { call: 'isTrueForAll', args: [true] })
    expect(fields).toHaveLength(1)
    expect(fields[0].kind).toBe('call')
  })

  it('carries the value node on a bare variable-access condition row', () => {
    const value = { call: 'globalVariable', args: [{ variable: 'score' }] }
    const fields = conditionFields([], compare, value)
    expect(fields).toHaveLength(1)
    expect(fields[0]).toMatchObject({ kind: 'call', call: 'globalVariable' })
    expect(fields[0].node).toEqual(value)
  })

  it('carries the value node on the operator row of a two-arg reserved call', () => {
    for (const value of [
      {
        call: 'playerVariable',
        args: [{ call: 'eventPlayer', args: [] }, { variable: 'aim' }],
      },
      {
        call: 'memberAccess',
        args: [
          { call: 'globalVariable', args: [{ variable: 'm' }] },
          { string: 'x' },
        ],
      },
    ]) {
      const fields = conditionFields([], compare, value)
      expect(fields).toHaveLength(3)
      expect(fields[1]).toMatchObject({ kind: 'call', call: value.call, path: [] })
      expect(fields[1].node).toEqual(value)
    }
  })

  it('uses the value call\'s own parameters when lookup declares them', () => {
    const fields = conditionFields(
      [{ name: 'array', displayName: 'Array', type: 'Array', required: true }],
      compare,
      { call: 'isTrueForAll', args: [true] },
    )
    expect(fields).toHaveLength(1)
    expect(fields[0]).toMatchObject({ label: 'Array', kind: 'boolean', path: [0] })
  })
})

describe('paramFacts', () => {
  it('reads callable.parameters from a lookup entry', () => {
    expect(paramFacts(entry(modifyParams))).toHaveLength(3)
    expect(paramFacts(null)).toEqual([])
  })
})

describe('admitsCall', () => {
  it('admits the returns types a slot type declares', () => {
    expect(admitsCall('Any', 'Number')).toBe(true)
    expect(admitsCall('Player|Array', 'Player')).toBe(true)
    expect(admitsCall('Player|Array', 'Array')).toBe(true)
    expect(admitsCall('Player|Array', 'Number')).toBe(false)
    expect(admitsCall('Object|Array', 'Object')).toBe(true)
    expect(admitsCall('Object', 'Array')).toBe(false)
    expect(admitsCall(undefined, 'Number')).toBe(true)
    expect(admitsCall('Any', undefined)).toBe(true)
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
