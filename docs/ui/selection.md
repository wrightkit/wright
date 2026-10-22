# Selection and batch actions (editing layer)

Context: part of the [`wright ui` design](README.md). This document covers selecting statements, acting on many at once, and how pending edits are marked.

- **Selection.**
  - Each statement row has a checkbox. It is hidden until hover, or until any row in the column is selected.
  - Click selects one row. Shift-click selects a range.
  - A range never includes the hidden `End` of a block it does not fully contain (#899).
- **Where the actions appear.**
  - The batch actions sit in the column heading row, on the right, at the same level as the heading. This matches the place of the in-game top-right controls.
  - With no selection, the right side shows `+`.
  - With a selection, it shows `N selected`, then square buttons: Disable, Enable, Delete, and Clear.
  - The bar under the column is removed.
- **Sticky heading.**
  - While an open rule's column is taller than the viewport, its heading row sticks to the top of the viewport until the column ends.
  - The selection count and actions therefore stay reachable at any scroll position.
  - The sticky heading has the `--dlg` background, so rows do not show through it.
- **Large columns.** A column with more than 30 statements gets a filter field in its heading row, under the title:
  - Typing filters the column to statements whose text in the session locale contains the query. A call-name prefix also matches.
  - Hidden rows keep their selection.
  - Select-all then selects the visible matches only, and the count says `N selected (M hidden)` when selected rows are hidden.
  - Esc clears the filter.
- **Structural selection.** Two context-menu entries on a statement row:
  - "Select same action": every statement in the column with the same call id.
  - "Select block": a control-flow opener with its whole body, up to and including its `End`.
- **Keyboard.**
  - ⌘/Ctrl+A selects the visible rows of the column under the pointer.
  - Shift+↑/↓ extends the selection from the last clicked row.
  - Delete deletes the selection. Esc clears the filter first, then the selection.

Pending marks:

- An inserted or changed row has a gold left bar and a warm tint, and is tagged.
- A row marked for deletion has a red left bar and is struck through.
- An edited event row has a gold left bar and an undo button.
