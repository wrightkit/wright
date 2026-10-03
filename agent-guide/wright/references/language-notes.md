# Language-specific notes

Confirm against `wright --help` and the `capabilities` session operation; support differs by version and language.

## Raw Workshop (`.ws`, `.txt`)

- Fully owned by `workshop-rs`. Semantic queries, validated edit transactions, and semantic rename are supported here where the installed version declares them.
- A semantic rename previews first and refuses on stale source, name collision, or unsupported kind (rules are not rename targets). Treat a refusal as final.

## OverPy (`.opy`)

- Compiled natively by Wright with the first-party OPY provider (`--opy-provider` overrides it). `check`, `lint`, `analyze`, and `compile` apply. `inspect` does not: on OPY it returns `source-provider-unsupported`, so answer symbol and flow questions from the source and `analyze`, and say what `inspect` would have confirmed.
- Edits are provider-owned. The raw edit operations refuse OPY input and name the provider operations; use those when the version offers them, otherwise edit the source directly and re-verify.
- `convert` runs Workshop to OPY only. Output is canonical, without comments, macros, or formatting, and unrepresentable constructs are rejected without partial output.

## DEL/OSTW

- The input kind is recognized, but no provider ships. Every workflow returns an explicit unavailable refusal; do not substitute another tool's result.
