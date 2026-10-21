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
   */
  import SqButton from './SqButton.svelte'

  let {
    name,
    event,
    disabled = false,
    open = false,
    elements,
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
    ontoggle: () => void
  } = $props()
</script>

<div class="rule-bar">
  <span class="chev" class:open>
    <SqButton
      title={open ? 'Collapse rule' : 'Expand rule'}
      label={open ? `Collapse ${name}` : `Expand ${name}`}
      onclick={ontoggle}
    >
      <path d="M9 6l6 6-6 6" />
    </SqButton>
  </span>
  <button type="button" class="bar" class:disabled aria-expanded={open} onclick={ontoggle}>
    <span class="bar-name">{name}</span>
    <span class="bar-side">
      <span class="bar-event">{event}</span>
      {#if elements !== undefined}
        <span class="bar-count" data-debug>{elements}</span>
      {/if}
    </span>
  </button>
</div>
