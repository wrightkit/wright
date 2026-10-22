<script lang="ts">
  /**
   * The shared modal dialog chrome (docs/ui/dialogs.md) on Bits UI's
   * Dialog: focus trap, the dimmed overlay, and the layered dismissal
   * rules — Esc closes the topmost layer only, so a picker open above a
   * dialog keeps Esc for itself. `view` gives the spec's chrome (a
   * centred title over the scrolling `--dlg` body and a `--dlg-foot`
   * button band); the flat form keeps the editing-layer dialogs' padded
   * body. Callers own `open` and receive `onclose` when the dialog asks
   * to close; they never import `bits-ui` (ADR-0036).
   */
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
    /** Called when the dialog wants to close — Esc, an outside click, or a caller's button. */
    onclose: () => void
    /** The view chrome: centred `.dlg-title`, scrolling `.dlg-body`, `.dlg-foot` band. */
    view?: boolean
    /** Whether a click on the overlay closes the dialog — the view dialogs' backdrop click. */
    closeOnOutside?: boolean
    title?: string
    /** The title's class in the flat form; `view` always uses `.dlg-title`. */
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
