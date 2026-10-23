<script lang="ts">
  import { ChevronDown, Plus, Square, Play, Trash2, X } from '@lucide/svelte'
  import { tick } from 'svelte'
  import { SvelteSet } from 'svelte/reactivity'
  import SqButton from './SqButton.svelte'
  import Expr from './Expr.svelte'
  import FindingMark from './FindingMark.svelte'
  import RowMenu from './components/RowMenu.svelte'
  import type { ChromeStrings } from './chrome'
  import type { Names } from './names'
  import { mostSevere, SEVERITY_TOKEN, type RuleFindings } from './findings'
  import {
    insertMarkKey,
    keyFor,
    type BatchAction,
    type PendingChange,
    type StatementList,
  } from './edit'
  import {
    blockRange,
    collectIdentities,
    isEventPlayerMember,
    sameCallIndices,
    statementDepths,
    statementMatches,
    STATEMENT_KINDS,
    VALUE_KINDS,
  } from './nodes'
  import type { Event, Node } from './types/generated/rule-content-v1'
  import type { RuleState, RuleSummary } from './view'

  let {
    summary,
    state: ruleState,
    names,
    debug,
    pending,
    ruleFindings,
    reveal,
    t,
    onedit,
  }: {
    summary: RuleSummary
    state: RuleState
    names: Names
    debug: boolean
    pending: ReadonlyMap<string, PendingChange>
    /** This rule's placed findings (`docs/ui/diagnostics.md § In place`). */
    ruleFindings: RuleFindings
    /**
     * The row a finding reveal targets (§ In the dialog): a column
     * filter hiding it is dropped, then the row scrolls in and flashes
     * once; `nonce` re-arms each reveal.
     */
    reveal: { rule: number; list: StatementList; index: number; nonce: number } | null
    /** The session locale's chrome strings. */
    t: ChromeStrings
    onedit: {
      open: (list: StatementList, index: number, argPath?: number[]) => void
      batch: (list: StatementList, indices: number[], action: BatchAction) => void
      /** The heading `+`: open the editor for a new statement in `list`. */
      insert: (list: StatementList, selected: number[], length: number) => void
    }
  } = $props()

  // The rule's identities resolve before its statements render, so a name
  // never flashes its raw id: `peek` answers synchronously once warmed.
  let namesReady = $state(false)
  let generation = 0
  $effect(() => {
    if (ruleState.phase !== 'ready') {
      namesReady = false
      return
    }
    const identities = collectIdentities(ruleState.content).map((item) => item.identity)
    namesReady = false
    const gen = ++generation
    void names.warm(identities).then(() => {
      if (gen === generation) namesReady = true
    })
  })

  // Selection is per column of the rule currently rendered; switching
  // rules or re-reading content drops it (staged marks survive — they
  // live in the page's pending map). The column filters reset with it.
  let selection = $state<{ conditions: Set<number>; actions: Set<number> }>({
    conditions: new SvelteSet(),
    actions: new SvelteSet(),
  })
  let anchor = $state<{ list: StatementList; index: number; extent: number } | null>(null)
  let hoverCol = $state<StatementList | null>(null)
  let root: HTMLElement | undefined = $state()
  let filters = $state<Record<StatementList, string>>({ conditions: '', actions: '' })
  $effect(() => {
    void summary.id
    void ruleState
    selection = { conditions: new SvelteSet(), actions: new SvelteSet() }
    anchor = null
    filters = { conditions: '', actions: '' }
  })

  /** A column shows its filter field once it renders past this many rows. */
  const FILTER_THRESHOLD = 30

  const sel = (list: StatementList) => selection[list]

  function tagFor(list: StatementList, index: number) {
    // An `insert`'s own key can't collide with the row key, but guard
    // anyway: the existing row's mark is `change`/`delete` only — the
    // `insert` badge belongs to the `:new` placeholder row.
    const tag = pending.get(keyFor(summary.id, list, index))?.tag
    return tag === 'insert' ? undefined : tag
  }

  /** A checkbox click: shift+click extends from the anchor to a range. */
  function selectRow(list: StatementList, index: number, event: MouseEvent) {
    const set = sel(list)
    if (event.shiftKey && anchor?.list === list) {
      extendRange(list, index)
      return
    }
    // `click` is delegated, so `currentTarget` is the delegation root here;
    // the target is the checkbox itself (an input has no children).
    if ((event.target as HTMLInputElement).checked) set.add(index)
    else set.delete(index)
    anchor = { list, index, extent: index }
  }

  function extendRange(list: StatementList, index: number) {
    if (anchor?.list !== list) return
    const lo = Math.min(anchor.index, index)
    const hi = Math.max(anchor.index, index)
    for (const i of shownIndices(list, visibleIndices(list))) {
      if (i >= lo && i <= hi) sel(list).add(i)
    }
    anchor.extent = index
  }

  function clearSelection(list: StatementList) {
    sel(list).clear()
    if (anchor?.list === list) anchor = null
  }

  /**
   * The column's select-all works on the rows the filter leaves visible:
   * checking adds them, unchecking removes them. Rows the filter hid are
   * outside the gesture — they keep their selection either way.
   */
  function selectAll(list: StatementList, shown: number[], checked: boolean) {
    const set = sel(list)
    if (checked) for (const index of shown) set.add(index)
    else for (const index of shown) set.delete(index)
    anchor = null
  }

  /**
   * A finding reveal (docs/ui/diagnostics.md § In the dialog): once the
   * target row is rendered — the rule may still be warming its names
   * when the click lands — a filter hiding it is dropped and it scrolls
   * in and flashes once. The `nonce` re-arms the effect per reveal.
   */
  $effect(() => {
    const target = reveal
    if (target === null) return
    void target.nonce
    void namesReady
    void tick().then(() => {
      const el = root?.querySelector<HTMLElement>(
        `.col[data-list="${target.list}"] .statement[data-index="${target.index}"]`,
      )
      if (!el) {
        // Not rendered: either the names are still warming or the column
        // filter hid it — clear the filter and let the flush retry.
        if (filters[target.list] !== '') filters[target.list] = ''
        return
      }
      el.scrollIntoView?.({ block: 'nearest' })
      el.classList.remove('flash')
      void el.offsetWidth
      el.classList.add('flash')
      el.addEventListener('animationend', () => el.classList.remove('flash'), { once: true })
    })
  })

  /** The flat-list indices that render as rows (`end` markers stay hidden). */
  function visibleIndices(list: StatementList): number[] {
    if (ruleState.phase !== 'ready') return []
    if (list === 'conditions') return ruleState.content.conditions.map((_, i) => i)
    const depths = statementDepths(ruleState.content.actions)
    return depths.flatMap((depth, i) => (depth === null ? [] : [i]))
  }

  /**
   * The rendered rows a column's filter leaves: the session-locale text
   * contains the query, or the call id takes it as a prefix. An empty
   * query filters nothing out.
   */
  function shownIndices(list: StatementList, visible: number[]): number[] {
    const query = filters[list]
    if (query === '' || ruleState.phase !== 'ready') return visible
    const kinds = list === 'conditions' ? VALUE_KINDS : STATEMENT_KINDS
    const { content } = ruleState
    return visible.filter((index) => {
      const node =
        list === 'conditions' ? content.conditions[index].value : content.actions[index]
      return statementMatches(node, names, kinds, query)
    })
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.defaultPrevented) return
    const el = event.target as HTMLElement | null
    if (
      el?.closest('input, textarea, select, dialog, [role="menu"], [contenteditable="true"]') &&
      !el?.closest('input.sel, input.sel-all, input.col-filter')
    )
      return
    // Every open RuleView registers a window listener, so a column in a
    // sibling rule's DOM leaves the gesture to that rule. The pointer
    // column applies only when the event has no target column.
    const col = el?.closest('.col')
    if (col && !root?.contains(col)) return
    const list = (col?.getAttribute('data-list') as StatementList | null) ?? hoverCol
    if (!list) return
    const set = sel(list)
    const inFilter = el?.closest('input.col-filter') != null
    if (!inFilter && (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'a') {
      event.preventDefault()
      selectAll(list, shownIndices(list, visibleIndices(list)), true)
    } else if (!inFilter && event.shiftKey && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
      if (anchor?.list !== list) return
      const shown = shownIndices(list, visibleIndices(list))
      const current = shown.indexOf(anchor.extent)
      if (current < 0) return
      const next = current + (event.key === 'ArrowDown' ? 1 : -1)
      event.preventDefault()
      if (next >= 0 && next < shown.length) extendRange(list, shown[next]!)
    } else if (!inFilter && event.key === 'Delete' && set.size > 0) {
      event.preventDefault()
      onedit.batch(list, [...set], 'delete')
    } else if (event.key === 'Escape') {
      // Esc peels one layer at a time (docs/ui/selection.md § Keyboard).
      if (filters[list] !== '') {
        filters[list] = ''
        event.preventDefault()
      } else if (set.size > 0) {
        clearSelection(list)
        event.preventDefault()
      }
    }
  }

  /** The singular noun an aria label names for a row of `list`. */
  const noun = (list: StatementList) =>
    list === 'conditions' ? t.conditionNoun : t.actionNoun

  /**
   * The row's flat-stream nodes: an action list renders its actions; a
   * condition list renders each condition's `value`.
   */
  function rowNodes(list: StatementList): Node[] {
    if (ruleState.phase !== 'ready') return []
    return list === 'conditions'
      ? ruleState.content.conditions.map((condition) => condition.value)
      : ruleState.content.actions
  }

  /**
   * The row's context menu entries (docs/ui/selection.md § Structural
   * selection): "Select same action" checks every row in the column
   * sharing the row's call id; "Select block" checks the opener, its
   * body, and its `End` — enabled only on control-flow openers.
   */
  function menuItems(list: StatementList, index: number) {
    const nodes = rowNodes(list)
    const block =
      list === 'actions' && ruleState.phase === 'ready'
        ? blockRange(ruleState.content.actions, index)
        : null
    return [
      {
        label: t.selectSameAction,
        disabled: sameCallIndices(nodes, index).length === 0,
        onselect: () => {
          for (const i of sameCallIndices(nodes, index)) sel(list).add(i)
          anchor = null
        },
      },
      {
        label: t.selectBlock,
        disabled: block === null,
        onselect: () => {
          if (block === null) return
          const set = sel(list)
          for (let i = block[0]; i <= block[1]; i++) set.add(i)
          anchor = null
        },
      },
    ]
  }

  /**
   * The event rows, in the in-game order: Event always, then Team, Player,
   * and Subroutine only when the event carries them. Labels are chrome in
   * the session locale; values are `lookup` names in the session locale
   * (warmed with the rest of the rule), with the raw identity as the
   * failed-lookup fallback.
   */
  function eventRows(event: Event): { key: string; label: string; value: string }[] {
    const rows = [
      { key: 'event', label: t.event, value: names.peek(event.id, ['event'])?.name ?? event.id },
    ]
    if (event.team) {
      rows.push({
        key: 'team',
        label: t.team,
        value:
          names.peek(`EventTeam.${event.team}`, ['enumMember'])?.name ??
          `EventTeam.${event.team}`,
      })
    }
    if (event.player) {
      const key = isEventPlayerMember(event.player)
        ? `EventPlayer.${event.player}`
        : `Hero.${event.player}`
      rows.push({ key: 'player', label: t.player, value: names.peek(key, ['enumMember'])?.name ?? key })
    }
    if (event.subroutine) {
      rows.push({ key: 'subroutine', label: t.subroutine, value: event.subroutine })
    }
    return rows
  }
