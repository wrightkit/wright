<script lang="ts">
  import { ContextMenu } from 'bits-ui'
  import type { Snippet } from 'svelte'

  let {
    items,
    ariaLabel,
    children,
  }: {
    items: { label: string; disabled?: boolean; onselect: () => void }[]
    ariaLabel: string
    children: Snippet<[{ props: Record<string, unknown> }]>
  } = $props()
</script>

<ContextMenu.Root>
  <ContextMenu.Trigger>
    {#snippet child({ props })}
      {@render children({ props })}
    {/snippet}
  </ContextMenu.Trigger>
  <ContextMenu.Portal>
    <ContextMenu.Content class="picker-pop" aria-label={ariaLabel}>
      {#each items as item (item.label)}
        <ContextMenu.Item
          class="picker-option"
          disabled={item.disabled ?? false}
          textValue={item.label}
          onSelect={item.onselect}
        >
          {item.label}
        </ContextMenu.Item>
      {/each}
    </ContextMenu.Content>
  </ContextMenu.Portal>
</ContextMenu.Root>
