import type { LookupEntry } from './types/generated/wright-agent-v1'
import type { Action, Node } from './types/generated/rule-content-v1'

/** One `callable.parameters[]` member as `lookup` emits it. */
export interface ParamFact {
  name?: string
  displayName?: string
  type?: string
  required?: boolean
  default?: unknown
  enum?: { domain?: string }
  content?: {
    fold?: { call?: string; arg?: number }
    operations?: string[]
  }
}

export type ArgFieldKind =
  | 'number'
  | 'string'
  | 'boolean'
  | 'enum'
  | 'variable'
  | 'operation'
  | 'call'
  | 'other'

export interface ArgField {
  path: number[]
  label: string
  kind: ArgFieldKind
  required: boolean
  insertable?: boolean
  scope?: 'globalVariable' | 'playerVariable'
  domain?: string
  operations?: string[]
  call?: string
  node?: Node
  default?: Node
  nested?: ArgField[]
}

/** The `callable.parameters` facts an action entry carries. */
export function paramFacts(entry: LookupEntry | null | undefined): ParamFact[] {
  const parameters = entry?.callable?.parameters
  return Array.isArray(parameters) ? (parameters as ParamFact[]) : []
}

function isObject(node: unknown): node is Record<string, unknown> {
  return typeof node === 'object' && node !== null && !Array.isArray(node)
}

function needsPicker(param: ParamFact, node: Node | undefined): boolean {
  const operations = param.content?.operations ?? []
  if (operations.length > 0) {
    return !(
      isObject(node) && 'call' in node && operations.includes(node.call as string)
    )
  }
  const foldsVariable =
    /^(Global |Player )?Variable$/.test(param.type ?? '') ||
    (param.content?.fold !== undefined &&
      (param.content.fold.call === 'globalVariable'
        ? param.content.fold.arg === 0
        : param.content.fold.arg === 1))
  if (!foldsVariable) return false
  return !(isObject(node) && 'variable' in node)
}

/** The node at `path`, descending into call args; `[]` reads the root. */
export function argAt(
  args: readonly Node[] | undefined,
  path: readonly number[],
): Node | undefined {
  if (path.length === 0) return undefined
  let nodes: readonly Node[] | undefined = args
  let node: Node | undefined
  for (const step of path) {
    node = nodes?.[step]
    nodes = isObject(node) ? (node.args as Node[] | undefined) : undefined
  }
  return node
}

function variableScope(foldCall: string | undefined, callId: string): ArgField['scope'] {
  return /player/i.test(foldCall ?? callId) ? 'playerVariable' : 'globalVariable'
}

function fieldKind(param: ParamFact, node: Node | undefined): ArgFieldKind {
  if (isObject(node) && 'variable' in node) return 'variable'
  if (
    isObject(node) &&
    'call' in node &&
    Array.isArray(node.args) &&
    node.args.length === 0 &&
    (param.content?.operations?.length ?? 0) > 0
  ) {
    return 'operation'
  }
  if (typeof param.enum?.domain === 'string' || (isObject(node) && 'enum' in node)) {
    return 'enum'
  }
  if (typeof node === 'boolean' || (node === undefined && param.type === 'Boolean')) {
    return 'boolean'
  }
  if (typeof node === 'number' || (node === undefined && param.type === 'Number')) {
    return 'number'
  }
  if (
    (isObject(node) && 'string' in node) ||
    (node === undefined && param.type === 'String')
  ) {
    return 'string'
  }
  if (isObject(node) && 'call' in node) return 'call'
  return 'other'
}

function fieldFor(
  param: ParamFact,
  path: number[],
  node: Node | undefined,
  foldCall: string | undefined,
): ArgField {
  const field: ArgField = {
    path,
    label: param.displayName ?? param.name ?? '',
    kind: 'other',
    required: param.required !== false && param.default === undefined,
  }
  field.kind = fieldKind(param, node)
  if (param.default !== undefined) field.default = defaultNode(param.default, param)
  if (field.kind === 'variable') {
    field.scope = variableScope(foldCall, '')
  } else if (field.kind === 'operation') {
    field.operations = param.content?.operations ?? []
  } else if (field.kind === 'enum') {
    field.domain =
      param.enum?.domain ??
      (isObject(node) && typeof node.enum === 'string' ? node.enum : undefined)
  }
  if (needsPicker(param, node)) field.insertable = false
  return field
}

/**
 * Map the callable's surface parameters onto the action's content
 * arguments. Non-fold parameters advance one content position each; a
 * fold group's members share the position its first member takes and
 * address an argument inside the folded call. The row's kind follows the
 * node currently at that position and the choices the parameter declares.
 *
 * `nestedFacts` resolves a `call` field's own `lookup` parameters by the
 * nested call id so its rows render indented under it.
 */
