# Execution trace contract (#1079)

- Status: stable, versioned trace format
- Contract: `wright-trace/v1`
- Owner: the `wright` CLI renders the trace; `wright-driver::trace` records it.

`wright compile` and `wright check` accept `--trace[=FORMAT]`, where `FORMAT`
is `text` (the default when the value is omitted) or `json`. The trace goes
to standard error. It never changes standard output, the exit code, the
diagnostics, or any `wright-result/v1` payload. Without the flag nothing is
recorded and nothing extra is written.

The JSON form writes newline-delimited objects, one per line, with schema
`wright-trace/v1`. A reader selects lines by `type`:

- one `environment` line, first. It has `schema` and `facts`, an object of
  the workflow facts: `wrightVersion`, `os`, `arch`, `allocator`, and when
  known `inputKind`, `providerId`, `providerVersion`, `providerPath`,
  `lppVersion`, `sourceFiles`, `locale`, `outputBytes`.
- one `span` line per span, in start order. Its fields are:
  - `id`: integer; `0` is the root span, and ids follow start order;
  - `parent`: the parent span `id`, or `null` for the root;
  - `name`: the phase name, such as `catalog.load`, `input.resolve`,
    `provider.connect`, `provider.compile`, `parse`, `validate`, `emit`,
    `output.write`, `provider.shutdown`, or `lpp.request`;
  - `startNs`: nanoseconds from the start of the trace, on a monotonic clock;
  - `durationNs`: nanoseconds, on a monotonic clock;
  - `status`: `ok` or `error`. A span that returned an error, including a
    failing compile step, is `error`; spans that finished before the failure
    keep `ok`;
  - `facts`: an object of span facts. An `lpp.request` span carries `method`,
    `requestBytes`, `responseBytes` (line bytes without the terminator), and
    `error` when the exchange failed. The root carries `exitCode`.

The root span is named after the command (`compile` or `check`). Its direct
children are the top-level phases, so their durations account for the
root's duration minus unrecorded gaps. Project loading is part of the
`provider.compile` span: the provider loads the project during its compile
request, and Wright does not time inside the provider.

The text form writes an indented tree: a `trace (wright-trace/v1)` header,
the environment facts, then each span with its start offset, duration,
status, and facts. Its layout is for people and is not a stable contract;
agents use the JSON form.
