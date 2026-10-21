<script lang="ts">
  /**
   * The structured Workshop rule view (ADR-0032) under the in-game page
   * shell (#879): spec backdrop and tokens, the title row, and the icon
   * toolbar (docs/ui/design.md § Tokens, § Main screen 1–2). The rule
   * list is the in-game expanding bars (#880, § Main screen 3): one bar
   * per rule in `rules` order, any number open at once, the page
   * scrolling as a whole. Rule order and content come from `rules` —
   * one `rules?all&content` call when every rule can be expressed,
   * otherwise per-rule `rules?rule&content` calls so a refusal replaces
   * only that rule's body. Names come from `lookup` by identity; the
   * Variables and Diagnostics dialogs read `symbols` and `findings`.
   *
   * Editing layer: statement edits stage in the page's pending map, submit
   * as one `editStatements` request against the `project` identity and the
   * listing's generation, and apply through the host `/apply` route — the
   * page never composes or writes Workshop text. A stale refusal clears
   * the staged edits and re-reads the program.
   */
  import { SvelteMap, SvelteSet } from 'svelte/reactivity'
  import { apply, call } from './lib/api'
  import Dialogs from './lib/Dialogs.svelte'
  import PendingChanges from './lib/PendingChanges.svelte'
  import RuleBar from './lib/RuleBar.svelte'
  import StatementEditor from './lib/StatementEditor.svelte'
  import {
    buildRequest,
    effectiveDisabled,
    isStale,
    setDisabled,
    stageDeletes,
    type BatchAction,
    type PendingChange,
    type StatementList,
    type StatementNode,
  } from './lib/edit'
  import { Names } from './lib/names'
  import RuleView from './lib/RuleView.svelte'
  import SqButton from './lib/SqButton.svelte'
  import {
    debugEnabled,
    ruleStateFromError,
    ruleStateFromResponse,
    ruleStateFromSummary,
    type RuleState,
    type RuleSummary,
  } from './lib/view'
  import type {
    CostEstimateResult,
    EditStatementsResult,
    ProjectResult,
    ToolErrorInfo,
  } from './lib/types/generated/wright-agent-v1'

  interface RuleEntry {
    summary: RuleSummary
    state: RuleState
  }

  interface Refusal {
    code?: string
    message?: string
  }

  const names = new Names(call)
  const debug = debugEnabled(location.search)

  let rules = $state<RuleEntry[] | null>(null)
  let loadError = $state<string | null>(null)
  // Open state is per rule, keyed by the listing id: any number of rules
  // stay open at once and a rule filtered out keeps its open state (#880).
  const openRules = new SvelteSet<number>()
  let filter = $state('')
  let fileName = $state<string | null>(null)
  let symbols = $state<unknown[] | null>(null)
  let findings = $state<unknown[] | null>(null)
  let dialog = $state<'variables' | 'diagnostics' | 'pending' | null>(null)

  // The program's identity (`project`) and generation (the rules listing's
  // `snapshot`): the freshness pair an `editStatements` request carries.
  let inputIdentity = $state<string | null>(null)
  let snapshot = $state<number | undefined>(undefined)
  // Staged statement edits keyed `rule:list:index`; the map is `$state`
  // so staging a change re-renders the marks and the toolbar count.
  let pending = $state<Map<string, PendingChange>>(new SvelteMap())
  let editTarget = $state<{ rule: number; list: StatementList; index: number } | null>(null)
  let submitting = $state(false)
  let submitRefusal = $state<Refusal[] | null>(null)
  // Bumped when the bars' event-name warm lands so `peek` re-reads; the
  // cells stay empty until the first warm resolves rather than flashing
  // a raw event id.
  let namesVersion = $state(0)
  // `?debug` only: element counts by `elementCount.rules[].index`, which
  // is the `rules` listing id (#691); null until the call lands.
  let elementCounts = $state<Record<number, number> | null>(null)

  const filtered = $derived(
    (rules ?? []).filter((entry) =>
      entry.summary.name.toLowerCase().includes(filter.trim().toLowerCase()),
    ),
  )

  /** The rule the open statement editor's target belongs to. */
  const editEntry = $derived.by(() => {
    const target = editTarget
    if (target === null) return null
    return (rules ?? []).find((entry) => entry.summary.id === target.rule) ?? null
  })

  const editNode = $derived.by(() => {
    if (editTarget === null || !editEntry || editEntry.state.phase !== 'ready') return null
    const node = editEntry.state.content[editTarget.list][editTarget.index] as
      | StatementNode
      | undefined
    return node ? { ...editTarget, node } : null
  })
  const editDisabled = $derived(
    editNode && editEntry && editTarget
      ? effectiveDisabled(pending, editEntry.summary.id, editTarget.list, editTarget.index, editNode.node)
      : false,
  )
  const pendingRequest = $derived(
    debug && dialog === 'pending' && inputIdentity !== null
      ? buildRequest(pending, inputIdentity, snapshot)
      : null,
  )

  async function loadOne(id: number, generation?: number): Promise<RuleState> {
    try {
      // `rule` is a numeric address the listing issued, so its `snapshot`
      // generation rides along (#595).
      return ruleStateFromResponse(await call({ op: 'rules', rule: id, snapshot: generation, content: true }))
    } catch (error) {
      return ruleStateFromError(error)
    }
  }

  /**
   * `project` carries both freshness and title data: `inputIdentity` is the
   * precondition `editStatements` requires, and `sourceFiles` lists the
   * loaded sources in canonical file-id order so `[0]` is the entry source
   * and only its base name shows in the title row (#879).
   */
  async function loadProject() {
    try {
      const response = await call({ op: 'project' })
      if ('result' in response) {
        const result = response.result as ProjectResult
        inputIdentity = result.inputIdentity ?? null
        const entry = result.sourceFiles?.[0]
        const base = entry?.split(/[\\/]/).filter(Boolean).pop()
        fileName = base && base !== '.' ? base : null
      }
    } catch {
      // An unreachable endpoint leaves the identity unset and the title
      // row without a file name; Submit refuses rather than composing a
      // request without a precondition.
    }
  }

  async function load() {
    // The bulk read expresses every rule in one call; when any rule's
    // content cannot be expressed the call refuses wholesale, so the page
    // falls back to per-rule reads and keeps each refusal in place.
    if (debug) void loadElementCounts()
    const bulk = await call({ op: 'rules', all: true, content: true })
    if ('result' in bulk) {
      const result = bulk.result
      if (result !== null && typeof result === 'object' && !Array.isArray(result)) {
        snapshot = (result as { snapshot?: number }).snapshot
      }
      const summaries = (
        Array.isArray(result) ? result : ((result as { rules?: RuleSummary[] }).rules ?? [])
      ) as RuleSummary[]
      rules = summaries.map((summary) => ({
        summary,
        state: ruleStateFromSummary(summary),
      }))
      warmEvents(summaries)
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
      const generation =
        result !== null && typeof result === 'object' && !Array.isArray(result)
          ? (result as { snapshot?: number }).snapshot
          : undefined
      snapshot = generation
      rules = summaries.map((summary) => ({ summary, state: { phase: 'loading' } }))
      // Bars render while the per-rule reads finish: an open click during
      // the reads is a state `rules` never overwrites.
      warmEvents(summaries)
      await Promise.all(
        rules.map(async (entry) => {
          entry.state = await loadOne(entry.summary.id, generation)
        }),
      )
    }
  }

  /**
   * Warm the bars' event ids so `peek` answers synchronously, then bump
   * `namesVersion` so the cells re-read their names. `subroutine:` ids
   * are not catalog identities and need no lookup.
   */
  function warmEvents(summaries: RuleSummary[]) {
    const ids = summaries
      .map((summary) => summary.event)
      .filter(
        (event): event is string =>
          typeof event === 'string' && !event.startsWith('subroutine:'),
      )
    void names.warm(ids).then(() => {
      namesVersion += 1
    })
  }

  /**
   * The bar's event cell (#880 requirement 2): the event's `lookup` name
   * in the session locale, the subroutine name for a subroutine rule,
   * and the raw event id when the lookup found no name.
   */
  function barEvent(summary: RuleSummary): string {
    const event = summary.event
    if (event === undefined) return ''
    if (event.startsWith('subroutine:')) return event.slice('subroutine:'.length)
    if (namesVersion === 0) return ''
    return names.peek(event, ['event'])?.name ?? event
  }

  /**
   * `?debug` only: `elementCount.rules` ranks rules by count with `index`
   * naming the `rules` listing id, so the bars can show each rule's
   * element count. A refused or unreachable call leaves the cells empty.
   */
  async function loadElementCounts() {
    try {
      const response = await call({ op: 'costEstimate', all: true })
      if ('result' in response) {
        const entries =
          ((response.result as CostEstimateResult).elementCount?.rules ?? []) as {
            index?: unknown
            count?: unknown
          }[]
        const counts: Record<number, number> = {}
        for (const entry of entries) {
          if (typeof entry.index === 'number' && typeof entry.count === 'number') {
            counts[entry.index] = entry.count
          }
        }
        elementCounts = counts
      }
    } catch {
      // A debug decoration never blocks the page.
    }
  }

  function toggle(id: number) {
    if (openRules.has(id)) openRules.delete(id)
    else openRules.add(id)
  }

  void loadProject()
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

  /**
   * The `rules` content node the open editor acts on. Each open rule's
   * view gets callbacks bound to its listing id, so with several rules
   * open the target is the rule that issued the call.
   */
  const oneditFor = (rule: number) => ({
    open: (list: StatementList, index: number) => {
      editTarget = { rule, list, index }
    },
    batch: (list: StatementList, indices: number[], action: BatchAction) => {
      const entry = (rules ?? []).find((item) => item.summary.id === rule)
      if (!entry || entry.state.phase !== 'ready') return
      if (action === 'delete') {
        stageDeletes(pending, rule, entry.summary.name, list, indices)
        return
      }
      const target = action === 'disable'
      for (const index of indices) {
        const node = entry.state.content[list][index] as StatementNode | undefined
        if (node) setDisabled(pending, rule, entry.summary.name, list, index, node, target)
      }
    },
  })

  function editorSave(disabled: boolean) {
    if (!editNode || !editEntry || !editTarget) return
    setDisabled(
      pending,
      editEntry.summary.id,
      editEntry.summary.name,
      editTarget.list,
      editTarget.index,
      editNode.node,
      disabled,
    )
  }

  /** A stale refusal's staged edits addressed a superseded program. */
  async function refreshAfterStale() {
    pending.clear()
    editTarget = null
    await loadProject()
    await load()
  }

  /**
   * Submit the staged edits: `editStatements` plans and validates them —
   * refusing on a stale `inputIdentity` or generation before anything is
   * written — and the host `/apply` re-checks and writes the returned
   * transaction. Refusals surface their diagnostics in the dialog; a
   * stale refusal also drops the staging and re-reads the program.
   */
  async function submitPending() {
    if (inputIdentity === null) {
      submitRefusal = [
        { code: 'input-unknown', message: 'the project identity has not loaded; nothing was written' },
      ]
      return
    }
    submitRefusal = null
    submitting = true
    try {
      const response = await call(buildRequest(pending, inputIdentity, snapshot))
      if ('error' in response) {
        const err = response.error as ToolErrorInfo
        submitRefusal = [err]
        if (err.code === 'stale-id') await refreshAfterStale()
        return
      }
      const result = response.result as EditStatementsResult
      if (!result.ok || !result.transaction) {
        submitRefusal = result.diagnostics ?? [
          { code: 'edit-refused', message: 'the service refused the edits' },
        ]
        if (isStale(result.diagnostics)) await refreshAfterStale()
        return
      }
      const applied = await apply(result.transaction)
      if (!applied.ok) {
        submitRefusal = applied.diagnostics
        if (isStale(applied.diagnostics)) await refreshAfterStale()
        return
      }
      pending.clear()
      editTarget = null
      dialog = null
      await loadProject()
      await load()
    } catch (error) {
      submitRefusal = [
        {
          code: 'endpoint-unreachable',
          message: error instanceof Error ? error.message : String(error),
        },
      ]
    } finally {
      submitting = false
    }
  }
