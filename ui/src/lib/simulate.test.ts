/**
 * The simulation panel's logic tests (#836): the room→scenario mapping
 * carries only occupied slots as `p1`…`pN`, the run length defaults from
 * `lobby.matchTimeLimit` or 60, and each outcome renders its own state —
 * `not_modeled` and `missing_input` carrying the not-verified wording.
 */
import { describe, expect, it } from 'vitest'
import {
  buildScenario,
  logRows,
  matchTimeLimit,
  outcomeText,
  variableRows,
  type RoomSlot,
} from './simulate'
import type { AthenaTrace, SettingsNode } from './types/generated/wright-agent-v1'

const lobby = (value: string): SettingsNode[] => [
  {
    name: 'Lobby',
    children: [{ name: 'Match Time Limit', value }],
  },
]

describe('buildScenario', () => {
  it('carries only occupied slots as sequential players', () => {
    const slots: RoomSlot[] = [
      { hero: 'ANA', team: 'team1' },
      { hero: null, team: 'team1' },
      { hero: 'JUNKER_QUEEN', team: 'team2' },
    ]
    expect(buildScenario(slots, 90)).toEqual({
      scenarioVersion: 1,
      seconds: 90,
      players: [
        { id: 'p1', hero: 'ANA', team: 'team1' },
        { id: 'p2', hero: 'JUNKER_QUEEN', team: 'team2' },
      ],
    })
  })

  it('emits no players for an empty room', () => {
    const slots: RoomSlot[] = Array.from({ length: 12 }, () => ({
      hero: null,
      team: 'team1',
    }))
    expect(buildScenario(slots, 60).players).toEqual([])
  })
})

describe('matchTimeLimit', () => {
  it('reads the lobby match time limit, folding case and spaces', () => {
    expect(matchTimeLimit(lobby('600'))).toBe(600)
    expect(
      matchTimeLimit([
        { name: 'lobby', children: [{ name: 'matchTimeLimit', value: '300' }] },
      ]),
    ).toBe(300)
  })

  it('is null without a settings block or the member', () => {
    expect(matchTimeLimit(null)).toBeNull()
    expect(matchTimeLimit([])).toBeNull()
    expect(matchTimeLimit([{ name: 'Lobby', children: [] }])).toBeNull()
    expect(matchTimeLimit(lobby('soon'))).toBeNull()
    expect(matchTimeLimit(lobby('0'))).toBeNull()
  })
})

const trace = (outcome: AthenaTrace['outcome']): AthenaTrace => ({
  outcome,
  logs: [],
  globals: {},
  players: {},
  hud: [],
  missingInputs: [],
  expectations: [],
})

describe('outcomeText', () => {
  it('renders each outcome as its own state', () => {
    expect(outcomeText(trace({ status: 'complete' }))).toEqual({
      status: 'complete',
      detail: null,
    })
    expect(
      outcomeText(trace({ status: 'not_modeled', construct: 'chaseGlobalVariable', tick: 12 })),
    ).toEqual({
      status: 'not_modeled',
      detail: 'stopped at chaseGlobalVariable at tick 12',
    })
    expect(outcomeText(trace({ status: 'missing_input', tick: 4 })).status).toBe('missing_input')
    expect(outcomeText(trace({ status: 'missing_input' })).detail).toBe('waiting on an input')
  })
})

describe('variableRows and logRows', () => {
  it('projects each `Val` variant as display text — `null` has no `value`', () => {
    const rows = variableRows({
      ...trace({ status: 'complete' }),
      globals: {
        score: { type: 'number', value: 3 },
        flag: { type: 'bool', value: true },
        name: { type: 'text', value: 'hi' },
        unset: { type: 'null' },
        seen: { type: 'array', value: [{ type: 'number', value: 1 }, { type: 'null' }] },
        who: { type: 'player', value: 'p1' },
      },
      // Player state carries hero/team/world only at athena v0.2.0 — no
      // variables are read from it.
      players: { p1: { hero: 'Ana', team: 'team1', world: {} } },
    })
    expect(rows).toEqual([
      { name: 'score', value: '3' },
      { name: 'flag', value: 'true' },
      { name: 'name', value: 'hi' },
      { name: 'unset', value: 'null' },
      { name: 'seen', value: '[1, null]' },
      { name: 'who', value: 'p1' },
    ])
  })

  it('projects logs as tick/text rows', () => {
    expect(
      logRows({ ...trace({ status: 'complete' }), logs: [{ tick: 1, text: 'hello' }] }),
    ).toEqual([{ tick: '1', text: 'hello' }])
  })
})
