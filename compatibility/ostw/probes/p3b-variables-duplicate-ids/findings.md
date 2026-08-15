# P3b findings — duplicate explicit IDs

Reference: OSTW v3.4.0 (identity in `probe.json`). Evidence:
`result.entry-only.json`, `workshop.entry-only.txt`.

## Observed facts

- `globalvar Number i 127;` then `globalvar Number j 127;` →
  severity-1 diagnostic on `main.ostw` line 1, range 17–18 (the `127` of `j`):
  `The id 127 is already reserved in the global collection.`
- `playervar Number p1 3;` then `playervar Number p2 3;` →
  severity-1 diagnostic on `main.ostw` line 4, range 17–19 (the `3` of `p2`):
  `The id 3 is already reserved in the player collection.`
- Result: reject, elementCount -1. The payload is the error log listing both errors.

## Decision support (interpretation)

- Duplicate explicit IDs are HARD errors (not warnings, not silent aliasing),
  reported at the second declaration's ID literal, with the collection named
  (global vs player) in the message. Global and player collections reserve
  independently.
- #118 collision policy: validate explicit IDs per collection and reject
  duplicates with a source-located diagnostic.

## Caveats

- Only duplicate-vs-duplicate was probed; auto-vs-explicit collision cannot occur
  because auto allocation skips reserved IDs (p3a).
