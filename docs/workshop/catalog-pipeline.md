# Workshop Catalog Data Pipeline

Status: cutover (wright#143) — catalog ownership moved to `workshop-rs`
Scope: where the canonical Workshop catalog lives, how it is validated and
updated, and how Wright consumes it

The canonical Workshop catalog is **owned by `workshop-rs`**
(ADR-0001 Decision 6): the dataset lives at
`crates/workshop-rs/src/catalog/data/catalog.json` in the `workshop-rs`
repository, with its machine-readable provenance record embedded in the
dataset and surfaced by `workshop-rs-cli version --json`. It is the
locale-identity layer for the canonical Workshop parser and emitter; parser
and emitter code never contain locale-specific branches.

Wright no longer authors or generates catalog data. The `wright-workshop`
re-export-only adapter from the wright#143 cutover has been removed: Wright
consumes `workshop-rs` directly and contains no catalog implementation and no
`wright-catalog-gen` binary. The consumed `workshop-rs` revision is pinned in one place,
`[workspace.dependencies]` in the root `Cargo.toml`; catalog or locale changes
route to `workshop-rs` and are picked up here by updating that pin.

## Sources and licensing

* The catalog covers the supported Workshop surface in `en-US`; spellings are
  transcribed from the compatibility corpus workshop snapshots and recorded
  in the `workshop-rs` support matrix and provenance record.
* OverPy's translation tables are GPL-3.0 reference data and are not
  automatically reusable as implementation data
  ([`docs/licensing.md`](../licensing.md),
  [`ADR-0004`](../adr/0004-overpy-licensing-boundary.md)). `workshop-rs` is
  MIT-licensed and commits only MIT-compatible data with recorded
  provenance; adding a new locale requires a permissible reference source,
  provenance review, and the `workshop-rs` update pipeline — it is not a
  mechanical code change.
* Every committed catalog file carries `provenance` (generator, source,
  license, reviewed status).

## Commands

Catalog validation and regeneration live in the `workshop-rs` repository
(its `workshop-rs-cli` and catalog-update pipeline). Wright-side commands
for the removed implementation no longer exist.

## Update and review process

1. Edit the catalog data in `workshop-rs`
   (`crates/workshop-rs/src/catalog/data/catalog.json`) with provenance
   updated.
2. Run the `workshop-rs` catalog checks; they must pass. Colliding, missing,
   or ambiguous aliases fail validation rather than silently selecting a
   meaning.
3. Bump the pinned `workshop-rs` revision in Wright's root `Cargo.toml`
   (`[workspace.dependencies]`) to the reviewed commit, then run the Wright
   validation gates.

A game patch that changes Workshop strings or the target surface is handled
as the same bounded data update in `workshop-rs`; it never becomes a parser
or emitter code rewrite.
