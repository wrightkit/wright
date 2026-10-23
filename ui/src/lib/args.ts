/**
 * The statement editor's argument forms (ADR-0032 layer 3, #835, #918):
 * one form row per content argument, built from the `lookup` callable's
 * `parameters` facts — the rows and their labels are fixture facts, not
 * text the page composes. A fold group's members map inside the folded
 * variable-target call that occupies the group's first member's content
 * position; a modify-operation position offers exactly the call ids
 * `content.operations` declares. A call argument shows its call name as
 * plain text with the chosen call's own `lookup` parameters as nested
 * rows. The forms emit `rule-content-v1` arg nodes only — no Workshop
 * text is composed here.
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
  /**
   * Indices into the statement's `args` tree; a second level reaches
   * inside the folded `globalVariable`/`playerVariable` call a fold group
   * contributes to, or inside a `call` argument's own arguments.
   */
  path: number[]
  /** The row label — the localized parameter label or semantic name. */
  label: string
  kind: ArgFieldKind
  /** Whether an empty slot disables OK (a required parameter). */
  required: boolean
  /** Which `symbols` kind a `variable` field's picker lists. */
  scope?: 'globalVariable' | 'playerVariable'
  /** The enum domain an `enum` field lists members of via `listEntries`. */
  domain?: string
  /** The modify-operation call ids an `operation` field lists. */
  operations?: string[]
  /**
   * The call id a `call` row displays when the field has no node at its
   * own position — a condition's operator row names the value call.
   */
  call?: string
  /**
   * The `lookup`-declared default for this position as a `rule-content-v1`
   * node (`Wait.IGNORE_CONDITION` → `{"enum":"Wait","member":"IGNORE_CONDITION"}`,
   * `"null"` → `null`, a number string → the literal); `undefined` when the
   * parameter declares none.
   */
  default?: Node
  /**
   * A `call` field's nested rows: the chosen call's own `lookup`
   * parameters, addressed inside this field's node.
   */
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

/** The variable scope a `{"variable"}` slot draws its choices from. */
function variableScope(foldCall: string | undefined, callId: string): ArgField['scope'] {
  return /player/i.test(foldCall ?? callId) ? 'playerVariable' : 'globalVariable'
}

/**
 * The form control a parameter position takes, from the `lookup` facts
 * and the node currently occupying the position.
 *
 * - `variable`: a `{"variable": name}` node — a picker over the declared
 *   variables of the slot's scope;
 * - `operation`: a `content.operations` position — a picker over those
 *   modify-operation call ids;
 * - `enum`: the parameter's declared domain or the node's own enum — a
 *   picker over the domain's `listEntries` members;
 * - `boolean`: a `Boolean` slot or a `true`/`false` node — the
 *   two-literal picker;
 * - `number`: a `Number` slot or a bare-number node — the numeric input;
 * - `string`: a `String` slot or a `{"string"}` node — the text input;
 * - `call`: a call node in value position — the call name as plain text
 *   (#919 turns it into a picker) with the call's own parameters as
 *   nested rows;
 * - `other`: an empty slot the facts do not pin down — it shows "unset".
 */
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

/** One field row's facts from its parameter and the node at its path. */
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
  if (param.default !== undefined) field.default = defaultNode(param.default)
  if (field.kind === 'variable') {
    field.scope = variableScope(foldCall, '')
  } else if (field.kind === 'operation') {
    field.operations = param.content?.operations ?? []
  } else if (field.kind === 'enum') {
    field.domain =
      param.enum?.domain ??
      (isObject(node) && typeof node.enum === 'string' ? node.enum : undefined)
  }
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
    const field = fieldFor(param, path, node, foldFact?.call)
    if (field.kind === 'variable') {
      field.scope = variableScope(foldFact?.call, action.call)
    }
    if (field.kind === 'call' && isObject(node) && 'call' in node) {
      const nestedParams = nestedFacts?.(node.call as string)
      if (nestedParams && nestedParams.length > 0) {
        const nested = argFields(
          nestedParams,
          { call: node.call as string, args: (node.args as Node[]) ?? [] },
          nestedFacts,
        ).map((inner) => ({ ...inner, path: [...field.path, ...inner.path] }))
        if (nested.length > 0) field.nested = nested
      }
    }
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
      }
      attachNested(left, value.args[0], nestedFacts)
      attachNested(right, value.args[1], nestedFacts)
      return [left, operator, right]
    }
  }
  const single =
    isObject(value) && 'call' in value
      ? {
          // A call whose `lookup` entry declares no parameter facts: the
          // form has no slot decomposition for it — the call name alone,
          // read-only here (the picker that replaces it is #919's).
          path: [],
          label: compare[0]?.displayName ?? compare[0]?.name ?? '',
          kind: 'call' as const,
          required: true,
          call: value.call as string,
        }
      : fieldFor(compare[0] ?? {}, [0], value, undefined)
  attachNested(single, value, nestedFacts, true)
  return [single]
}

/** A `call` field's nested rows, addressed inside the node at `path`. */
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
 * an enum node, a decimal the number literal, and anything else the
 * string node — the same spellings `editStatements` accepts.
 */
export function defaultNode(declared: unknown): Node | undefined {
  if (declared === null) return null
  if (typeof declared === 'number' || typeof declared === 'boolean') return declared
  if (typeof declared !== 'string' || declared === '') return undefined
  if (declared === 'null') return null
  if (/^-?\d+(\.\d+)?$/.test(declared)) return Number(declared)
  const split = declared.lastIndexOf('.')
  if (split > 0) return enumNode(declared)
  return stringNode(declared)
}

function setAtPath(node: Node, path: readonly number[], value: Node): Node {
  if (!isObject(node) || !Array.isArray(node.args)) return node
  const args = grow(node.args as Node[], path[0]).map((arg, index) =>
    index === path[0] ? (path.length === 1 ? value : setAtPath(arg, path.slice(1), value)) : arg,
  )
  return { ...(node as Record<string, unknown>), args } as Node
}

/** The args array padded with `null`s to cover `index` — an unset slot
    receives its first value through the same replace path. */
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
  // The `call` pseudo-field on a condition's operator has no arg position;
  // its value is the call id itself — always set when present.
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
