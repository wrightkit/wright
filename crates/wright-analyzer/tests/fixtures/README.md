# Analysis fixture provenance

The `.opy` sources in this directory exercise positive and negative cases for
the static analyses in `wright_analyzer::analysis`:

* `duplicate-condition.opy` — positive: an `if`/`elif` pair with identical
  conditions (the `elif` can never be taken).
* `expensive-loop.opy` — positive: a `distance()` geometry predicate
  evaluated inside a `while` body.
* `repeated-value-parabola.opy` — positive (`repeated-value`, parabola
  shape): one action inside a `For Global Variable` loop containing two
  duplicated shape families (`distance(...)` twice; `time - offsets[I]`
  three times), each firing exactly one finding at its first occurrence.
* `repeated-value-santa.opy` — positive (`repeated-value`, santa shape): a
  `For Global Variable` loop whose sibling modify actions evaluate the
  identical `vectorTowards(...)` sub-expression three times in each of two
  families, with corpus-realistic inner calls (each duplicated shape has at
  least two call nodes).
* `repeated-value-shapes.opy` — positive (`repeated-value`): maximal-shape
  subsumption (a duplicated inner shape nested in a duplicated larger shape
  yields only the maximal finding) and per-scope reporting (one finding per
  loop scope for the same duplicated shape).
* `repeated-value-negative.opy` — negative (`repeated-value`): cross-rule,
  loop-free, trivial, non-identical, nested-loop-boundary, loop-bounds, and
  duplicated single-call (bare array read / single-call predicate)
  cases.
* `while-without-wait-positive.opy` — positive (`while-without-wait`): a
  `While(True)` loop whose body contains no `wait` call.
* `while-without-wait-negative.opy` — negative (`while-without-wait`):
  literal/computed/nested-in-`if` waits, and a waardless `For Global
  Variable` loop.

Each `.opy` source is converted with the pinned adapter
(`adapter/bin/wright-adapter.js`, adapter 0.1.0 over `overpy@9.7.10`) into the
checked-in `.json` protocol payload, which the Rust tests parse, convert, and
lower through `wright-core`. Regenerate a payload after an intentional change
with:

```sh
node adapter/bin/wright-adapter.js \
  --input wright-analyzer/tests/fixtures/<name>.opy \
  --root wright-analyzer/tests/fixtures \
  --output wright-analyzer/tests/fixtures/<name>.json
```

Both sources and payloads are Wright-authored (AGPL-3.0-or-later); no
third-party material is involved.
