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
    pressed?: boolean
    onclick?: () => void
    icon: LucideIcon
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
