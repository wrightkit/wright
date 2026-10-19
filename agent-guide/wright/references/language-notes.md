# Language-specific notes

Confirm against `wright --help` and the `capabilities` session operation; support differs by version and language.

## Raw Workshop (`.ws`, `.txt`)

- Fully owned by `workshop-rs`. Semantic queries and validated edit transactions are supported here where the installed version declares them.
- `compile --to <LOCALE>` re-emits the program in another Workshop client locale (e.g. `en-US`, `zh-CN`) without changing the input; the serve `compile` operation takes the same `to` field. A target locale missing mappings for the program fails explicitly, with no fallback.
- To rename a symbol, edit the source directly and re-verify with `check`.

## OverPy (`.opy`)

- Compiled natively by Wright with the first-party OPY provider (`--opy-provider` overrides it). `check`, `lint`, `analyze`, `inspect`, and `compile` apply; the `inspect` query subcommands read the provider's compiled artifact. A provider without compile capability declines with its own diagnostic.
- Edits are provider-owned. The raw edit operations refuse OPY input and name the provider operations; use those when the version offers them, otherwise edit the source directly and re-verify with `check` — that same edit-and-check path covers renames. The provider-backed editor rename (F2) remains in `wright-lsp`.
- `convert` runs Workshop to OPY only, and only when the provider negotiates the `reconstruct` capability; a `capability-unavailable` refusal means the installed provider does not offer it. When available, output is canonical, without comments, macros, or formatting, and unrepresentable constructs are rejected without partial output.

## DEL/OSTW

- The input kind is recognized, but no provider ships. Every workflow returns an explicit unavailable refusal; do not substitute another tool's result.
