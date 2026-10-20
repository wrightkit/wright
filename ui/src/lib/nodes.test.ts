import { describe, expect, it } from 'vitest'
import { nodeClass, statementDepths } from './nodes'
import type { Action } from './types/generated/rule-content-v1'

describe('nodeClass', () => {
  it('classifies every rule-content-v1 node kind', () => {
    // Primitive literal forms.
    expect(nodeClass(3.5)).toBe('number')
    expect(nodeClass(true)).toBe('literal')
    expect(nodeClass(false)).toBe('literal')
    expect(nodeClass(null)).toBe('literal')
    // Tagged forms.
    expect(nodeClass({ string: 'hi' })).toBe('string')
    expect(nodeClass({ localizedString: 'hud.header' })).toBe('string')
    expect(nodeClass({ enum: 'Wait', member: 'IGNORE_CONDITION' })).toBe('enum')
    expect(nodeClass({ variable: 'score' })).toBe('variable')
    expect(nodeClass({ subroutine: 'shared' })).toBe('variable')
  })

  it('classifies call nodes by their lookup kind', () => {
    const call = { call: 'setGlobalVariable', args: [] }
    expect(nodeClass(call, 'action')).toBe('action')
    expect(nodeClass(call, 'structural')).toBe('action')
    expect(nodeClass({ call: 'add', args: [] }, 'value')).toBe('value')
    expect(nodeClass({ call: '==', args: [1, 2] }, 'operator')).toBe('operator')
    // An unresolvable id still reads as a value expression.
    expect(nodeClass(call)).toBe('value')
  })

  it('classifies the documented variable-access reserved calls', () => {
    expect(nodeClass({ call: 'globalVariable', args: [{ variable: 'x' }] })).toBe('variable')
    expect(
      nodeClass({ call: 'playerVariable', args: [{ call: 'eventPlayer', args: [] }, { variable: 'x' }] }),
    ).toBe('variable')
    expect(
      nodeClass({ call: 'memberAccess', args: [1, { string: 'm' }] }),
    ).toBe('variable')
    // Other reserved forms keep the call-colour default.
    expect(nodeClass({ call: 'rawWorkshopAction', args: [] }, 'action')).toBe('action')
  })
})

describe('statementDepths', () => {
  const calls = (...ids: string[]): Action[] =>
    ids.map((call) => ({ call, args: [] }) as Action)

  it('indents bodies inside openers and hides End markers', () => {
    expect(
      statementDepths(calls('if', 'setGlobalVariable', 'elseIf', 'setGlobalVariable', 'else', 'setGlobalVariable', 'end', 'wait')),
    ).toEqual([0, 1, 0, 1, 0, 1, null, 0])
  })

  it('tracks nested loops and for-blocks', () => {
    expect(
      statementDepths(
        calls('while', 'forGlobalVariable', 'wait', 'end', 'end', 'wait'),
      ),
    ).toEqual([0, 1, 2, null, null, 0])
  })

  it('keeps a stray End from driving depth negative', () => {
    expect(statementDepths(calls('end', 'wait'))).toEqual([null, 0])
  })

  it('treats disabled openers as openers', () => {
    const actions: Action[] = [
      { call: 'while', args: [], disabled: true },
      { call: 'wait', args: [] },
      { call: 'end', args: [] },
    ]
    expect(statementDepths(actions)).toEqual([0, 1, null])
  })
})
