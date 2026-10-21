<script lang="ts">
  /**
   * The Diagnostics and Variables view dialogs (docs/ui/design.md §
   * Dialogs): the shared chrome — a centred title over the scrolling
   * `--dlg` body and a `--dlg-foot` band holding the one orange Confirm —
   * around the two read-only views. Esc and a backdrop click close, as
   * does Confirm.
   *
   * Diagnostics layers the `costEstimate` summary cards (a failure renders
   * one error card in their place), the ten most element-heavy rules, and
   * the `findings` rows over each other. Clicking a top rule or a finding
   * that carries a rule asks the page to reveal that rule, which closes
   * the dialog. Variables groups the `symbols` declarations into the
   * Global and Player columns with the subroutines centred under them;
   * the headings resolve through `lookup` in the session locale like
   * every other Workshop name, with English as the fallback.
   */
  import type {
    CostEstimateResult,
    Finding,
    Symbol,
    ToolErrorInfo,
    ToolResponse,
  } from './types/generated/wright-agent-v1'
  import { closeDialog, showModal } from './dialog'
  import type { Names } from './names'

  let {
    which,
    symbols,
    findings,
    costEstimate,
    names,
    onreveal,
    onclose,
  }: {
    which: 'variables' | 'diagnostics' | null
    symbols: unknown[] | null
    findings: unknown[] | null
    /** The `costEstimate?all` response; null until it lands. */
    costEstimate: ToolResponse | null
    names: Names
    /** Reveal a `rules` listing id: close the dialog and open its bar. */
    onreveal: (rule: number) => void
    onclose: () => void
  } = $props()

  let dialogEl: HTMLDialogElement | undefined = $state()

  $effect(() => {
    if (!dialogEl) return
    if (which !== null && !dialogEl.open) showModal(dialogEl)
    if (which === null && dialogEl.open) closeDialog(dialogEl)
  })

  const VARIABLE_HEADINGS = ['global', 'player', 'subroutines']

  // The variables headings' `lookup` warm: bumped when it lands so the
  // heads re-read `peek`, which answers the fallback until then.
  let headingNames = $state(0)
  $effect(() => {
    if (which === 'variables') {
      void names.warm(VARIABLE_HEADINGS).then(() => {
        headingNames += 1
      })
    }
  })

  /** A heading's session-locale name, capitalised for display. */
  function heading(id: string, fallback: string): string {
    void headingNames
    const name = names.peek(id, ['structural'])?.name
    return name ? name.charAt(0).toUpperCase() + name.slice(1) : fallback
  }

  interface SymbolGroups {
    globalVariable: Symbol[]
    playerVariable: Symbol[]
    subroutine: Symbol[]
  }

  // Rules already have their own column on the page; the dialog shows
  // declared variables and subroutines.
  const variableGroups = $derived.by<SymbolGroups>(() => {
    const groups: SymbolGroups = { globalVariable: [], playerVariable: [], subroutine: [] }
    for (const raw of symbols ?? []) {
      const symbol = raw as Symbol
      if (symbol.kind in groups) groups[symbol.kind as keyof SymbolGroups].push(symbol)
    }
    return groups
  })

  const noSymbols = $derived(
    symbols !== null &&
      variableGroups.globalVariable.length === 0 &&
      variableGroups.playerVariable.length === 0 &&
      variableGroups.subroutine.length === 0,
  )

  const costResult = $derived(
    costEstimate && 'result' in costEstimate
      ? (costEstimate.result as CostEstimateResult)
      : null,
  )
  // The response members' index signatures keep `in` from narrowing the
  // union itself, so the error side reads as an explicit cast.
  const costError = $derived.by<ToolErrorInfo | null>(() => {
    if (costEstimate === null) return null
    return 'error' in costEstimate ? (costEstimate.error as ToolErrorInfo) : null
  })
  const elementCount = $derived(costResult?.elementCount)
  const exact = $derived(costResult?.exact)

  interface TopRule {
    index: number
    name: string
    count: number
  }

  // `elementCount.rules[].index` is the `rules` listing id (#691): the ten
  // largest by `count`, index ascending on ties like the service's order.
  const topRules = $derived.by<TopRule[]>(() => {
    const entries = (elementCount?.rules ?? []) as {
      index?: unknown
      name?: unknown
      count?: unknown
    }[]
    return entries
      .filter((entry) => typeof entry.index === 'number')
      .map((entry) => ({
        index: entry.index as number,
        name: typeof entry.name === 'string' ? entry.name : `Rule ${entry.index}`,
        count: typeof entry.count === 'number' ? entry.count : 0,
      }))
      .sort((a, b) => b.count - a.count || a.index - b.index)
      .slice(0, 10)
  })

  const elementPercent = $derived(
    elementCount?.total !== undefined && elementCount.budget > 0
      ? Math.min(100, (elementCount.total / elementCount.budget) * 100)
      : 0,
  )

  const emittedKb = $derived(
    exact?.emittedBytes != null ? `${(exact.emittedBytes / 1024).toFixed(1)} KB` : null,
  )

  const findingList = $derived((findings ?? []) as Finding[])

  function locationOf(finding: Finding): string {
    const span = finding.span
    if (!span) return ''
    return `${span.path ?? ''}:${span.start.line}:${span.start.col}`
  }
