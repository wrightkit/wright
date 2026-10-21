<script lang="ts">
  /**
   * The Variables and Diagnostics dialogs: `symbols` grouped by kind and
   * `findings` with severity, code, message, and location. Both are
   * read-only views of the service results.
   */
  import type { Finding, Symbol } from './types/generated/wright-agent-v1'
  import { closeDialog, showModal } from './dialog'

  let {
    which,
    symbols,
    findings,
    onclose,
  }: {
    which: 'variables' | 'diagnostics' | null
    symbols: unknown[] | null
    findings: unknown[] | null
    onclose: () => void
  } = $props()

  let dialogEl: HTMLDialogElement | undefined = $state()

  $effect(() => {
    if (!dialogEl) return
    if (which !== null && !dialogEl.open) showModal(dialogEl)
    if (which === null && dialogEl.open) closeDialog(dialogEl)
  })

  // Rules already have their own column; the dialog shows declared
  // variables and subroutines.
  const symbolGroups = $derived.by(() => {
    const groups = new Map<string, Symbol[]>()
    for (const raw of symbols ?? []) {
      const symbol = raw as Symbol
      if (symbol.kind === 'rule') continue
      const list = groups.get(symbol.kind) ?? []
      list.push(symbol)
      groups.set(symbol.kind, list)
    }
    return [...groups.entries()]
  })

  const findingList = $derived((findings ?? []) as Finding[])

  function locationOf(finding: Finding): string {
    const span = finding.span
    if (!span) return ''
    return `${span.path ?? ''}:${span.start.line}:${span.start.col}`
  }
</script>

<dialog
  bind:this={dialogEl}
  onclose={onclose}
  class="dialog"
>
  {#if which === 'variables'}
    <h2>Variables</h2>
    {#if symbols === null}
      <p class="muted">Loading…</p>
    {:else if symbolGroups.length === 0}
      <p class="muted">No variables or subroutines declared.</p>
    {:else}
      {#each symbolGroups as [kind, items] (kind)}
        <h3 class="symbol-kind">{kind}</h3>
        <ul class="symbol-list">
          {#each items as symbol (symbol.id)}
            <li>{symbol.name}</li>
          {/each}
        </ul>
      {/each}
    {/if}
  {:else if which === 'diagnostics'}
    <h2>Diagnostics</h2>
    {#if findings === null}
      <p class="muted">Loading…</p>
    {:else if findingList.length === 0}
      <p class="muted">No diagnostics.</p>
    {:else}
      <ul class="finding-list">
        {#each findingList as finding, i (i)}
          <li class="finding severity-{finding.severity}">
            <span class="sev">{finding.severity}</span>
            <code>{finding.code}</code>
            <span class="msg">{finding.message}</span>
            {#if locationOf(finding)}<span class="loc">{locationOf(finding)}</span>{/if}
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
  <form method="dialog"><button type="submit">Close</button></form>
</dialog>