export function argFields(
  params: readonly ParamFact[],
  action: Action,
  nestedFacts?: (call: string) => ParamFact[] | null,
): ArgField[] {
  const fields: ArgField[] = []
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
    const field = fieldFor(param, path, node, foldFact?.call)
    if (field.kind === 'variable') {
      field.scope = variableScope(foldFact?.call, action.call)
    }
    attachNested(field, node, nestedFacts)
    fields.push(field)
  }
  return fields
}

/**
 * A condition's form rows (docs/ui/editor.md § Form · First row): the
 * condition's `value` is the call being edited, so when `lookup` names
 * its parameters the form is the value call's own argument rows. A
 * `Compare`-shaped call — a two-argument operator whose `lookup` entry
 * declares no parameters, like `>=` — takes three rows instead: the
 * left value, the operator shown as its call name (the picker #919
 * adds), and the right value, labelled by `compare`'s parameters. Any
 * non-call value is one literal row.
 */
export function conditionFields(
  ownParams: readonly ParamFact[],
  compare: readonly ParamFact[],
  value: Node,
  nestedFacts?: (call: string) => ParamFact[] | null,
): ArgField[] {
  if (isObject(value) && 'call' in value && Array.isArray(value.args)) {
    if (ownParams.length > 0) {
      return argFields(
        ownParams,
        { call: value.call as string, args: value.args as Node[] },
        nestedFacts,
      )
    }
    if (value.args.length === 2) {
      const labels = compare.map((param) => param.displayName ?? param.name ?? '')
      const left = fieldFor(compare[0] ?? {}, [0], value.args[0], undefined)
      if (labels[0]) left.label = labels[0]
      const right = fieldFor(compare[2] ?? compare[1] ?? {}, [1], value.args[1], undefined)
      if (labels[2] ?? labels[1]) right.label = labels[2] ?? labels[1]
      const operator: ArgField = {
        path: [],
        label: labels[1] ?? '',
        kind: 'call',
        required: true,
        call: value.call as string,
        node: value,
      }
      attachNested(left, value.args[0], nestedFacts)
      attachNested(right, value.args[1], nestedFacts)
      return [left, operator, right]
    }
  }
  const single =
    isObject(value) && 'call' in value
      ? {
          path: [],
          label: compare[0]?.displayName ?? compare[0]?.name ?? '',
          kind: 'call' as const,
          required: true,
          call: value.call as string,
          node: value,
        }
      : fieldFor(compare[0] ?? {}, [0], value, undefined)
  attachNested(single, value, nestedFacts, true)
  return [single]
}

function attachNested(
  field: ArgField,
  node: Node | undefined,
  nestedFacts: ((call: string) => ParamFact[] | null) | undefined,
  root = false,
): void {
  if (field.kind !== 'call' || !isObject(node) || !('call' in node)) return
  const nestedParams = nestedFacts?.(node.call as string)
  if (!nestedParams || nestedParams.length === 0) return
  const nested = argFields(
    nestedParams,
    { call: node.call as string, args: (node.args as Node[]) ?? [] },
    nestedFacts,
  ).map((inner) => ({ ...inner, path: root ? inner.path : [...field.path, ...inner.path] }))
  if (nested.length > 0) field.nested = nested
}

/**
 * The `rule-content-v1` node a `lookup` parameter `default` names
 * (`name-lookup.md` §21.4): `null` is the null literal, `Domain.MEMBER`
 * an enum node, a decimal the number literal, a camel-case identifier on
 * a non-`String` parameter a zero-arg call node (`isButtonHeld.player`
 * defaults to the `eventPlayer` call, not the string), and anything else
 * the string node — the same spellings `editStatements` accepts.
 */
export function defaultNode(
  declared: unknown,
  param?: Pick<ParamFact, 'type'>,
): Node | undefined {
  if (declared === null) return null
  if (typeof declared === 'number' || typeof declared === 'boolean') return declared
  if (typeof declared !== 'string' || declared === '') return undefined
  if (declared === 'null') return null
  if (/^-?\d+(\.\d+)?$/.test(declared)) return Number(declared)
  const split = declared.lastIndexOf('.')
  if (split > 0) return enumNode(declared)
  if (param?.type !== 'String' && /^[a-z][A-Za-z0-9_]*$/.test(declared)) {
    return { call: declared, args: [] }
  }
  return stringNode(declared)
}

function setAtPath(node: Node, path: readonly number[], value: Node): Node {
  if (!isObject(node) || !Array.isArray(node.args)) return node
  const args = grow(node.args as Node[], path[0]).map((arg, index) =>
    index === path[0] ? (path.length === 1 ? value : setAtPath(arg, path.slice(1), value)) : arg,
  )
  return { ...(node as Record<string, unknown>), args } as Node
}

