/**
 * Placing `findings` on the main screen (docs/ui/diagnostics.md § In
 * place): each finding maps to its rule by the `rule` listing id, then to
 * one statement row — the `action` index in `actions` first, else the
 * first condition whose `span` contains the finding's, else the rule bar
 * only. A rule's marks show the most severe severity its findings reach;
 * a row's mark lists its findings on hover.
 */
import type { Finding, SourceSpan } from './types/generated/wright-agent-v1'
import type { RuleContent } from './view'

/** The three severities, most severe first (`error` < `warning` < `info`). */
const SEVERITIES = ['error', 'warning', 'info'] as const
export type Severity = (typeof SEVERITIES)[number]

const rank = (severity: Severity): number => SEVERITIES.indexOf(severity)

/**
 * The severity a group of findings' mark carries — the most severe member.
 * An empty group reports `info`, which no caller observes (groups only
 * exist once a finding landed in them).
 */
export function mostSevere(findings: readonly Finding[]): Severity {
  let top: Severity = 'info'
  for (const finding of findings) {
    if (rank(finding.severity) < rank(top)) top = finding.severity
  }
  return top
}

/** Where a finding lands inside its rule. */
export type FindingPlacement =
  | { list: 'actions'; index: number }
  | { list: 'conditions'; index: number }
  | { list: 'rule' }

const atOrBefore = (a: SourceSpan['start'], b: SourceSpan['start']): boolean =>
  a.line < b.line || (a.line === b.line && a.col <= b.col)

/**
 * Whether `span` covers `inside` whole (docs/agent-contract.md § Rule
 * content: the span's `file`/`path` identify its source; endpoints compare
 * as `(line, col)` pairs, end inclusive).
 */
export function spanContains(span: SourceSpan, inside: SourceSpan): boolean {
  if (span.file !== inside.file || span.path !== inside.path) return false
  return atOrBefore(span.start, inside.start) && atOrBefore(inside.end, span.end)
}

/**
 * One finding's place in a rule's content (docs/ui/diagnostics.md § In
 * place · Statement row): the action at `action`, else the first
 * condition whose span contains the finding's, else the rule bar only.
 * `action: null` skips the first step; an absent or unlocated span falls
 * to the rule.
 */
export function placeFinding(
  finding: Pick<Finding, 'action' | 'span'>,
  content: RuleContent,
): FindingPlacement {
  if (finding.action !== null) {
    // An index `actions` does not hold — an `end` marker the row layer
    // hides or an out-of-range index — lands on the rule bar; the row it
    // addressed is a statement the flat stream carries but the body never
    // renders, and every finding stays visible.
    const row = content.actions[finding.action]
    return row !== undefined && row.call !== 'end'
      ? { list: 'actions', index: finding.action }
      : { list: 'rule' }
  }
  const span = finding.span
  if (span !== null) {
    const index = content.conditions.findIndex(
      (condition) => condition.span !== undefined && condition.span !== null && spanContains(condition.span, span),
    )
    if (index >= 0) return { list: 'conditions', index }
  }
  return { list: 'rule' }
}

/**
 * One rule's placed findings: `actions`/`conditions` key the flat
 * statement index to its findings; `rule` holds the bar-only ones.
 */
export interface RuleFindings {
  actions: Map<number, Finding[]>
  conditions: Map<number, Finding[]>
  rule: Finding[]
}

/** An empty placement — every rule renders the same absence. */
const EMPTY: RuleFindings = { actions: new Map(), conditions: new Map(), rule: [] }

/**
 * Group `findings` by `rule` listing id, then place each inside its
 * rule's content. A finding whose rule has no loaded content — or that
 * falls to the bar under `placeFinding` — lands on the rule bar: every
 * finding stays visible at its nearest expressible row.
 */
export function placeFindings(
  findings: readonly Finding[],
  contentByRule: ReadonlyMap<number, RuleContent>,
): Map<number, RuleFindings> {
  const placed = new Map<number, RuleFindings>()
  for (const finding of findings) {
    if (finding.rule === null) continue
    const slots: RuleFindings = placed.get(finding.rule) ?? {
      actions: new Map(),
      conditions: new Map(),
      rule: [],
    }
    const content = contentByRule.get(finding.rule)
    const placement = content ? placeFinding(finding, content) : { list: 'rule' as const }
    if (placement.list === 'rule') {
      slots.rule.push(finding)
    } else {
      const bucket: Map<number, Finding[]> = slots[placement.list]
      const row = bucket.get(placement.index) ?? []
      row.push(finding)
      bucket.set(placement.index, row)
    }
    placed.set(finding.rule, slots)
  }
  return placed
}

/** The placement for one rule, empty when it carries no findings. */
export function findingsFor(
  placed: ReadonlyMap<number, RuleFindings>,
  rule: number,
): RuleFindings {
  return placed.get(rule) ?? EMPTY
}

/** Every finding a rule carries, severest first — the bar's hover list. */
export function allFor(slots: RuleFindings): Finding[] {
  return [...slots.actions.values(), ...slots.conditions.values(), slots.rule]
    .flat()
    .sort((a, b) => rank(a.severity) - rank(b.severity))
}
