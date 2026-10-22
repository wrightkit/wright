# Dialogs

Context: part of the [`wright ui` design](README.md). This document covers the shared dialog chrome and the Variables and Pending changes dialogs. The Diagnostics dialog is in [diagnostics.md](diagnostics.md) and the editor is in [editor.md](editor.md).

Every secondary view is a modal dialog. It is never a page, a pane, or a drawer. Each dialog has:

- a navy body (`--dlg`);
- a centred title;
- a darker footer band (`--dlg-foot`) holding centred buttons.

The buttons:

- Primary: orange (`--orange`). This is Confirm in the view dialogs.
- Secondary: light grey.
- In the editor:
  - Cancel is blue (`--blue`);
  - OK is a large bold gold (`--gold`) button.

The dialogs are:

- **Diagnostics**: see [diagnostics.md](diagnostics.md).
- **Variables.**
  - Two columns, Global and Player, from `symbols`.
  - Subroutines centred under them.
  - Each name in a dark box.
  - Read-only until declaration edits exist.
- **Editor** (editing layer): see [editor.md](editor.md).
- **Pending changes** (editing layer).
  - A readable list grouped by rule.
  - Event changes come first, then conditions, then actions.
  - Each change is tagged insert, change, or delete.
  - The footer has Discard all, Cancel, and Submit.
