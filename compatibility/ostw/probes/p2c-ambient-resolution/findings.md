# P2c findings — ambient/module resolution of a missing import

See `../p2a-simple-missing/findings.md` for the shared P2 facts/interpretation.

Probe-specific facts:
- `sub/a.ostw` imports `"ghost.del"` while `ghost.del` EXISTS at the project root.
  Both runs reject with the identical diagnostic naming the missing
  `<root>/sub/ghost.del` — the relative path from the importing file.
- The all-open run opened `ghost.del` (among others); the outcome is unchanged:
  an opened document is not a resolvable module for a path that misses it.

Decision support: no ambient/module source can satisfy an active import; the
resolution is strictly the normalized relative path from the importing file.
#118 must implement path-relative import resolution and a hard missing-import
diagnostic; workspace-wide module registration would diverge from the reference.
