// @vitest-environment jsdom
import { createRawSnippet, flushSync, mount, unmount } from 'svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import Dialog from './lib/components/Dialog.svelte'
import Picker from './lib/components/Picker.svelte'

/** Drain pending promise continuations, then flush Svelte's effects and DOM. */
async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0))
    flushSync()
  }
}

let host: HTMLDivElement
let app: ReturnType<typeof mount> | undefined

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn()
  host = document.createElement('div')
  document.body.appendChild(host)
})

afterEach(() => {
  if (app) void unmount(app)
  app = undefined
  document.body.innerHTML = ''
})

const click = (el: Element) => {
  el.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  flushSync()
}

const pointerDown = (el: Element) => {
  el.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }))
  flushSync()
}

const escape = () => {
  document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
  flushSync()
}

/** A pointerdown outside every layer — nonzero coordinates clear jsdom's
    zero rect. Bits debounces the outside interaction by 10 ms. */
const outsidePointer = async () => {
  document.body.dispatchEvent(
    new MouseEvent('pointerdown', { bubbles: true, cancelable: true, clientX: 500, clientY: 500 }),
  )
  await new Promise((resolve) => setTimeout(resolve, 25))
}

const dialog = () => document.body.querySelector<HTMLElement>('.dialog')
const popover = () => document.body.querySelector<HTMLElement>('.picker-pop')

const OPTIONS = [
  { id: 'a', label: 'Alpha' },
  { id: 'b', label: 'Beta' },
  { id: 'g', label: 'Gamma' },
]

function mountPicker(selected: string | null, onselect = vi.fn()) {
  app = mount(Picker, {
    target: host,
    props: {
      options: OPTIONS,
      selected,
      placeholder: 'Pick one',
      ariaLabel: 'choice',
      onselect,
    },
  })
  flushSync()
  return { onselect }
}

describe('the Dialog wrapper', () => {
  it('renders the view chrome and asks to close on Esc', async () => {
    const onclose = vi.fn()
    app = mount(Dialog, {
      target: host,
      props: {
        open: true,
        view: true,
        title: 'Diagnostics',
        onclose,
        children: createRawSnippet(() => ({ render: () => '<p>body row</p>' })),
        footer: createRawSnippet(() => ({ render: () => '<button>Confirm</button>' })),
      },
    })
    flushSync()

    const el = dialog()!
    expect(el, 'the dialog content is portaled into the body').not.toBeNull()
    expect(el.classList.contains('view')).toBe(true)
    expect(el.querySelector('.dlg-title')?.textContent).toBe('Diagnostics')
    expect(el.querySelector('.dlg-body')?.textContent).toContain('body row')
    expect(el.querySelector('.dlg-foot button')?.textContent).toBe('Confirm')
    expect(document.body.querySelector('.dlg-overlay')).not.toBeNull()

    escape()
    expect(onclose).toHaveBeenCalledTimes(1)
  })

  it('closes on an outside click only when closeOnOutside is set', async () => {
    const onclose = vi.fn()
    app = mount(Dialog, {
      target: host,
      props: {
        open: true,
        closeOnOutside: true,
        title: 'Variables',
        onclose,
        children: createRawSnippet(() => ({ render: () => '<p>body</p>' })),
      },
    })
    flushSync()

    await outsidePointer()
    await settle()
    expect(onclose).toHaveBeenCalledTimes(1)
  })

  it('ignores an outside click without closeOnOutside', async () => {
    const onclose = vi.fn()
    app = mount(Dialog, {
      target: host,
      props: {
        open: true,
        title: 'Pending',
        onclose,
        children: createRawSnippet(() => ({ render: () => '<p>body</p>' })),
      },
    })
    flushSync()

    await outsidePointer()
    await settle()
    expect(onclose).not.toHaveBeenCalled()
  })
})

describe('the Picker wrapper', () => {
  it('opens on the trigger, filters, and reports the chosen option', async () => {
    const { onselect } = mountPicker('a')

    const trigger = host.querySelector<HTMLButtonElement>('.picker-ctl')!
    expect(trigger.textContent).toContain('Alpha')
    pointerDown(trigger)
    await settle()

    expect(popover(), 'the popover is portaled into the body').not.toBeNull()
    expect(popover()!.querySelectorAll('.picker-option')).toHaveLength(3)

    const filter = popover()!.querySelector<HTMLInputElement>('.picker-filter')!
    filter.value = 'bet'
    filter.dispatchEvent(new Event('input', { bubbles: true }))
    flushSync()

    const options = [...popover()!.querySelectorAll('.picker-option')]
    expect(options.map((option) => option.textContent)).toEqual(['Beta'])

    options[0].dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }))
    options[0].dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
    await settle()

    expect(onselect).toHaveBeenCalledWith('b')
    expect(popover(), 'the popover closed on select').toBeNull()
  })

  it('reports the current option on a reselect click', async () => {
    const { onselect } = mountPicker('a')
    pointerDown(host.querySelector<HTMLButtonElement>('.picker-ctl')!)
    await settle()

    const option = popover()!.querySelector<HTMLElement>('.picker-option[data-selected]')!
    option.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }))
    option.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
    await settle()

    expect(onselect).toHaveBeenCalledTimes(1)
    expect(onselect).toHaveBeenCalledWith('a')
    expect(popover(), 'the popover closed on reselect').toBeNull()
  })

  it('shows the empty state when the filter matches nothing', async () => {
    mountPicker(null)
    pointerDown(host.querySelector<HTMLButtonElement>('.picker-ctl')!)
    await settle()

    const filter = popover()!.querySelector<HTMLInputElement>('.picker-filter')!
    filter.value = 'zzz'
    filter.dispatchEvent(new Event('input', { bubbles: true }))
    flushSync()

    expect(popover()!.querySelectorAll('.picker-option')).toHaveLength(0)
    expect(popover()!.querySelector('.picker-empty')).not.toBeNull()
  })

  it('returns focus to its control when Esc closes the popover', async () => {
    mountPicker('a')
    const trigger = host.querySelector<HTMLButtonElement>('.picker-ctl')!
    pointerDown(trigger)
    await settle()
    expect(popover()).not.toBeNull()

    escape()
    await settle()
    expect(popover(), 'Esc closed the popover').toBeNull()
    expect(document.activeElement, 'focus returned to the control').toBe(trigger)
  })
})

describe('nested Esc layering', () => {
  it('Esc closes only the picker, then the dialog', async () => {
    const onclose = vi.fn()
    app = mount(Dialog, {
      target: host,
      props: {
        open: true,
        title: 'Editor',
        onclose,
        children: createRawSnippet(() => ({ render: () => '<p>body</p>' })),
      },
    })
    flushSync()
    const pickerApp = mount(Picker, {
      target: host,
      props: {
        options: OPTIONS,
        selected: 'a',
        placeholder: 'Pick one',
        ariaLabel: 'choice',
        onselect: vi.fn(),
      },
    })
    flushSync()

    pointerDown(host.querySelector<HTMLButtonElement>('.picker-ctl')!)
    await settle()
    expect(popover()).not.toBeNull()
    expect(dialog()).not.toBeNull()

    escape()
    await settle()
    expect(popover(), 'the first Esc closed only the picker').toBeNull()
    expect(dialog(), 'the dialog is still open').not.toBeNull()
    expect(onclose).not.toHaveBeenCalled()

    escape()
    expect(onclose, 'the second Esc reached the dialog').toHaveBeenCalledTimes(1)

    void unmount(pickerApp)
  })
})
