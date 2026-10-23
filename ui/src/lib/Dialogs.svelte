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
  import type { ChromeStrings } from './chrome'
  import Dialog from './components/Dialog.svelte'
  import { severityGroups, type Severity } from './findings'
  import SeverityMark from './SeverityMark.svelte'
  import type { Names } from './names'

  let {
    which,
    symbols,
    findings,
    findingsError = null,
    costEstimate,
    names,
    onreveal,
    t,
    onclose,
    groupOpen,
    onGroupToggle,
  }: {
    which: 'variables' | 'diagnostics' | null
    symbols: unknown[] | null
    findings: unknown[] | null
    /** The `findings` refusal or transport error; the list stays null while it shows. */
    findingsError?: ToolErrorInfo | null
    /** The `costEstimate?all` response; null until it lands. */
    costEstimate: ToolResponse | null
    names: Names
    /** Reveal a rule: close the dialog and open its bar; the row form scrolls and flashes it. */
    onreveal: (rule: number, finding?: Finding) => void
    /** The session locale's chrome strings. */
    t: ChromeStrings
    onclose: () => void
    /**
     * The severity headings' open state, kept by the page so the choice
     * persists for the session (docs/ui/diagnostics.md § In the dialog).
     */
    groupOpen: Record<Severity, boolean>
    onGroupToggle: (severity: Severity, open: boolean) => void
  } = $props()

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

  // The severity groups the dialog lists, most severe first
  // (docs/ui/diagnostics.md § In the dialog): error and warning open,
  // info collapsed, the choice kept in the page's session state.
  const groups = $derived(severityGroups(findingList, groupOpen))

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

<Dialog
  view
  closeOnOutside
  open={which !== null}
  title={which === 'variables' ? t.variables : t.diagnostics}
  {onclose}
>
  {#if which === 'variables'}
    {#if symbols === null}
      <p class="muted">{t.loading}</p>
    {:else if noSymbols}
      <p class="muted">{t.noVariables}</p>
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
      <p class="muted">{t.loading}</p>
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
        <!-- Findings grouped by severity, most severe first; each heading
             toggles its level (error and warning open, info collapsed,
             the choice kept for the session). -->
        {#each groups as group (group.severity)}
          <details
            class="severity-group"
            open={group.open}
            ontoggle={(event) =>
              onGroupToggle(group.severity, event.currentTarget.open)}
          >
            <summary class="severity-heading severity-{group.severity}">
              <SeverityMark severity={group.severity} />
              <span class="severity-name">{t.severityName(group.severity)}</span>
              <span class="severity-count">({group.findings.length})</span>
            </summary>
            <ul class="finding-list">
              {#each group.findings as finding, i (i)}
                {@const rule = finding.rule}
                <li>
                  {#if rule !== null}
                    <button
                      type="button"
                      class="finding severity-{finding.severity}"
                      onclick={() => onreveal(rule, finding)}
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
          </details>
        {/each}
      {:else if findingsError !== null}
        <p class="refusal findings-refusal"><code>{findingsError.code}</code> {findingsError.message}</p>
      {:else if findings !== null}
        <p class="muted">{t.noDiagnostics}</p>
      {/if}
    {/if}
  {/if}
  {#snippet footer()}
    <button type="button" class="primary" onclick={onclose}>{t.confirm}</button>
  {/snippet}
</Dialog>
