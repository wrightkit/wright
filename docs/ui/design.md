# `wright ui` design

Context: for anyone changing the look or interaction of `wright ui` (`ui/`). ADR-0032 decides the form factor, ownership, and stack. This document decides how the UI looks and behaves. Where a layer is not built yet, its section says so. That section is still the target for the layer.

## Principle

The UI looks and behaves like the in-game Workshop editor. A person who knows the in-game editor should recognise every screen without reading anything.

It does not copy game assets. WrightKit draws its own icons and backdrop and uses system fonts. No image, icon, font, or text from the game client or a community site enters `ui/`.

Where the UI needs something the game editor lacks, it adds it in the same visual language, not a second one. Examples are filtering, diagnostics, pending changes, and debug detail.

## Language

- Workshop names come from `lookup` in the session locale: actions, values, events, enum members, and parameter labels.
- The session locale starts as the locale the project was read in, so a zh-CN project opens in zh-CN.
- The toolbar has a locale selector over the locales the catalog provides. Changing it re-resolves names and changes nothing else.
- UI chrome text follows the same locale where WrightKit has a translation, and falls back to en-US. Chrome text belongs to WrightKit. It is never taken from the game.

## Main screen

The page is one column. It is not a sidebar plus a detail pane.

1. **Title.**
   - "Workshop" in the session locale, set large and italic.
   - Beside it, smaller, the project's file name.
2. **Toolbar.**
   - A single row above a 2 px light rule line.
   - Left:
     - the rule count, shown as `Rules (139)`;
     - the diagnostics button.
   - Right:
     - the rule-name filter (a white input);
     - the locale selector (a white select);
     - a separator;
     - the view buttons: Variables, and pending changes with a count badge;
     - a separator;
     - Add rule, disabled until rule-level edits exist.
   - Every toolbar action is a white square icon button with a tooltip. No text buttons.
3. **Rule list.**
   - Every rule is a full-width bar in file order, one under another.
   - Each bar has:
     - on its left, a white square chevron button outside the bar;
     - in the bar, the rule name;
     - the event, right-aligned, as muted text in the session locale;
     - in debug mode only, the element count.
   - A disabled rule has a dimmed, struck-through name.
   - Clicking the bar or the chevron expands the rule in place, below its bar. Several rules may be open at once. The page scrolls; the list does not have its own scroll box.
4. **Expanded rule.** The body is indented under its bar and has:
   - **Event rows.**
     - One row each for Event, then Team, Player, or Subroutine as the event needs.
     - Each row is a dark label cell, about three quarters of the width, then a white value cell with a dropdown caret.
     - In the read-only layer the value cell is text. In the editing layer it is a select.
   - **Two columns.**
     - Conditions on the left, Actions on the right. On narrow screens they stack.
     - Each column head is the title in a large light weight with a muted count, `Conditions (1)`, over a light rule line.
     - In the editing layer the head has a select-all checkbox on its left and a white `+` square button on its right.
   - **Statement rows.**
     - Each row is a translucent dark bar, one per statement, with syntax colouring.
     - Control-flow bodies are indented 18 px per level under their opener. `End` is not shown.
     - A disabled statement is shown at half opacity and struck through.

## Statement text

- A statement is shown the way the in-game editor shows it: the call name, then its arguments in parentheses, in the session locale. Comparisons and arithmetic are infix.
- The logic layer is hidden in the main screen. It is shown only in the editor dialog. The logic layer means:
  - parameter labels;
  - argument slot positions;
  - call ids;
  - anything a person cannot read in the game.
- Global variables read `Global.name`. Player variables read `owner.name`.
- An argument whose value is unset is shown in the warning colour.

Syntax classes and colours:

| Class | Applies to | Colour |
| --- | --- | --- |
| action | action call name (bold) | `#ffffff` |
| value | value call name | `#8fd0ff` |
| enum | enum member | `#f2c86b` |
| number | number literal | `#b7e28a` |
| string | string literal | `#f6a77c` |
| variable | variable name | `#d1a6ff` |
| operator | infix operator | `#ff9e64` |
| literal | `true`, `false`, `null` | `#6fe0e6` |
| punctuation | parentheses, commas | `#a9b0c2` |

## Dialogs

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

