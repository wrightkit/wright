<script lang="ts">
  /**
   * The read-only structured Workshop rule view (#804, ADR-0032). Rule order
   * and content come from `rules` — one `rules?all&content` call when every
   * rule can be expressed, otherwise per-rule `rules?rule&content` calls so
   * a refusal replaces only that rule's body. Names come from `lookup` by
   * identity; the Variables and Diagnostics dialogs read `symbols` and
   * `findings`. Nothing here edits or writes.
   */
  import { call } from './lib/api'
  import Dialogs from './lib/Dialogs.svelte'
  import { Names } from './lib/names'
  import RuleView from './lib/RuleView.svelte'
  import {
    debugEnabled,
    ruleStateFromError,
    ruleStateFromResponse,
    ruleStateFromSummary,
    type RuleState,
    type RuleSummary,
  } from './lib/view'
  import type { ToolErrorInfo } from './lib/types/wright-agent-v1'

  interface RuleEntry {
    summary: RuleSummary
    state: RuleState
  }

  const names = new Names(call)
  const debug = debugEnabled(location.search)

  let rules = $state<RuleEntry[] | null>(null)
  let loadError = $state<string | null>(null)
  let selected = $state<number | null>(null)
  let filter = $state('')
  let symbols = $state<unknown[] | null>(null)
  let findings = $state<unknown[] | null>(null)
  let dialog = $state<'variables' | 'diagnostics' | null>(null)

  const filtered = $derived(
    (rules ?? []).filter((entry) =>
      entry.summary.name.toLowerCase().includes(filter.trim().toLowerCase()),
    ),
  )
  const openRule = $derived(filtered.find((entry) => entry.summary.id === selected))

  async function loadOne(id: number): Promise<RuleState> {
    try {
      return ruleStateFromResponse(await call({ op: 'rules', rule: id, content: true }))
    } catch (error) {
      return ruleStateFromError(error)
    }
  }

  async function load() {
    // The bulk read expresses every rule in one call; when any rule's
    // content cannot be expressed the call refuses wholesale, so the page
    // falls back to per-rule reads and keeps each refusal in place.
    const bulk = await call({ op: 'rules', all: true, content: true })
    if ('result' in bulk) {
      const result = bulk.result
      const summaries = (
        Array.isArray(result) ? result : ((result as { rules?: RuleSummary[] }).rules ?? [])
      ) as RuleSummary[]
      rules = summaries.map((summary) => ({
        summary,
        state: ruleStateFromSummary(summary),
      }))
    } else {
      const list = await call({ op: 'rules', all: true })
      if ('error' in list) {
        const error = list.error as ToolErrorInfo
        loadError = `${error.code}: ${error.message}`
        return
      }
      const result = list.result
      const summaries = (
        Array.isArray(result) ? result : ((result as { rules?: RuleSummary[] }).rules ?? [])
      ) as RuleSummary[]
      rules = summaries.map((summary) => ({ summary, state: { phase: 'loading' } }))
      await Promise.all(
        rules.map(async (entry) => {
          entry.state = await loadOne(entry.summary.id)
        }),
      )
    }
    selected = rules[0]?.summary.id ?? null
  }

  void load()

  function openVariables() {
    if (symbols === null) {
      void call({ op: 'symbols', all: true }).then((response) => {
        if ('result' in response) {
          const result = response.result
          symbols = Array.isArray(result)
            ? result
            : ((result as { symbols?: unknown[] }).symbols ?? [])
        }
      })
    }
    dialog = 'variables'
  }

  function openDiagnostics() {
    if (findings === null) {
      void call({ op: 'findings', all: true }).then((response) => {
        if ('result' in response) {
          const result = response.result
          findings = Array.isArray(result)
            ? result
            : ((result as { findings?: unknown[] }).findings ?? [])
        }
      })
    }
    dialog = 'diagnostics'
  }
</script>

<div class="layout" data-e2e="app">
  <header class="top">
    <h1>wright ui</h1>
    <input
      class="filter"
      type="search"
      placeholder="Filter rules…"
      bind:value={filter}
      data-e2e="rule-filter"
    />
    <button type="button" onclick={openVariables} data-e2e="open-variables">Variables</button>
    <button type="button" onclick={openDiagnostics} data-e2e="open-diagnostics">
      Diagnostics
    </button>
    {#if debug}<span class="debug" data-debug>debug</span>{/if}
  </header>

  <nav class="rule-list" aria-label="Rules" data-e2e="rules">
    {#if loadError}
      <p class="refusal" data-e2e="refusal">{loadError}</p>
    {:else if rules === null}
      <p class="muted">Loading rules…</p>
    {:else if filtered.length === 0}
      <p class="muted">No rules match.</p>
    {:else}
      {#each filtered as entry (entry.summary.id)}
        <button
          type="button"
          class="rule-item"
          class:selected={entry.summary.id === selected}
          class:disabled={entry.state.phase === 'ready' && entry.state.content.disabled}
          onclick={() => (selected = entry.summary.id)}
          data-e2e="rule-item"
          data-rule-id={entry.summary.id}
        >
          <span class="rule-name" data-e2e="rule-name">{entry.summary.name}</span>
          {#if entry.state.phase === 'ready' && entry.state.content.disabled}
            <span class="disabled-badge" data-e2e="disabled">disabled</span>
          {/if}
          {#if debug}<span class="debug" data-debug>#{entry.summary.id}</span>{/if}
        </button>
      {/each}
    {/if}
  </nav>

  <main class="view">
    {#if openRule}
      <RuleView summary={openRule.summary} state={openRule.state} {names} {debug} />
    {:else}
      <p class="muted">Select a rule.</p>
    {/if}
  </main>

  <Dialogs which={dialog} {symbols} {findings} onclose={() => (dialog = null)} />
</div>
