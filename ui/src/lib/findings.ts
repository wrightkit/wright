import type { Finding, SourceSpan } from './types/generated/wright-agent-v1'
import type { RuleContent } from './view'

/** The three severities, most severe first (`error` < `warning` < `info`). */
export const SEVERITIES = ['error', 'warning', 'info'] as const
export type Severity = (typeof SEVERITIES)[number]

const rank = (severity: Severity): number => SEVERITIES.indexOf(severity)

/** docs/ui/diagnostics.md § Severity: the token a severity's marks read. */
export const SEVERITY_TOKEN: Record<Severity, string> = {
  error: 'error',
  warning: 'warn',
  info: 'muted',
}

/** The severity a group of findings' mark carries — the most severe member. */
export function mostSevere(findings: readonly Finding[]): Severity {
  let top: Severity = 'info'
  for (const finding of findings) {
    if (rank(finding.severity) < rank(top)) top = finding.severity
  }
  return top
}

/** Findings per severity, in spec order; a level with no findings holds []. */
export function severityCounts(findings: readonly Finding[]): Record<Severity, number> {
  const counts: Record<Severity, number> = { error: 0, warning: 0, info: 0 }
  for (const finding of findings) counts[finding.severity] += 1
  return counts
}

/**
 * docs/ui/diagnostics.md § In place · Toolbar: the severities the
 * Diagnostics button badges — error and warning whenever either has
 * findings; info joins them only when neither error nor warning exists.
 */
export function badgeSeverities(findings: readonly Finding[]): Severity[] {
  const counts = severityCounts(findings)
  const hot = SEVERITIES.filter((severity) => severity !== 'info' && counts[severity] > 0)
  return hot.length > 0 ? hot : counts.info > 0 ? ['info'] : []
}

/**
 * docs/ui/diagnostics.md § In the dialog: the severity groups shown under
 * their toggling headings — error and warning open by default, info
 * collapsed. `open` is the session-persisted choice.
 */
export interface SeverityGroup {
  severity: Severity
  findings: Finding[]
  open: boolean
}

/** The groups the dialog renders, most severe first, empties skipped. */
export function severityGroups(
  findings: readonly Finding[],
  open: Readonly<Record<Severity, boolean>>,
): SeverityGroup[] {
  return SEVERITIES.flatMap((severity) => {
    const group = findings.filter((finding) => finding.severity === severity)
    return group.length > 0 ? [{ severity, findings: group, open: open[severity] }] : []
  })
}

/** The dialog's default heading state: error and warning open, info closed. */
export const DEFAULT_GROUP_OPEN: Record<Severity, boolean> = {
  error: true,
  warning: true,
  info: false,
}

/**
 * docs/ui/diagnostics.md § In place · Filter: the least severe level the
 * toolbar shows — the last severity the Diagnostics button badges, so
 * info counts only when no error or warning exists. No findings means
 * no level is shown and the toggle keeps every rule (null).
 */
export function lowestShownSeverity(findings: readonly Finding[]): Severity | null {
  return badgeSeverities(findings).at(-1) ?? null
}

/** A rule the "Only rules with findings" toggle keeps at the shown level. */
export function hasFindingAtOrAbove(
  slots: RuleFindings,
  lowest: Severity,
): boolean {
  return allFor(slots).some((finding) => rank(finding.severity) <= rank(lowest))
}

/** Where a finding lands inside its rule. */
export type FindingPlacement =
  | { list: 'actions'; index: number }
  | { list: 'conditions'; index: number }
  | { list: 'rule' }

const atOrBefore = (a: SourceSpan['start'], b: SourceSpan['start']): boolean =>
  a.line < b.line || (a.line === b.line && a.col <= b.col)

/** docs/agent-contract.md § Rule content: span endpoints compare as `(line, col)` pairs, end inclusive. */
export function spanContains(span: SourceSpan, inside: SourceSpan): boolean {
  if (span.file !== inside.file || span.path !== inside.path) return false
  return atOrBefore(span.start, inside.start) && atOrBefore(inside.end, span.end)
}

export function placeFinding(
  finding: Pick<Finding, 'action' | 'span'>,
  content: RuleContent,
): FindingPlacement {
  if (finding.action !== null) {
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

/** One rule's placed findings by column and bar; `rule` holds the bar-only ones. */
export interface RuleFindings {
  actions: Map<number, Finding[]>
  conditions: Map<number, Finding[]>
  rule: Finding[]
}

const EMPTY: RuleFindings = { actions: new Map(), conditions: new Map(), rule: [] }

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
