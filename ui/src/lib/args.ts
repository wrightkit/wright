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
  | 'comparison'
  | 'call'
  | 'other'

/**
 * The comparison operator call ids `rule-content-v1` documents for the
 * condition's operator slot and the `__Operator__` parameter type — the
 * `CallId` contract's `==`, `!=`, `<`, `<=`, `>`, `>=`. `lookup` resolves
 * each as an `operator` entry; the kind is not listable, so the picker
 * names this documented set (docs/ui/editor.md § Pickers).
 */
export const COMPARISON_OPERATORS = ['==', '!=', '<', '<=', '>', '>='] as const

/**
 * Parameter `type`s a `{"variable": name}` slot declares — the
 * variable-target surface parameters and the folded `globalVariable`/
 * `playerVariable` members (docs/ui/editor.md § Controls by slot type:
 * a variable slot is a picker over the scope's declared `symbols`).
 */
const VARIABLE_PARAM_TYPES = new Set(['Variable', 'Global Variable', 'Player Variable'])

/**
 * Parameter `type`s that name no value picker: literal-pinned and
 * non-value slots. Every other declared type (`Any`, `Object|Array`,
 * `Player`, `Vector`, …) admits value calls, so an empty such slot is a
 * `call` field — the picker the spec's value slot assigns.
 */
const NON_CALL_PARAM_TYPES = new Set([
  'Variable',
  'Global Variable',
  'Player Variable',
  'Subroutine',
  '__Operator__',
  'Operation',
])

