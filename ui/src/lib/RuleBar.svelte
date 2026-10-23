<script lang="ts">
  import { ChevronRight } from '@lucide/svelte'
  import FindingMark from './FindingMark.svelte'
  import SqButton from './SqButton.svelte'
  import type { ChromeStrings } from './chrome'
  import type { Finding } from './types/generated/wright-agent-v1'

  let {
    name,
    event,
    disabled = false,
    open = false,
    elements,
    findings = [],
    t,
    ontoggle,
  }: {
    name: string
    event: string
    disabled?: boolean
    open?: boolean
    elements?: number
    findings?: Finding[]
    t: ChromeStrings
    ontoggle: () => void
  } = $props()
</script>

<div class="rule-bar">
  <span class="chev" class:open>
    <SqButton
      title={open ? t.collapseRule : t.expandRule}
      label={open ? t.collapseRuleLabel(name) : t.expandRuleLabel(name)}
      onclick={ontoggle}
      icon={ChevronRight}
    />
  </span>
  <button type="button" class="bar" class:disabled aria-expanded={open} onclick={ontoggle}>
    <span class="bar-name">{name}</span>
    <span class="bar-side">
      <span class="bar-event">{event}</span>
      {#if findings.length > 0}
        <FindingMark {findings} count {t} />
      {/if}
      {#if elements !== undefined}
        <span class="bar-count" data-debug>{elements}</span>
      {/if}
    </span>
  </button>
</div>

<style>
  .rule-bar {
    display: flex;
    align-items: stretch;
    gap: var(--chev-gap);
  }

  .chev {
    display: inline-flex;
    width: var(--chev-size);
  }

  .chev :global(svg) {
    transition: transform 0.15s ease;
  }

  .chev.open :global(svg) {
    transform: rotate(90deg);
  }

  .bar {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--rule-bar-gap);
    min-height: var(--rule-bar-height);
    box-sizing: border-box;
    border: 0;
    border-radius: 3px;
    background: var(--bar);
    color: var(--text);
    text-align: left;
    padding: var(--rule-bar-pad);
    cursor: pointer;
  }

  .bar:hover {
    background: var(--bar-hover);
  }

  .bar.disabled .bar-name {
    color: var(--dim);
    text-decoration: line-through;
  }

  .bar-side {
    margin-left: auto;
    display: flex;
    align-items: baseline;
    gap: 12px;
  }

  .bar-event {
    color: var(--muted);
  }

  .bar-count {
    font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    font-size: 0.8rem;
    color: var(--muted);
  }
</style>
