# P1 findings — entry closure, omitted event, rule priority

Reference: OSTW v3.4.0, contentCommit `769ce7aab097178cfe905bf21f0326d8e0d12e6b`,
asset SHA-256 `1ae882898961eac8ac25234a18fa3b130a02836651f7f936b9ece85f181e3a88`.
Evidence: `result.entry-only.json`, `result.all-open.json`, `workshop.entry-only.txt`,
`workshop.all-open.txt`. Facts below are the recorded JSON/payload contents.

## Observed facts

- entry-only run (only `main.ostw` opened): accept, elementCount 44, 0 diagnostics,
  workshopCode SHA-256 `3a28eb1e…6e8c`. Payload contains all four graph files'
  content: globals `rootMarker/aMarker/bMarker/cMarker` and rules from main, a, b, c,
  plus a synthesized `Initial Global` rule that sets the four initializer-bearing
  globals (rootMarker=1, aMarker=2, bMarker=3, cMarker=4).
- all-open run (every `.ostw`/`.del` opened, including the invalid `invalid.del`):
  accept, elementCount 8, 0 diagnostics, workshopCode SHA-256 `c5ae4e2f…f6091`.
  Payload contains ONLY `sub/c.ostw`'s content (`cMarker`, `Initial Global`,
  rule "P1 rule in c only").
- The invalid document (`invalid.del`, unterminated string, bad token `= ;`) produces
  NO diagnostics in either run.
- Omitted-event rule: `rule: "P1 root rule no event" { … }` emits
  `event { Ongoing - Global; }` — a global rule.
- Explicit priorities: the emitted rule order is `"P1 root rule priority minus one"`
  first, then `Initial Global`, then the unprioritized rules in source order
  (no-event, a, b, c), then `"P1 root rule priority plus two"` last. The Workshop
  text payload carries no priority value — only the sorted order.
- Scatter experiments (scratch, same pinned server):
  - Opening only `sub/c.ostw` while `ds.toml` says `entry_point="main.ostw"`:
    accept, elementCount 8, same SHA-256 `c5ae4e2f…` as the all-open last compile.
  - Opening only `sub/b.ostw`: accept, elementCount 18 (b+c closure).
  - A root whose `ds.toml` entry and only document is the invalid file: reject,
    diagnostic `Invalid expression term 'string'` at line 0.
- Message-stream observation (drain dump): the server fires one debounced compile
  per processed didOpen; the deterministic LAST compile is the last-opened
  document's own transitive import closure. Earlier compiles are timing-dependent
  and are never recorded by the runner.

## Decision support (interpretation)

- The reference language server does NOT select compilation by `ds.toml`
  `entry_point`: the compile unit is the last-opened document plus its transitive
  import closure (cycles deduplicated). Other open documents do not join the
  compile and an unrelated invalid document neither breaks the compile nor
  publishes diagnostics unless it is itself the compile root.
- #118's "entry-point-only compilation membership" is therefore a Wright-owned
  decision (matching #117's frontend design) that diverges from reference LSP
  behavior; the reference provides no oracle support for entry-closure-only
  membership, and the corpus baseline "accept" is not an entry-closure compile
  (see p1 findings on the corpus result in the issue comment).
- Omitted event = `Ongoing - Global` (every-frame global rule). Rule priority
  literals sort emitted rules (-1 first, +2 last, implicit 0 in source order);
  the synthesized `Initial Global` rule slots into the implicit-priority group.
  The Workshop text output loses the priority value itself (order only) — a
  structural/formatting property, not runtime semantics.
- Cross-file declarations and rules compile program-wide from the closure; rule
  `if` conditions and `disabled` were not exercised here (corpus-proven only).

## Caveats

- elementCount and rule order are structural evidence, not runtime behavior.
- The last-compile capture is deterministic for the last-opened document; any
  intermediate per-document compiles are timing-dependent and were ignored.
