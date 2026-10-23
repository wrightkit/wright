<script lang="ts">
  import { tick, untrack } from 'svelte'
  import { Combobox } from 'bits-ui'

  interface Option {
    id: string
    label: string
  }

  let {
    options,
    selected = null,
    placeholder = '',
    ariaLabel,
    onselect,
  }: {
    options: Option[]
    selected: string | null
    placeholder?: string
    ariaLabel: string
    onselect: (id: string) => void
  } = $props()

  let open = $state(false)
  let filter = $state('')
  let sel = $state(untrack(() => selected ?? ''))
  let triggerEl: HTMLElement | null = $state(null)
  let inputEl: HTMLInputElement | null = $state(null)
  let contentEl: HTMLElement | null = $state(null)

  $effect(() => {
    const want = selected ?? ''
    if (sel !== want) sel = want
  })

  $effect(() => {
    if (!open) return
    filter = ''
    void tick().then(() => inputEl?.focus())
  })

  let wasOpen = false
  $effect(() => {
    const isOpen = open
    const closed = wasOpen && !isOpen
    wasOpen = isOpen
    if (closed) void tick().then(restoreFocus)
  })

  const filtered = $derived.by(() => {
    const needle = filter.trim().toLowerCase()
    if (!needle) return options
    return options.filter(
      (option) =>
        option.label.toLowerCase().includes(needle) || option.id.toLowerCase().includes(needle),
    )
  })

  function onValueChange(value: string) {
    open = false
    if (value !== '') {
      onselect(value)
      return
    }
    sel = selected ?? ''
    if (selected !== null) onselect(selected)
  }

  function restoreFocus() {
    const active = document.activeElement
    if (active === document.body || active === null || contentEl?.contains(active)) {
      triggerEl?.focus()
    }
  }
</script>

<Combobox.Root
  type="single"
  bind:open
  bind:value={sel}
  inputValue={filter}
  {onValueChange}
>
  <Combobox.Trigger bind:ref={triggerEl} class="picker-ctl" aria-label={ariaLabel}>
    <span class="picker-value" class:placeholder={selected === null}>
      {selected === null
        ? placeholder
        : (options.find((option) => option.id === selected)?.label ?? selected)}
    </span>
  </Combobox.Trigger>
  <Combobox.Portal>
    <Combobox.Content
      bind:ref={contentEl}
      class="picker-pop"
      customAnchor={triggerEl}
      align="start"
      sideOffset={8}
      collisionPadding={8}
    >
      <Combobox.Input
        bind:ref={inputEl}
        class="picker-filter"
        oninput={(event) => (filter = event.currentTarget.value)}
      />
      <Combobox.Viewport class="picker-list">
        {#each filtered as option (option.id)}
          <Combobox.Item class="picker-option" value={option.id} label={option.label}>
            {option.label}
          </Combobox.Item>
        {:else}
          <div class="picker-empty">—</div>
        {/each}
      </Combobox.Viewport>
    </Combobox.Content>
  </Combobox.Portal>
</Combobox.Root>

<style>
  /* The trigger, popover, and options are Bits UI markup, so these
     selectors are global; only the value span is this file's markup. */
  :global(.picker-ctl) {
    flex: 1;
    min-width: 0;
    padding: 0.3rem 0.6rem;
    border: 0;
    border-radius: 3px;
    background: var(--white);
    color: var(--on-white);
    text-align: left;
    font-size: 0.85rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }

  :global(.picker-ctl) .placeholder {
    color: var(--muted);
  }

  :global(.picker-pop) {
    z-index: 60;
    min-width: 14rem;
    max-width: 22rem;
    padding: 0.4rem;
    border-radius: 3px;
    background: var(--white);
    color: var(--on-white);
    box-shadow: 0 8px 28px rgb(0 0 0 / 0.45);
  }

  :global(.picker-filter) {
    width: 100%;
    box-sizing: border-box;
    margin-bottom: 0.3rem;
    padding: 0.3rem 0.5rem;
    border: 1px solid color-mix(in srgb, var(--on-white) 25%, transparent);
    border-radius: 2px;
    background: var(--white);
    color: var(--on-white);
    font-size: 0.85rem;
  }

  :global(.picker-list) {
    margin: 0;
    padding: 0;
    list-style: none;
    max-height: 14rem;
    overflow-y: auto;
  }

  /* Shared with the row context menu's items (`RowMenu`). */
  :global(.picker-option) {
    display: block;
    width: 100%;
    box-sizing: border-box;
    padding: 0.3rem 0.5rem;
    border: 0;
    background: transparent;
    color: var(--on-white);
    text-align: left;
    font-size: 0.85rem;
    cursor: pointer;
  }

  :global(.picker-option:hover),
  :global(.picker-option[data-highlighted]),
  :global(.picker-option[data-selected]) {
    background: color-mix(in srgb, var(--orange) 22%, var(--white));
  }

  :global(.picker-empty) {
    padding: 0.3rem 0.5rem;
    color: var(--muted);
  }
</style>
