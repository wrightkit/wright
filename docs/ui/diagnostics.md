# Diagnostics

Context: part of the [`wright ui` design](README.md). This document covers how findings from `findings` are shown: their severity, their place on the main screen, and the Diagnostics dialog.

## Severity

There are three severities, from most to least severe:

| Severity | Colour | Mark |
| --- | --- | --- |
| error | `--error` | filled circle |
| warning | `--warn` | filled triangle |
| info | `--muted` | outlined circle |

## In place

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
- **Filter.** A toolbar toggle "Only rules with findings" limits the rule list to rules with a finding at or above the lowest shown severity.

## In the dialog

- Summary cards in a grid. Each card is a rounded dark panel with a label, a large italic number over a 2 px rule, and a short description. The figures come from `findings` and `costEstimate`.
- Then the rules with the most elements. Clicking one closes the dialog and reveals that rule.
- Then the findings:
  - grouped by severity, most severe first;
  - the group headings are toggles that hide or show their level. Error and warning are shown by default, and info is collapsed. The choice persists for the session;
  - clicking a finding closes the dialog, reveals its rule, scrolls to its row, and flashes the row once.
