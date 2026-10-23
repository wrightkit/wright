import type {
  EditStatementsRequest,
  RuleContentStatement,
  StatementEdit,
} from './types/generated/wright-agent-v1'
import type { SpannedAction, SpannedCondition } from './view'

/** The pending-dialog tags (design.md): a new statement is an `insert`, a toggle a `change`, a removal a `delete`. */
export type PendingTag = 'change' | 'delete' | 'insert'

/** The two `rule-content-v1` statement lists an edit addresses. */
export type StatementList = 'conditions' | 'actions'

/** The batch bar's operations on a selection. */
export type BatchAction = 'disable' | 'enable' | 'delete'

/** One staged statement edit, addressed against the pre-edit program. */
export interface PendingChange {
  rule: number
  ruleName: string
  list: StatementList
  index: number
  tag: PendingTag
  statement?: RuleContentStatement
  statements?: RuleContentStatement[]
  position?: 'before'
}

export type StatementNode = SpannedCondition | SpannedAction

export function keyFor(rule: number, list: string, index: number): string {
  return `${rule}:${list}:${index}`
}

/**
 * The pending-map key of an `insert` staged at `index` — namespaced so
 * the insert edit never collides with the `change`/`delete` `keyFor`
 * addresses an existing row at the same index (#920 review): a `+` at N
 * and a delete of row N stage side by side instead of clobbering.
 */
export function insertKey(rule: number, list: string, index: number): string {
  return `insert:${rule}:${list}:${index}`
}

/** The pending-map key of the placeholder row an `insert` at `index` adds. */
export function insertMarkKey(rule: number, list: string, index: number): string {
  return `${rule}:${list}:${index}:new`
}

/** `span`/`spans` are output-only read keys; the `disabled` modifier is set deliberately. */
export function clean(node: StatementNode): RuleContentStatement {
  const { span: _span, spans: _spans, disabled: _disabled, ...rest } = node as unknown as Record<
    string,
    unknown
  >
  return rest as RuleContentStatement
}

/** The statement's disabled state as the pending edits would leave it. */
export function effectiveDisabled(
  pending: ReadonlyMap<string, PendingChange>,
  rule: number,
  list: StatementList,
  index: number,
  node: StatementNode,
): boolean {
  const change = pending.get(keyFor(rule, list, index))
  if (change?.tag === 'change') return change.statement?.disabled === true
  return node.disabled === true
}

/**
 * Stage the statement's `disabled` state as a `replace` edit — a batch
 * action's target, the editor toggle's value. Setting the state the
 * statement already has, staged edits included, is a no-op; setting it
 * back to the original unstages instead of producing a no-op `replace`.
 * A row marked for deletion takes no further staging.
 */
export function setDisabled(
  pending: Map<string, PendingChange>,
  rule: number,
  ruleName: string,
  list: StatementList,
  index: number,
  node: StatementNode,
  target: boolean,
): void {
  const key = keyFor(rule, list, index)
  const staged = pending.get(key)
  if (staged?.tag === 'delete') return
  if (effectiveDisabled(pending, rule, list, index, node) === target) return
  const statement = staged?.tag === 'change' ? { ...staged.statement } : clean(node)
  if (target) statement.disabled = true
  else delete statement.disabled
  stageStatement(pending, rule, ruleName, list, index, node, statement)
}

/**
 * Stage an arbitrary replacement statement — the argument forms' edit.
 * A row marked for deletion takes no further staging; a statement equal
 * to the original unstages instead of producing a no-op `replace`.
 */
export function stageStatement(
  pending: Map<string, PendingChange>,
  rule: number,
  ruleName: string,
  list: StatementList,
  index: number,
  node: StatementNode,
  statement: RuleContentStatement,
): void {
  const key = keyFor(rule, list, index)
  if (pending.get(key)?.tag === 'delete') return
  const original = clean(node)
  if (node.disabled === true) original.disabled = true
  if (JSON.stringify(original) === JSON.stringify(statement)) {
    pending.delete(key)
    return
  }
  pending.set(key, { rule, ruleName, list, index, tag: 'change', statement })
}

/** Stage deletions; a staged `change` at the address is superseded. */
export function stageDeletes(
  pending: Map<string, PendingChange>,
  rule: number,
  ruleName: string,
  list: StatementList,
  indices: Iterable<number>,
): void {
  for (const index of indices) {
    if (pending.get(keyFor(rule, list, index))?.tag === 'insert') continue
    pending.set(keyFor(rule, list, index), { rule, ruleName, list, index, tag: 'delete' })
  }
}

/**
 * The position a column's `+` targets (docs/ui/editor.md § Form ·
 * Insert): after the selected row when exactly one row is selected,
 * else the column end. As an `editStatements` address that is the row's
 * index + 1, or the list length — the flat index space `rules` reports,
 * hidden `end` markers included.
 */
export function insertAddress(length: number, selected: readonly number[]): number {
  return selected.length === 1 ? selected[0] + 1 : length
}

/**
 * Stage an `insert`: the new statements at `index`, plus a `before`
 * marker entry the column renders as the pending row where the insert
 * lands — after the row at `index - 1`, or at the column end.
 */
export function stageInsert(
  pending: Map<string, PendingChange>,
  rule: number,
  ruleName: string,
  list: StatementList,
  index: number,
  statements: RuleContentStatement[],
): void {
  pending.set(insertKey(rule, list, index), {
    rule,
    ruleName,
    list,
    index,
    tag: 'insert',
    statements,
  })
  pending.set(insertMarkKey(rule, list, index), {
    rule,
    ruleName,
    list,
    index,
    tag: 'insert',
    position: 'before',
  })
}

/**
 * The `editStatements` request for every staged change: `inputIdentity`
 * is the `project` digest the page read the program at, `snapshot` the
 * generation that issued the numeric rule ids.
 */
export function buildRequest(
  pending: ReadonlyMap<string, PendingChange>,
  inputIdentity: string,
  snapshot: number | undefined,
): EditStatementsRequest {
  const edits: StatementEdit[] = [...pending.values()].flatMap((change): StatementEdit[] => {
    if (change.position === 'before') return []
    if (change.tag === 'delete') {
      return [{ rule: change.rule, list: change.list, index: change.index, change: 'delete' }]
    }
    if (change.tag === 'insert') {
      return [
        {
          rule: change.rule,
          list: change.list,
          index: change.index,
          change: 'insert',
          statements: (change.statements ?? []) as [RuleContentStatement, ...RuleContentStatement[]],
        },
      ]
    }
    return [
      {
        rule: change.rule,
        list: change.list,
        index: change.index,
        change: 'replace',
        statement: change.statement,
      },
    ]
  })
  return {
    op: 'editStatements',
    inputIdentity,
    snapshot,
    edits: edits as [StatementEdit, ...StatementEdit[]],
  }
}

/**
 * Whether a refused result is a freshness failure — `edit-stale-source`
 * for an `inputIdentity`/`source_identity` mismatch, `stale-id` for a
 * numeric address scoped to a superseded generation. Stale pending edits
 * address a program the page no longer displays, so the caller clears
 * them and re-reads.
 */
export function isStale(diagnostics: readonly { code?: string }[] | undefined): boolean {
  return (diagnostics ?? []).some(
    (diagnostic) => diagnostic.code === 'edit-stale-source' || diagnostic.code === 'stale-id',
  )
}
