<script lang="ts">
  /**
   * One rule's structured view: event header, conditions, and the indented
   * action stream. A rule whose content `rules` cannot express renders the
   * service refusal in place while other rules still render. `?debug` adds
   * line numbers, call ids, and the node JSON; they are absent otherwise.
   *
   * Editing layer (ADR-0032): statement rows carry hover-revealed
   * checkboxes, shift-click ranges, a select-all per column, and a
   * selection bar with Disable/Enable/Delete/Clear. Clicking a row opens
   * its editor. Staged edits mark their rows (change/delete) until the
   * pending list submits through `editStatements`.
   */
  import { SvelteSet } from 'svelte/reactivity'
  import Expr from './Expr.svelte'
  import type { Names } from './names'
  import {
    keyFor,
    type BatchAction,
    type PendingChange,
    type StatementList,
  } from './edit'
  import { collectIdentities, isEventPlayerMember, statementDepths, STATEMENT_KINDS } from './nodes'
  import type { Event } from './types/generated/rule-content-v1'
  import type { RuleState, RuleSummary } from './view'

  let {
    summary,
    state: ruleState,
    names,
    debug,
    pending,
    onedit,
  }: {
    summary: RuleSummary
    state: RuleState
    names: Names
    debug: boolean
    pending: ReadonlyMap<string, PendingChange>
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

  function eventName(event: Event): string {
    if (event.id === 'subroutine') return event.subroutine ?? 'subroutine'
    return names.peek(event.id, ['event'])?.name ?? event.id
  }

  function filterNames(event: Event): string[] {
    const out: string[] = []
    if (event.team) {
      out.push(
        names.peek(`EventTeam.${event.team}`, ['enumMember'])?.name ?? `EventTeam.${event.team}`,
      )
    }
    if (event.player) {
      const key = isEventPlayerMember(event.player)
        ? `EventPlayer.${event.player}`
        : `Hero.${event.player}`
      out.push(names.peek(key, ['enumMember'])?.name ?? key)
    }
    return out
  }
</script>

<svelte:window onkeydown={onkeydown} />

<section class="rule-view">
  <h2 class="rule-title">
    {summary.name}
    {#if debug}<span class="debug" data-debug>#{summary.id}</span>{/if}
  </h2>
  {#if ruleState.phase === 'loading' || (ruleState.phase === 'ready' && !namesReady)}
    <p class="muted">Loading rule…</p>
  {:else if ruleState.phase === 'refused'}
    <div class="refusal">
      <strong>The service could not express this rule's content.</strong>
      <code>{ruleState.refusal.code}</code>
      <p>{ruleState.refusal.message}</p>
    </div>
  {:else}
    {@const content = ruleState.content}
    <header class="event">
      <span class="tok value">{eventName(content.event)}</span>
      {#each filterNames(content.event) as filter (filter)}
        <span class="event-filter">{filter}</span>
      {/each}
      {#if content.disabled}
        <span class="disabled-badge">disabled</span>
      {/if}
      {#if debug}
        <span class="debug" data-debug>event id: {content.event.id}</span>
      {/if}
    </header>

    {#if content.conditions.length > 0}
      {@const condSel = selection.conditions}
      {@const condVisible = content.conditions.map((_, i) => i)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="col"
        onmouseenter={() => (hoverCol = 'conditions')}
        onmouseleave={() => {
          if (hoverCol === 'conditions') hoverCol = null
        }}
      >
        <h3>
          <input
            type="checkbox"
            class="sel-all"
            aria-label="select all conditions"
            checked={condSel.size === condVisible.length && condVisible.length > 0}
            indeterminate={condSel.size > 0 && condSel.size < condVisible.length}
            onchange={(event) =>
              selectAll('conditions', condVisible, event.currentTarget.checked)}
          />
          Conditions
        </h3>
        <ol class="statements" class:has-selection={condSel.size > 0}>
          {#each content.conditions as condition, i (i)}
            {@const tag = tagFor('conditions', i)}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions, a11y_no_noninteractive_element_interactions -->
            <li
              class="statement"
              class:disabled={condition.disabled}
              class:selected={condSel.has(i)}
              class:pending-change={tag === 'change'}
              class:pending-delete={tag === 'delete'}
              onclick={() => onedit.open('conditions', i)}
            >
              <input
                type="checkbox"
                class="sel"
                aria-label="select condition {i + 1}"
                checked={condSel.has(i)}
                onclick={(event) => {
                  event.stopPropagation()
                  selectRow('conditions', i, event)
                }}
              />
              {#if debug}
                <span class="debug" data-debug>line {condition.span?.start.line ?? '?'}</span>
              {/if}
              <Expr node={condition.value} {names} />
              {#if condition.disabled}<span class="disabled-badge">disabled</span
                >{/if}
              {#if tag}<span class="pending-tag {tag}">{tag}</span>{/if}
              {#if debug}
                <details class="debug" data-debug>
                  <summary>node</summary>
                  <pre>{JSON.stringify($state.snapshot(condition), null, 2)}</pre>
                </details>
              {/if}
            </li>
          {/each}
        </ol>
        {#if condSel.size > 0}
          <div class="sel-bar" data-list="conditions">
            <span class="sel-count">{condSel.size} selected</span>
            <button type="button" onclick={() => onedit.batch('conditions', [...condSel], 'disable')}
              >Disable</button
            >
            <button type="button" onclick={() => onedit.batch('conditions', [...condSel], 'enable')}
              >Enable</button
            >
            <button type="button" onclick={() => onedit.batch('conditions', [...condSel], 'delete')}
              >Delete</button
            >
            <button type="button" onclick={() => selection.conditions.clear()}>Clear</button>
          </div>
        {/if}
      </div>
    {/if}

    {@const actSel = selection.actions}
    {@const actDepths = statementDepths(content.actions)}
    {@const actVisible = actDepths.flatMap((depth, i) => (depth === null ? [] : [i]))}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="col"
      onmouseenter={() => (hoverCol = 'actions')}
      onmouseleave={() => {
        if (hoverCol === 'actions') hoverCol = null
      }}
    >
      <h3>
        <input
          type="checkbox"
          class="sel-all"
          aria-label="select all actions"
          checked={actSel.size === actVisible.length && actVisible.length > 0}
          indeterminate={actSel.size > 0 && actSel.size < actVisible.length}
          onchange={(event) => selectAll('actions', actVisible, event.currentTarget.checked)}
        />
        Actions
      </h3>
      <ol class="statements" class:has-selection={actSel.size > 0}>
        {#each content.actions as action, i (i)}
          {@const depth = actDepths[i]}
          {#if depth !== null}
            {@const tag = tagFor('actions', i)}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions, a11y_no_noninteractive_element_interactions -->
            <li
              class="statement"
              class:disabled={action.disabled}
              class:selected={actSel.has(i)}
              class:pending-change={tag === 'change'}
              class:pending-delete={tag === 'delete'}
              style="--depth: {depth}"
              onclick={() => onedit.open('actions', i)}
            >
              <input
                type="checkbox"
                class="sel"
                aria-label="select action {i + 1}"
                checked={actSel.has(i)}
                onclick={(event) => {
                  event.stopPropagation()
                  selectRow('actions', i, event)
                }}
              />
              {#if debug}
                <span class="debug" data-debug
                  >line {action.span?.start.line ?? '?'} · {action.call}</span
                >
              {/if}
              <Expr node={action} kinds={STATEMENT_KINDS} {names} />
              {#if action.disabled}<span class="disabled-badge">disabled</span
                >{/if}
              {#if tag}<span class="pending-tag {tag}">{tag}</span>{/if}
              {#if debug}
                <details class="debug" data-debug>
                  <summary>node</summary>
                  <pre>{JSON.stringify($state.snapshot(action), null, 2)}</pre>
                </details>
              {/if}
            </li>
          {/if}
        {/each}
      </ol>
      {#if actSel.size > 0}
        <div class="sel-bar" data-list="actions">
          <span class="sel-count">{actSel.size} selected</span>
          <button type="button" onclick={() => onedit.batch('actions', [...actSel], 'disable')}
            >Disable</button
          >
          <button type="button" onclick={() => onedit.batch('actions', [...actSel], 'enable')}
            >Enable</button
          >
          <button type="button" onclick={() => onedit.batch('actions', [...actSel], 'delete')}
            >Delete</button
          >
          <button type="button" onclick={() => selection.actions.clear()}>Clear</button>
        </div>
      {/if}
    </div>
  {/if}
</section>
