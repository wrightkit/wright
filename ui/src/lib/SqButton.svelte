<script lang="ts">
  import type { LucideIcon } from '@lucide/svelte'

  let {
    title,
    label = title,
    disabled = false,
    badge,
    onclick,
    icon: Icon,
  }: {
    title: string
    label?: string
    disabled?: boolean
    badge?: number
    onclick?: () => void
    icon: LucideIcon
  } = $props()
</script>

<button type="button" class="sq" {title} aria-label={label} {disabled} {onclick}>
  <Icon size={22} strokeWidth={2} aria-hidden="true" />
  {#if badge}<span class="sq-badge">{badge}</span>{/if}
</button>
