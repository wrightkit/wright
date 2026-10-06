# ADR-0022: One shared surface for the semantic queries

- Status: Proposed
- Date: 2026-10-06
- Related: [Issue #429](https://github.com/wrightkit/wright/issues/429), [Issue #134](https://github.com/wrightkit/wright/issues/134), [ADR-0017](0017-domain-intelligence-query-contract.md), [ADR-0020](0020-native-agent-tool-adapter.md), [Product contract](../architecture/product-contract.md)

Recorded from the decision comment of 2026-09-29 on `#134`, moved here because a decision belongs in an ADR and not in an issue thread.

## Context

When this was decided, `symbols`, `references`, `usage`, `cfg`, `callGraph`, `costEstimate`, `persistentObjects`, and `targetMetadata` were reachable only through `wright serve`, and the CLI referenced none of them: humans saw a summary of what agents could query, two tiers of tooling for one capability.

## Decision

Principle 4 conformance is a 1.0 acceptance surface. The project-semantic queries are promoted to flat CLI commands resolved by name (#429), and the CLI and the agent paths return the same driver model.

The benchmark's no-injection arm audits whether this shared surface is self-sufficient; it does not test the model. The agent guide (#415) is therefore a defect list for the shared surface, and its correct trajectory is to shrink. For each line of guidance: would a human need to be told this too? If so, fix help, error text, or docs; if not, it is genuinely agent-specific, which should be rare. Making errors and help self-describing outranks the guide.

Concrete case that motivated it: `smallMessag` reported `unknown action` with no suggestion. A human goes to the docs; an agent pulled the 15 KB `targetMetadata`. Both paid for one missing did-you-mean in the owning repository.

## Superseded record

The same comment recorded "No MCP transport in 1.0". That record is superseded by [ADR-0020](0020-native-agent-tool-adapter.md). An MCP adapter stays under the rule above: it adapts `ToolService` and does not define a second tier with different results or defaults.

## Consequences

Any agent-facing default (result size, brief forms) is defined once for all surfaces; a surface does not get a different default from the CLI.
