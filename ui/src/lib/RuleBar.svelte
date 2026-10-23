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
