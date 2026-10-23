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

  // Bits only invokes onOpenChange when its own open setter runs; our
  // onValueChange close (same-value keyboard reselect) bypasses it.
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
