/**
 * Pure presentation helpers over `workshop-rs/rule-content-v1` nodes. The
 * page holds no Workshop semantics of its own: names come from `lookup`,
 * and the only structural reading here is what the format documents —
 * reserved call forms and the flat action stream whose nesting is
 * recoverable from `If`/`Else If`/`Else`/`While`/`For` openers and `End`.
 */
import type { Action, CallNode, Condition, Event, Node } from './types/generated/rule-content-v1'

/** The eight syntax classes the issue's palette names. */
export type SyntaxClass =
  | 'action'
  | 'value'
  | 'enum'
  | 'number'
  | 'string'
  | 'variable'
  | 'operator'
  | 'literal'

/** Reserved call ids whose documented forms read as variable accesses. */
const VARIABLE_CALLS = new Set(['globalVariable', 'playerVariable', 'memberAccess'])

/**
 * The syntax class for a node. `kind` is the `lookup` entry kind for a call
 * id (value, operator, action, structural, …) when one resolved; statement
 * position passes the kind resolved with action/structural preferred, value
 * position prefers value/operator.
 */
export function nodeClass(node: Node, kind?: string): SyntaxClass {
  if (typeof node === 'number') return 'number'
  if (node === null || typeof node === 'boolean') return 'literal'
  if ('string' in node || 'localizedString' in node) return 'string'
  if ('enum' in node) return 'enum'
  if ('variable' in node || 'subroutine' in node) return 'variable'
  if (VARIABLE_CALLS.has((node as CallNode).call)) return 'variable'
  switch (kind) {
    case 'operator':
      return 'operator'
    case 'action':
    case 'structural':
      return 'action'
    default:
      return 'value'
  }
}

/**
 * Structural call ids that open a block closed by `end` (the
 * rule-content-v1 flat stream documents `If`/`Else If`/`Else`/`While`/`For`
 * openers and `End`). `elseIf`/`else` continue the enclosing block at its
 * own depth.
 */
const OPENERS = new Set(['if', 'while', 'forGlobalVariable', 'forPlayerVariable'])
const BRANCHES = new Set(['elseIf', 'else'])

/**
 * Display depth per action: `end` markers are hidden (`null`) and only
 * dedent; a branch statement renders at the opener's depth; an opener
 * renders then indents its body. A stray `end` cannot drive the depth
 * negative.
 */
export function statementDepths(actions: Action[]): (number | null)[] {
  let depth = 0
  return actions.map((action) => {
    if (action.call === 'end') {
      depth = Math.max(0, depth - 1)
      return null
    }
    if (BRANCHES.has(action.call)) {
      return Math.max(0, depth - 1)
    }
    const at = depth
    if (OPENERS.has(action.call)) depth += 1
    return at
  })
}

/** Whether the identity is a `SLOT_n`/`ALL` EventPlayer member. */
export function isEventPlayerMember(member: string): boolean {
  return member === 'ALL' || /^SLOT_\d+$/.test(member)
}

/**
 * The `lookup` identity an enum member resolves by: `<Domain>.<MEMBER>`
 * (`Wait.IGNORE_CONDITION` → "Ignore Condition").
 */
export function enumIdentity(node: { enum: string; member: string }): string {
  return `${node.enum}.${node.member}`
}

/**
 * Every `lookup` identity a rule-content document references — call ids,
 * enum members, localized strings, the event and its filters — with the
 * kind preference the lookup should resolve under. Statement-position
 * calls prefer action/structural entries; value-position calls prefer
 * value/operator entries.
 */
export function collectIdentities(content: {
  event: Event
  conditions: Condition[]
  actions: Action[]
}): { identity: string; kinds: string[] }[] {
  const out: { identity: string; kinds: string[] }[] = []
  out.push({ identity: content.event.id, kinds: ['event'] })
  if (content.event.team) {
    out.push({ identity: `EventTeam.${content.event.team}`, kinds: ['enumMember'] })
  }
  if (content.event.player) {
    const player = content.event.player
    out.push(
      isEventPlayerMember(player)
        ? { identity: `EventPlayer.${player}`, kinds: ['enumMember'] }
        : { identity: `Hero.${player}`, kinds: ['enumMember'] },
    )
  }
  for (const condition of content.conditions) collectNode(condition.value, out)
  for (const action of content.actions) {
    out.push({ identity: action.call, kinds: STATEMENT_KINDS })
    for (const arg of action.args) collectNode(arg, out)
  }
  return out
}

function collectNode(node: Node, out: { identity: string; kinds: string[] }[]): void {
  if (node === null || typeof node !== 'object') return
  if ('enum' in node) {
    out.push({ identity: enumIdentity(node), kinds: ['enumMember'] })
  } else if ('localizedString' in node) {
    out.push({ identity: node.localizedString, kinds: [] })
  } else if ('call' in node) {
    out.push({ identity: node.call, kinds: VALUE_KINDS })
    for (const arg of node.args) collectNode(arg, out)
  }
}

/** `lookup` kind preference for a call id used as a statement. */
export const STATEMENT_KINDS = ['action', 'structural', 'value', 'operator']
/** `lookup` kind preference for a call id used in value position. */
export const VALUE_KINDS = ['value', 'operator', 'structural', 'action', 'enumMember']
