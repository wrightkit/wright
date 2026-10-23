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

<style>
  /* The overlay, panel, and slotted buttons are Bits UI / caller
     markup, so these selectors are global; the view chrome
     (`dlg-title`, `dlg-body`, `dlg-foot`) is this file's markup. */
  :global(.dlg-overlay) {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgb(0 0 0 / 0.5);
  }

  :global(.dialog) {
    color-scheme: dark;
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    z-index: 50;
    background: var(--dlg);
    color: var(--text);
    border: 1px solid rgb(255 255 255 / 0.15);
    border-radius: 6px;
    padding: var(--dialog-body-pad);
    min-width: 24rem;
    max-width: 42rem;
    max-height: 80vh;
    overflow-y: auto;
  }

  :global(.dialog) h2 {
    margin-top: 0;
    font-size: var(--dialog-title-size);
  }

  /* Every dialog's buttons share the card-dark button look; the feature
     dialogs layer their own `primary`/`ok` variants on top. */
  :global(.dialog button) {
    background: var(--card);
    border: 1px solid rgb(255 255 255 / 0.18);
    border-radius: 3px;
    color: var(--text);
    padding: 0.3rem 0.8rem;
    cursor: pointer;
  }

  :global(.dialog button:hover) {
    border-color: var(--gold);
  }

  :global(.dialog.view) {
    display: flex;
    flex-direction: column;
    width: min(44rem, calc(100vw - 4rem));
    max-width: 44rem;
    padding: 0;
    border-radius: 3px;
  }

  :global(.dialog.view) .dlg-title {
    flex: none;
    margin: 0;
    padding: 26px 32px 0;
    text-align: center;
    font-size: var(--dialog-title-size);
  }

  :global(.dialog.view) .dlg-body {
    min-height: 0;
    overflow-y: auto;
    padding: var(--dialog-body-pad);
  }

  :global(.dialog.view) .dlg-foot {
    flex: none;
    display: flex;
    justify-content: center;
    gap: var(--dialog-foot-gap);
    padding: var(--dialog-foot-pad);
    background: var(--dlg-foot);
    border-radius: 0 0 3px 3px;
  }

  /* The view dialogs' footer action: the centred orange primary. */
  :global(.dialog.view .primary) {
    min-width: 9rem;
    padding: 0.45rem 1.5rem;
    border: 0;
    background: var(--orange);
    color: var(--white);
    font-weight: 600;
  }

  :global(.dialog.view .primary:hover) {
    filter: brightness(1.12);
  }

  /* The simulate panel's run action is the same orange primary in a
     narrower form; it comes after `.dialog button` to override it. */
  :global(.sim-run .primary) {
    min-width: 7rem;
    padding: 0.45rem 1.5rem;
    border: 0;
    background: var(--orange);
    color: var(--white);
    font-weight: 600;
  }

  :global(.sim-run .primary:hover:not(:disabled)) {
    filter: brightness(1.12);
  }

  :global(.sim-run .primary:disabled) {
    opacity: 0.55;
    cursor: default;
  }

  /* The shared dialog footer row the pending dialog and the statement
     editor use, with its gold-bordered OK. It must override `.dialog
     button`, so it comes after it here. */
  :global(.editor-foot) {
    display: flex;
    justify-content: center;
    gap: var(--dialog-foot-gap);
    margin-top: 14px;
    padding-top: var(--dialog-foot-pad);
    border-top: 1px solid rgb(255 255 255 / 0.15);
  }

  :global(.editor-foot button) {
    background: var(--dlg);
    border: 1px solid rgb(255 255 255 / 0.15);
    border-radius: 4px;
    color: var(--text);
    padding: 0.25rem 0.8rem;
    cursor: pointer;
  }

  :global(.editor-foot button:hover) {
    border-color: var(--gold);
  }

  :global(.editor-foot button:disabled) {
    opacity: 0.5;
    cursor: default;
    border-color: rgb(255 255 255 / 0.15);
  }

  :global(.editor-foot .ok) {
    border-color: var(--gold);
    color: var(--gold);
    font-weight: 600;
  }
</style>
