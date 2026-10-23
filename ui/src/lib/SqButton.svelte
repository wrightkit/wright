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

<style>
  .sq {
    position: relative;
    width: 40px;
    height: 40px;
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 3px;
    background: var(--white);
    color: var(--on-white);
    cursor: pointer;
    flex: none;
  }

  .sq-badge {
    position: absolute;
    top: -6px;
    right: -6px;
    min-width: 18px;
    padding: 0 4px;
    border-radius: 9px;
    background: var(--orange);
    color: var(--white);
    font-size: 11px;
    font-weight: 700;
    line-height: 18px;
    text-align: center;
  }

  .sq:hover:not(:disabled) {
    filter: brightness(0.93);
  }

  .sq.pressed {
    box-shadow: inset 0 0 0 2px var(--orange);
  }

  .sq:disabled {
    opacity: 0.45;
    cursor: default;
  }
</style>
