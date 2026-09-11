# OPY Provider Boundary

Status: current

Wright does not implement the OPY language. `opy-rs` owns OPY syntax,
preprocessing, semantics, project loading, compilation, and reconstruction.
Wright consumes those capabilities only through the Language Provider Protocol
(LPP) provider boundary.

## Wright contract

| Workflow | Wright behavior |
| --- | --- |
| `.opy` check, compile, lint, or analyze | Route to the configured OPY provider; provider failures are explicit and never fall back to a local frontend. |
| Workshop input | Parse and analyze canonical Workshop text through `workshop-rs` in-process. |
| Workshop to OPY conversion | Request the provider's reconstruction capability; refuse explicitly when it is not advertised. |
| Inspect, editor mutation, or serialized OPY HIR | No local implementation; return a stable provider-unsupported diagnostic. |

The authoritative OPY syntax and compatibility matrix is maintained in
`opy-rs`. Wright evidence is limited to provider integration tests, CLI and
machine-result contracts, canonical Workshop regressions, and compatibility
fixtures consumed without executing an upstream compiler.
