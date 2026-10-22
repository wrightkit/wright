<script lang="ts">
  import type { LucideIcon } from '@lucide/svelte'
  import type { Snippet } from 'svelte'

  let {
    title,
    label = title,
    disabled = false,
    badge,
    pressed,
    onclick,
    icon: Icon,
    children,
  }: {
    title: string
    label?: string
    disabled?: boolean
    badge?: number
    /** A toggle button reports its state (`aria-pressed`) and reads pressed. */
    pressed?: boolean
    onclick?: () => void
    icon: LucideIcon
    /** Extra content after the icon — the Diagnostics button's severity badges. */
    children?: Snippet
  } = $props()
</script>

<button
  type="button"
  class="sq"
  {title}
  aria-label={label}
  {disabled}
  aria-pressed={pressed}
  class:pressed={pressed === true}
  {onclick}
>
  <Icon size={22} strokeWidth={2} aria-hidden="true" />
  {#if badge}<span class="sq-badge">{badge}</span>{/if}
  {@render children?.()}
</button>
