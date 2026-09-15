# Wright Compatibility Matrix

Status: current provider-boundary baseline

| Surface | Owner | Wright evidence |
| --- | --- | --- |
| OPY source workflows | `opy-rs` through LPP | `wright-driver` provider integration and CLI contract tests |
| DEL / OSTW source workflows | `deltin-rs` or a future provider | Explicit `source-provider-unavailable` diagnostics |
| Canonical Workshop text and WIR | `workshop-rs` | `wright-analyzer`, `wright-transform`, and consumer tests |
| Driver and CLI result contracts | Wright | `wright-driver` and `wright-cli` tests |

Wright has no native OPY parser, HIR, lowering, manifest, or reconstruction
implementation. It does not claim OPY syntax or semantic compatibility from a
successful Wright build; those claims require the owner repository's corpus
and provider evidence. A missing provider capability is reported explicitly,
with no silent fallback.
