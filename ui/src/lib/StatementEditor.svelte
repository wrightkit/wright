<script lang="ts">
  /**
   * The statement editor (ADR-0032 editing layer): opened by clicking a
   * statement row. The disable toggle stages a `replace` edit through the
   * pending-changes model; the statement's arguments render read-only —
   * editable argument controls belong to the dropdown layer (#835).
   */
  import { closeDialog, showModal } from './dialog'
  import Expr from './Expr.svelte'
  import type { Names } from './names'
  import { STATEMENT_KINDS } from './nodes'
  import type { StatementNode } from './edit'
  import type { Node } from './types/generated/rule-content-v1'
  import type { SpannedAction, SpannedCondition } from './view'

  let {
    target,
    names,
    disabled,
    onsave,
    onclose,
  }: {
    target: { list: 'conditions' | 'actions'; index: number; node: StatementNode } | null
    names: Names
    /** The statement's effective disabled state, pending edits included. */
    disabled: boolean
    onsave: (disabled: boolean) => void
    onclose: () => void
  } = $props()

  let dialogEl: HTMLDialogElement | undefined = $state()
  let checked = $state(false)
  let shownFor: unknown = null

  $effect(() => {
    if (!dialogEl) return
    if (target !== null) {
      if (shownFor !== target) {
        checked = disabled
        shownFor = target
      }
      if (!dialogEl.open) showModal(dialogEl)
    } else {
      shownFor = null
      if (dialogEl.open) closeDialog(dialogEl)
    }
  })

  const title = $derived.by(() => {
    if (!target) return ''
    if (target.list === 'actions') {
      const action = target.node as SpannedAction
      return names.peek(action.call, STATEMENT_KINDS)?.name ?? action.call
    }
    return 'Condition'
  })

  const exprNode = $derived(
    target === null
      ? undefined
      : target.list === 'conditions'
        ? (target.node as SpannedCondition).value
        : (target.node as SpannedAction as Node),
  )
</script>

<dialog bind:this={dialogEl} onclose={onclose} class="dialog editor">
  {#if target}
    <h2 class="editor-title">{title}</h2>
    <label class="editor-disable">
      <input type="checkbox" bind:checked={checked} data-testid="editor-disable" />
      Disabled
    </label>
    <div class="editor-statement">
      {#if exprNode !== undefined}
        <Expr
          node={exprNode}
          {names}
          kinds={target.list === 'actions' ? STATEMENT_KINDS : undefined}
        />
      {/if}
    </div>
    <div class="editor-foot">
      <button type="button" class="cancel" onclick={() => dialogEl && closeDialog(dialogEl)}
        >Cancel</button
      >
      <button
        type="button"
        class="ok"
        onclick={() => {
          onsave(checked)
          if (dialogEl) closeDialog(dialogEl)
        }}>OK</button
      >
    </div>
  {/if}
</dialog>
