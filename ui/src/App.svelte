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
   * Session locale (#883, § Language): `project` → `origin.locale`, else
   * `en-US`. Every `lookup` carries it; the toolbar selector re-resolves
   * the on-screen names over `targetMetadata` → `locales` and changes
   * nothing else — `rules`, open bars, and the filter stay put. Chrome
   * text comes from `chrome.ts`'s locale table, never from the game.
   *
   * Editing layer: statement edits stage in the page's pending map, submit
   * as one `editStatements` request against the `project` identity and the
   * listing's generation, and apply through the host `/apply` route — the
   * page never composes or writes Workshop text. A stale refusal clears
   * the staged edits and re-reads the program.
   */
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
    isStale,
    keyFor,
    setDisabled,
    stageDeletes,
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
  import { collectUsed } from './lib/nodes'
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
  // The session locale (#883): `project` → `origin.locale` once it loads,
  // `en-US` before and when the project does not name one. `names`
  // carries it on every `lookup`.
  let sessionLocale = $state('en-US')
  // `targetMetadata` → `locales`: the catalog locales the selector lists.
  let targetLocales = $state<string[]>([])
  const t = $derived(chromeStrings(sessionLocale))
  // The selector's options: the catalog locales plus the session locale
  // when the catalog does not spell it, so `value` always matches an
  // option (the service spells locales `zh-cn`/`en-us`).
  const localeOptions = $derived(
    targetLocales.some((locale) => locale.toLowerCase() === sessionLocale.toLowerCase())
      ? targetLocales
      : [sessionLocale, ...targetLocales],
  )
  // Open state is per rule, keyed by the listing id: any number of rules
  // stay open at once and a rule filtered out keeps its open state (#880).
  const openRules = new SvelteSet<number>()
  // The row a finding reveal targets (§ In the dialog): the owning
  // RuleView clears a column filter hiding it, scrolls it in, and
  // flashes it once the statement rows have mounted; `nonce` re-arms
  // each reveal.
  let revealRow = $state<{
    rule: number
    list: StatementList
    index: number
    nonce: number
  } | null>(null)
  let revealNonce = 0
  let filter = $state('')
  // The "Only rules with findings" toolbar toggle (#922, § In place ·
  // Filter): off keeps every rule; on limits the list to rules with a
  // finding at or above the lowest severity the button badges.
  let onlyFindings = $state(false)
  // The dialog's severity-heading state (§ In the dialog): error and
  // warning open, info collapsed; the page holds it so the choice
  // persists for the session across dialog opens.
  const severityOpen = $state<Record<Severity, boolean>>({ ...DEFAULT_GROUP_OPEN })
  let fileName = $state<string | null>(null)
  let symbols = $state<unknown[] | null>(null)
  let findings = $state<Finding[] | null>(null)
  let findingsError = $state<ToolErrorInfo | null>(null)
  let dialog = $state<'variables' | 'diagnostics' | 'pending' | 'simulate' | null>(null)
  // The loaded project and the simulation panel's hero options (#836):
  // `project` holds `inputIdentity` and the read-only settings tree the
  // panel displays; `heroes` loads once, on the panel's first open.
  let project = $state<ProjectResult | null>(null)
  let heroes = $state<LookupEntry[] | null>(null)
  // The locale `heroes` was listed in — a session-locale switch (#883)
  // re-lists them so the panel's labels match every other on-screen name.
  let heroesLocale = $state<string | null>(null)

  // The program's identity (`project`) and generation (the rules listing's
  // `snapshot`): the freshness pair an `editStatements` request carries.
  let inputIdentity = $state<string | null>(null)
  let snapshot = $state<number | undefined>(undefined)
  // Staged statement edits keyed `rule:list:index`; the map is `$state`
  // so staging a change re-renders the marks and the toolbar count.
  let pending = $state<Map<string, PendingChange>>(new SvelteMap())
  let editTarget = $state<{
    rule: number
    list: StatementList
    index: number
    /** The clicked argument's `args` path, when the editor opens on it. */
    argPath?: number[]
  } | null>(null)
  let submitting = $state(false)
  let submitRefusal = $state<Refusal[] | null>(null)
  // Bumped when the bars' event-name warm lands so `peek` re-reads; the
  // cells stay empty until the first warm resolves rather than flashing
  // a raw event id.
  let namesVersion = $state(0)
  // `?debug` only: element counts by `elementCount.rules[].index`, which
  // is the `rules` listing id (#691); null until the call lands.
  let elementCounts = $state<Record<number, number> | null>(null)
  // The shared `costEstimate?all` response: the Diagnostics dialog's
  // cards and top rules and the `?debug` bar counts read the same call,
  // so one fetch serves both surfaces.
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

  // The Diagnostics button's badge set (§ In place · Toolbar): a count
  // per badged severity, error first, info only alone.
  const severityBadges = $derived.by(() => {
    const counts = severityCounts(findings ?? [])
    return badgeSeverities(findings ?? []).map((severity) => ({
      severity,
      count: counts[severity],
    }))
  })

  // The toggle's floor: the lowest severity the button shows (§ Filter).
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
  /** The identities the loaded program uses — the pickers' usage-first
      ordering source (docs/ui/editor.md § Pickers). */
  const usedIds = $derived.by(() => {
    const out = new Set<string>()
    for (const entry of rules ?? []) {
      if (entry.state.phase === 'ready') collectUsed(entry.state.content, out)
    }
    return out
  })

  /** The staged `change` at the open target, so the editor shows it. */
  const editStaged = $derived(
    editNode && editEntry && editTarget
      ? pending.get(keyFor(editEntry.summary.id, editTarget.list, editTarget.index))
      : undefined,
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
   * `project` carries freshness, title, and language data: `inputIdentity`
   * is the precondition `editStatements` requires, `sourceFiles` lists the
   * loaded sources in canonical file-id order so `[0]` is the entry source
   * and only its base name shows in the title row (#879), and
   * `origin.locale` names the session locale — `en-US` when absent, never
   * guessed from file text (#883). `names.setLocale` runs on every path so
   * the parked `lookup`s unblock even when `project` never answers.
   */
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
      // An unreachable endpoint leaves the identity unset and the title
      // row without a file name; Submit refuses rather than composing a
      // request without a precondition.
    } finally {
      void names.setLocale(sessionLocale)
    }
  }

  /**
   * `targetMetadata` → `locales` feeds the locale selector (#883); an
   * unreachable endpoint leaves the selector on the session locale alone.
   */
  async function loadMetadata() {
    try {
      const response = await call({ op: 'targetMetadata' })
      if ('result' in response) {
        targetLocales = (response.result as TargetMetadataResult).locales ?? []
        // Adopt the catalog's spelling for the session locale so the
        // selector has exactly one matching option; the service resolves
        // locales case-insensitively so nothing re-warms.
        const catalogSpelling = targetLocales.find(
          (locale) => locale.toLowerCase() === sessionLocale.toLowerCase(),
        )
        if (catalogSpelling) sessionLocale = catalogSpelling
      }
    } catch {
      // The selector keeps the session locale as its only option.
    }
  }

  async function load() {
    // The bulk read expresses every rule in one call; when any rule's
    // content cannot be expressed the call refuses wholesale, so the page
    // falls back to per-rule reads and keeps each refusal in place.
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

  /**
   * The `costEstimate?all` response, fetched once and shared: a refusal or
   * an unreachable endpoint is kept as the `{error}` body so the callers
   * render it rather than retrying per surface.
   */
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

  /**
   * `?debug` only: `elementCount.rules` ranks rules by count with `index`
   * naming the `rules` listing id, so the bars can show each rule's
   * element count. A refused or unreachable call leaves the cells empty.
   */
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

  /**
   * The toolbar selector (#883): switch the session locale and re-resolve
   * every on-screen name in place — `rules` is not re-read, open bars stay
   * open, and the filter keeps its text.
   */
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

  /**
   * The simulation panel (#836): its hero options are `listEntries` over
   * the `Hero` domain in the session locale, re-listed when the locale
   * changed since the last listing.
   */
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
          // A locale switch during the fetch must not install stale labels.
          if (locale !== sessionLocale) return
          if ('result' in response) {
            const entries = (response.result as { entries?: LookupEntry[] }).entries
            heroes = entries ?? []
            heroesLocale = locale
          }
        } catch {
          // A failed listing does not satisfy the locale: `heroesLocale`
          // stays stale so the next open retries instead of locking in
          // the previous locale's labels.
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

  /**
   * A finding's row in its rule's body (§ In the dialog): the placed
   * statement or null when the finding lands on the bar alone. The
   * placement is recomputed against the open rule's loaded content — the
   * reveal may run before `placedFindings` saw it ready.
   */
  function findingRow(finding: Finding): { list: StatementList; index: number } | null {
    if (finding.rule === null) return null
    const entry = (rules ?? []).find((item) => item.summary.id === finding.rule)
    if (!entry || entry.state.phase !== 'ready') return null
    const placement = placeFinding(finding, entry.state.content)
    return placement.list === 'rule' ? null : { list: placement.list, index: placement.index }
  }

  /**
   * A dialog's reveal request (design.md § Dialogs): close the dialog,
   * open the rule's bar, and scroll it into view. A filter that would
   * hide the target is dropped — the reveal has to land — as is an
   * "only findings" toggle the finding's severity does not reach.
   * `elementCount` indexes and `finding.rule` both name `rules` listing
   * ids. A located finding hands its row to the open RuleView, which
   * drops a column filter hiding it, scrolls to it, and flashes it once
   * (§ In the dialog).
   */
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
    // The slot scrolls at once; the row refines the position once the
    // rule's body has mounted its statements (the name warm may still be
    // out), and a column filter hiding it is dropped — the reveal has to
    // land, like the rule-name filter above.
    if (row !== null) {
      revealRow = { rule: target, list: row.list, index: row.index, nonce: ++revealNonce }
    }
    await tick()
    pageEl
      ?.querySelector<HTMLElement>(`.rule-slot[data-rule-id="${target}"]`)
      ?.scrollIntoView?.({ block: 'nearest' })
  }

  /**
   * The `rules` content node the open editor acts on. Each open rule's
   * view gets callbacks bound to its listing id, so with several rules
   * open the target is the rule that issued the call.
   */
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
  })

  function editorSave(disabled: boolean, callId?: string, args?: Node[], value?: Node) {
    if (!editNode || !editEntry || !editTarget) return
    const statement = clean(editNode.node)
    if (callId !== undefined) {
      if (editTarget.list === 'actions') {
        ;(statement as { call?: string }).call = callId
      }
    }
    if (args !== undefined && editTarget.list === 'actions') {
      ;(statement as { args?: Node[] }).args = args
    }
    if (value !== undefined && editTarget.list === 'conditions') {
      ;(statement as { value?: Node }).value = value
    }
    if (disabled) statement.disabled = true
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
      <!-- The locale selector (#883): one option per catalog locale, each
           in its own name; the session locale is selected. -->
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
        badge={pending.size}
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
    changes={[...pending.values()]}
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
    used={usedIds}
    focusPath={editTarget?.argPath ?? null}
    onsave={editorSave}
    onclose={() => (editTarget = null)}
  />
</div>
