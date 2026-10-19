# ADR-0027: Project run cache and comparison against the previous run

- Status: Accepted
- Date: 2026-10-17
- Related: [Issue #723](https://github.com/wrightkit/wright/issues/723), [Issue #722](https://github.com/wrightkit/wright/issues/722), [Issue #724](https://github.com/wrightkit/wright/issues/724), [ADR-0025](0025-essential-default-output.md), [Agent contract](../agent-contract.md), [Diagnostics schema](../diagnostics-schema.md)

## Context

The Workshop client enforces a 32768-element budget, and a project close to it is optimized by repeating compile-and-measure many times. A real optimization loop on a 324-rule OPY project ran about 30 compiles (32,029 to 26,104 elements), recorded each total by hand, and could not tell which rules changed between two runs. `wright inspect cost` reports a single snapshot. A rule that grew while others shrank is invisible in the total.

Wright is stateless today: the same command on the same input gives the same output. Comparison needs a record of an earlier run. Asking a person to save a JSON file and pass it back on every invocation is not a natural interaction. An agent session may not share the caller's filesystem, so a file path is not a stable agent input either.

Established tools split this into three layers. Criterion.rs stores results under the project and compares each run with the previous one by default, offers named baselines for a fixed reference, and has a lenient mode for CI. pytest-benchmark stores runs in a project directory, compares against a saved run, and fails the suite on a threshold. The default needs no configuration, a named reference covers the case where "previous run" drifts, and the gate is a threshold that does not depend on stored history.

## Decision

### Layers

1. **The run cache is a convenience layer for people.** It never changes a command's verdict, exit code, or default JSON.
2. **The gate is explicit and does not read the cache.** `--max-total <N>` on `inspect cost` fails with exit code 1 when the element total exceeds N and names the overage. It behaves identically with no cache, so CI works without history.
3. **Comparison is requested explicitly by agents.** Default JSON and agent operation results carry no comparison. A comparison is returned only when the caller asks for it, so default output stays deterministic (ADR-0025).

### What is compared

4. **The default reference is the previous successful run of the same key.** For `compile` a successful run is one that passes; for `inspect cost` it is one that produced a count. A failing run does not replace the reference.
5. **Named baselines fix a reference.** `--save-baseline <name>` records the current run under a name; `--baseline <name>` compares against it without overwriting it. A named baseline changes only when the user saves over it or deletes it.
6. **No history.** For each key the cache holds the previous successful run and the named baselines. Older runs are not kept. History is a compatible extension if a real need appears.
7. **No reference is not an error.** With no previous run, an unknown baseline name, or an unreadable or incompatible record, the comparison is omitted. An unknown baseline name given explicitly with `--baseline` is a usage error, because the caller asked for a specific reference. *Amended under `CI` by [ADR-0030 decision 14](0030-renderers-and-the-cli-action-boundary.md): a `--baseline` with no usable record degrades to no comparison with no output instead of a usage error; outside `CI` the usage error stands.*

### Who writes

8. **`compile` and `inspect cost` write the cache.** A record holds a summary (rule names and element counts, the total, and the budget), never the compiled program. The compiled output file is unchanged by this decision.
9. **The cache can be turned off.** `--no-cache` on the command, a non-empty `WRIGHT_NO_CACHE`, or a truthy `CI` (the same check the renderer already uses for CI detection) disable both reading and writing. A CI checkout has no history to compare with and may have a read-only or discarded working directory. There is no flag to force the cache on under `CI`; it is added if a real need appears (for example restoring `.wright/` with a CI cache). The gate in decision 2 is unaffected. *Amended by [ADR-0030 decision 14](0030-renderers-and-the-cli-action-boundary.md): under a truthy `CI` the explicit `--baseline` and `--save-baseline` flags do read and write `.wright/` so a platform cache can carry baselines between runs; only the implicit previous-run record stays off under `CI`.*
10. **Cache failures never fail a command.** A read-only directory, a full disk, a corrupt record, or a concurrent write is ignored and does not change the exit code or the verdict.

### Key and layout

11. **The cache lives in `.wright/` at the project root** (`--root` if given, otherwise the current directory). It is not meant to be committed. Wright does not edit the user's `.gitignore`; documentation tells users to ignore it.
12. **One file per key.** The key is the resolved input file, the input kind, the locale, and the profile. Entries that differ in any of these are never compared with each other (a development entry point and a release entry point have different totals by design). Each file is written atomically (written to a temporary file in the same directory, then renamed), so concurrent builds of different keys cannot corrupt each other and the same key ends with one complete record.

### Shape of a record

13. **The store is generic; the payload belongs to the command.** A record carries a command kind, a schema version, and a payload. The store does not interpret the payload. Only the `cost` kind is implemented. A reader that sees an unknown kind or schema version treats the record as absent (decision 7). Comparison semantics for other commands, such as matching lint findings across runs, are separate decisions.

## Consequences

- A person sees the change since the last run on every `compile`, without learning a flag, and can pin a reference by name.
- `compile` gains a side effect (a small write under `.wright/`). The decision above limits it to a summary, makes it silent on failure, and makes it opt-out.
- The cache is local machine state. Two machines, or CI with no cache, can show different comparisons for the same input. This is why the gate and agent comparisons never depend on it implicitly.
- Documentation to update with the implementation: `docs/cli/commands.md` (`--no-cache`, `--save-baseline`, `--baseline`, `--max-total`), `docs/agent-contract.md` (the `costEstimate` comparison request and result), and a short note on ignoring `.wright/`.
- The `costEstimate` operation takes the reference as inline data supplied by the caller, not a file path, because an agent session may not share the caller's filesystem. The CLI reads the cache or a named baseline and supplies the reference to the same comparison logic, so people and agents use one implementation.
- Source locations in cost output (#722) and per-node-shape breakdown (#724) are separate and independent of this cache; a comparison can carry them when they exist.

## Compatibility impact

No source-language syntax, diagnostic, or compiled-output change. The bytes written to the compiled program file are unchanged. Cost JSON gains only additive fields (see [Diagnostics schema](../diagnostics-schema.md)), so `wright-result/v1` is unchanged. Tests required with the implementation:

- a record round-trip, and two keys that differ only in locale or profile never comparing;
- a failing compile leaving the previous reference intact;
- a missing, corrupt, or unknown-version record yielding no comparison and an unchanged exit code;
- concurrent runs of different keys leaving each record complete;
- `--max-total` behaving identically with and without a cache;
- `--no-cache`, `WRIGHT_NO_CACHE`, and a truthy `CI` each leaving `.wright/` untouched.

## Scope boundaries

- Keeping history, and any command to read it, is not decided here.
- Cross-run comparison for `lint` and `check`, including when two findings count as the same finding after lines move, is not decided here.
- A shared team baseline (for example the default branch's cost produced by CI) is not decided here. The explicit `--baseline` input is the intended route, but how a named baseline is exported or imported is a separate decision.
- Whether Wright should create or amend `.gitignore` is not decided; this ADR chooses not to.
- Element-reduction passes and the meaning of `--profile` for element count are tracked in #725.