</script>

{#snippet findingRow(finding: Finding)}
  <span class="sev">{finding.severity}</span>
  <code class="code">{finding.code}</code>
  <span class="msg">{finding.message}</span>
  {#if locationOf(finding)}<span class="loc">{locationOf(finding)}</span>{/if}
{/snippet}

<!-- A click landing on the dialog element itself is a backdrop click; it closes the dialog. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<dialog
  bind:this={dialogEl}
  onclose={onclose}
  class="dialog view"
  onclick={(event) => {
    if (event.target === dialogEl && dialogEl) closeDialog(dialogEl)
  }}
>
  <h2 class="dlg-title">{which === 'variables' ? 'Variables' : 'Diagnostics'}</h2>
  <div class="dlg-body">
    {#if which === 'variables'}
      {#if symbols === null}
        <p class="muted">Loading…</p>
      {:else if noSymbols}
        <p class="muted">No variables or subroutines declared.</p>
      {:else}
        <div class="var-columns">
          <section>
            <h3>{heading('global', 'Global')}</h3>
            <ul class="var-list">
              {#each variableGroups.globalVariable as symbol (symbol.id)}
                <li>{symbol.name}</li>
              {/each}
            </ul>
          </section>
          <section>
            <h3>{heading('player', 'Player')}</h3>
            <ul class="var-list">
              {#each variableGroups.playerVariable as symbol (symbol.id)}
                <li>{symbol.name}</li>
              {/each}
            </ul>
          </section>
        </div>
        <section class="var-subs">
          <h3>{heading('subroutines', 'Subroutines')}</h3>
          <ul class="var-list">
            {#each variableGroups.subroutine as symbol (symbol.id)}
              <li>{symbol.name}</li>
            {/each}
          </ul>
        </section>
      {/if}
    {:else if which === 'diagnostics'}
      {#if findings === null && costEstimate === null}
        <p class="muted">Loading…</p>
      {:else}
        {#if costError}
          <div class="cards">
            <section class="stat-card">
              <h3>Cost estimate</h3>
              <p class="desc"><code>{costError.code}</code> {costError.message}</p>
            </section>
          </div>
        {:else if costResult}
          <div class="cards">
            <section class="stat-card">
              <h3>Total elements</h3>
              <p class="figure">{elementCount?.total ?? '—'}</p>
              <div class="meter">
                <span class:over={elementPercent >= 100} style="width: {elementPercent}%"
                ></span>
              </div>
              <p class="desc">
                Budget {elementCount?.budget}{#if costResult.elementCountUnavailable}
                  — {costResult.elementCountUnavailable}{/if}
              </p>
            </section>
            <section class="stat-card">
              <h3>Output size</h3>
              <p class="figure">{emittedKb ?? '—'}</p>
              <p class="desc">
                {#if emittedKb === null && exact?.emittedBytesUnavailable}
                  {exact.emittedBytesUnavailable}.{/if}
                {exact?.programRules ?? 0} rules, {exact?.programActions ?? 0} actions,
                {exact?.waitActions ?? 0} waits
              </p>
            </section>
            <section class="stat-card">
              <h3>Rule length</h3>
              <p class="desc">Wright reports only the whole-program output size.</p>
            </section>
          </div>
          {#if topRules.length > 0}
            <h3 class="section-title">Rules with the most elements</h3>
            <ul class="top-rules">
              {#each topRules as rule (rule.index)}
                <li>
                  <button type="button" onclick={() => onreveal(rule.index)}>
                    <span class="name">{rule.name}</span>
                    <span class="count">{rule.count}</span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        {/if}
        {#if findingList.length > 0}
          <ul class="finding-list">
            {#each findingList as finding, i (i)}
              {@const rule = finding.rule}
              <li>
                {#if rule !== null}
                  <button
                    type="button"
                    class="finding severity-{finding.severity}"
                    onclick={() => onreveal(rule)}
                  >
                    {@render findingRow(finding)}
                  </button>
                {:else}
                  <div class="finding severity-{finding.severity}">
                    {@render findingRow(finding)}
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        {:else if findings !== null}
          <p class="muted">No diagnostics.</p>
        {/if}
      {/if}
    {/if}
  </div>
  <div class="dlg-foot">
    <button
      type="button"
      class="primary"
      onclick={() => dialogEl && closeDialog(dialogEl)}>Confirm</button
    >
  </div>
</dialog>
