# P2 findings — missing import boundary

Reference: OSTW v3.4.0 (identity in `probe.json`). Evidence per probe:
`result.entry-only.json` / `result.all-open.json` + `workshop.*.txt`.

## Observed facts

- p2a (main.ostw imports `"ghost.del"`, file absent): reject, elementCount -1,
  one severity-1 diagnostic on `main.ostw` line 0, range 7–18 (the quoted import
  path): `The file '/workspace/compatibility/ostw/probes/p2a-simple-missing/ghost.del' does not exist.`
- p2b (interface/Leaf.del imports `"../OSTWUtils/OnScreenText.del"`, mirroring the
  corpus edge): reject, elementCount -1, one severity-1 diagnostic on
  `interface/Leaf.del` line 0, range 7–38: `The file '/workspace/compatibility/ostw/probes/p2b-protectban-shape/OSTWUtils/OnScreenText.del' does not exist.`
  The reported path is the normalized absolute path (`interface/../OSTWUtils`
  collapsed to `<root>/OSTWUtils`).
- p2c (sub/a.ostw imports `"ghost.del"`; `ghost.del` EXISTS at the project root):
  reject in BOTH runs, elementCount -1, identical diagnostic on `sub/a.ostw`
  line 0: `…/p2c-ambient-resolution/sub/ghost.del does not exist.` Opening the
  root-level `ghost.del` (all-open run) does not change the outcome.
- No diagnostic `code` field is emitted by the reference; message + range only.
- The rejected runs' `workshopCode` payload is the error log text (same message
  with `main.ostw: Error at 0, 7:` prefix), not Workshop code.

## Decision support (interpretation)

- Import resolution is strictly relative to the importing file and path-normalized;
  no ambient/module source (a same-named file elsewhere in the workspace root) can
  satisfy an import, and opening the document does not register it as a module.
- A missing active import is a hard error: the whole compile rejects
  (elementCount -1). #118's `ostw-missing-import` boundary is confirmed: the three
  `../OSTWUtils/…` edges of the reachable protect-ban graph reject the entry-closure
  compile with source-located diagnostics exactly like p2b.
- No external library lookup exists in the pinned reference; the boundary must be
  a Wright-owned diagnostic with the recorded message shape.

## Caveats

- Diagnostic wording is reference-observed; Wright's D-level contract allows its
  own wording as long as category and source region match.
