<script lang="ts">
  /**
   * The pending-changes dialog (ADR-0032 editing layer): staged statement
   * edits grouped by rule, tagged change/delete, submitted through
   * `editStatements` and the host `/apply` write path. A refusal —
   * including a stale identity — renders here and writes nothing; a stale
   * refusal also clears the staged edits, which addressed a program the
   * page no longer displays.
   */
  import type { EditStatementsRequest } from './types/generated/wright-agent-v1'
  import type { ChromeStrings } from './chrome'
  import { closeDialog, showModal } from './dialog'
  import type { PendingChange } from './edit'

  let {
    open,
    changes,
    refusal,
    submitting,
    request,
    debug,
    t,
    onsubmit,
    ondiscard,
    onclose,
  }: {
    open: boolean
    changes: PendingChange[]
    /** Diagnostics from the refused `editStatements` result or apply. */
    refusal: { code?: string; message?: string }[] | null
    submitting: boolean
    /** The `editStatements` request Submit would send — `?debug` only. */
    request: EditStatementsRequest | null
    debug: boolean
    /** The session locale's chrome strings. */
    t: ChromeStrings
    onsubmit: () => void
    ondiscard: () => void
    onclose: () => void
  } = $props()

  let dialogEl: HTMLDialogElement | undefined = $state()

  $effect(() => {
    if (!dialogEl) return
    if (open && !dialogEl.open) showModal(dialogEl)
    if (!open && dialogEl.open) closeDialog(dialogEl)
  })

  const groups = $derived.by(() => {
    const byRule = new Map<number, { name: string; items: PendingChange[] }>()
    for (const change of changes) {
      const group = byRule.get(change.rule) ?? { name: change.ruleName, items: [] }
      group.items.push(change)
      byRule.set(change.rule, group)
    }
    // The dialog orders a rule's lists the way the document does
    // (design.md): event first — no event edits exist yet — then
    // conditions, then actions.
    const order = { conditions: 0, actions: 1 }
    for (const group of byRule.values()) {
      group.items.sort((a, b) => order[a.list] - order[b.list] || a.index - b.index)
    }
    return [...byRule.entries()]
  })
</script>

<dialog bind:this={dialogEl} onclose={onclose} class="dialog pending">
  <h2>{t.pendingChanges}</h2>
  {#if refusal}
    <div class="refusal" data-testid="pending-refusal">
      {#each refusal as diagnostic, i (i)}
        <p><code>{diagnostic.code}</code> {diagnostic.message}</p>
      {/each}
    </div>
  {/if}
  {#if changes.length === 0}
    <p class="muted">{t.noPending}</p>
  {:else}
    {#each groups as [rule, group] (rule)}
      <h3 class="pending-rule">{group.name}</h3>
      <ul class="pending-list">
        {#each group.items as change (`${change.list}:${change.index}`)}
          <li>
            <span class="pending-tag {change.tag}">{change.tag}</span>
            {change.list}[{change.index}]
          </li>
        {/each}
      </ul>
    {/each}
  {/if}
  {#if debug && request}
    <details class="debug" data-debug>
      <summary>editStatements request</summary>
      <pre>{JSON.stringify(request, null, 2)}</pre>
    </details>
  {/if}
  <div class="editor-foot">
    <button
      type="button"
      disabled={submitting || changes.length === 0}
      onclick={ondiscard}>{t.discardAll}</button
    >
    <button type="button" onclick={() => dialogEl && closeDialog(dialogEl)}>{t.cancel}</button>
    <button
      type="button"
      class="ok"
      disabled={submitting || changes.length === 0}
      onclick={onsubmit}>{submitting ? t.applying : t.submit}</button
    >
  </div>
</dialog>
