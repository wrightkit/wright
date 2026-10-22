<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity'
  import SqButton from './SqButton.svelte'
  import Expr from './Expr.svelte'
  import type { ChromeStrings } from './chrome'
  import type { Names } from './names'
  import {
    keyFor,
    type BatchAction,
    type PendingChange,
    type StatementList,
  } from './edit'
  import {
    collectIdentities,
    isEventPlayerMember,
    statementDepths,
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
    t,
    onedit,
  }: {
    summary: RuleSummary
    state: RuleState
    names: Names
    debug: boolean
    pending: ReadonlyMap<string, PendingChange>
    /** The session locale's chrome strings. */
    t: ChromeStrings
    onedit: {
      open: (list: StatementList, index: number) => void
      batch: (list: StatementList, indices: number[], action: BatchAction) => void
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
  // live in the page's pending map).
  let selection = $state<{ conditions: Set<number>; actions: Set<number> }>({
    conditions: new SvelteSet(),
    actions: new SvelteSet(),
  })
  let anchor = $state<{ list: StatementList; index: number; extent: number } | null>(null)
  let hoverCol = $state<StatementList | null>(null)
  $effect(() => {
    void summary.id
    void ruleState
    selection = { conditions: new SvelteSet(), actions: new SvelteSet() }
    anchor = null
  })

  const sel = (list: StatementList) => selection[list]

  function tagFor(list: StatementList, index: number) {
    return pending.get(keyFor(summary.id, list, index))?.tag
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
    for (const i of visibleIndices(list)) {
      if (i >= lo && i <= hi) sel(list).add(i)
    }
    anchor.extent = index
  }

  function clearSelection(list: StatementList) {
    sel(list).clear()
    if (anchor?.list === list) anchor = null
  }

  function selectAll(list: StatementList, visible: number[], checked: boolean) {
    const set = sel(list)
    set.clear()
    if (checked) for (const index of visible) set.add(index)
    anchor = null
  }

  /** The flat-list indices that render as rows (`end` markers stay hidden). */
  function visibleIndices(list: StatementList): number[] {
    if (ruleState.phase !== 'ready') return []
    if (list === 'conditions') return ruleState.content.conditions.map((_, i) => i)
    const depths = statementDepths(ruleState.content.actions)
    return depths.flatMap((depth, i) => (depth === null ? [] : [i]))
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.defaultPrevented) return
    const el = event.target as HTMLElement | null
    if (
      el?.closest('input, textarea, select, dialog, [contenteditable="true"]') &&
      !el?.closest('input.sel, input.sel-all')
    )
      return
    const list = hoverCol
    if (!list) return
    const set = sel(list)
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'a') {
      event.preventDefault()
      selectAll(list, visibleIndices(list), true)
    } else if (event.shiftKey && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
      if (anchor?.list !== list) return
      const visible = visibleIndices(list)
      const next = visible.indexOf(anchor.extent) + (event.key === 'ArrowDown' ? 1 : -1)
      event.preventDefault()
      if (next >= 0 && next < visible.length) extendRange(list, visible[next]!)
    } else if (event.key === 'Delete' && set.size > 0) {
      event.preventDefault()
      onedit.batch(list, [...set], 'delete')
    } else if (event.key === 'Escape' && set.size > 0) {
      event.preventDefault()
      clearSelection(list)
    }
  }

  /** The singular noun an aria label names for a row of `list`. */
  const noun = (list: StatementList) =>
    list === 'conditions' ? t.conditionNoun : t.actionNoun

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
     debug line number, the statement text, the pending tag, and the debug
     node JSON. `depth` is the row's control-flow nesting level. -->
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
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions, a11y_no_noninteractive_element_interactions -->
  <li
    class="statement"
    class:disabled
    class:selected={set.has(index)}
    class:pending-change={tag === 'change'}
    class:pending-delete={tag === 'delete'}
    style="--depth: {depth}"
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
    <span class="statement-text"><Expr {node} {kinds} {names} /></span>
    {#if tag}<span class="pending-tag {tag}">{tag}</span>{/if}
    {#if debug}
      <details class="debug" data-debug>
        <summary>node</summary>
        <pre>{JSON.stringify($state.snapshot(node), null, 2)}</pre>
      </details>
    {/if}
  </li>
{/snippet}

{#snippet columnActions(list: StatementList)}
  {@const set = sel(list)}
  <span class="column-actions">
    {#if set.size > 0}
      <span class="sel-count">{t.selectedCount(set.size)}</span>
      <SqButton title={t.disable} onclick={() => onedit.batch(list, [...set], 'disable')}>
        <path d="M6 6h12v12H6z" />
      </SqButton>
      <SqButton title={t.enable} onclick={() => onedit.batch(list, [...set], 'enable')}>
        <path d="m8 5 11 7-11 7z" />
      </SqButton>
      <SqButton title={t.delete} onclick={() => onedit.batch(list, [...set], 'delete')}>
        <path d="M3 6h18M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7M14 10v7" />
      </SqButton>
      <SqButton title={t.clear} onclick={() => clearSelection(list)}>
        <path d="m6 6 12 12M6 18 18 6" />
      </SqButton>
    {:else}
      <SqButton title={t.addStatementTitle(noun(list))} label={t.addStatement(noun(list))} disabled>
        <path d="M12 5v14M5 12h14" />
      </SqButton>
    {/if}
  </span>
{/snippet}

<section class="rule-view">
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
            >{row.value}<svg
              viewBox="0 0 24 24"
              width="14"
              height="14"
              fill="none"
              stroke="currentColor"
              stroke-width="2.5"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg
            ></span
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
    {@const condSel = selection.conditions}
    {@const condVisible = content.conditions.map((_, i) => i)}
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
            checked={condSel.size === condVisible.length && condVisible.length > 0}
            indeterminate={condSel.size > 0 && condSel.size < condVisible.length}
            onchange={(event) =>
              selectAll('conditions', condVisible, event.currentTarget.checked)}
          />
          {t.conditions} <span class="col-count">({content.conditions.length})</span>
          {@render columnActions('conditions')}
        </h3>
        <ol class="statements" class:has-selection={condSel.size > 0}>
          {#each content.conditions as condition, i (i)}
            {@render stmtRow(
              'conditions',
              i,
              condition.value,
              VALUE_KINDS,
              0,
              condition.disabled,
              condition.span?.start.line ?? '?',
            )}
          {/each}
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
            checked={actSel.size === actVisible.length && actVisible.length > 0}
            indeterminate={actSel.size > 0 && actSel.size < actVisible.length}
            onchange={(event) => selectAll('actions', actVisible, event.currentTarget.checked)}
          />
          {t.actions} <span class="col-count">({actVisible.length})</span>
          {@render columnActions('actions')}
        </h3>
        <ol class="statements" class:has-selection={actSel.size > 0}>
          {#each content.actions as action, i (i)}
            {@const depth = actDepths[i]}
            {#if depth !== null}
              {@render stmtRow(
                'actions',
                i,
                action,
                STATEMENT_KINDS,
                depth,
                action.disabled,
                action.span?.start.line ?? '?',
              )}
            {/if}
          {/each}
          {#if actVisible.length === 0}
            <li class="none">{t.none}</li>
          {/if}
        </ol>
      </div>
    </div>
  {/if}
</section>
