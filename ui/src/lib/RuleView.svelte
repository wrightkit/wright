<script lang="ts">
  /**
   * One rule's body, indented under its bar (docs/ui/design.md § Main
   * screen item 4): the event rows — Event, then Team, Player, or
   * Subroutine as the event carries them — then the Conditions/Actions
   * columns, whose statement rows indent 18 px per control-flow level and
   * never show `End`. A rule whose content `rules` cannot express renders
   * the service refusal in the error style while other rules still open.
   * `?debug` adds line numbers at the row start and the node JSON; they
   * are absent from the DOM otherwise.
   *
   * Editing layer (ADR-0032): statement rows carry hover-revealed
   * checkboxes, shift-click ranges, a select-all per column, and a
   * selection bar with Disable/Enable/Delete/Clear. Clicking a row opens
   * its editor. Staged edits mark their rows (change/delete) until the
   * pending list submits through `editStatements`.
   */
  import { ChevronDown } from '@lucide/svelte'
  import { SvelteSet } from 'svelte/reactivity'
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
  let anchor = $state<{ list: StatementList; index: number } | null>(null)
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
      const [lo, hi] =
        anchor.index <= index ? [anchor.index, index] : [index, anchor.index]
      for (let i = lo; i <= hi; i++) set.add(i)
      return
    }
    // `click` is delegated, so `currentTarget` is the delegation root here;
    // the target is the checkbox itself (an input has no children).
    if ((event.target as HTMLInputElement).checked) set.add(index)
    else set.delete(index)
    anchor = { list, index }
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
    } else if (event.key === 'Delete' && set.size > 0) {
      event.preventDefault()
      onedit.batch(list, [...set], 'delete')
    } else if (event.key === 'Escape' && set.size > 0) {
      set.clear()
      anchor = null
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
        {#if condSel.size > 0}
          <div class="sel-bar" data-list="conditions">
            <span class="sel-count">{t.selectedCount(condSel.size)}</span>
            <button type="button" onclick={() => onedit.batch('conditions', [...condSel], 'disable')}
              >{t.disable}</button
            >
            <button type="button" onclick={() => onedit.batch('conditions', [...condSel], 'enable')}
              >{t.enable}</button
            >
            <button type="button" onclick={() => onedit.batch('conditions', [...condSel], 'delete')}
              >{t.delete}</button
            >
            <button type="button" onclick={() => selection.conditions.clear()}>{t.clear}</button>
          </div>
        {/if}
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
        {#if actSel.size > 0}
          <div class="sel-bar" data-list="actions">
            <span class="sel-count">{t.selectedCount(actSel.size)}</span>
            <button type="button" onclick={() => onedit.batch('actions', [...actSel], 'disable')}
              >{t.disable}</button
            >
            <button type="button" onclick={() => onedit.batch('actions', [...actSel], 'enable')}
              >{t.enable}</button
            >
            <button type="button" onclick={() => onedit.batch('actions', [...actSel], 'delete')}
              >{t.delete}</button
            >
            <button type="button" onclick={() => selection.actions.clear()}>{t.clear}</button>
          </div>
        {/if}
      </div>
    </div>
  {/if}
</section>
