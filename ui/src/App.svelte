<script lang="ts">
  import { Activity, FileText, List, ListFilter, Play, Plus } from '@lucide/svelte'
  import { tick } from 'svelte'
  import { SvelteMap, SvelteSet } from 'svelte/reactivity'
  import { apply, call } from './lib/api'
  import { chromeStrings, localeName } from './lib/chrome'
  import Dialogs from './lib/Dialogs.svelte'
  import PendingChanges from './lib/PendingChanges.svelte'
  import RuleBar from './lib/RuleBar.svelte'
  import StatementEditor from './lib/StatementEditor.svelte'
  import {
    buildRequest,
    clean,
    effectiveDisabled,
    insertAddress,
    isStale,
    keyFor,
    setDisabled,
    stageDeletes,
    stageInsert,
    stageStatement,
    type BatchAction,
    type PendingChange,
    type StatementList,
    type StatementNode,
  } from './lib/edit'
  import {
    allFor,
    badgeSeverities,
    DEFAULT_GROUP_OPEN,
    findingsFor,
    hasFindingAtOrAbove,
    lowestShownSeverity,
    placeFinding,
    placeFindings,
    severityCounts,
    type Severity,
  } from './lib/findings'
  import { Names } from './lib/names'
  import RuleView from './lib/RuleView.svelte'
  import SimulatePanel from './lib/SimulatePanel.svelte'
  import SqButton from './lib/SqButton.svelte'
  import {
    debugEnabled,
    ruleStateFromError,
    ruleStateFromResponse,
    ruleStateFromSummary,
    type RuleContent,
    type RuleState,
    type RuleSummary,
  } from './lib/view'
  import type {
    CostEstimateResult,
    EditStatementsResult,
    Finding,
    LookupEntry,
    ProjectResult,
    TargetMetadataResult,
    ToolErrorInfo,
    ToolResponse,
  } from './lib/types/generated/wright-agent-v1'
  import type { Node } from './lib/types/generated/rule-content-v1'

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
  let sessionLocale = $state('en-US')
  let targetLocales = $state<string[]>([])
  const t = $derived(chromeStrings(sessionLocale))
  const localeOptions = $derived(
    targetLocales.some((locale) => locale.toLowerCase() === sessionLocale.toLowerCase())
      ? targetLocales
      : [sessionLocale, ...targetLocales],
  )
  const openRules = new SvelteSet<number>()
  let revealRow = $state<{
    rule: number
    list: StatementList
    index: number
    nonce: number
  } | null>(null)
  let revealNonce = 0
  let filter = $state('')
  let onlyFindings = $state(false)
  const severityOpen = $state<Record<Severity, boolean>>({ ...DEFAULT_GROUP_OPEN })
  let fileName = $state<string | null>(null)
  let symbols = $state<unknown[] | null>(null)
  let findings = $state<Finding[] | null>(null)
  let findingsError = $state<ToolErrorInfo | null>(null)
  let dialog = $state<'variables' | 'diagnostics' | 'pending' | 'simulate' | null>(null)
  let project = $state<ProjectResult | null>(null)
  let heroes = $state<LookupEntry[] | null>(null)
  let heroesLocale = $state<string | null>(null)

  let inputIdentity = $state<string | null>(null)
  let snapshot = $state<number | undefined>(undefined)
  let pending = $state<Map<string, PendingChange>>(new SvelteMap())
  let editTarget = $state<{
    rule: number
    list: StatementList
    index: number
    argPath?: number[]
    insert?: { index: number; afterRow: number | null }
  } | null>(null)
  let submitting = $state(false)
  let submitRefusal = $state<Refusal[] | null>(null)
  let namesVersion = $state(0)
  let elementCounts = $state<Record<number, number> | null>(null)
  let costEstimate = $state<ToolResponse | null>(null)
  let costRequest: Promise<void> | null = null
  let pageEl: HTMLDivElement | undefined = $state()

  const placedFindings = $derived.by(() => {
    const contents = new Map<number, RuleContent>()
    for (const entry of rules ?? []) {
      if (entry.state.phase === 'ready') contents.set(entry.summary.id, entry.state.content)
    }
    return placeFindings(findings ?? [], contents)
  })

  const severityBadges = $derived.by(() => {
    const counts = severityCounts(findings ?? [])
    return badgeSeverities(findings ?? []).map((severity) => ({
      severity,
      count: counts[severity],
    }))
  })

  const shownFloor = $derived(lowestShownSeverity(findings ?? []))

  const filtered = $derived(
    (rules ?? [])
      .filter(
        (entry) =>
          !onlyFindings ||
          shownFloor === null ||
          hasFindingAtOrAbove(findingsFor(placedFindings, entry.summary.id), shownFloor),
      )
      .filter((entry) =>
        entry.summary.name.toLowerCase().includes(filter.trim().toLowerCase()),
      ),
  )

  const editEntry = $derived.by(() => {
    const target = editTarget
    if (target === null) return null
    return (rules ?? []).find((entry) => entry.summary.id === target.rule) ?? null
  })

  const editNode = $derived.by(() => {
    if (editTarget === null || !editEntry || editEntry.state.phase !== 'ready') return null
    if (editTarget.insert) {
      const node =
        editTarget.list === 'actions'
          ? ({ call: '', args: [] } as StatementNode)
          : ({ value: null } as StatementNode)
      return { ...editTarget, node }
    }
    const node = editEntry.state.content[editTarget.list][editTarget.index] as
      | StatementNode
      | undefined
    return node ? { ...editTarget, node } : null
  })
  const editDisabled = $derived(
    editNode && editEntry && editTarget && !editTarget.insert
      ? effectiveDisabled(pending, editEntry.summary.id, editTarget.list, editTarget.index, editNode.node)
      : false,
  )
  const editStaged = $derived(
    editNode && editEntry && editTarget && !editTarget.insert
      ? pending.get(keyFor(editEntry.summary.id, editTarget.list, editTarget.index))
      : undefined,
  )
  const pendingRequest = $derived(
    debug && dialog === 'pending' && inputIdentity !== null
      ? buildRequest(pending, inputIdentity, snapshot)
      : null,
  )
  const pendingCount = $derived(
    [...pending.values()].filter((change) => change.position !== 'before').length,
  )
  const pendingChanges = $derived(
    [...pending.values()].filter((change) => change.position !== 'before'),
  )

  async function loadOne(id: number, generation?: number): Promise<RuleState> {
    try {
      return ruleStateFromResponse(await call({ op: 'rules', rule: id, snapshot: generation, content: true }))
    } catch (error) {
      return ruleStateFromError(error)
    }
  }

  async function loadProject() {
    try {
      const response = await call({ op: 'project' })
      if ('result' in response) {
        const result = response.result as ProjectResult
        project = result
        inputIdentity = result.inputIdentity ?? null
        const entry = result.sourceFiles?.[0]
        const base = entry?.split(/[\\/]/).filter(Boolean).pop()
        fileName = base && base !== '.' ? base : null
        sessionLocale = result.origin?.locale ?? 'en-US'
      }
    } catch {
    } finally {
      void names.setLocale(sessionLocale)
    }
  }

  async function loadMetadata() {
    try {
      const response = await call({ op: 'targetMetadata' })
      if ('result' in response) {
        targetLocales = (response.result as TargetMetadataResult).locales ?? []
        const catalogSpelling = targetLocales.find(
          (locale) => locale.toLowerCase() === sessionLocale.toLowerCase(),
        )
        if (catalogSpelling) sessionLocale = catalogSpelling
      }
    } catch {
    }
  }

  async function load() {
    if (debug) void loadElementCounts()
    void loadFindings()
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
      warmEvents(summaries)
      await Promise.all(
        rules.map(async (entry) => {
          entry.state = await loadOne(entry.summary.id, generation)
        }),
      )
    }
  }

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

  function barEvent(summary: RuleSummary): string {
    const event = summary.event
    if (event === undefined) return ''
    if (event.startsWith('subroutine:')) return event.slice('subroutine:'.length)
    if (namesVersion === 0) return ''
    return names.peek(event, ['event'])?.name ?? event
  }

  async function loadFindings() {
    try {
      const response = await call({ op: 'findings', all: true })
      if ('result' in response) {
        const result = response.result
        findings = (
          Array.isArray(result)
            ? result
            : ((result as { findings?: Finding[] }).findings ?? [])
        ) as Finding[]
        findingsError = null
      } else {
        findings = null
        findingsError = response.error as ToolErrorInfo
      }
    } catch (error) {
      findings = null
      findingsError = {
        code: 'endpoint-unreachable',
        message: error instanceof Error ? error.message : String(error),
      }
    }
  }

  function ensureCostEstimate(): Promise<void> {
    costRequest ??= call({ op: 'costEstimate', all: true })
      .then((response) => {
        costEstimate = response
      })
      .catch((error) => {
        costEstimate = {
          error: {
            code: 'endpoint-unreachable',
            message: error instanceof Error ? error.message : String(error),
          },
        }
      })
    return costRequest
  }

  async function loadElementCounts() {
    await ensureCostEstimate()
    if (costEstimate && 'result' in costEstimate) {
      const entries =
        ((costEstimate.result as CostEstimateResult).elementCount?.rules ?? []) as {
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
  }

  function toggle(id: number) {
    if (openRules.has(id)) openRules.delete(id)
    else openRules.add(id)
  }

  function changeLocale(locale: string) {
    sessionLocale = locale
    void names.setLocale(locale)
  }

  void loadProject()
  void loadMetadata()
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

  function openSimulate() {
    dialog = 'simulate'
    if (heroes === null || heroesLocale !== sessionLocale) {
      const locale = sessionLocale
      void (async () => {
        try {
          const response = await call({
            op: 'listEntries',
            language: 'workshop',
            kind: 'enumMember',
            within: 'Hero',
            locale,
          })
          if (locale !== sessionLocale) return
          if ('result' in response) {
            const entries = (response.result as { entries?: LookupEntry[] }).entries
            heroes = entries ?? []
            heroesLocale = locale
          }
        } catch {
          heroes = heroes ?? []
        }
      })()
    }
  }

  function openDiagnostics() {
    if (findings === null) void loadFindings()
    void ensureCostEstimate()
    dialog = 'diagnostics'
  }

  function findingRow(finding: Finding): { list: StatementList; index: number } | null {
    if (finding.rule === null) return null
    const entry = (rules ?? []).find((item) => item.summary.id === finding.rule)
    if (!entry || entry.state.phase !== 'ready') return null
    const placement = placeFinding(finding, entry.state.content)
    return placement.list === 'rule' ? null : { list: placement.list, index: placement.index }
  }

  async function revealFinding(finding?: Finding, id?: number) {
    const target = finding?.rule ?? id
    if (target === undefined || target === null) return
    dialog = null
    if (!filtered.some((entry) => entry.summary.id === target)) {
      filter = ''
      onlyFindings = false
    }
    openRules.add(target)
    const row = finding !== undefined ? findingRow(finding) : null
    if (row !== null) {
      revealRow = { rule: target, list: row.list, index: row.index, nonce: ++revealNonce }
    }
    await tick()
    pageEl
      ?.querySelector<HTMLElement>(`.rule-slot[data-rule-id="${target}"]`)
      ?.scrollIntoView?.({ block: 'nearest' })
  }

  const oneditFor = (rule: number) => ({
    open: (list: StatementList, index: number, argPath?: number[]) => {
      editTarget = { rule, list, index, argPath }
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
    insert: (list: StatementList, selected: number[], length: number) => {
      const index = insertAddress(length, selected)
      editTarget = {
        rule,
        list,
        index,
        insert: { index, afterRow: selected.length === 1 ? selected[0] : null },
      }
    },
  })

  function editorSave(disabled: boolean, args?: Node[], value?: Node, call?: string) {
    if (!editNode || !editEntry || !editTarget) return
    const statement = clean(editNode.node)
    if (call !== undefined && editTarget.list === 'actions') {
      ;(statement as { call?: string }).call = call
    }
    if (args !== undefined && editTarget.list === 'actions') {
      ;(statement as { args?: Node[] }).args = args
    }
    if (value !== undefined && editTarget.list === 'conditions') {
      ;(statement as { value?: Node }).value = value
    }
    if (disabled) statement.disabled = true
    if (editTarget.insert) {
      stageInsert(
        pending,
        editEntry.summary.id,
        editEntry.summary.name,
        editTarget.list,
        editTarget.insert.index,
        [statement],
      )
      return
    }
    stageStatement(
      pending,
      editEntry.summary.id,
      editEntry.summary.name,
      editTarget.list,
      editTarget.index,
      editNode.node,
      statement,
    )
  }

  async function refreshAfterStale() {
    pending.clear()
    editTarget = null
    await loadProject()
    await load()
  }

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

<div class="page" bind:this={pageEl}>
  <header class="title-row">
    <h1>{t.title}</h1>
    {#if fileName}<span class="file-name">{fileName}</span>{/if}
  </header>

  <div class="toolbar" role="toolbar" aria-label={t.rulesToolbar}>
    <span class="rule-count">{t.rulesCount(rules?.length ?? 0)}</span>
    <SqButton title={t.diagnostics} icon={Activity} onclick={openDiagnostics}>
      {#if severityBadges.length > 0}
        <span class="severity-badges">
          {#each severityBadges as badge (badge.severity)}
            <span class="sev-badge severity-{badge.severity}">{badge.count}</span>
          {/each}
        </span>
      {/if}
    </SqButton>
    <SqButton
      title={t.onlyRulesWithFindings}
      icon={ListFilter}
      pressed={onlyFindings}
      disabled={findings === null || findings.length === 0}
      onclick={() => (onlyFindings = !onlyFindings)}
    />
    <SqButton title={t.simulation} icon={Play} onclick={openSimulate} />
    <div class="end">
      <input
        class="filter"
        type="search"
        placeholder={t.filterPlaceholder}
        bind:value={filter}
      />
      <select
        class="locale"
        aria-label={t.language}
        value={sessionLocale}
        onchange={(event) => changeLocale(event.currentTarget.value)}
      >
        {#each localeOptions as option (option)}
          <option value={option}>{localeName(option)}</option>
        {/each}
      </select>
      <span class="sep" aria-hidden="true"></span>
      <SqButton title={t.variables} icon={List} onclick={openVariables} />
      <SqButton
        title={t.pendingChanges}
        icon={FileText}
        badge={pendingCount}
        onclick={() => {
          submitRefusal = null
          dialog = 'pending'
        }}
      />
      <span class="sep" aria-hidden="true"></span>
      <SqButton title={t.addRuleTitle} label={t.addRuleLabel} icon={Plus} disabled />
      {#if debug}<span class="debug" data-debug>debug</span>{/if}
    </div>
  </div>

  {#if findingsError}
    <p class="refusal findings-refusal" role="alert">
      {t.findingsUnavailable} <code>{findingsError.code}</code> {findingsError.message}
    </p>
  {/if}

  <section class="rule-list" aria-label={t.rulesRegion}>
    {#if loadError}
      <p class="refusal">{loadError}</p>
    {:else if rules === null}
      <p class="muted">{t.loadingRules}</p>
    {:else if filtered.length === 0}
      <p class="muted">{t.noRulesMatch}</p>
    {:else}
      {#each filtered as entry (entry.summary.id)}
        {@const isOpen = openRules.has(entry.summary.id)}
        {@const ruleSlots = findingsFor(placedFindings, entry.summary.id)}
        <div class="rule-slot" data-rule-id={entry.summary.id}>
          <RuleBar
            name={entry.summary.name}
            event={barEvent(entry.summary)}
            disabled={entry.state.phase === 'ready' && entry.state.content.disabled}
            open={isOpen}
            elements={debug ? (elementCounts?.[entry.summary.id]) : undefined}
            findings={allFor(ruleSlots)}
            {t}
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
                ruleFindings={ruleSlots}
                reveal={revealRow?.rule === entry.summary.id ? revealRow : null}
                {t}
                onedit={oneditFor(entry.summary.id)}
              />
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  </section>

  <Dialogs
    which={dialog === 'pending' || dialog === 'simulate' ? null : dialog}
    {symbols}
    {findings}
    {findingsError}
    {costEstimate}
    {names}
    {t}
    onreveal={(rule, finding) => void revealFinding(finding, rule)}
    onclose={() => (dialog = null)}
    groupOpen={severityOpen}
    onGroupToggle={(severity, open) => {
      severityOpen[severity] = open
    }}
  />

  <SimulatePanel
    open={dialog === 'simulate'}
    {project}
    {heroes}
    {t}
    onclose={() => (dialog = null)}
  />

  <PendingChanges
    open={dialog === 'pending'}
    changes={pendingChanges}
    refusal={submitRefusal}
    {submitting}
    request={pendingRequest}
    {debug}
    {t}
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
    staged={editStaged?.tag === 'change' ? editStaged.statement : undefined}
    {t}
    locale={sessionLocale}
    focusPath={editTarget?.argPath ?? null}
    isInsert={editTarget?.insert !== undefined}
    onsave={editorSave}
    onclose={() => (editTarget = null)}
  />
</div>
