import { describe, expect, it } from 'vitest'
import { nodeClass, statementDepths, statementMatches, statementText, STATEMENT_KINDS } from './nodes'
import type { Action, Node } from './types/generated/rule-content-v1'

describe('nodeClass', () => {
  it('classifies every rule-content-v1 node kind', () => {
    expect(nodeClass(3.5)).toBe('number')
    expect(nodeClass(true)).toBe('literal')
    expect(nodeClass(false)).toBe('literal')
    expect(nodeClass(null)).toBe('literal')
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

const NAMES: Record<string, { name: string; kind: string }> = {
  wait: { name: 'Wait', kind: 'action' },
  '==': { name: '==', kind: 'operator' },
  isAlive: { name: 'Is Alive', kind: 'value' },
  eventPlayer: { name: 'Event Player', kind: 'value' },
  global: { name: 'global', kind: 'structural' },
  setGlobalVariable: { name: 'Set Global Variable', kind: 'structural' },
  'Status.STUNNED': { name: 'Stunned', kind: 'enumMember' },
}
const names = { peek: (identity: string) => NAMES[identity] ?? null }

describe('statementText', () => {
  it('renders each node form the way Expr does, without the spans', () => {
    expect(statementText(1.5, names, [])).toBe('1.5')
    expect(statementText(true, names, [])).toBe('true')
    expect(statementText({ string: 'hi' }, names, [])).toBe('"hi"')
    expect(statementText({ localizedString: 'hud.header' }, names, [])).toBe('hud.header')
    expect(statementText({ enum: 'Status', member: 'STUNNED' }, names, [])).toBe('Stunned')
    expect(statementText({ variable: 'score' }, names, [])).toBe('score')
    expect(
      statementText(
        { ambiguous: 'Up', candidates: [{ enum: 'Vector', member: 'UP' }] },
        names,
        [],
      ),
    ).toBe('Up')
    expect(statementText({ call: 'wait', args: [0.016] }, names, STATEMENT_KINDS)).toBe('Wait(0.016)')
    expect(
      statementText({ call: 'globalVariable', args: [{ variable: 'score' }] }, names, []),
    ).toBe('Global.score')
    expect(
      statementText(
        { call: 'memberAccess', args: [{ call: 'eventPlayer', args: [] }, { string: 'health' }, 5] },
        names,
        [],
      ),
    ).toBe('Event Player.health[5]')
    expect(
      statementText(
        { call: '==', args: [{ call: 'isAlive', args: [{ call: 'eventPlayer', args: [] }] }, true] },
        names,
        [],
      ),
    ).toBe('Is Alive(Event Player) == true')
    expect(statementText({ call: 'mysteryCall', args: [] }, names, [])).toBe('mysteryCall')
  })
})

describe('statementMatches', () => {
  const setVar = { call: 'setGlobalVariable', args: [{ variable: 'x' }, 1] } as Node

  it('contains-matches the session-locale text case-insensitively', () => {
    expect(statementMatches(setVar, names, STATEMENT_KINDS, 'SET GLOBAL')).toBe(true)
    expect(statementMatches(setVar, names, STATEMENT_KINDS, 'variable(x')).toBe(true)
    expect(statementMatches(setVar, names, STATEMENT_KINDS, 'player')).toBe(false)
  })

  it('matches a call-id prefix the localized text cannot contain', () => {
    expect(statementMatches(setVar, names, STATEMENT_KINDS, 'setglobal')).toBe(true)
    expect(statementMatches(setVar, names, STATEMENT_KINDS, 'setglobx')).toBe(false)
    expect(statementMatches({ variable: 'x' }, names, [], 'x')).toBe(true)
    expect(statementMatches(3, names, [], '3')).toBe(true)
  })
})
