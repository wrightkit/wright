<script lang="ts">
  import { Dialog as BitsDialog } from 'bits-ui'
  import type { Snippet } from 'svelte'

  let {
    open,
    onclose,
    view = false,
    closeOnOutside = false,
    title,
    titleClass,
    id,
    class: className = '',
    children,
    footer,
  }: {
    open: boolean
    onclose: () => void
    view?: boolean
    closeOnOutside?: boolean
    title?: string
    titleClass?: string
    id?: string
    class?: string
    children: Snippet
    footer?: Snippet
  } = $props()
</script>

<BitsDialog.Root
  {open}
  onOpenChange={(next) => {
    if (!next) onclose()
  }}
>
  <BitsDialog.Portal>
    <BitsDialog.Overlay class="dlg-overlay" />
    <BitsDialog.Content
      {id}
      class={['dialog', view ? 'view' : '', className].filter(Boolean).join(' ')}
      onInteractOutside={(event) => {
        if (!closeOnOutside) event.preventDefault()
      }}
    >
      {#if view}
        <BitsDialog.Title level={2}>
          {#snippet child({ props })}
            <h2 {...props} class="dlg-title">{title}</h2>
          {/snippet}
        </BitsDialog.Title>
        <div class="dlg-body">{@render children()}</div>
        <div class="dlg-foot">{@render footer?.()}</div>
      {:else}
        {#if title}
          <BitsDialog.Title level={2}>
            {#snippet child({ props })}
              <h2 {...props} class={titleClass}>{title}</h2>
            {/snippet}
          </BitsDialog.Title>
        {/if}
        {@render children()}
      {/if}
    </BitsDialog.Content>
  </BitsDialog.Portal>
</BitsDialog.Root>
