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
  /** Set on the placeholder mark an insert renders: view state, never an edit. */
  mark?: boolean
}

export type StatementNode = SpannedCondition | SpannedAction

export function keyFor(rule: number, list: string, index: number): string {
  return `${rule}:${list}:${index}`
}

/**
 * The pending-map key of an `insert` staged at `index`, in its own
 * `insert:` namespace so it never collides with the `change`/`delete`
 * `keyFor` addresses an existing row at the same index. The two are
 * still the same `editStatements` address — the service refuses a pair
 * at one address with `edit-duplicate-address` — so an insert and a
 * row edit at one index cannot coexist: the later staging supersedes
 * (see `stageInsert`/`stageDeletes`).
 */
export function insertKey(rule: number, list: string, index: number): string {
  return `insert:${rule}:${list}:${index}`
}

/**
 * The pending-map key of the placeholder mark a staged `insert` renders
 * where the new row lands — also `insert:`-namespaced, a different entry
 * from the edit so the mark is view state, never a second edit.
 */
export function insertMarkKey(rule: number, list: string, index: number): string {
  return `insert:${rule}:${list}:${index}:mark`
}

/** Drop a staged insert's edit and placeholder mark at `index`. */
function clearInsertAt(
  pending: Map<string, PendingChange>,
  rule: number,
  list: string,
  index: number,
): void {
  pending.delete(insertKey(rule, list, index))
  pending.delete(insertMarkKey(rule, list, index))
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
  // A row edit and an `insert` at one index share the editStatements
  // address; the later staging supersedes.
  clearInsertAt(pending, rule, list, index)
  pending.set(key, { rule, ruleName, list, index, tag: 'change', statement })
}

/** Stage deletions; a staged `change` or `insert` at the address is superseded. */
export function stageDeletes(
  pending: Map<string, PendingChange>,
  rule: number,
  ruleName: string,
  list: StatementList,
  indices: Iterable<number>,
): void {
  for (const index of indices) {
    clearInsertAt(pending, rule, list, index)
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
 * Stage an `insert`: the new statements at `index` under `insertKey`,
 * plus the placeholder mark under `insertMarkKey` the column renders
 * where the row lands. Two inserts at the same index re-stage (the
 * second replaces the first). An insert at `index` shares the
 * `editStatements` address of a `change`/`delete` on the existing row
 * at that index — the service refuses both (`edit-duplicate-address`)
 * — so staging the insert supersedes a staged row edit there, and
 * staging a row edit supersedes the insert (the later gesture wins,
 * like `change`/`delete` supersession).
 */
export function stageInsert(
  pending: Map<string, PendingChange>,
  rule: number,
  ruleName: string,
  list: StatementList,
  index: number,
  statements: RuleContentStatement[],
): void {
  pending.delete(keyFor(rule, list, index))
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
    mark: true,
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
    if (change.mark === true) return []
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
