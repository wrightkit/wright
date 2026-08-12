# Analysis fixture provenance

The `.opy` sources in this directory exercise positive cases for the v0.2
static analyses in `wright_analyzer::analysis`:

* `duplicate-condition.opy` — an `if`/`elif` pair with identical conditions
  (the `elif` can never be taken).
* `expensive-loop.opy` — a `distance()` geometry predicate evaluated inside a
  `while` body.

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
