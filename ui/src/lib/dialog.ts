/**
 * `HTMLDialogElement` modal helpers. Browsers implement `showModal`/`close`;
 * jsdom exposes the element and its `open` reflection but not the methods,
 * so the fallback toggles the attribute and dispatches the `close` event the
 * components' `onclose` handlers rely on.
 */
export function showModal(dialog: HTMLDialogElement): void {
  if (dialog.open) return
  if (typeof dialog.showModal === 'function') dialog.showModal()
  else dialog.setAttribute('open', '')
}

export function closeDialog(dialog: HTMLDialogElement): void {
  if (!dialog.open) return
  if (typeof dialog.close === 'function') {
    dialog.close()
  } else {
    dialog.removeAttribute('open')
    dialog.dispatchEvent(new Event('close'))
  }
}
