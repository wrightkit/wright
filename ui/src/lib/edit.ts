/**
 * The editing layer's pending-change model (ADR-0032 layer 2): staged
 * statement edits live against the program the page last read, submit
 * through `editStatements`, and apply through the host `/apply` route.
 * Nothing here composes Workshop text — statements round-trip as the
 * `rule-content-v1` nodes `rules` returned, minus the output-only
 * `span`/`spans` keys the write path refuses.
 */
import type {
  EditStatementsRequest,
  RuleContentStatement,
  StatementEdit,
} from './types/generated/wright-agent-v1'
import type { SpannedAction, SpannedCondition } from './view'

/** The pending-dialog tags (design.md): a toggle is a `change`, a removal a `delete`. */
export type PendingTag = 'change' | 'delete'

/** The two `rule-content-v1` statement lists an edit addresses. */
export type StatementList = 'conditions' | 'actions'

/** The batch bar's operations on a selection. */
export type BatchAction = 'disable' | 'enable' | 'delete'

/** One staged statement edit, addressed against the pre-edit program. */
export interface PendingChange {
  /** The rule's numeric index — a `rules` listing id, sent with `snapshot`. */
  rule: number
  /** The rule's declared name, kept only for the dialog's grouping. */
  ruleName: string
  list: StatementList
  index: number
  tag: PendingTag
  /** The `change` edit's replacement statement — spans stripped, `disabled` set or removed. */
  statement?: RuleContentStatement
}

export type StatementNode = SpannedCondition | SpannedAction

export function keyFor(rule: number, list: string, index: number): string {
  return `${rule}:${list}:${index}`
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
  if (pending.get(key)?.tag === 'delete') return
  if (effectiveDisabled(pending, rule, list, index, node) === target) return
  if (target === (node.disabled === true)) {
    pending.delete(key)
    return
  }
  const statement = clean(node)
  if (target) statement.disabled = true
  pending.set(key, { rule, ruleName, list, index, tag: 'change', statement })
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
    pending.set(keyFor(rule, list, index), { rule, ruleName, list, index, tag: 'delete' })
  }
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
  const edits: StatementEdit[] = [...pending.values()].map((change) =>
    change.tag === 'delete'
      ? { rule: change.rule, list: change.list, index: change.index, change: 'delete' }
      : {
          rule: change.rule,
          list: change.list,
          index: change.index,
          change: 'replace',
          statement: change.statement,
        },
  )
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
