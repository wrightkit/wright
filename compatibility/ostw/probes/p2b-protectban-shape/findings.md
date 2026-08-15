# P2b findings — protect-ban-shaped missing import

See `../p2a-simple-missing/findings.md` for the shared P2 facts/interpretation.

Probe-specific fact: the `interface/Leaf.del` → `"../OSTWUtils/OnScreenText.del"`
edge (same shape as the corpus's `interface/HeroSelect.del` →
`../OSTWUtils/{OnScreenText,Cursor,StringSorting}.del`) rejects the compile with
one severity-1 diagnostic whose range covers the quoted path (line 0, chars 7–38)
and whose message names the NORMALIZED resolved path `<root>/OSTWUtils/OnScreenText.del`.

Decision support: the three missing OSTWUtils edges of the reachable graph are
hard-reject edges with source-located diagnostics; they cannot be satisfied by any
ambient source. They remain the `ostw-missing-import` boundary for #118.