</script>

<svelte:window onkeydown={onkeydown} />

<!-- The statement-row shell the two columns share: the checkbox, the
     debug line number, the statement text, the pending tag, the findings
     mark, and the debug node JSON. -->
{#snippet stmtRow(
  list: StatementList,
  index: number,
  node: Node,
  kinds: string[],
  depth: number,
  disabled: boolean | undefined,
  line: number | string,
)}
  {@const set = sel(list)}
  {@const tag = tagFor(list, index)}
  {@const rowFindings = ruleFindings[list].get(index) ?? []}
  {@const rowSeverity = rowFindings.length > 0 ? mostSevere(rowFindings) : null}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions, a11y_no_noninteractive_element_interactions -->
  <RowMenu ariaLabel={t.statementMenu(noun(list), index + 1)} items={menuItems(list, index)}>
    {#snippet children({ props })}
      <li
        {...props}
        class="statement"
        data-index={index}
        class:disabled
        class:selected={set.has(index)}
        class:pending-change={tag === 'change'}
        class:pending-delete={tag === 'delete'}
        class:has-findings={rowSeverity !== null}
        style="--depth: {depth};{rowSeverity !== null ? ` --finding: var(--${SEVERITY_TOKEN[rowSeverity]})` : ''}"
        onclick={() => onedit.open(list, index)}
      >
        <input
          type="checkbox"
          class="sel"
          aria-label={t.selectRow(noun(list), index + 1)}
          checked={set.has(index)}
          onclick={(event) => {
            event.stopPropagation()
            selectRow(list, index, event)
          }}
        />
        {#if debug}<span class="line-no" data-debug>{line}</span>{/if}
        <span class="statement-text"><Expr
            {node}
            {kinds}
            {names}
            onarg={(argPath) => onedit.open(list, index, argPath)}
          /></span>
        {#if tag}<span class="pending-tag {tag}">{tag}</span>{/if}
        {#if rowFindings.length > 0}
          <FindingMark findings={rowFindings} {t} />
        {/if}
        {#if debug}
          <details class="debug" data-debug>
            <summary>node</summary>
            <pre>{JSON.stringify($state.snapshot(node), null, 2)}</pre>
          </details>
        {/if}
      </li>
    {/snippet}
  </RowMenu>
{/snippet}

<!-- The placeholder a staged `insert` renders: the gold pending mark
     (docs/ui/selection.md § Pending marks) and the tag, sitting where
     the new row lands — after the selected row, or at the column end.
     It is not a statement row: no checkbox, no click target. -->
{#snippet pendingInsertRow(depth: number)}
  <li
    class="statement pending-insert pending-change"
    data-insert-mark
    style="--depth: {depth};"
  >
    <span class="pending-tag insert">insert</span>
  </li>
{/snippet}

{#snippet columnActions(list: StatementList, hidden: number)}
  {@const set = sel(list)}
  <span class="column-actions">
    {#if set.size > 0}
      <span class="sel-count">{hidden > 0
        ? t.selectedHiddenCount(set.size, hidden)
        : t.selectedCount(set.size)}</span>
      <SqButton icon={Square} title={t.disable} onclick={() => onedit.batch(list, [...set], 'disable')} />
      <SqButton icon={Play} title={t.enable} onclick={() => onedit.batch(list, [...set], 'enable')} />
      <SqButton icon={Trash2} title={t.delete} onclick={() => onedit.batch(list, [...set], 'delete')} />
      <SqButton icon={X} title={t.clear} onclick={() => clearSelection(list)} />
    {/if}
    <!-- docs/ui/selection.md: with no selection the right side is the
         `+` alone; a selection brings the batch row in beside it — the
         `+` stays so one selected row can still take an insert after it
         (docs/ui/editor.md § Form · Insert). -->
    <SqButton
      icon={Plus}
      title={t.addStatementTitle(noun(list))}
      label={t.addStatement(noun(list))}
      onclick={() => onedit.insert(list, [...set], rowNodes(list).length)}
    />
  </span>
{/snippet}

<section class="rule-view" bind:this={root}>
  {#if ruleState.phase === 'loading' || (ruleState.phase === 'ready' && !namesReady)}
    <p class="muted">{t.loadingRule}</p>
  {:else if ruleState.phase === 'refused'}
    <div class="refusal">
      <strong>{t.contentRefused}</strong>
      <code>{ruleState.refusal.code}</code>
      <p>{ruleState.refusal.message}</p>
    </div>
  {:else}
    {@const content = ruleState.content}
    <div class="event-rows">
      {#each eventRows(content.event) as row (row.key)}
        <div class="event-row">
          <span class="event-label">{row.label}</span>
          <span class="event-value"
            >{row.value}<ChevronDown size={14} strokeWidth={2} aria-hidden="true" /></span
          >
        </div>
      {/each}
    </div>
    {#if debug}
      <div class="debug" data-debug>rule #{summary.id} · event id: {content.event.id}</div>
    {/if}

    {@const actSel = selection.actions}
    {@const actDepths = statementDepths(content.actions)}
    {@const actVisible = actDepths.flatMap((depth, i) => (depth === null ? [] : [i]))}
    {@const actShown = shownIndices('actions', actVisible)}
    {@const actShownSet = new Set(actShown)}
    {@const actVisibleSet = new Set(actVisible)}
    {@const actHiddenSel = [...actSel].filter(
      (i) => actVisibleSet.has(i) && !actShownSet.has(i),
    ).length}
    {@const condSel = selection.conditions}
    {@const condVisible = content.conditions.map((_, i) => i)}
    {@const condShown = shownIndices('conditions', condVisible)}
    {@const condShownSet = new Set(condShown)}
    {@const condVisibleSet = new Set(condVisible)}
    {@const condHiddenSel = [...condSel].filter(
      (i) => condVisibleSet.has(i) && !condShownSet.has(i),
    ).length}
    <div class="rule-cols">
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="col"
        data-list="conditions"
        onmouseenter={() => (hoverCol = 'conditions')}
        onmouseleave={() => {
          if (hoverCol === 'conditions') hoverCol = null
        }}
      >
        <h3>
          <input
            type="checkbox"
            class="sel-all"
            aria-label={t.selectAll(t.conditionNoun)}
            checked={condShown.length > 0 && condShown.every((i) => condSel.has(i))}
            indeterminate={condShown.some((i) => condSel.has(i)) &&
              !condShown.every((i) => condSel.has(i))}
            onchange={(event) =>
              selectAll('conditions', condShown, event.currentTarget.checked)}
          />
          {t.conditions} <span class="col-count">({content.conditions.length})</span>
          {@render columnActions('conditions', condHiddenSel)}
        </h3>
        {#if condVisible.length > FILTER_THRESHOLD}
          <input
            class="col-filter"
            type="search"
            placeholder={t.filterColumn(t.conditionNoun)}
            aria-label={t.filterColumn(t.conditionNoun)}
            bind:value={filters.conditions}
          />
        {/if}
        <ol class="statements" class:has-selection={condSel.size > 0}>
          {#each condShown as i (i)}
            {@const condition = content.conditions[i]}
            {@render stmtRow(
              'conditions',
              i,
              condition.value,
              VALUE_KINDS,
              0,
              condition.disabled,
              condition.span?.start.line ?? '?',
            )}
            {#if pending.has(insertMarkKey(summary.id, 'conditions', i + 1))}
              {@render pendingInsertRow(0)}
            {/if}
          {/each}
          {#if
            content.conditions.length === 0 &&
            pending.has(insertMarkKey(summary.id, 'conditions', 0))}
            {@render pendingInsertRow(0)}
          {/if}
          {#if content.conditions.length === 0}
            <li class="none">{t.none}</li>
          {/if}
        </ol>
      </div>

      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="col"
        data-list="actions"
        onmouseenter={() => (hoverCol = 'actions')}
        onmouseleave={() => {
          if (hoverCol === 'actions') hoverCol = null
        }}
      >
        <h3>
          <input
            type="checkbox"
            class="sel-all"
            aria-label={t.selectAll(t.actionNoun)}
            checked={actShown.length > 0 && actShown.every((i) => actSel.has(i))}
            indeterminate={actShown.some((i) => actSel.has(i)) &&
              !actShown.every((i) => actSel.has(i))}
            onchange={(event) => selectAll('actions', actShown, event.currentTarget.checked)}
          />
          {t.actions} <span class="col-count">({actVisible.length})</span>
          {@render columnActions('actions', actHiddenSel)}
        </h3>
        {#if actVisible.length > FILTER_THRESHOLD}
          <input
            class="col-filter"
            type="search"
            placeholder={t.filterColumn(t.actionNoun)}
            aria-label={t.filterColumn(t.actionNoun)}
            bind:value={filters.actions}
          />
        {/if}
        <ol class="statements" class:has-selection={actSel.size > 0}>
          {#each actShown as i (i)}
            {@const action = content.actions[i]}
            {@render stmtRow(
              'actions',
              i,
              action,
              STATEMENT_KINDS,
              actDepths[i]!,
              action.disabled,
              action.span?.start.line ?? '?',
            )}
            {#if pending.has(insertMarkKey(summary.id, 'actions', i + 1))}
              {@render pendingInsertRow(actDepths[i]!)}
            {/if}
          {/each}
          {#if actVisible.length === 0 && pending.has(insertMarkKey(summary.id, 'actions', 0))}
            {@render pendingInsertRow(0)}
          {/if}
          {#if actVisible.length === 0}
            <li class="none">{t.none}</li>
          {/if}
        </ol>
      </div>
    </div>
  {/if}
</section>
