# Main screen

Context: part of the [`wright ui` design](README.md). This document covers the page the person sees first: the title, toolbar, rule list, an opened rule, and how statements read.

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
