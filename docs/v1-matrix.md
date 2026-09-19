# Wright integration matrix

Status: current provider-boundary baseline

| Surface | Owner | Wright-owned test contract |
| --- | --- | --- |
| OPY source workflows | `opy-rs` through LPP | `wright-driver` provider seam and CLI failure-routing tests |
| DEL / OSTW source workflows | `deltin-rs` or a future provider | Explicit `source-provider-unavailable` diagnostics |
| Canonical Workshop text and WIR | `workshop-rs` | `wright-analyzer`, `wright-transform`, and embedding tests |
| Driver and CLI result contracts | Wright | `wright-driver` and `wright-cli` tests |

Wright does not claim OPY or DEL/OSTW language completeness from a successful
build. Provider capabilities and source-language semantics remain bounded by
the owning repository's public contract, with no silent fallback.
