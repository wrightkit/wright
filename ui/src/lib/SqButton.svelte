<script lang="ts">
  /**
   * A white square icon button (docs/ui/design.md § Square buttons): 40 px in
   * the toolbar, `#f4f5f8` on `#111`, 3 px radius, `title` tooltip. The icon
   * is an inline SVG drawn for WrightKit — the children render inside the
   * shared 24 px stroke shell. `badge` adds the spec's count badge (§ Main
   * screen 2) at the button's corner; `0` shows nothing.
   */
  import type { Snippet } from 'svelte'

  let {
    title,
    label = title,
    disabled = false,
    badge,
    onclick,
    children,
  }: {
    title: string
    label?: string
    disabled?: boolean
    badge?: number
    onclick?: () => void
    children: Snippet
  } = $props()
</script>

<button type="button" class="sq" {title} aria-label={label} {disabled} {onclick}>
  <svg
    viewBox="0 0 24 24"
    width="22"
    height="22"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    {@render children()}
  </svg>
  {#if badge}<span class="sq-badge">{badge}</span>{/if}
</button>