</script>

<div class="page">
  <header class="title-row">
    <h1>Workshop</h1>
    {#if fileName}<span class="file-name">{fileName}</span>{/if}
  </header>

  <div class="toolbar" role="toolbar" aria-label="Rules toolbar">
    <span class="rule-count">Rules ({rules?.length ?? 0})</span>
    <SqButton title="Diagnostics" onclick={openDiagnostics}>
      <path d="M3.5 12.5h4l2.5-7 4 13 2.5-6h4" />
    </SqButton>
    <div class="end">
      <input
        class="filter"
        type="search"
        placeholder="Filter rules…"
        bind:value={filter}
      />
      <!-- #883 adds the locale selector here -->
      <span class="sep" aria-hidden="true"></span>
      <SqButton title="Variables" onclick={openVariables}>
        <path d="M9.5 6h11M9.5 12h11M9.5 18h11" />
        <circle cx="4.5" cy="6" r="1.4" fill="currentColor" stroke="none" />
        <circle cx="4.5" cy="12" r="1.4" fill="currentColor" stroke="none" />
        <circle cx="4.5" cy="18" r="1.4" fill="currentColor" stroke="none" />
      </SqButton>
      <SqButton
        title="Pending changes"
        badge={pending.size}
        onclick={() => {
          submitRefusal = null
          dialog = 'pending'
        }}
      >
        <path d="M6 4h12v16H6z" />
        <path d="M9 9h6M9 12.5h6M9 16h4" />
      </SqButton>
      <span class="sep" aria-hidden="true"></span>
      <SqButton title="Add rule: rule-level edits are not available" label="Add rule" disabled>
        <path d="M12 5v14M5 12h14" />
      </SqButton>
      {#if debug}<span class="debug" data-debug>debug</span>{/if}
    </div>
  </div>

  <section class="rule-list" aria-label="Rules">
    {#if loadError}
      <p class="refusal">{loadError}</p>
    {:else if rules === null}
      <p class="muted">Loading rules…</p>
    {:else if filtered.length === 0}
      <p class="muted">No rules match.</p>
    {:else}
      {#each filtered as entry (entry.summary.id)}
        {@const isOpen = openRules.has(entry.summary.id)}
        <div class="rule-slot" data-rule-id={entry.summary.id}>
          <RuleBar
            name={entry.summary.name}
            event={barEvent(entry.summary)}
            disabled={entry.state.phase === 'ready' && entry.state.content.disabled}
            open={isOpen}
            elements={debug ? (elementCounts?.[entry.summary.id]) : undefined}
            ontoggle={() => toggle(entry.summary.id)}
          />
          {#if isOpen}
            <div class="rule-body">
              <RuleView
                summary={entry.summary}
                state={entry.state}
                {names}
                {debug}
                {pending}
                onedit={oneditFor(entry.summary.id)}
              />
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  </section>

  <Dialogs which={dialog === 'pending' ? null : dialog} {symbols} {findings} onclose={() => (dialog = null)} />

  <PendingChanges
    open={dialog === 'pending'}
    changes={[...pending.values()]}
    refusal={submitRefusal}
    {submitting}
    request={pendingRequest}
    {debug}
    onsubmit={() => void submitPending()}
    ondiscard={() => {
      pending.clear()
      submitRefusal = null
    }}
    onclose={() => (dialog = null)}
  />

  <StatementEditor
    target={editNode}
    {names}
    disabled={editDisabled}
    onsave={editorSave}
    onclose={() => (editTarget = null)}
  />
</div>
