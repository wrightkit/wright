# Agent contract: the simulate operation

Context: for consumers of `simulate` on `wright-agent/v1`. The rest of the
contract is in [agent-contract.md](agent-contract.md); the decision is
[ADR-0033](adr/0033-simulate-operation.md).

`simulate` runs the loaded program's canonical Workshop text — the same
emission `wright compile` writes, so a source-language project runs its
provider-compiled Workshop output — under `athena` and returns `athena`'s
trace unchanged inside a `wright-result/v1` envelope. `athena` owns
execution semantics and the trace contract — see its
`docs/simulator-interface.md`; Wright loads the project, checks the request
preconditions, invokes `athena`, and carries the trace. It never
reinterprets, reorders, or defaults trace content. The MCP tool is
`wright_simulate`; `capabilities.athena_version` names the linked `athena`
release.

## Request

```json
{"op": "simulate", "inputIdentity": "sha256:…", "seconds": 30}
```

| Field | Rule |
| --- | --- |
| `inputIdentity` | Required freshness precondition, as in `editStatements` (ADR-0029): the `inputIdentity` `project` reports for the served program, or the identity of the supplied `sources` text. A mismatch refuses `edit-stale-source` and runs nothing. |
| `sources` | Optional, same meaning as on `editStatements`: when supplied, the program is loaded from that text and `inputIdentity` is checked against it; the served snapshot is untouched. |
| `seconds` | Simulated seconds for a scenario-free run — `athena`'s `Options.seconds`; a positive finite number. Required when `scenario` is omitted. |
| `scenario` | An `athena` scenario value (players, injected events, expected observations, and the scenario's own `seconds`). Passed through uninterpreted; `athena` validates it. |

`seconds` and `scenario` are mutually exclusive — a scenario owns the
simulated time, so sending both is two sources of truth and refuses
`malformed-request`; sending neither leaves no simulated time and refuses
the same way.

## Result

The `result` member carries `{"trace": …}` — the `athena` trace unchanged.
`trace.outcome` says how the run ended and must be checked before reading
the state tables:

- `{"status": "complete"}` — the whole requested time was simulated.
- `{"status": "not_modeled", "construct": …, "tick": …}` — the run stopped
  at a construct the simulator does not model. Explicit, never a silent
  default, and not a verification failure in itself.
- `{"status": "missing_input"}` — the run stopped at a live input the
  scenario did not supply; `missingInputs` names it.

`logs`, `globals`, `players`, `hud`, and `effects` describe the state when
the run ended; `timeline` records the activity of every tick that had any —
one `entries` element per tick, `{tick, time, rules, changes}`, with
`truncated` set once `athena`'s bounds dropped records; `missingInputs` and
`expectations` report scenario facts. A result with unmodeled behavior or
missing inputs is result content — the verdict wording belongs to the
presentation contract and `simulate` never presents it as verified.

## Refusals

A request that cannot be served refuses instead of returning a trace:

| Code | When |
| --- | --- |
| `edit-stale-source` | `inputIdentity` does not match the program to simulate |
| `edit-unknown-source` | `sources` carries no text for the loaded input |
| `malformed-request` | `seconds` and `scenario` together, neither, or a non-positive/non-finite `seconds` |
| `malformed-scenario` | `athena` rejects the scenario document |
| `simulate-parse` | the program text does not parse as Workshop |
| `simulate-hero-values` | the scenario's hero-name value resolution fails |
| `workshop-emission` | the loaded program's canonical Workshop text cannot be emitted |
| loader diagnostics | the project does not load, as on every program-reading operation |

## What `simulate` does not do

There is no `wright simulate` CLI command (ADR-0033 defers it until a CI
consumer exists); the operation is reachable through `wright serve`, MCP,
and the UI through the driver. A scenario's omitted inputs are reported,
never defaulted; unsupported constructs end the trace at `not_modeled`,
never a fabricated continuation.