export interface ArgField {
  path: number[]
  label: string
  kind: ArgFieldKind
  required: boolean
  /**
   * `false` when the slot's accepted node is a tagged shape the `+`
   * form's literal/enum controls cannot write — a `{"variable"}`,
   * `{"subroutine"}` (no declared subroutines), or `content.operations`
   * position — until #919's pickers fill it. OK stays disabled while a
   * `+` form holds one; a slot already holding the node it accepts is
   * not flagged.
   */
  insertable?: boolean
  scope?: 'globalVariable' | 'playerVariable'
  domain?: string
  operations?: string[]
  /** The parameter's declared `type` (`Subroutine`, `Any`, `Number`, …). */
  type?: string
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

/**
 * Whether the slot the parameter names needs a `rule-content-v1` node
 * the `+` form's literal/enum controls cannot produce — derived from
 * the param's declared `type` and `content` facts, not the node shape:
 *
 * - `content.operations`: the position takes a zero-argument modify
 *   operation call (`{"call":"add","args":[]}`) whose id is one of the
 *   declared ids;
 * - a `Variable`/`Global Variable`/`Player Variable` type, or a fold
 *   member occupying the variable position of its folded
 *   `globalVariable`/`playerVariable` call: the position takes a
 *   `{"variable": name}` node bound to a declared symbol;
 * - `Subroutine`: the position takes a `{"subroutine": name}` node bound
 *   to a declared subroutine (`startRule`) — insertable only when the
 *   program declares one (`hasSubroutines`).
 *
 * A slot already holding the node it accepts is not flagged — a
 * re-staged form or a default-seeded folded call shell may carry it.
 */
export function needsPicker(
  param: ParamFact,
  node: Node | undefined,
  hasSubroutines = false,
): boolean {
  const operations = param.content?.operations ?? []
  if (operations.length > 0) {
    return !(
      isObject(node) && 'call' in node && operations.includes(node.call as string)
    )
  }
  if (param.type === 'Subroutine') {
    if (isObject(node) && 'subroutine' in node) return false
    return !hasSubroutines
  }
  const fold = param.content?.fold
  const foldsVariable =
    /^(Global |Player )?Variable$/.test(param.type ?? '') ||
    (fold !== undefined &&
      ((fold.call === 'globalVariable' && fold.arg === 0) ||
        (fold.call === 'playerVariable' && fold.arg === 1)))
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
  // The position declares its modify-operation choices regardless of the
  // node currently occupying it.
  if ((param.content?.operations?.length ?? 0) > 0) return 'operation'
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
  // A JSON `null` is a pad `defaultArgs`/`fillGaps`/`grow` left where a
  // filled later position crosses an unset one — it is no value, so the
  // slot falls through to its declared control below rather than
  // rendering an uneditable static `null`. The picked Null literal is
  // the `{call:"null"}` node this never masks.
  if (node !== undefined && node !== null) return 'other'
  if (VARIABLE_PARAM_TYPES.has(param.type ?? '')) return 'variable'
  if (param.type === '__Operator__') return 'comparison'
  if (param.type !== undefined && !NON_CALL_PARAM_TYPES.has(param.type)) return 'call'
  return 'other'
}

function fieldFor(
  param: ParamFact,
  path: number[],
  node: Node | undefined,
  foldCall: string | undefined,
  hasSubroutines = false,
): ArgField {
  const field: ArgField = {
    path,
    label: param.displayName ?? param.name ?? '',
    kind: 'other',
    required: param.required !== false && param.default === undefined,
  }
  field.kind = fieldKind(param, node)
  field.type = param.type
  if (param.default !== undefined) field.default = defaultNode(param.default, param)
  if (field.kind === 'variable') {
    field.scope = variableScope(foldCall, '')
  } else if (field.kind === 'operation') {
    field.operations = param.content?.operations ?? []
  } else if (field.kind === 'comparison') {
    field.operations = [...COMPARISON_OPERATORS]
  } else if (field.kind === 'enum') {
    field.domain =
      param.enum?.domain ??
      (isObject(node) && typeof node.enum === 'string' ? node.enum : undefined)
  }
  if (needsPicker(param, node, hasSubroutines)) field.insertable = false
  return field
}

/**
 * Map the callable's surface parameters onto the action's content
 * arguments. Non-fold parameters advance one content position each; a
 * fold group's members share the position its first member takes and
 * address an argument inside the folded call. The row's kind follows the
 * node currently at that position and the choices the parameter
 * declares. `hasSubroutines` lets a `Subroutine` slot render its name
 * picker instead of flagging it picker-bound.
 *
 * `nestedFacts` resolves a `call` field's own `lookup` parameters by the
 * nested call id so its rows render indented under it.
 */
export function argFields(
  params: readonly ParamFact[],
  action: Action,
  nestedFacts?: (call: string) => ParamFact[] | null,
  hasSubroutines = false,
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
    const field = fieldFor(param, path, node, foldFact?.call, hasSubroutines)
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
      const isComparison = (COMPARISON_OPERATORS as readonly string[]).includes(
        value.call as string,
      )
      const operator: ArgField = {
        path: [],
        label: labels[1] ?? '',
        // A comparison operator id gets the comparison picker; a reserved
        // two-arg call (`playerVariable`, `memberAccess`) stays a `call`
        // row so its picker lists the admitted values and its statement
        // text renders `Global.name`-style, not the raw id.
        kind: isComparison ? 'comparison' : 'call',
        required: true,
        operations: isComparison ? [...COMPARISON_OPERATORS] : undefined,
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
  if (param !== undefined && VARIABLE_PARAM_TYPES.has(param.type ?? '')) {
    return variableNode(declared)
  }
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
 * The `{"subroutine": name}` node a `Subroutine` slot accepts —
 * `editStatements` validates it against the program's declared
 * subroutines.
 */
export function subroutineNode(name: string): Node {
  return { subroutine: name }
}

/**
 * Whether a value entry's declared `returns` fits a slot's declared
 * parameter `type` — the same union-membership the owner's acceptance
 * rules apply (`semantic_types_compatible`): `Any`/`Unknown` on either
 * side admits, `Object` admits any non-`Array`/`Void` actual, and
 * `returns`'s own `|` union splits like the slot's. A value with no
 * declared `returns` is admitted (the owner declares none to limit).
 */
export function admitsCall(slotType: string | undefined, returns: string | undefined): boolean {
  if (returns === undefined) return true
  return (slotType ?? 'Any').split('|').some((expected) =>
    returns.split('|').some(
      (actual) =>
        expected === 'Any' ||
        expected === 'Unknown' ||
        actual === 'Any' ||
        actual === 'Unknown' ||
        actual === expected ||
        (expected === 'Object' && actual !== 'Array' && actual !== 'Void') ||
        actual === 'Object',
    ),
  )
}

/**
 * The argument array a freshly chosen call gets: every parameter position
 * that declares a `lookup` default is filled with `defaultNode` (a
 * declared `null` is the Null literal), fold groups materialize their
 * folded `globalVariable`/`playerVariable` call so member writes have a
 * call to land in, and positions the facts leave unset stay absent — a
 * `null` pad appears only where a filled later position crosses them,
 * matching `fillGaps`. Positions past the last fill are omitted so an
 * unset required slot reads "unset" and an omitted optional keeps the
 * catalog default (docs/ui/editor.md § Controls: "A default from `lookup`
 * is filled in when a call is chosen").
 */
export function defaultArgs(params: readonly ParamFact[]): Node[] {
  let last = -1
  params.forEach((param, index) => {
    if (param.default !== undefined || param.content?.fold !== undefined) last = index
  })
  const args: Node[] = []
  let pos = 0
  let foldPos = 0
  for (let i = 0; i <= last; i += 1) {
    const param = params[i]
    const fold = param.content?.fold
    if (typeof fold?.call === 'string' && typeof fold.arg === 'number') {
      if (params[i - 1]?.content?.fold?.call !== fold.call) {
        foldPos = pos
        pos += 1
      }
      const current = args[foldPos]
      const inner: Node[] =
        isObject(current) && current.call === fold.call && Array.isArray(current.args)
          ? [...(current.args as Node[])]
          : []
      if (param.default !== undefined) {
        while (inner.length <= fold.arg) inner.push(null)
        inner[fold.arg] = defaultNode(param.default, param) ?? null
      }
      args[foldPos] = { call: fold.call, args: inner }
    } else {
      args[pos] =
        param.default === undefined ? null : (defaultNode(param.default, param) ?? null)
      pos += 1
    }
  }
  return args
}

/** The `{"call": id, "args": defaults}` node a `call` picker's selection produces. */
export function callNode(call: string, params: readonly ParamFact[]): Node {
  return { call, args: defaultArgs(params) }
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
  if (field.path.length === 0) {
    return field.kind === 'call' || field.kind === 'comparison'
      ? field.call !== undefined
      : false
  }
  // A JSON `null` pad is not a value — the slot it occupies is unset.
  return argAt(action.args, field.path) != null
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
