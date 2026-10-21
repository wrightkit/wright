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
  - Then the findings, one per row in alternating row shades, coloured by severity.
  - Clicking a rule or a finding closes the dialog and reveals that rule.
- **Variables.**
  - Two columns, Global and Player, from `symbols`.
  - Subroutines centred under them.
  - Each name in a dark box.
  - Read-only until declaration edits exist.
- **Editor** (editing layer).
  - The title is the action or condition name, large and italic.
  - Under it, a form: each argument is a row of a grey-blue label cell and a blue value control, side by side. The label cell lightens while its control has focus.
  - Nested calls are indented under a left rule.
  - A disable toggle sits above the form.
  - Clicking a statement opens the editor. Clicking an argument opens the editor focused on that argument. Hovering an argument outlines the one a click would open.
- **Pending changes** (editing layer).
  - A readable list grouped by rule.
  - Event changes come first, then conditions, then actions.
  - Each change is tagged insert, change, or delete.
  - The footer has Discard all, Cancel, and Submit.

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

- Each statement row has a checkbox, hidden until hover or until any row in the column is selected.
- Shift-click selects a range.
- The column's select-all checkbox shows an indeterminate state when part of the column is selected.
- While a selection exists, a bar under the column shows the count. It has square buttons for Disable, Enable, Delete, and Clear.
- Keyboard:
  - ⌘/Ctrl+A selects the column under the pointer;
  - Delete deletes the selection;
  - Esc clears it.
- A selected row has a gold inset outline.

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

## Not decided here

- The HUD view, which waits on its own design issue.
- Themes other than this one, including a light or high-contrast variant.
- Mobile layouts below the stacked-column breakpoint.
