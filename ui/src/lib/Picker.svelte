<script lang="ts">
  /**
   * The design.md § Pickers dropdown: a control whose popover sits in the
   * top layer — a white filter input above a scrolling option list with a
   * highlighted active option. It opens below its control and flips above
   * when the space below is short, repositions on scroll and resize, and
   * Esc closes the picker only. Usage-first ordering is the caller's job:
   * `options` arrive already ordered.
   */
  import { tick } from 'svelte'

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
  // The highlighted option while the list is open — arrow keys move it,
  // Enter picks it, typing resets it to the first match.
  let activeIndex = $state(0)
  let above = $state(false)
  let x = $state(0)
  let y = $state(0)
  let controlEl: HTMLButtonElement | undefined = $state()
  let filterEl: HTMLInputElement | undefined = $state()
  let popoverEl: HTMLDivElement | undefined = $state()

  const filtered = $derived.by(() => {
    const needle = filter.trim().toLowerCase()
    if (!needle) return options
    return options.filter(
      (option) =>
        option.label.toLowerCase().includes(needle) || option.id.toLowerCase().includes(needle),
    )
  })

  function position() {
    if (!controlEl) return
    const rect = controlEl.getBoundingClientRect()
    const height = popoverEl?.offsetHeight ?? 260
    above = rect.bottom + height + 8 > window.innerHeight && rect.top - height - 8 > 0
    x = rect.left
    y = above ? rect.top - 8 : rect.bottom + 8
  }

  async function toggle() {
    if (open) {
      close()
      return
    }
    filter = ''
    activeIndex = 0
    open = true
    await tick()
    position()
    filterEl?.focus()
  }

  function close() {
    open = false
  }

  function choose(id: string) {
    onselect(id)
    close()
  }

  function onKeydown(event: KeyboardEvent) {
    // Esc closes the picker only, never the dialog under it.
    if (event.key === 'Escape') {
      event.stopPropagation()
      close()
      controlEl?.focus()
      return
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      const step = event.key === 'ArrowDown' ? 1 : -1
      activeIndex = Math.min(
        Math.max(activeIndex + step, 0),
        Math.max(filtered.length - 1, 0),
      )
      popoverEl
        ?.querySelectorAll<HTMLButtonElement>('.picker-option')
        [activeIndex]?.scrollIntoView({ block: 'nearest' })
      return
    }
    if (event.key === 'Enter') {
      event.preventDefault()
      const option = filtered[activeIndex]
      if (option) choose(option.id)
    }
  }

  function onDocPointer(event: PointerEvent) {
    const target = event.target as Node | null
    if (!target) return
    if (controlEl?.contains(target) || popoverEl?.contains(target)) return
    close()
  }

  function onDocKey(event: KeyboardEvent) {
    if (event.key === 'Escape' && open) {
      event.stopPropagation()
      close()
      controlEl?.focus()
    }
  }

  $effect(() => {
    if (!open) return
    document.addEventListener('pointerdown', onDocPointer, true)
    document.addEventListener('keydown', onDocKey, true)
    window.addEventListener('scroll', position, true)
    window.addEventListener('resize', position)
    return () => {
      document.removeEventListener('pointerdown', onDocPointer, true)
      document.removeEventListener('keydown', onDocKey, true)
      window.removeEventListener('scroll', position, true)
      window.removeEventListener('resize', position)
    }
  })
</script>

<button
  bind:this={controlEl}
  type="button"
  class="picker-ctl"
  aria-label={ariaLabel}
  aria-expanded={open}
  onclick={toggle}
>
  <span class="picker-value" class:placeholder={selected === null}>
    {selected === null ? placeholder : (options.find((o) => o.id === selected)?.label ?? selected)}
  </span>
</button>
{#if open}
  <div
    bind:this={popoverEl}
    class="picker-pop"
    class:above
    role="listbox"
    style:left="{x}px"
    style:top={above ? 'auto' : `${y}px`}
    style:bottom={above ? `${window.innerHeight - y}px` : 'auto'}
  >
    <input
      bind:this={filterEl}
      bind:value={filter}
      oninput={() => (activeIndex = 0)}
      class="picker-filter"
      type="text"
      onkeydown={onKeydown}
    />
    <ul class="picker-list">
      {#each filtered as option, index (option.id)}
        <li>
          <button
            type="button"
            class="picker-option"
            class:active={option.id === selected || index === activeIndex}
            role="option"
            aria-selected={option.id === selected}
            onclick={() => choose(option.id)}
          >
            {option.label}
          </button>
        </li>
      {:else}
        <li class="picker-empty">—</li>
      {/each}
    </ul>
  </div>
{/if}
