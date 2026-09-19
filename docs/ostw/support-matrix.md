# DEL / OSTW integration coverage

Status: current provider boundary

`deltin-rs` owns DEL/OSTW syntax, project semantics, compatibility tests, and
reconstruction. Wright has no shipped provider. The current Wright contract is
an explicit `source-provider-unavailable` diagnostic for `.del`/`.ostw` source
workflows and the `ostw` conversion target.

The provider refusal is covered by the public CLI tests in
`crates/wright-cli/tests/cli.rs`. A future provider must be verified through
Wright's provider, CLI, and public-result tests while the language owner keeps
the source-language support matrix.
