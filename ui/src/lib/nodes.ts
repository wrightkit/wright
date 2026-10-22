/**
 * Pure presentation helpers over `workshop-rs/rule-content-v1` nodes. The
 * page holds no Workshop semantics of its own: names come from `lookup`,
 * and the only structural reading here is what the format documents —
 * reserved call forms and the flat action stream whose nesting is
 * recoverable from `If`/`Else If`/`Else`/`While`/`For` openers and `End`.
 */
import type { Action, CallNode, Condition, Event, Node } from './types/generated/rule-content-v1'

/**
 * The syntax classes the design spec's colour table names (docs/ui/
 * design.md § Statement text). `punctuation` covers the structural marks
 * `Expr` emits — parentheses, commas, member dots, index brackets — which
 * no node produces.
 */
export type SyntaxClass =
  | 'action'
  | 'value'
  | 'enum'
  | 'number'
  | 'string'
  | 'variable'
  | 'operator'
  | 'literal'
  | 'punctuation'

/** Reserved call ids whose documented forms read as variable accesses. */
const VARIABLE_CALLS = new Set(['globalVariable', 'playerVariable', 'memberAccess'])

/**
 * The syntax class for a `lookup` entry kind: statement-position calls
 * resolve with action/structural preferred, value-position calls prefer
 * value/operator.
 */
export function kindClass(kind: string | undefined): SyntaxClass {
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
  return kindClass(kind)
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

/** The warmed display-name reads the text helpers take (`Names.peek`). */
export interface PeekNames {
  peek(identity: string, kinds?: string[]): { name: string; kind: string } | null
}

/**
 * The plain text a statement row renders (docs/ui/main-screen.md §
 * Statement text): `Expr` without the syntax spans — the call name then
 * its arguments in parentheses, comparisons and arithmetic infix, and the
 * variable-access forms reading `Global.name`/`owner.name`. The column
 * filter matches against the same text the user reads, so it must resolve
 * names through the same warmed cache.
 */
export function statementText(node: Node, names: PeekNames, kinds: string[]): string {
  const text = (arg: Node) => statementText(arg, names, VALUE_KINDS)
  if (typeof node === 'number') return String(node)
  if (node === null || typeof node === 'boolean') return String(node)
  if ('string' in node) return `"${node.string}"`
  if ('localizedString' in node) {
    return names.peek(node.localizedString)?.name ?? node.localizedString
  }
  if ('enum' in node) {
    return names.peek(enumIdentity(node), ['enumMember'])?.name ?? `${node.enum}.${node.member}`
  }
  if ('variable' in node) return node.variable
  if ('subroutine' in node) return node.subroutine
  if ('ambiguous' in node) return node.ambiguous
  if (node.call === 'globalVariable') {
    // The scope label's canonical display name is the lowercase `global`
    // keyword; the editor shows the capitalized form (`Global.score`).
    const scope = names.peek('global', ['structural'])
    const label = (scope?.name ?? 'Global').replace(/^\p{Ll}/u, (c) => c.toUpperCase())
    return `${label}.${text(node.args[0])}`
  }
  if (node.call === 'playerVariable') return `${text(node.args[0])}.${text(node.args[1])}`
  if (node.call === 'memberAccess') {
    const member =
      typeof node.args[1] === 'object' && node.args[1] !== null && 'string' in node.args[1]
        ? node.args[1].string
        : text(node.args[1])
    const index = node.args.length > 2 ? `[${text(node.args[2])}]` : ''
    return `${text(node.args[0])}.${member}${index}`
  }
  const head = names.peek(node.call, kinds)
  const name = head?.name ?? node.call
  if (head?.kind === 'operator' && node.args.length === 2) {
    return `${text(node.args[0])} ${name} ${text(node.args[1])}`
  }
  if (node.args.length === 0) return name
  return `${name}(${node.args.map(text).join(', ')})`
}

export function statementMatches(
  node: Node,
  names: PeekNames,
  kinds: string[],
  query: string,
): boolean {
  const q = query.toLowerCase()
  if (statementText(node, names, kinds).toLowerCase().includes(q)) return true
  return (
    typeof node === 'object' &&
    node !== null &&
    'call' in node &&
    node.call.toLowerCase().startsWith(q)
  )
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
    // A global variable read renders `Global.name`; `global` is the
    // structural catalog entry whose localized name is the scope word.
    if (node.call === 'globalVariable') {
      out.push({ identity: 'global', kinds: ['structural'] })
    }
    for (const arg of node.args) collectNode(arg, out)
  }
}

/** `lookup` kind preference for a call id used as a statement. */
export const STATEMENT_KINDS = ['action', 'structural', 'value', 'operator']
/** `lookup` kind preference for a call id used in value position. */
export const VALUE_KINDS = ['value', 'operator', 'structural', 'action', 'enumMember']
