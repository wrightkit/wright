import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import process from 'node:process'
import { describe, expect, it } from 'vitest'
import {
  allFor,
  badgeSeverities,
  DEFAULT_GROUP_OPEN,
  findingsFor,
  hasFindingAtOrAbove,
  lowestShownSeverity,
  mostSevere,
  placeFinding,
  placeFindings,
  severityCounts,
  severityGroups,
  spanContains,
  type RuleFindings,
} from './findings'
import type { Action, Condition } from './types/generated/rule-content-v1'
import type { Finding, SourceSpan } from './types/generated/wright-agent-v1'
import type { RuleContent } from './view'

const span = (
  startLine: number,
  startCol: number,
  endLine: number,
  endCol: number,
  path = 'demo.ws',
): SourceSpan => ({
  file: 0,
  path,
  start: { line: startLine, col: startCol },
  end: { line: endLine, col: endCol },
})

const finding = (
  extra: Partial<Finding> = {},
): Finding =>
  ({
    code: 'finding-code',
    severity: 'warning',
    message: 'a finding',
    span: null,
    rule: 7,
    ruleAddress: 'demo',
    action: null,
    value: null,
    evidence: 'heuristic',
    boundedness: null,
    repeated: null,
    ...extra,
  }) as Finding

const action = (
  call: string,
  at?: SourceSpan,
): Action & { span?: SourceSpan } => ({ call, args: [], ...(at ? { span: at } : {}) })

const condition = (
  at?: SourceSpan,
): Condition & { span?: SourceSpan } => ({
  value: { call: 'compare', args: [1, { call: '==', args: [] }, 1] },
  ...(at ? { span: at } : {}),
})

const content = (
  conditions: (Condition & { span?: SourceSpan })[],
  actions: (Action & { span?: SourceSpan })[],
): RuleContent => ({
  format: 'workshop-rs/rule-content-v1',
  event: { id: 'global' },
  disabled: false,
  conditions,
  actions,
})

describe('spanContains', () => {
  it('contains an equal or inner span, ordered by (line, col) endpoints', () => {
    const outer = span(10, 4, 10, 40)
    expect(spanContains(outer, span(10, 4, 10, 40))).toBe(true)
    expect(spanContains(outer, span(10, 9, 10, 25))).toBe(true)
    expect(spanContains(outer, span(10, 3, 10, 25))).toBe(false)
    expect(spanContains(outer, span(10, 9, 10, 41))).toBe(false)
    expect(spanContains(outer, span(11, 1, 11, 5))).toBe(false)
    expect(spanContains(outer, span(9, 99, 10, 4))).toBe(false)
    expect(spanContains(outer, { ...span(10, 9, 10, 25), path: 'other.ws' })).toBe(false)
    expect(spanContains(outer, { ...span(10, 9, 10, 25), file: 1 })).toBe(false)
  })
})

describe('placeFinding', () => {
  const body = content(
    [condition(span(6119, 9, 6119, 119)), condition(span(6120, 5, 6120, 30))],
    [action('setGlobalVariable', span(644, 9, 644, 45)), action('end')],
  )

  it('places a finding with an `action` index on that action', () => {
    const placed = placeFinding(finding({ action: 0, span: span(1, 1, 1, 5) }), body)
    expect(placed).toEqual({ list: 'actions', index: 0 })
  })

  it('places an unindexed finding on the condition whose span contains its span', () => {
    const placed = placeFinding(finding({ action: null, span: span(6119, 9, 6119, 25) }), body)
    expect(placed).toEqual({ list: 'conditions', index: 0 })
  })

  it('places on the second condition when only its span contains the finding', () => {
    const placed = placeFinding(finding({ span: span(6120, 10, 6120, 20) }), body)
    expect(placed).toEqual({ list: 'conditions', index: 1 })
  })

  it('falls to the rule bar when no condition span contains the finding', () => {
    expect(placeFinding(finding({ span: span(7000, 1, 7000, 5) }), body)).toEqual({ list: 'rule' })
    expect(placeFinding(finding({ span: null }), body)).toEqual({ list: 'rule' })
    const unspanned = content([condition()], [])
    expect(placeFinding(finding({ span: span(10, 1, 10, 5) }), unspanned)).toEqual({ list: 'rule' })
  })

  it('falls to the rule bar when `action` names no rendered row', () => {
    expect(placeFinding(finding({ action: 1 }), body)).toEqual({ list: 'rule' })
    expect(placeFinding(finding({ action: 2 }), body)).toEqual({ list: 'rule' })
  })
})

describe('placeFindings', () => {
  it('groups findings by `rule` listing id and places each in its rule', () => {
    const body = content(
      [condition(span(10, 5, 10, 40))],
      [action('wait'), action('end')],
    )
    const contents = new Map<number, RuleContent>([[7, body]])
    const findings = [
      finding({ rule: 7, action: 0 }),
      finding({ rule: 7, span: span(10, 9, 10, 20) }),
      finding({ rule: 7, span: span(999, 1, 999, 5) }),
      finding({ rule: 7, action: 1 }),
      finding({ rule: 9, action: 0 }),
      finding({ rule: null }),
    ]
    const placed = placeFindings(findings, contents)

    const for7 = findingsFor(placed, 7)
    expect(for7.actions.get(0)).toHaveLength(1)
    expect(for7.conditions.get(0)).toHaveLength(1)
    expect(for7.rule).toHaveLength(2)

    expect(findingsFor(placed, 9).rule).toHaveLength(1)
    expect(placed.has(0)).toBe(false)
    expect(findingsFor(placed, 42)).toEqual<RuleFindings>({
      actions: new Map(),
      conditions: new Map(),
      rule: [],
    })
  })
})

