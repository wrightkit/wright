# Wright Architecture — compatibility pointer

The current architecture contracts are maintained under
[`docs/architecture/`](architecture/README.md):

- [`ownership.md`](architecture/ownership.md) — repository/product ownership, dependency direction, and capability ceiling;
- [`integration.md`](architecture/integration.md) — source/provider integration, contract preservation, and failure routing;
- [`tooling.md`](architecture/tooling.md) — Wright-owned lint/analyze/inspect/edit/agent/CI/LSP tooling model.

This path is retained for existing links. The previous living architecture combined ownership, source-form integration, tooling, compilation/conversion, and transient provider implementation details. That baseline remains available in Git history but is no longer a second current architecture authority.

Current implementation reality comes from source, Cargo metadata, tests, CI, provider integrations, and real-project workflows. Language-specific support claims are bounded by the owning implementation plus verified Wright integration.
