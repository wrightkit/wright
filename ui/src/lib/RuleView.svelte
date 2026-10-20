<script lang="ts">
  /**
   * One rule's structured view: event header, conditions, and the indented
   * action stream. A rule whose content `rules` cannot express renders the
   * service refusal in place while other rules still render. `?debug` adds
   * line numbers, call ids, and the node JSON; they are absent otherwise.
   */
  import Expr from './Expr.svelte'
  import type { Names } from './names'
  import { collectIdentities, isEventPlayerMember, statementDepths, STATEMENT_KINDS } from './nodes'
  import type { Event } from './types/generated/rule-content-v1'
  import type { RuleState, RuleSummary } from './view'

  let {
    summary,
    state: ruleState,
    names,
    debug,
  }: { summary: RuleSummary; state: RuleState; names: Names; debug: boolean } = $props()

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

<section class="rule-view" data-e2e="rule-view">
  <h2 class="rule-title">
    {summary.name}
    {#if debug}<span class="debug" data-debug>#{summary.id}</span>{/if}
  </h2>
  {#if ruleState.phase === 'loading' || (ruleState.phase === 'ready' && !namesReady)}
    <p class="muted">Loading rule…</p>
  {:else if ruleState.phase === 'refused'}
    <div class="refusal" data-e2e="refusal">
      <strong>The service could not express this rule's content.</strong>
      <code>{ruleState.refusal.code}</code>
      <p>{ruleState.refusal.message}</p>
    </div>
  {:else}
    {@const content = ruleState.content}
    <header class="event" data-e2e="event">
      <span class="tok value">{eventName(content.event)}</span>
      {#each filterNames(content.event) as filter (filter)}
        <span class="filter">{filter}</span>
      {/each}
      {#if content.disabled}
        <span class="disabled-badge" data-e2e="disabled">disabled</span>
      {/if}
      {#if debug}
        <span class="debug" data-debug>event id: {content.event.id}</span>
      {/if}
    </header>

    {#if content.conditions.length > 0}
      <h3>Conditions</h3>
      <ol class="statements">
        {#each content.conditions as condition, i (i)}
          <li class="statement" class:disabled={condition.disabled} data-e2e="condition">
            {#if debug}
              <span class="debug" data-debug>line {condition.span?.start.line ?? '?'}</span>
            {/if}
            <Expr node={condition.value} {names} />
            {#if condition.disabled}<span class="disabled-badge" data-e2e="disabled">disabled</span
              >{/if}
            {#if debug}
              <details class="debug" data-debug>
                <summary>node</summary>
                <pre>{JSON.stringify($state.snapshot(condition), null, 2)}</pre>
              </details>
            {/if}
          </li>
        {/each}
      </ol>
    {/if}

    <h3>Actions</h3>
    {@const depths = statementDepths(content.actions)}
    <ol class="statements">
      {#each content.actions as action, i (i)}
        {@const depth = depths[i]}
        {#if depth !== null}
          <li
            class="statement"
            class:disabled={action.disabled}
            style="--depth: {depth}"
            data-e2e="statement"
          >
            {#if debug}
              <span class="debug" data-debug
                >line {action.span?.start.line ?? '?'} · {action.call}</span
              >
            {/if}
            <Expr node={action} kinds={STATEMENT_KINDS} {names} />
            {#if action.disabled}<span class="disabled-badge" data-e2e="disabled">disabled</span
              >{/if}
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
  {/if}
</section>
