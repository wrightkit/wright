# Execution trace contract (#1079)

- Status: stable, versioned trace format
- Contract: `wright-trace/v1`
- Owner: the `wright` CLI renders the trace; `wright-driver::trace` records it.

`wright compile` and `wright check` accept `--trace[=FORMAT]`, where `FORMAT`
is `text` or `json`. A bare `--trace` means `text`. The value must follow `=`,
so `--trace=json` is the only form that selects `json`. The space form is not a
format: `json` is read as the input path. With no input argument, `wright check
--trace json` checks the input `json` in the current directory and fails with
`input-io` (exit 1), and the trace of that failed run is written. With an input
argument, `wright check --trace json <input>` fails with an unexpected-argument
usage error (exit 2). The trace
goes to standard error. It never changes standard output, the exit code, the
diagnostics, or any `wright-result/v1` payload. Without the flag nothing is
recorded and nothing extra is written.

Standard error can carry more than the trace. In the default text result
format, the human diagnostics are written to standard error before the trace
lines. To read the trace alone, with every line parsing as JSON, run
`wright <command> -f json --trace=json`. In that form standard error holds
only trace lines on success. A usage error (exit 2) writes its message to
standard error, without trace lines.

If the process panics, the spans are closed on unwind but the trace is not
rendered. A panicking run writes no trace.

Facts that name a path replace the user's home directory prefix with `~`, so
`providerPath` becomes `~/Library/...`. Other absolute paths are written as
they are and may still appear, for example a provider installed outside the
home directory. Review the output before publishing it.

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
children are the top-level phases. Parallel phases such as `source.read` and
`parse` are siblings and may overlap, so summing their durations overcounts
the root's elapsed time. Project loading is part of the `provider.compile`
span: the provider loads the project during its compile request, and Wright
does not time inside the provider.

`source.read` measures the concurrent source-map member reads, including
worker startup, and carries `files` and successfully read `bytes`.
`source.apply` measures retaining the read text in the mapped program after
parse. Only the part of the read span extending past parse, its launch cost,
and the apply span add to the critical path.
`provider.shutdown.request` sends `lpp/shutdown` and closes stdin after the
last source-provider call, so the provider exits while Wright finishes the
workflow. `provider.shutdown` measures the remaining exit wait at session
drop, which keeps the five-second limit before killing the process. To
measure the shutdown work still on the critical path, include both the
request span and the final exit-wait span.

The text form writes an indented tree: a `trace (wright-trace/v1)` header,
the environment facts, then each span with its start offset, duration,
status, and facts. Its layout is for people and is not a stable contract;
agents use the JSON form.
