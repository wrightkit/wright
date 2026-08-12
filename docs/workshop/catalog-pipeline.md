# Workshop Catalog Data Pipeline

Status: reproducible pipeline for milestone M5 (#30)
Scope: how Wright generates, validates, and updates the canonical Workshop
catalog localization data

The catalog (`crates/wright-workshop/src/catalog/data/catalog.json`) is
Wright-authored data with recorded provenance. It is the locale-identity
layer for the native Workshop parser/emitter; parser and emitter code never
contain locale-specific branches.

## Sources and licensing

* The v0.2 catalog covers the M5 P0 surface in `en-US`; spellings are
  transcribed from the compatibility corpus workshop snapshots and recorded
  in the support matrix
  ([`docs/workshop/support-matrix.md`](support-matrix.md)).
* OverPy's translation tables are GPL-3.0 reference data and are not
  automatically reusable as implementation data
  ([`LICENSE-BOUNDARY.md`](../../LICENSE-BOUNDARY.md),
  [`ADR-0004`](../../docs/adr/0004-overpy-licensing-boundary.md)). Adding a
  new locale therefore requires a permissible reference source, provenance
  review, and this pipeline; it is not a code change.
* Every committed catalog file carries `provenance` (generator, source,
  license, reviewed status).

## Commands

Run from the crate directory or pass `--file`:

```sh
cargo run -p wright-workshop --bin wright-catalog-gen -- check
cargo run -p wright-workshop --bin wright-catalog-gen -- build
```

* `check` validates the catalog (schema, duplicate canonical ids, colliding
  or missing aliases, undeclared locales) without writing.
* `build` validates and rewrites the file in canonical deterministic form
  (sorted object keys, stable formatting). Re-running is byte-idempotent, so
  regeneration is reproducible from the documented input (the data file).

## Update and review process

1. Edit the data file (add entries, aliases, enum members, or locales) with
   provenance updated.
2. Run `check`; it must pass. Colliding, missing, or ambiguous aliases fail
   validation rather than silently selecting a meaning.
3. Run `build`; review the canonical diff.
4. Commit the data change and the regenerated file together, referencing the
   evidence (fixture/corpus source, provenance review).

A game patch that changes Workshop strings or the target surface is handled
as the same bounded data update; it never becomes a parser or emitter code
rewrite.