- **Diagnostics.**
  - Summary cards in a grid: each card is a rounded dark panel with a label, a large italic number over a 2 px rule, and a short description. The figures come from `findings` and `inspect cost`.
  - Then the rules with the most elements.
  - Then the findings, grouped by severity as [Diagnostics in place](#diagnostics-in-place) describes.
  - Clicking a rule or a finding closes the dialog and reveals that rule.
- **Variables.**
  - Two columns, Global and Player, from `symbols`.
  - Subroutines centred under them.
  - Each name in a dark box.
  - Read-only until declaration edits exist.
- **Editor** (editing layer). See [Editor](#editor-editing-layer).
- **Pending changes** (editing layer).
  - A readable list grouped by rule.
  - Event changes come first, then conditions, then actions.
  - Each change is tagged insert, change, or delete.
  - The footer has Discard all, Cancel, and Submit.

## Editor (editing layer)

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
- **Debug.** The node JSON preview appears under the form only with `?debug`.

## Diagnostics in place

Findings come from `findings`. There are three severities, from most to least severe:

| Severity | Colour | Mark |
| --- | --- | --- |
| error | `--error` | filled circle |
| warning | `--warn` | filled triangle |
| info | `--muted` | outlined circle |

- **Toolbar.**
  - The Diagnostics button carries one count badge per severity that has findings, in its severity colour.
  - The badges show error first, and info only when no error or warning exists.
- **Rule bar.**
  - A rule with findings shows the mark and count of its most severe level, right of the event.
  - Hovering the mark lists the rule's findings by severity.
- **Statement row.**
  - A finding is placed on a statement as follows:
    - with an `action` index, on the action at that index in the rule's `actions`;
    - with `action: null` and a span inside a condition's span, on that condition;
    - otherwise, on the rule bar only.
  - A row with findings has a 3 px left bar in its most severe colour and the mark at its right end.
  - Hovering the mark shows the findings for that row: severity, code, and message.
- **Dialog.**
  - The findings list is grouped by severity, most severe first.
  - The group headings are toggles that hide or show their level. Error and warning are shown by default, and info is collapsed. The choice persists for the session.
  - Clicking a finding reveals its rule, scrolls to its row, and flashes the row once.
- **Filter.** A toolbar toggle "Only rules with findings" limits the rule list to rules with a finding at or above the lowest shown severity.

## Pickers

- A dropdown is a popover in the top layer. It is never a native `<select>` once it has more options than fit on screen. Because it sits in the top layer, it is not clipped by the dialog, and opening it does not scroll the form.
- It opens below its control and flips above when the space below is short. It repositions on scroll and resize.
- It has:
  - a white filter input at the top;
  - a scrolling option list, with a highlighted active option;
  - the matching options from the project's own usage first.
- Esc closes the picker only, not the dialog under it.

## Hover card (editing layer)

- After 450 ms over an argument, a card shows:
  - the element's name and type;
  - its slot (which parameter of which call);
  - the parameters, from `lookup`.
- Descriptions are added when `lookup` provides them (wrightkit/workshop-rs#517).
- The card is navy with a gold top border. It does not take pointer events.

## Selection and batch actions (editing layer)

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

## Debug mode

`?debug` in the page URL turns on the developer details. Without it, none of these are in the DOM or visible:

- line numbers;
- call ids;
- element counts on rule bars;
- node JSON in the editor;
- the raw `editStatements` request in pending changes.

## Tokens

These colours define the look. Components use the tokens, not literals.

| Token | Value | Use |
| --- | --- | --- |
| `--bar` | `rgb(44 48 68 / .72)` | rule bars, statement rows, event labels |
| `--bar-hover` | `rgb(44 48 68 / .86)` | hover and selected rows |
| `--dlg` | `#262c40` | dialog body |
| `--dlg-foot` | `#171b26` | dialog footer band |
| `--card` | `#1d2131` | cards, change items, preview boxes |
| `--lbl` / `--lbl-focus` | `#3a4566` / `#9c9fa8` | editor label cells |
| `--ctl` / `--ctl-hi` | `#2c5383` / `#3a6db0` | editor value controls |
| `--num` | `#343d58` | number and text inputs |
| `--opt-hi` | `#4a8fcb` | active picker option |
| `--text` / `--muted` / `--dim` | `#f2f4f8` / `#b9bfce` / `#8a91a6` | text levels |
| `--orange` | `#cf6a26` | primary buttons, badges |
| `--gold` | `#d9a03f` | OK, selection, pending marks |
| `--blue` | `#3d9be9` | Cancel |
| `--warn` / `--error` | `#f0b04a` / `#ff7a70` | warnings, unset values, errors, deletions |
| white control | `#f4f5f8` on `#111` | square buttons, inputs, event value cells |

- **Backdrop.** A light blue-grey diagonal gradient, `#c3c9e6` to `#98a6cc` to `#6f80aa`, under a fixed pattern of faint large triangles, drawn as inline SVG. The page never has a flat dark background.
- **Shapes.** Square corners, at most 3 px radius. Cards are the one exception, at 14 px.
- **Type.** The system UI sans-serif with a CJK fallback (`-apple-system, "PingFang SC", "Noto Sans SC", system-ui, sans-serif`), 15 px base. Monospace is used only in debug detail. No web fonts are bundled.
- **Square buttons.** 40 px in the toolbar, 36 px in column heads, 26–32 px inside rows and bars. The icon is 22 px or proportionally smaller.

## Spacing and sizes

| Element | Value |
| --- | --- |
| Page padding | 18 px top, 28 px sides, 60 px bottom |
| Title | 54 px, italic, bold, 2 px letter-spacing; file name 26 px italic |
| Toolbar | 6 px gap; 10 px padding below the 2 px rule line |
| Rule list | rules 6 px apart; list starts 14 px under the toolbar |
| Rule bar | 40 px high; 0 14 px padding; 12 px gap; chevron 40 px with 6 px gap to the bar |
| Rule body | 70 px left indent from the list edge; 14 px above and below |
| Event row | 38 px; label padding 8 14 px; rows 4 px apart; 16 px below the last row |
| Columns | 40 px gap; heading 24 px light, 6 px below, 2 px line |
| Statement row | min 40 px; padding 6 8 px; 4 px apart; text 14 px; 18 px indent per control-flow level |
| Dialog | body padding 26 32 22 px; title 28 px; footer padding 14 px, 12 px button gap |
| Editor form row | 36 px controls; 6 px between label and control; rows 5 px apart; nested calls 14 px indent plus 10 px after a 2 px rule |

## Not decided here

- The HUD view, which waits on its own design issue.
- Themes other than this one, including a light or high-contrast variant.
- Mobile layouts below the stacked-column breakpoint.
