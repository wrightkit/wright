# Wright Current Architecture

This directory routes Wright's **current** architecture contracts.

Keep these evidence classes separate:

- documents here state current durable product/integration contracts;
- source, Cargo metadata, tests, CI, provider integrations, and real-project workflows establish implementation reality;
- owning language repositories establish their own semantic completeness and support claims;
- `docs/adr/` records point-in-time decisions and rationale. An accepted ADR is not by itself proof of current implementation reality.

## Routing

| Concern | Current contract / authority |
| --- | --- |
| Product/repository ownership and dependency direction | [`ownership.md`](ownership.md) |
| Language/provider integration and failure routing | [`integration.md`](integration.md) |
| Wright-owned lint/analyze/inspect/edit/agent/CI/LSP tooling model | [`tooling.md`](tooling.md) |
| CLI/driver behavior | [`../cli.md`](../cli.md) |
| Embedding/tool API | [`../embedding.md`](../embedding.md) |
| Language services/LSP | [`../language-services.md`](../language-services.md) |
| Compatibility methodology | [`../compatibility.md`](../compatibility.md) |
| Architecture decision history | [`../adr/README.md`](../adr/README.md) |

Current capability claims must be rebuilt from the owning implementation plus Wright integration evidence. Do not encode provider versions, current migration state, transient adapter behavior, feature counts, or Issue progress in this directory.