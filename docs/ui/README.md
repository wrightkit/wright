# `wright ui` design

Context: for anyone changing the look or interaction of `wright ui` (`ui/`). ADR-0032 decides the form factor, ownership, and stack. These documents decide how the UI looks and behaves. Where a layer is not built yet, its section says so, and that section is still the target for the layer.

## Principle

The UI looks and behaves like the in-game Workshop editor. A person who knows the in-game editor should recognise every screen without reading anything.

It does not copy game assets. WrightKit draws its own icons and backdrop and uses system fonts. No image, icon, font, or text from the game client or a community site enters `ui/`.

Where the UI needs something the game editor lacks, it adds it in the same visual language, not a second one. Examples are filtering, diagnostics, pending changes, and debug detail.

## Language

- Workshop names come from `lookup` in the session locale: actions, values, events, enum members, and parameter labels.
- The session locale starts as the locale the project was read in, so a zh-CN project opens in zh-CN.
- The toolbar has a locale selector over the locales the catalog provides. Changing it re-resolves names and changes nothing else.
- UI chrome text follows the same locale where WrightKit has a translation, and falls back to en-US. Chrome text belongs to WrightKit. It is never taken from the game.

## Debug mode

`?debug` in the page URL turns on the developer details. Without it, none of these are in the DOM or visible:

- line numbers;
- call ids;
- element counts on rule bars;
- node JSON in the editor;
- the raw `editStatements` request in pending changes.

## Documents

| Document | Covers |
| --- | --- |
| [main-screen.md](main-screen.md) | title, toolbar, rule list, an opened rule, statement text and syntax colours |
| [editor.md](editor.md) | the statement editor form, pickers, and the hover card |
| [selection.md](selection.md) | selection, batch actions, large columns, and pending marks |
| [diagnostics.md](diagnostics.md) | severities, findings on rules and statements, and the Diagnostics dialog |
| [dialogs.md](dialogs.md) | dialog chrome, Variables, and Pending changes |
| [visual-system.md](visual-system.md) | colour tokens, backdrop, shapes, type, spacing and sizes |

## Not decided here

- The HUD view, which waits on its own design issue.
- Themes other than this one, including a light or high-contrast variant.
- Mobile layouts below the stacked-column breakpoint.
