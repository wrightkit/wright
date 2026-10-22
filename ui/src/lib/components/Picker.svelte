<script lang="ts">
  /**
   * The design.md § Pickers dropdown on Bits UI's Combobox: the trigger
   * shows the current option, and the top-layer popover anchors to it —
   * a white filter input above a scrolling option list with a
   * highlighted active option. Floating UI opens it below, flips it
   * above when the space below is short, and repositions on scroll and
   * resize; the escape layer closes the picker only, never the dialog
   * under it. Usage-first ordering is the caller's job: `options`
   * arrive already ordered.
   */
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
  // Bits' value mirrors `selected` — captured once, kept in sync by the
  // effect below so the prop stays authoritative. A click on the
  // selected option is a '' deselect inside the combobox.
  let sel = $state(untrack(() => selected ?? ''))
  let triggerEl: HTMLElement | null = $state(null)
  let inputEl: HTMLInputElement | null = $state(null)

  $effect(() => {
    const want = selected ?? ''
    if (sel !== want) sel = want
  })

  // Each open resets the filter and moves focus to it — the combobox's
  // content suppresses autofocus because the input normally keeps it.
  $effect(() => {
    if (!open) return
    filter = ''
    void tick().then(() => inputEl?.focus())
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
    // A choose closes the popover — including a click on the selected
    // option, which bits reports as a '' deselect. The wrapper reports
    // the clicked option; the caller decides what clearing it means.
    open = false
    if (value !== '') {
      onselect(value)
      return
    }
    sel = selected ?? ''
    if (selected !== null) onselect(selected)
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
      <div class="picker-list">
        {#each filtered as option (option.id)}
          <Combobox.Item class="picker-option" value={option.id} label={option.label}>
            {option.label}
          </Combobox.Item>
        {:else}
          <div class="picker-empty">—</div>
        {/each}
      </div>
    </Combobox.Content>
  </Combobox.Portal>
</Combobox.Root>
