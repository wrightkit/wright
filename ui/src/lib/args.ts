/**
 * The statement editor's argument forms (ADR-0032 layer 3, #835): one
 * form row per content argument, built from the `lookup` callable's
 * `parameters` facts. A fold group's members map inside the folded
 * variable-target call that occupies the group's first member's content
 * position; a modify-operation position offers exactly the call ids
 * `content.operations` declares. The forms emit `rule-content-v1` arg
 * nodes only — no Workshop text is composed here.
 */
import type { LookupEntry } from './types/generated/wright-agent-v1'
import type { Action, Node } from './types/generated/rule-content-v1'

/** One `callable.parameters[]` member as `lookup` emits it. */
export interface ParamFact {
  name?: string
  /** The editor's localized parameter label. */
  displayName?: string
  type?: string
  required?: boolean
  default?: unknown
  enum?: { domain?: string }
  content?: {
    /** A folded variable-target call this parameter contributes to. */
    fold?: { call?: string; arg?: number }
    /** The modify-operation call ids this position accepts. */
    operations?: string[]
  }
}

export type ArgFieldKind = 'variable' | 'operation' | 'enum' | 'other'

export interface ArgField {
  /**
   * Indices into `Action.args`; a second level reaches inside the folded
   * `globalVariable`/`playerVariable` call a fold group contributes to.
   */
  path: number[]
  /** The row label — the localized parameter label or semantic name. */
  label: string
  kind: ArgFieldKind
  /** Which `symbols` kind a `variable` field's picker lists. */
  scope?: 'globalVariable' | 'playerVariable'
  /** The enum domain an `enum` field lists members of via `listEntries`. */
  domain?: string
  /** The modify-operation call ids an `operation` field lists. */
  operations?: string[]
}

/** The `callable.parameters` facts an action entry carries. */
export function paramFacts(entry: LookupEntry | null | undefined): ParamFact[] {
  const parameters = entry?.callable?.parameters
  return Array.isArray(parameters) ? (parameters as ParamFact[]) : []
}

function isObject(node: unknown): node is Record<string, unknown> {
  return typeof node === 'object' && node !== null && !Array.isArray(node)
}

/** The node at `path`, descending into call args. */
export function argAt(args: readonly Node[] | undefined, path: readonly number[]): Node | undefined {
  let nodes: readonly Node[] | undefined = args
  let node: Node | undefined
  for (const step of path) {
    node = nodes?.[step]
    nodes = isObject(node) ? (node.args as Node[] | undefined) : undefined
  }
  return node
}

/** The variable scope a `{"variable"}` slot draws its choices from. */
function variableScope(foldCall: string | undefined, callId: string): ArgField['scope'] {
  return /player/i.test(foldCall ?? callId) ? 'playerVariable' : 'globalVariable'
}

/**
 * Map the callable's surface parameters onto the action's content
 * arguments. Non-fold parameters advance one content position each; a
 * fold group's members share the position its first member takes and
 * address an argument inside the folded call. The row's kind follows the
 * node currently at that position and the choices the parameter declares.
 */
export function argFields(params: readonly ParamFact[], action: Action): ArgField[] {
  const fields: ArgField[] = []
  // A fold group keeps the content position its first member claimed.
  let pos = 0
  let foldPos = 0
  for (let i = 0; i < params.length; i += 1) {
    const param = params[i]
    const foldFact = param.content?.fold
    let path: number[]
    if (typeof foldFact?.call === 'string' && typeof foldFact.arg === 'number') {
      if (params[i - 1]?.content?.fold?.call !== foldFact.call) {
        foldPos = pos
        pos += 1
      }
      path = [foldPos, foldFact.arg]
    } else {
      path = [pos]
      pos += 1
    }
    const node = argAt(action.args, path)
    const field: ArgField = { path, label: param.displayName ?? param.name ?? '', kind: 'other' }
    if (isObject(node) && 'variable' in node) {
      field.kind = 'variable'
      field.scope = variableScope(foldFact?.call, action.call)
    } else if (
      isObject(node) &&
      'call' in node &&
      Array.isArray(node.args) &&
      node.args.length === 0 &&
      param.content?.operations?.length
    ) {
      field.kind = 'operation'
      field.operations = param.content.operations
    } else if (isObject(node) && 'enum' in node && typeof param.enum?.domain === 'string') {
      field.kind = 'enum'
      field.domain = param.enum.domain
    }
    fields.push(field)
  }
  return fields
}

function setAtPath(node: Node, path: readonly number[], value: Node): Node {
  if (!isObject(node) || !Array.isArray(node.args)) return node
  const args = (node.args as Node[]).map((arg, index) =>
    index === path[0] ? (path.length === 1 ? value : setAtPath(arg, path.slice(1), value)) : arg,
  )
  return { ...(node as Record<string, unknown>), args } as Node
}

/** The action with the argument at `path` replaced — a new tree, never a mutation. */
export function withArg(action: Action, path: readonly number[], node: Node): Action {
  const args = action.args.map((arg, index) =>
    index === path[0] ? (path.length === 1 ? node : setAtPath(arg, path.slice(1), node)) : arg,
  )
  return { ...action, args }
}

/** The `{"variable": name}` node a variable picker's selection produces. */
export function variableNode(name: string): Node {
  return { variable: name }
}

/** The zero-argument operator call an operation picker's selection produces. */
export function operationNode(call: string): Node {
  return { call, args: [] }
}

/** The `{"enum","member"}` node a `Domain.MEMBER` picker identity produces. */
export function enumNode(identity: string): Node {
  const split = identity.lastIndexOf('.')
  return { enum: identity.slice(0, split), member: identity.slice(split + 1) }
}

/** The picker `id` matching the node currently at a field's position. */
export function currentId(node: Node | undefined): string | null {
  if (!isObject(node)) return null
  if ('variable' in node) return node.variable as string
  if ('call' in node) return node.call as string
  if ('enum' in node) return `${node.enum}.${node.member}`
  return null
}
