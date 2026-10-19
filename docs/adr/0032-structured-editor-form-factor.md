# ADR-0032: The structured Workshop editor: form factor, ownership, and stack

- Status: Accepted
- Date: 2026-10-19
- Related: [ADR-0020](0020-native-agent-tool-adapter.md), [ADR-0022](0022-shared-surface-for-semantic-queries.md), [ADR-0024](0024-structured-rule-content.md), [ADR-0029](0029-structured-statement-edits.md), [ADR-0031](0031-paginated-vocabulary-listing.md), [Agent contract](../agent-contract.md)

## Context

ADR-0029 names a structured view as a consumer of statement edits. It leaves the form factor of an editing view undecided, and ADR-0031 adds the listing that view needs for its dropdowns. A prototype now covers the interaction end to end against a real project (`practice-range`, 139 rules):

- In-game-style rule list, event row, and condition and action columns, with syntax-coloured statements.
- A click on a statement opens its editor. A click on an argument opens the editor at that argument.
- Recursive argument forms built from `lookup` facts: variables, strings, enums, numbers, and nested calls.
- Dropdown suggestions from the project's own usage.
- Pending edits per statement address, disable toggles, and the `editStatements` request they produce.

The prototype is about 1,200 lines of plain JavaScript behind a Python bridge to `wright serve`. It shows what a real implementation needs:

- The editor's state is a deep tree of `rule-content-v1` nodes that forms mutate in place. Without reactivity the prototype rebuilds the whole form after every change, then restores focus and scroll by hand.
- Several defects came from untyped nodes. One built `{"call":"true"}` for a literal. Another treated a catalog default string as a call id. The `wright-agent/v1` and `rule-content-v1` schemas are committed, but the prototype cannot use them.
- A browser cannot speak to `wright serve` over stdio, so something has to carry requests and write accepted edits to disk.

Goal principle 1 makes `wright` the product the user runs. The goal's non-goals exclude a full IDE or a heavy UI product, and principle 10 asks for a small, predictable core.

## Decision

1. **Form factor: a local web UI started by `wright ui [INPUT]`.**
   - The command binds one project, like `wright serve`. `INPUT` defaults to the current directory.
   - It serves the UI on a loopback address and opens it in the default browser.
   - It is not a hosted service, an editor extension, or a desktop shell. Those need their own distribution channel, and no user workflow needs them yet.

2. **Scope: raw Workshop projects, the same scope as ADR-0029.**
   - The UI shows rules, events, conditions, and actions from `rules` content (ADR-0024), and edits statements through `editStatements`.
   - Other inputs are refused with the owning implementation named, as `editStatements` refuses them.
   - Rule-level edits and declarations stay out until the operations behind them exist.

3. **The UI is an adapter over `ToolService`, like the MCP adapter (ADR-0020).**
   - The browser sends `wright-agent/v1` request bodies to one local endpoint and receives the service's result bodies unchanged.
   - The UI implements no Workshop semantics. It has no text emission, no name matching, and no validation of its own. Missing semantics are fixed in the owning operation or owner, not in the UI.
   - The endpoint belongs to `wright ui`. It is not a public transport and has no contract of its own beyond the `wright-agent/v1` bodies it carries.

4. **The local endpoint accepts only its own page.**
   - The server binds `127.0.0.1` on an ephemeral port.
   - At startup it creates a random session token and passes it to the page in the URL fragment it opens. Every request must carry the token.
   - Requests whose `Host` or `Origin` is not the server's own loopback origin are refused, which blocks DNS rebinding and cross-site requests. The server sends no CORS headers.

5. **`wright ui` writes accepted edits; the page never touches files.** `editStatements` returns a validated transaction and writes nothing (ADR-0029). When the person submits, the host:
   - re-checks `inputIdentity` against the files on disk;
   - applies the transaction;
   - reloads the session.

   A stale identity is reported to the page and nothing is written. This is host behavior of `wright ui`, not a new `wright-agent/v1` operation.

6. **Ownership: the `wright` repository, in a top-level `ui/` directory.** The UI is user-facing tooling, and its assets ship inside the `wright` binary, so it does not get its own repository. Keeping it outside `crates/` keeps the Rust workspace free of Node tooling.

