# CLI source-provider and library integration

[← CLI contract index](../cli.md)

## The `.opy` source implementation

`.opy` `check`, `compile`, `lint`, and `analyze` inputs use the owner-backed
`opy-rs` implementation through Wright's narrow LPP adapter. Wright first
negotiates LPP 1.4; a provider without it keeps the LPP 1.1 file-target or
LPP 1.2 directory-target session. No Node or OverPy is involved, and provider
failures never fall back to the native frontend. `lint` and `analyze` parse the
provider's canonical Workshop artifact through `workshop-rs` and reuse the
existing Wright analyzer and lint registry.

In an LPP 1.4 session Wright lists `workshop-rs/mapped-text-v1` and
`workshop-rs/text-v1` in `acceptedArtifactFormats` on `lpp/compile`. When the
provider returns the mapped format, Wright applies its `SourceMap` to the
re-parsed program before validation and transforms, so findings and validation
diagnostics resolve to authored source paths (root-relative when under the
project root). Nodes without an authored origin, and nodes a transform
restructures, carry no span: their findings have `span: null` rather than a
fabricated location. If the map does not match the artifact's shape, Wright
reports a `source-map-mismatch` warning and treats the whole artifact as
unmapped. Without the mapped format, the analysis origin and lint spans are
explicitly `provider-artifact` / `<provider-artifact>` rather than fabricated
OPY locations.

An entry path is required for provider-backed workflows, so stdin
`.opy` is rejected; `--root` supplies the project root for file and directory
inputs. The
provider executable is resolved by the #244 bootstrap path and can be
overridden with `--opy-provider`. `inspect` remains on the native frontend for
explicit OPY file targets; OPY directory targets use the owner-backed compile
path so the owner selects the project entry.
The source surface is declared in
[`opy/support-matrix.md`](../opy/support-matrix.md).

## Library reuse

External Rust consumers depend on `wright-driver` (never the CLI) and drive
`CompilerSession::new(config)` → `compile`/`check`/`analyze`/`inspect`/`lint`
or `convert(ConvertTarget)`, each returning a typed `Envelope<T>`. Loading is
idempotent (`Session::load`), and the driver exposes the resolved locale,
input identity, and origin metadata. See `crates/wright-driver/tests/driver.rs`
and the provider/integration tests for the reusable test surface.