function grow(args: readonly Node[], index: number): Node[] {
  const out = [...args]
  while (out.length <= index) out.push(null)
  return out
}

/**
 * `action` with each field's declared default applied to an omitted
 * position before the slot `path` writes into — a gap the write would
 * otherwise leave a `null` in. Trailing optional positions stay
 * omitted: editing only `Wait`'s duration does not materialize
 * `Ignore Condition` the source left implicit. A `call` field recurses
 * into its nested rows so an argument inside one fills the same way.
 */
export function fillGaps(
  action: Action,
  fields: readonly ArgField[],
  path: readonly number[],
): Action {
  const args = [...action.args]
  let changed = false
  const upto = path[0] ?? 0
  for (const field of fields) {
    if (field.path.length !== 1) continue
    const index = field.path[0]
    if (index < upto) {
      while (args.length <= index) {
        args.push(field.default !== undefined ? field.default : null)
        changed = true
      }
    }
    if (index === upto && path.length > 1 && field.nested) {
      const node = args[index]
      if (isObject(node) && 'call' in node && Array.isArray(node.args)) {
        const filled = fillGaps(
          { call: node.call as string, args: node.args as Node[] },
          field.nested.map((inner) => ({ ...inner, path: inner.path.slice(1) })),
          path.slice(1),
        )
        if (filled.args !== node.args) {
          args[index] = { ...(node as Record<string, unknown>), args: filled.args } as Node
          changed = true
        }
      }
    }
  }
  return changed ? { ...action, args } : action
}

/** The node with the argument at `path` replaced — a new tree, never a mutation. */
export function withArg(action: Action, path: readonly number[], node: Node): Action {
  if (path.length === 0) return node as Action
  const args = grow(action.args, path[0]).map((arg, index) =>
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

/** The `{"string": text}` node the text input edits. */
export function stringNode(text: string): Node {
  return { string: text }
}

/**
 * The initial args for a call the form just picked (docs/ui/editor.md §
 * Form: "a default from `lookup` is filled in when a call is chosen"):
 * one position per non-fold parameter, holding its declared `default`
 * node; a required slot with none stays empty (its `undefined` reads as
 * unset, so OK stays disabled until it is filled). Optional slots past
 * the first all-empty tail stay omitted — `Wait`'s optional
 * `waitBehavior` materializes only when `lookup` defaults it. Fold-group
 * members share their folded call's position, so a variable-target
 * action seeds its `globalVariable`/`playerVariable` shell.
 */
export function defaultArgs(params: readonly ParamFact[]): Node[] {
  const args: Node[] = []
  let foldPos = -1
  for (let i = 0; i < params.length; i += 1) {
    const param = params[i]
    const foldCall = param.content?.fold?.call
    const foldArg = param.content?.fold?.arg
    if (typeof foldCall === 'string' && typeof foldArg === 'number') {
      if (params[i - 1]?.content?.fold?.call !== foldCall) {
        foldPos = args.length
        args.push({ call: foldCall, args: [] })
      }
      const node = defaultNode(param.default, param)
      if (node === undefined) continue
      const shell = args[foldPos]
      if (isObject(shell) && 'call' in shell && Array.isArray(shell.args)) {
        const inner = grow(shell.args as Node[], foldArg)
        inner[foldArg] = node
        shell.args = inner
      }
      continue
    }
    args.push(defaultNode(param.default, param) ?? undefined as unknown as Node)
  }
  while (args.length > 0 && args[args.length - 1] === undefined) args.pop()
  return args
}

/**
 * The picker `id` matching the node currently at a field's position.
 * Literal fields map `true`/`false` to their picker ids.
 */
export function currentId(node: Node | undefined): string | null {
  if (node === true) return 'true'
  if (node === false) return 'false'
  if (!isObject(node)) return null
  if ('variable' in node) return node.variable as string
  if ('call' in node) return node.call as string
  if ('enum' in node) return `${node.enum}.${node.member}`
  return null
}

/** Whether the slot a field edits holds a value; an unset required slot disables OK. */
export function slotSet(action: Action, field: ArgField): boolean {
  if (field.path.length === 0) return field.kind === 'call' ? field.call !== undefined : false
  return argAt(action.args, field.path) !== undefined
}

/** Every unset required field under `action`, nested `call` rows included. */
export function unsetRequired(action: Action, fields: readonly ArgField[]): ArgField[] {
  const missing: ArgField[] = []
  for (const field of fields) {
    if (field.required && !slotSet(action, field)) missing.push(field)
    if (field.nested) missing.push(...unsetRequired(action, field.nested))
  }
  return missing
}
