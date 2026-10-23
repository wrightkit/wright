<script lang="ts">
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
    findingsError?: ToolErrorInfo | null
    costEstimate: ToolResponse | null
    names: Names
    onreveal: (rule: number, finding?: Finding) => void
    t: ChromeStrings
    onclose: () => void
    groupOpen: Record<Severity, boolean>
    onGroupToggle: (severity: Severity, open: boolean) => void
  } = $props()

  const VARIABLE_HEADINGS = ['global', 'player', 'subroutines']

  let headingNames = $state(0)
  $effect(() => {
    if (which === 'variables') {
      void names.warm(VARIABLE_HEADINGS).then(() => {
        headingNames += 1
      })
    }
  })

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

<style>
  :global(.dialog.view) .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(12rem, 1fr));
    gap: 0.75rem;
    margin-bottom: 1rem;
  }

  :global(.dialog.view) .stat-card {
    background: var(--card);
    border-radius: 14px;
    padding: 0.9rem 1rem;
  }

  :global(.dialog.view) .stat-card h3 {
    margin: 0;
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }

  :global(.dialog.view) .stat-card .figure {
    margin: 0.15rem 0 0;
    padding-bottom: 0.5rem;
    border-bottom: 2px solid color-mix(in srgb, var(--text) 50%, transparent);
    font-size: 1.8rem;
    font-style: italic;
    font-weight: 700;
    line-height: 1.2;
  }

  :global(.dialog.view) .stat-card .meter {
    height: 0.45rem;
    margin-top: 0.55rem;
    border-radius: 3px;
    background: rgb(0 0 0 / 0.35);
    overflow: hidden;
  }

  :global(.dialog.view) .stat-card .meter span {
    display: block;
    height: 100%;
    background: var(--orange);
  }

  :global(.dialog.view) .stat-card .meter span.over {
    background: var(--error);
  }

  :global(.dialog.view) .stat-card .desc {
    margin: 0.55rem 0 0;
    font-size: 0.82rem;
    color: var(--muted);
  }

  :global(.dialog.view) .stat-card .desc code {
    color: var(--error);
  }

  :global(.dialog.view) .section-title {
    margin: 0.25rem 0 0.4rem;
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }

  :global(.dialog.view) .top-rules,
  :global(.dialog.view) .finding-list {
    list-style: none;
    margin: 0 0 0.75rem;
    padding: 0;
  }

  :global(.dialog.view) .top-rules button {
    display: flex;
    justify-content: space-between;
    gap: 0.75rem;
    width: 100%;
    padding: 0.35rem 0.6rem;
    border: 0;
    border-radius: 3px;
    background: none;
    color: var(--text);
    text-align: left;
  }

  :global(.dialog.view) .top-rules button:hover {
    background: var(--bar-hover);
  }

  :global(.dialog.view) .top-rules .count {
    font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    color: var(--muted);
  }

  :global(.dialog.view) .severity-group {
    margin-bottom: 0.4rem;
  }

  :global(.dialog.view) .severity-heading {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.2rem 0;
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    cursor: pointer;
    list-style: none;
  }

  :global(.dialog.view) .severity-heading::-webkit-details-marker {
    display: none;
  }

  :global(.dialog.view) .severity-heading::before {
    content: '';
    width: 0;
    height: 0;
    border-left: 4px solid transparent;
    border-right: 4px solid transparent;
    border-top: 6px solid var(--muted);
    transform: rotate(-90deg);
  }

  :global(.dialog.view) .severity-group[open] > .severity-heading::before {
    transform: rotate(0deg);
  }

  :global(.dialog.view) .severity-heading .severity-count {
    color: var(--muted);
  }

  :global(.dialog.view) .severity-heading.severity-error .severity-name {
    color: var(--error);
  }

  :global(.dialog.view) .severity-heading.severity-warning .severity-name {
    color: var(--warn);
  }

  :global(.dialog.view) .severity-heading.severity-info .severity-name {
    color: var(--muted);
  }

  :global(.dialog.view) .finding-list {
    margin: 0.15rem 0 0.5rem;
  }

  :global(.dialog.view) .finding-list li:nth-child(even) {
    background: var(--card);
  }

  :global(.dialog.view) .finding {
    display: grid;
    grid-template-columns: auto auto 1fr;
    gap: 0.1rem 0.6rem;
    width: 100%;
    padding: 0.45rem 0.6rem;
    border: 0;
    background: none;
    color: var(--text);
    text-align: left;
    font-size: 0.85rem;
  }

  :global(.dialog.view) button.finding {
    cursor: pointer;
  }

  :global(.dialog.view) button.finding:hover {
    background: var(--bar-hover);
  }

  :global(.dialog.view) .finding .sev {
    padding-top: 0.1rem;
    font-size: 0.7rem;
    text-transform: uppercase;
  }

  :global(.dialog.view) .finding.severity-error .sev {
    color: var(--error);
  }

  :global(.dialog.view) .finding.severity-warning .sev {
    color: var(--warn);
  }

  :global(.dialog.view) .finding.severity-info .sev {
    color: var(--muted);
  }

  :global(.dialog.view) .finding .code {
    font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    font-size: 0.78rem;
    color: var(--dim);
  }

  :global(.dialog.view) .finding .loc {
    grid-column: 1 / -1;
    color: var(--muted);
    font-size: 0.75rem;
  }

  :global(.dialog.view) .var-columns {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1rem;
  }

  :global(.dialog.view) .var-columns h3,
  :global(.dialog.view) .var-subs h3 {
    margin: 0 0 0.55rem;
    text-align: center;
    font-size: 0.95rem;
  }

  :global(.dialog.view) .var-list {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    list-style: none;
    margin: 0;
    padding: 0;
  }

  :global(.dialog.view) .var-list li {
    padding: 0.35rem 0.65rem;
    border-radius: 3px;
    background: var(--card);
    font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    font-size: 0.85rem;
  }

  :global(.dialog.view) .var-subs {
    margin-top: 1rem;
  }

  :global(.dialog.view) .var-subs .var-list {
    flex-direction: row;
    flex-wrap: wrap;
    justify-content: center;
  }
</style>
