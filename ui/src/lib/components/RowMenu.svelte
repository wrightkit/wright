<script lang="ts">
  import { ContextMenu } from 'bits-ui'
  import type { Snippet } from 'svelte'

  /**
   * A statement row's context menu (docs/ui/selection.md § Structural
   * selection): Bits UI's context menu behind the components-layer
   * wrapper (ADR-0036 decision 4), drawn in the popover style of
   * docs/ui/editor.md § Pickers. The trigger's `child` snippet keeps the
   * row's own `<li>` — the trigger adds no wrapper element.
   */
  let {
    items,
    ariaLabel,
    children,
  }: {
    /** The menu's entries: `disabled` entries render but cannot be selected. */
    items: { label: string; disabled?: boolean; onselect: () => void }[]
    /** The menu content's aria label. */
    ariaLabel: string
    /** The row the menu triggers on (its `li`). */
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
