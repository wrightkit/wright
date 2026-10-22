<script lang="ts">
  /**
   * One rule's collapsed bar (docs/ui/design.md § Main screen item 3): a
   * 40 px white square chevron button outside the bar on its left, and
   * the bar itself (`--bar`, `--bar-hover` on hover) holding the rule
   * name on the left and the event on the right in `--muted`. Clicking
   * the bar or the chevron toggles the rule open below its bar, and the
   * chevron rotates while open. A disabled rule's name is `--dim` and
   * struck through — there is no separate `disabled` badge. In `?debug`
   * mode the bar also carries the rule's element count in monospace;
   * without debug the count is absent from the DOM.
   *
   * A rule carrying findings (docs/ui/diagnostics.md § In place · Rule
   * bar) shows the mark and count of its most severe level right of the
   * event; hovering the mark lists the rule's findings by severity.
   */
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
    /**
     * The resolved event text; the caller owns `lookup`, the
     * `subroutine:` prefix, and the failed-lookup id fallback.
     */
    event: string
    disabled?: boolean
    open?: boolean
    /** `?debug` only: the rule's element count; absent otherwise. */
    elements?: number
    /** The rule's findings, bar- and row-placed together; empty renders nothing. */
    findings?: Finding[]
    /** The session locale's chrome strings. */
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
