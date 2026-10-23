# ADR-0037: The structured editor moves to an experimental web application over a WebAssembly Wright core

- Status: Accepted
- Date: 2026-10-23
- Related: [ADR-0020](0020-native-agent-tool-adapter.md), [ADR-0029](0029-structured-statement-edits.md), [ADR-0032](0032-structured-editor-form-factor.md), [ADR-0033](0033-simulate-operation.md), [ADR-0036](0036-ui-dependency-policy.md), [Agent contract](../agent-contract.md), #839, #840

## Context

ADR-0032 ships the structured editor as `wright ui`: a local web UI whose assets are embedded in the `wright` binary. Two observed costs follow from that form factor:

- Every UI change reaches users only through a new `wright` release, and a user must upgrade the CLI to get a change that touches no Wright behavior.
- The editor is only reachable by people who install a CLI. Players who write native Workshop in the in-game editor do not use a CLI. For them, the offline environment (edit and simulate without the game client, then paste into the game) is the main value, and Wright should stay invisible.

The goal already states that the environment ultimately runs in the browser and that the semantic core stays buildable for WebAssembly. #839 plans a browser host. The pieces below `wright` are ready for it: `workshop-rs` and `athena-sim` both run a `wasm32-unknown-unknown` check in CI. `wright-analyzer` and `wright-transform` depend only on `workshop-rs` and pure data crates. `wright-driver`, where `ToolService` lives, is not buildable for WebAssembly: it reads and writes files directly and depends on `wright-lpp` (stdio provider processes) and `reqwest`.

The editor is an experiment. It must be able to change fast, and to stop, without affecting `wright`.

## Decision

1. **The editor is a separate web application in a new `editor` repository, served at `web.wrightkit.dev`.**
   - It is a static site with its own release and deployment cadence, independent of `wright` releases.
   - It is experimental and makes no stability promise.
   - The same static build runs online and offline once cached. Local use needs no `wright` installation.

2. **`wright` publishes a WebAssembly build of its core.**
   - The core is `ToolService` over Workshop text held in memory. It takes `wright-agent/v1` request bodies and returns the service's result bodies unchanged.
   - It adds no contract of its own: `wright-agent/v1` and its committed schema are the interface.
   - It excludes LPP providers, network access, file system access, and process spawning. OPY and DEL inputs are refused with the owning implementation named, as `editStatements` refuses them today.
   - It includes `simulate` (ADR-0033); `athena-sim` already builds for WebAssembly.
   - Each core build is versioned with the `wright` release that produces it. The distribution channel (npm package or release asset) is chosen in the implementation issue.
   - CI builds the core for `wasm32-unknown-unknown`. The scenario tests and the `editStatements` round-trip tests run against the WebAssembly build as well as the native build.

3. **The core also provides the host write path.** ADR-0032 decision 5 assigns the write path to the host, not to the page. In the browser the core is the host:
   - it re-checks `inputIdentity` against its in-memory sources;
   - it applies the transaction an `editStatements` result returned;
   - it reloads the session.

   A stale identity is reported and nothing changes. This is host behavior, not a new `wright-agent/v1` operation.

4. **The editor pins one core version per editor release.**
   - Upgrading the core is an editor release. There is no user-selectable core version.
   - The editor shows the core version it runs, because its diagnostics can differ from a different local `wright` version.

5. **The editor owns no Workshop semantics.** ADR-0032 decisions 2, 3, 7, and 8 carry over unchanged: raw Workshop scope, an adapter over `wright-agent/v1` bodies, the Svelte 5 stack, and types generated from the committed schemas. ADR-0036 carries over as the editor's dependency policy.
   - The editor depends on the core package only. It does not depend on `workshop-rs`, `athena`, or their data directly, and its CI enforces that.

6. **Projects live in browser storage.**
   - A project enters by pasting or importing Workshop text and leaves by copying or exporting Workshop text, which matches the in-game workflow.
   - Reading and writing a local file directly through the File System Access API is an optional enhancement where the browser supports and grants it. It is never required.

