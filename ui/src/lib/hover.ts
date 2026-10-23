/**
 * The argument hover card's model (docs/ui/editor.md § Hover card):
 * which argument the pointer rests on, what the card names, and the
 * slot it fills. The page reads names and parameter facts from the
 * warmed `lookup` cache only — it composes no text of its own about an
 * element.
 */
import type { ChromeStrings } from './chrome'
import { enumIdentity, VALUE_KINDS, type PeekNames } from './nodes'
import type { LookupEntry } from './types/generated/wright-agent-v1'
import type { ParamFact } from './args'
import type { Node } from './types/generated/rule-content-v1'

/**
 * The `lookup` kinds a value-node argument resolves by, in preference
 * order — a call id resolves to a value or operator entry before an
 * action one.
 */
const ARG_KINDS = VALUE_KINDS

/** The warmed-cache reads the helpers take (`Names.peek` + `peekEntry`). */
export interface HoverNames extends PeekNames {
  peekEntry(identity: string, kinds?: string[]): LookupEntry | null
}

function isNode(node: unknown): node is Extract<Node, object> {
  return typeof node === 'object' && node !== null
}

function isCall(node: Node | undefined): node is Extract<Node, { call: string }> {
  return isNode(node) && 'call' in node
}

/** The node at `path` inside a statement's `args`, descending call args. */
export function nodeAtPath(args: readonly Node[], path: readonly number[]): Node | undefined {
  let nodes: readonly Node[] | undefined = args
  let node: Node | undefined
  for (const step of path) {
    node = nodes?.[step]
    nodes = isCall(node) ? node.args : undefined
  }
  return node
}

/**
 * The enclosing call at `path`: `root` — the statement's own call node,
 * or a condition's value — when the path is one argument deep, else the
 * call the last step descends from, and the argument index within it.
 * `argCount` is the enclosing call's argument count, so the caller can
 * tell an infix two-operand form from a longer call.
 */
export function enclosingCall(
  root: { call: string; args: Node[] },
  path: readonly number[],
): { call: string; index: number; argCount: number } | null {
  if (path.length === 0) return null
  let call = root.call
  let args: readonly Node[] = root.args
  for (let i = 0; i + 1 < path.length; i += 1) {
    const next = args[path[i]!]
    if (!isCall(next)) return null
    call = next.call
    args = next.args
  }
  return { call, index: path[path.length - 1]!, argCount: args.length }
}

/** The call-node root a statement's arg paths address, or null for a non-call root. */
export function statementRoot(node: Node): { call: string; args: Node[] } | null {
  return isCall(node) ? { call: node.call, args: node.args } : null
}

/** The variable scope an argument's enclosing call implies (`Set Player Variable` → player). */
function variableScope(call: string | undefined): 'global' | 'player' {
  return /player/i.test(call ?? '') ? 'player' : 'global'
}

/**
 * The argument's display name and its kind. `lookup` names a call, an
 * enum member, or a localized string; a declared variable or subroutine
 * is its own name; a literal names itself by its own text (the literal
 * is the element's content, not UI-invented prose). The scope word for
 * a `{"variable"}` node comes from the enclosing call's name. `kind`
 * is the `lookup` entry kind when one resolved, else the node's literal
 * kind — the card shows it as the type hint and nothing more.
 */
export function argName(
  node: Node,
  enclosing: { call: string } | null,
  names: PeekNames,
): { name: string; kind: string | null; scope?: 'global' | 'player' } | null {
  if (node === null || typeof node === 'boolean') {
    return { name: String(node), kind: 'literal' }
  }
  if (typeof node === 'number') return { name: String(node), kind: 'number' }
  if ('variable' in node) {
    return { name: node.variable, kind: 'variable', scope: variableScope(enclosing?.call) }
  }
  if ('subroutine' in node) return { name: node.subroutine, kind: 'variable' }
  if ('string' in node) return { name: node.string, kind: 'string' }
  if ('localizedString' in node) {
    const resolved = names.peek(node.localizedString)
    return { name: resolved?.name ?? node.localizedString, kind: resolved?.kind ?? 'string' }
  }
  if ('enum' in node) {
    const resolved = names.peek(enumIdentity(node), ['enumMember'])
    return { name: resolved?.name ?? `${node.enum}.${node.member}`, kind: resolved?.kind ?? 'enum' }
  }
  if ('ambiguous' in node) return { name: node.ambiguous, kind: 'enum' }
  if ('call' in node) {
    const resolved = names.peek(node.call, ARG_KINDS)
    return resolved ? { name: resolved.name, kind: resolved.kind } : { name: node.call, kind: null }
  }
  return null
}

export interface HoverSlot {
  /** The slot line — the parameter or operand the argument fills. */
  slot: string
  /** The enclosing call's parameters, the hovered one marked. */
  params: { label: string; marked: boolean }[]
}

/**
 * The slot an argument fills in its enclosing call (docs/ui/editor.md
 * § Hover card). An infix `operator` call's two operands read "left
 * operand" and "right operand" — the same test `Expr` renders infix
 * under; any other call's argument reads `parameter N (Label) of
 * CallName`, the label the localized parameter label `lookup` declares
 * and the call name resolved in the session locale. The slot line is
 * chrome text in the session locale. `params` is the enclosing call's
 * `callable.parameters` list, empty when `lookup` declares none (the
 * `==` operator carries no signature).
 */
export function hoverSlot(
  enclosing: { call: string; index: number; argCount: number },
  names: HoverNames,
  t: ChromeStrings,
): HoverSlot {
  // `ARG_KINDS` keeps the resolved entry kind aligned with the kind
  // `Expr` renders the call under, so an infix `==` resolves `operator`
  // and a call like `add` resolves its `value` signature.
  const entry = names.peekEntry(enclosing.call, ARG_KINDS)
  const callName = entry?.displayName ?? enclosing.call
  const params = (entry?.callable?.parameters as ParamFact[] | undefined) ?? []
  const param = params[enclosing.index]
  const slot =
    entry?.kind === 'operator' && enclosing.argCount === 2
      ? enclosing.index === 0
        ? t.argOperandLeft
        : t.argOperandRight
      : t.argParameterSlot(
          enclosing.index,
          param != null ? (param.displayName ?? param.name ?? null) : null,
          callName,
        )
  return {
    slot,
    params: params.map((p, i) => ({
      label: p.displayName ?? p.name ?? '',
      marked: i === enclosing.index,
    })),
  }
}
