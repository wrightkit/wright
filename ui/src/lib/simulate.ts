/**
 * The simulation panel's room→scenario mapping and trace reading (#836,
 * athena scenario-format): a room slot is `{hero, team}`, the scenario
 * carries only occupied slots as `p1`…`pN` players, and the run length
 * defaults from the project's `lobby.matchTimeLimit` setting or 60.
 */
import type { AthenaTrace, SettingsNode } from './types/generated/wright-agent-v1'

export interface RoomSlot {
  hero: string | null
  team: 'team1' | 'team2'
}

/** The scenario `players` entries occupied slots produce. */
export function buildScenario(
  slots: RoomSlot[],
  seconds: number,
): { scenarioVersion: 1; seconds: number; players: { id: string; hero: string; team: string }[] } {
  return {
    scenarioVersion: 1,
    seconds,
    players: slots
      .filter((slot) => slot.hero !== null)
      .map((slot, index) => ({ id: `p${index + 1}`, hero: slot.hero as string, team: slot.team })),
  }
}

/**
 * The project's `lobby.matchTimeLimit` in seconds when the settings block
 * declares one — matched on the canonical or authored spelling with case
 * and whitespace folded.
 */
export function matchTimeLimit(nodes: SettingsNode[] | null | undefined): number | null {
  const lobby = (nodes ?? []).find(
    (node) => node.name.replace(/\s+/g, '').toLowerCase() === 'lobby' && node.children,
  )
  for (const member of lobby?.children ?? []) {
    if (
      member.name.replace(/\s+/g, '').toLowerCase() === 'matchtimelimit' &&
      member.value !== undefined
    ) {
      const seconds = Number(member.value)
      if (Number.isFinite(seconds) && seconds > 0) return seconds
    }
  }
  return null
}

/** The trace's outcome row: the status and the wording the two
 * incomplete statuses carry (`#836` — never shown as verified). */
export function outcomeText(trace: AthenaTrace): { status: string; detail: string | null } {
  const outcome = trace.outcome
  if (outcome.status === 'complete') return { status: 'complete', detail: null }
  if (outcome.status === 'not_modeled') {
    const at = outcome.tick != null ? ` at tick ${outcome.tick}` : ''
    return {
      status: 'not_modeled',
      detail: `stopped at ${outcome.construct ?? 'an unmodeled construct'}${at}`,
    }
  }
  if (outcome.status === 'missing_input') {
    const at = outcome.tick != null ? ` at tick ${outcome.tick}` : ''
    return { status: 'missing_input', detail: `waiting on an input${at}` }
  }
  return { status: String(outcome.status), detail: null }
}

/** The trace's variable rows: each `globals` name and each player's
 * `variables` entry as `name → value` display text. */
export function variableRows(trace: AthenaTrace): { scope: string; name: string; value: string }[] {
  const rows: { scope: string; name: string; value: string }[] = []
  for (const [name, entry] of Object.entries(trace.globals ?? {})) {
    const value =
      entry !== null && typeof entry === 'object'
        ? ((entry as { value?: unknown }).value ?? entry)
        : entry
    rows.push({ scope: 'global', name, value: String(value) })
  }
  for (const [player, entry] of Object.entries(trace.players ?? {})) {
    const variables =
      entry !== null && typeof entry === 'object'
        ? ((entry as { variables?: Record<string, unknown> }).variables ?? {})
        : {}
    for (const [name, value] of Object.entries(variables)) {
      rows.push({ scope: player, name, value: String(value) })
    }
  }
  return rows
}

/** The trace's log rows as `tick: text` display pairs. */
export function logRows(trace: AthenaTrace): { tick: string; text: string }[] {
  return (trace.logs ?? []).map((entry) => {
    const log = entry !== null && typeof entry === 'object' ? (entry as Record<string, unknown>) : {}
    return { tick: String(log.tick ?? ''), text: String(log.text ?? '') }
  })
}
