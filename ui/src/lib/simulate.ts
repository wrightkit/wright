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

function valText(val: unknown): string {
  if (val === null || typeof val !== 'object') return String(val)
  const typed = val as { type?: string; value?: unknown }
  switch (typed.type) {
    case 'null':
      return 'null'
    case 'array':
      return `[${(Array.isArray(typed.value) ? typed.value : []).map(valText).join(', ')}]`
    default:
      return String(typed.value ?? 'null')
  }
}

/** The trace's variable rows: each `globals` name as `name → value`
 * display text. Player variables are not in the trace at `athena`
 * v0.2.0 — `players` carries hero/team/world only. */
export function variableRows(trace: AthenaTrace): { name: string; value: string }[] {
  return Object.entries(trace.globals ?? {}).map(([name, val]) => ({
    name,
    value: valText(val),
  }))
}

/** The trace's log rows as `tick: text` display pairs. */
export function logRows(trace: AthenaTrace): { tick: string; text: string }[] {
  return (trace.logs ?? []).map((entry) => {
    const log = entry !== null && typeof entry === 'object' ? (entry as Record<string, unknown>) : {}
    return { tick: String(log.tick ?? ''), text: String(log.text ?? '') }
  })
}
