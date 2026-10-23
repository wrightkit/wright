# Editor (editing layer)

Context: part of the [`wright ui` design](README.md). This document covers editing one statement: the editor form, the pickers it uses, and the hover card on arguments. Dialog chrome is in [dialogs.md](dialogs.md).

## Form

- **Open.**
  - Clicking a statement row opens the editor for that statement.
  - Clicking an argument opens it with that argument's control focused and scrolled into view.
  - Hovering an argument outlines the argument a click would open.
- **Title.** The statement's call name in the session locale, 34 px italic.
- **First row.**
  - For an action, the row is "Action" with a picker over all actions.
  - For a condition, there are three rows: the left value, the operator (a picker over the comparison operators `lookup` declares), and the right value.
- **Argument rows.**
  - Each parameter is one row: a `--lbl` cell with the parameter's localized label from `lookup`, then a control.
  - The label cell turns `--lbl-focus` while its control has focus.
- **Controls by slot type, from `lookup`:**
  - An enum slot uses a picker over the domain's members from `listEntries`.
  - A variable slot (a variable-target action) uses a picker over the declared variables of the slot's scope from `symbols`.
  - A value slot uses a picker over the values that `lookup` admits for the slot type, from `listEntries`. The chosen value's own arguments appear as nested rows under it, indented.
  - A number literal uses a numeric input.
  - A string literal uses a text input.
  - A boolean literal uses a picker of the two literals.
  - A default from `lookup` is filled in when a call is chosen. A slot with no value shows "unset" in `--warn`, and OK is disabled while any slot is unset.
- **Disable toggle.** It sits above the form.
- **OK.**
  - OK stages one `replace` edit for the statement, through the pending-changes model.
  - The new node is sent to `editStatements` for validation when the changes are submitted.
  - The UI composes no Workshop text.
- **Insert.**
  - The column heading's `+` opens the same editor with an empty action (or condition).
  - OK stages an `insert` at the end of the column, or after the selected row when exactly one row is selected.
  - A block opener (`If`, `While`, `For …`) stages with its matching `End` in the same insert edit — `editStatements` refuses a result whose action stream leaves an opener unclosed.
- **Debug.** The node JSON preview appears under the form only with `?debug`.

## Pickers

- A dropdown is a popover in the top layer. It is never a native `<select>` once it has more options than fit on screen. Because it sits in the top layer, it is not clipped by the dialog, and opening it does not scroll the form.
- It opens below its control and flips above when the space below is short. It repositions on scroll and resize.
- It has:
  - a white filter input at the top;
  - a scrolling option list, with a highlighted active option;
  - the matching options from the project's own usage first.
- Esc closes the picker only, not the dialog under it.

## Hover card

- After 450 ms over an argument, a card shows:
  - the element's name and type;
  - its slot (which parameter of which call);
  - the parameters, from `lookup`.
- Descriptions are added when `lookup` provides them (wrightkit/workshop-rs#517).
- The card is navy with a gold top border. It does not take pointer events.