7. **Stack: Svelte 5 in runes mode, TypeScript in strict mode, and Vite.**
   - **Svelte.** `$state` makes a deep tree of plain objects and arrays reactive in place, which matches how the editor mutates `rule-content-v1` nodes. `bind:` and component-scoped styles remove most of the prototype's hand-written form and style plumbing. The runtime is a few kilobytes, so size does not choose between small frameworks.
   - **Runes only.** Runes mode is set for the whole project in the compiler options, so the compiler rejects Svelte 3/4 component syntax, and `svelte-check` runs in CI.
   - **Snapshots at the boundary.** State proxies are not plain objects. Every value that leaves the reactive tree passes through `$state.snapshot` first: request bodies, copies, and anything handed to `structuredClone`.
   - **Dependencies.** The only runtime dependency is `svelte`. No component library, CSS framework, router, or state library. A new runtime dependency needs a demonstrated need recorded in its PR.
   - **Rejected stacks.** React was rejected as heavier than the problem needs. Preact with signals was the close alternative. It is equally small, but editing a deep tree needs either immutable path updates or one signal per node.

8. **Types come from the committed schemas.**
   - TypeScript types for requests, results, and rule content are generated from `schemas/wright-agent-v1.schema.json` and from `workshop-rs`'s `rule-content-v1.schema.json` at the pinned `workshop-rs` version.
   - Generated types are rebuilt in CI, and a stale copy fails the build.
   - No request or node type is written by hand.

9. **Build and distribution.**
   - `npm ci && npm run build` in `ui/` produces static assets. The assets are not committed.
   - A `ui` Cargo feature of `wright-cli` embeds them. Release builds enable the feature after building the assets.
   - Builds without the feature compile `wright ui` as a command that reports that this build has no UI and exits 2. Rust-only contributors and `cargo test` therefore need no Node toolchain.
   - The release archives, installers, and package-manager channels are unchanged: they carry the same single binary.

10. **Verification.**
    - CI gains one UI job: `svelte-check`, unit tests, the production build, and a size budget of 100 KB gzipped for JavaScript and CSS together. A larger bundle fails the job.
    - An end-to-end test drives `wright ui` on a committed raw Workshop fixture in a headless browser. The test opens a rule, edits a statement through the form, submits it, and checks that the file on disk equals the edit `editStatements` validated.

11. **Delivery in layers.**
    - The first release is read-only: the rule list, the event row, and coloured conditions and actions.
    - Statement editing follows once `editStatements` ships.
    - Full dropdowns follow once `listEntries` ships.
    - Each layer is usable on its own. The prototype is a reference for interaction and look, not code to port.

## Alternatives considered

- **A VS Code webview.** Rejected for now. It ties the editor to one IDE, and Wright's IDE integration is the language server (ADR-0023), which a webview would sit beside rather than reuse. The same assets could later load in a webview without changing this decision.
- **A desktop shell (Tauri or Electron).** Rejected. It means another binary and another distribution channel, and Electron contradicts the size goal.
- **A Rust/WASM UI framework (Leptos, Dioxus).** Rejected. The UI must use the `wright-agent/v1` boundary rather than link driver crates, so Rust types are not shared anyway. It also brings a heavier toolchain and a smaller ecosystem.
- **Plain JavaScript, as in the prototype.** Rejected for the reasons in Context: no reactivity and no types.
- **Committing the built assets so every Cargo build embeds them.** Rejected. Generated bundles in Git produce noisy diffs and can drift from their sources.
- **A public HTTP transport on `wright serve`.** Rejected. No consumer other than this UI needs it, and a public transport would need its own security and compatibility contract.
- **A separate `wright-ui` repository.** Rejected. It would ship inside `wright` anyway, and splitting it would make every contract change a cross-repository change.

## Consequences

- People get an in-game-like editor for their own project from the `wright` they already install, with no extra download.
- The `wright` repository gains a Node toolchain, a lockfile, and a CI job. Dependabot covers `ui/`.
- Every UI capability depends on a `ToolService` operation, so gaps the UI finds surface as owner or operation issues, as the prototype's did (#781, #782, #798, and wrightkit/workshop-rs#503, wrightkit/workshop-rs#504, wrightkit/workshop-rs#505, wrightkit/workshop-rs#506).
- Follow-up work:
  - `wright ui` with the local endpoint and the read-only view.
  - The UI CI job and the end-to-end test.
  - Edit submission once `editStatements` ships.
  - Documentation in `docs/cli` and a UI guide.

## Compatibility impact

No syntax, diagnostic, normalized-output, or semantic behavior changes. `wright-agent/v1` is unchanged: the UI carries its bodies without wrapping them. The new CLI command is additive. File writes go through `editStatements` transactions with an `inputIdentity` check, so the UI cannot write an edit Wright did not validate.

## Scope boundaries

This ADR does not decide:

- OverPy or OSTW source editing, or a read-only view of compiled source-language projects;
- rule-level edits, declarations, or workshop settings editing;
- a VS Code webview, a hosted UI, or sharing projects between users;
- the visual design system beyond following the in-game editor;
- localization of the UI chrome beyond the Workshop locales the catalog already provides.