describe('mostSevere', () => {
  it('is the most severe member: error over warning over info', () => {
    const sev = (severity: Finding['severity']) => finding({ severity })
    expect(mostSevere([sev('info'), sev('warning'), sev('info')])).toBe('warning')
    expect(mostSevere([sev('info'), sev('info')])).toBe('info')
    expect(mostSevere([sev('warning'), sev('error'), sev('info')])).toBe('error')
  })
})

describe('allFor', () => {
  it('lists every finding on the rule, severest first', () => {
    const slots: RuleFindings = {
      actions: new Map([[2, [finding({ severity: 'info' })]]]),
      conditions: new Map([[0, [finding({ severity: 'error' })]]]),
      rule: [finding({ severity: 'warning' })],
    }
    expect(allFor(slots).map((f) => f.severity)).toEqual(['error', 'warning', 'info'])
  })
})

describe('severity badges (#922)', () => {
  const sev = (severity: Finding['severity']) => finding({ severity })

  it('badges every shown severity with its own count', () => {
    expect(severityCounts([])).toEqual({ error: 0, warning: 0, info: 0 })
    expect(severityCounts([sev('error'), sev('error'), sev('info')])).toEqual({
      error: 2,
      warning: 0,
      info: 1,
    })
  })

  it.each([
    [[], []],
    [['error'], ['error']],
    [['warning'], ['warning']],
    [['info'], ['info']],
    [['error', 'warning', 'info'], ['error', 'warning']],
    [['warning', 'info'], ['warning']],
    [['error', 'info'], ['error']],
    [['error', 'warning'], ['error', 'warning']],
  ] as const)('badges %j as %j', (severities, expected) => {
    expect(badgeSeverities(severities.map(sev))).toEqual(expected)
  })

  it('filters the rule list by the lowest shown severity', () => {
    expect(lowestShownSeverity([sev('info'), sev('info')])).toBe('info')
    expect(lowestShownSeverity([sev('warning'), sev('info')])).toBe('warning')
    expect(lowestShownSeverity([sev('error'), sev('warning'), sev('info')])).toBe('warning')
    expect(lowestShownSeverity([sev('error')])).toBe('error')
    expect(lowestShownSeverity([])).toBeNull()

    const atFloor = (list: Finding['severity'][], floor: 'error' | 'warning' | 'info') =>
      hasFindingAtOrAbove(
        { actions: new Map(), conditions: new Map(), rule: list.map(sev) },
        floor,
      )
    expect(atFloor(['error'], 'warning')).toBe(true)
    expect(atFloor(['info', 'warning'], 'warning')).toBe(true)
    expect(atFloor(['info'], 'warning')).toBe(false)
    expect(atFloor(['warning', 'info'], 'info')).toBe(true)
    expect(atFloor([], 'info')).toBe(false)
  })
})

describe('severity groups (#922)', () => {
  const sev = (severity: Finding['severity']) => finding({ severity })

  it('groups most severe first and skips empty levels', () => {
    const groups = severityGroups(
      [sev('info'), sev('error'), sev('info'), sev('warning')],
      { error: true, warning: true, info: true },
    )
    expect(groups.map((group) => group.severity)).toEqual(['error', 'warning', 'info'])
    expect(groups.map((group) => group.findings.length)).toEqual([1, 1, 2])

    const only = severityGroups([sev('warning')], { ...DEFAULT_GROUP_OPEN })
    expect(only.map((group) => group.severity)).toEqual(['warning'])
  })

  it('opens error and warning by default and collapses info', () => {
    expect(DEFAULT_GROUP_OPEN).toEqual({ error: true, warning: true, info: false })
    const groups = severityGroups(
      [sev('info'), sev('error'), sev('warning')],
      DEFAULT_GROUP_OPEN,
    )
    expect(groups.map((group) => group.open)).toEqual([true, true, false])
  })
})

describe('bastion.ow findings placement', () => {
  const dir = join(process.cwd(), 'src', 'fixtures')
  const rules = JSON.parse(readFileSync(join(dir, 'bastion-rules.json'), 'utf8')) as (RuleContent & {
    id: number
    content: RuleContent
  })[]
  const findings = JSON.parse(
    readFileSync(join(dir, 'bastion-findings.json'), 'utf8'),
  ) as Finding[]

  it('places all 19 findings — 7 by action index, 12 inside condition spans', () => {
    const contents = new Map<number, RuleContent>()
    for (const rule of rules) contents.set(rule.id, rule.content)
    const placed = placeFindings(findings, contents)

    let actions = 0
    let conditions = 0
    let bar = 0
    const seen = new Set<Finding>()
    for (const finding of findings) {
      const slots = findingsFor(placed, finding.rule!)
      for (const row of slots.actions.values()) if (row.includes(finding)) { actions++; seen.add(finding) }
      for (const row of slots.conditions.values()) if (row.includes(finding)) { conditions++; seen.add(finding) }
      if (slots.rule.includes(finding)) { bar++; seen.add(finding) }
    }
    expect(actions).toBe(7)
    expect(conditions).toBe(12)
    expect(bar).toBe(0)
    expect(seen.size).toBe(19)
  })
})