7. **Workshop data is versioned with the core.**
   - The first core build embeds the catalog and locale data, as `workshop-rs` does today.
   - The first implementation issue measures the transfer size of the core and its data and sets a budget for them. The ADR-0032 budget of 100 KB gzipped continues to apply to the editor's JavaScript and CSS only.
   - If the measurement shows a need, loading data on demand (per locale, or deferring descriptions and gameplay data) is a separate `workshop-rs` decision, because data is compiled into that library today. Loaded data always has the same version as the core. There is no independently updated data feed.

8. **`wright ui` retires when the editor reaches parity with it.**
   - Parity means the read view, statement editing, and the simulation panel work in the editor on `practice-range`. A manual check records it on the tracking issue.
   - Until then `wright ui` keeps working unchanged.
   - Retirement removes `wright ui`, the `ui/` directory, the `ui` Cargo feature with its `tiny_http` and `getrandom` dependencies, the UI CI job, and `docs/ui/`, which moves to the `editor` repository.

9. **Order of work.** Each step leaves a working product.
   1. Build the WebAssembly core in `wright`, with the CI check and the size measurement.
   2. Create the `editor` repository from `ui/`, replacing the loopback host calls (`call` and `apply` in `ui/src/lib/api.ts`) with the core package.
   3. Record the parity check.
   4. Retire `wright ui`.

This decision supersedes ADR-0032 decisions 1 (form factor), 4 (loopback endpoint), 5 (host on disk), 6 (ownership), and 9 (build and distribution) once the steps that replace each of them land.

## Alternatives considered

- **Keep the editor in `wright` and add a browser host beside `wright ui`.** Rejected. Online users would get fast updates, but local users would still upgrade the CLI for UI changes, and two hosts would have to stay equivalent.
- **Local `wright ui` over the local `ToolService`, plus an online WebAssembly host.** Rejected for the same reason: the local path stays bound to the CLI release whenever the UI needs a newer operation.
- **A hosted page that calls the user's local `wright`.** Rejected. The page and the local binary drift in version, so `wright-agent/v1` would need version negotiation. The loopback endpoint would have to accept a cross-origin page, which reopens the DNS-rebinding and cross-site protections of ADR-0032 decision 4 and makes the endpoint a public transport.
- **A separate repository whose assets `wright` still embeds.** Rejected. It keeps the release coupling, and the embedding creates a release cycle between the two repositories.
- **Serving the editor at `wrightkit.dev/editor`.** Rejected. It shares response headers, Service Worker scope, and the build with the product website, so editor releases would wait on the website.
- **A user-selectable core version.** Rejected. Each editor build would have to support several cores, and the test matrix would grow with no demonstrated need.
- **A data feed updated independently of the core.** Rejected. Catalog identities and schema must match the code that reads them.

## Consequences

- UI changes reach users without a `wright` release. Players use the editor without installing anything.
- `wright-driver` needs an in-memory input path for `ToolService`, separated from file, provider, and network access. That work is required by the goal's WebAssembly commitment whether or not the experiment continues.
- If the experiment stops, the `editor` repository and the subdomain are retired. `wright` keeps the WebAssembly core.
- A local `wright` and the editor can report different diagnostics when their core versions differ.
- The online editor is a network deployment of AGPL-3.0 software and must offer its source to users.
- #839 is rescoped: the browser host is the `editor` repository, not a host inside `wright`. #840 follows.
- `docs/architecture/` and `docs/cli/` change when `wright ui` retires, not before.

## Compatibility impact

No syntax, diagnostic, normalized-output, or semantic behavior changes. `wright-agent/v1` is unchanged; the WebAssembly core carries its bodies without wrapping them. Retiring `wright ui` removes a CLI command; the release that removes it states that in its changelog. Edits still pass through `editStatements` transactions with an `inputIdentity` check, so the editor cannot apply an edit Wright did not validate.

## Scope boundaries

This ADR does not decide:

- the distribution channel of the core package;
- per-locale or on-demand data loading in `workshop-rs`;
- the File System Access interaction design;
- OverPy or OSTW source editing, or a browser view of compiled source-language projects;
- accounts, sharing, or project hosting, which the goal excludes;
- how the online editor offers its source under AGPL-3.0, which needs the owner's decision and is not legal advice here.
