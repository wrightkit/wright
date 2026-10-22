# Visual system

Context: part of the [`wright ui` design](README.md). This document holds the values that define the look: colour tokens, backdrop, shapes, type, and spacing. Components use these values and no literals.

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
- **Square buttons.** 40 px in the toolbar, 36 px in column heads, 26–32 px inside rows and bars. The icon is a Lucide icon at 22 px with stroke width 2, or proportionally smaller.

## Spacing and sizes

These values are part of the look. Components apply them as written, in `px`, and not as `rem` multiples.

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
