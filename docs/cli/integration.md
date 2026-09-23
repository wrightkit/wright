# CLI source-provider and library integration

[← CLI contract index](../cli.md)

## The `.opy` source implementation

`.opy` `check`, `compile`, `lint`, and `analyze` inputs use the owner-backed
`opy-rs` implementation through Wright's narrow LPP adapter. File targets use
LPP 1.1; directory targets use LPP 1.2 so `opy-rs` owns project entry
discovery. No Node or OverPy is involved, and provider failures
never fall back to the native frontend. `lint` and `analyze` parse the
provider's canonical Workshop artifact through `workshop-rs` and reuse the
existing Wright analyzer and lint registry. Provider artifacts have no OPY
span map, so their analysis origin and lint spans are explicitly
`provider-artifact` / `<provider-artifact>` rather than fabricated OPY
locations. An entry path is required for provider-backed workflows, so stdin
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
