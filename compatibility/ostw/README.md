# OSTW reference oracle

This directory is evaluation-only infrastructure. The OSTW binary is downloaded
to `target/ostw-reference/`; it is neither committed nor a Cargo dependency.

`reference.json` pins the release tag, immutable tag commit, release asset size,
and SHA-256. `latest` is never an evidence identity.

```sh
python3 compatibility/ostw/run_oracle.py --acquire --ping
python3 compatibility/ostw/run_oracle.py --acquire --update
python3 compatibility/ostw/run_oracle.py
python3 compatibility/ostw/run_oracle.py --probes
```

The runner drives `Deltinteger --langserver` using Content-Length framed JSON-RPC.
It opens a project workspace so `ds.toml` is visible, records diagnostics and the
custom `workshopCode` / `elementCount` notifications, and writes deterministic
JSON evidence. It never invokes the clipboard-bound default compiler path.

## Explicit compile/document roots

Pinned P1 evidence (#118) established that the upstream LSP compiles the
**last-opened document plus its transitive import closure**; `ds.toml.entry_point`
is not the LSP compile selector. Every recorded observation is therefore
produced by a session that opens exactly one document — the observation's
explicit `root` — so the result can only be that root's compile and can never
acquire meaning from `didOpen` ordering. `corpus.json` lists the reviewable
`roots` (with `entry-root` / `document-root` / `historical-document-root`
roles) per project; `results.json` (schema v2) records one observation per
root: accept/reject, `elementCount`, full source-located diagnostics, the
import-closure identity, and missing-import boundaries.

`probe.json` manifests may designate a probe as `differential-target`; the
runner aggregates accepted targets under `differentialTargets` in
`probes/results.json` and refuses to list a target the pinned reference
rejects. These are the immutable, reference-accepted forward-compilation
targets for #119.

## Determinism

The langserver debounces compiles ~50 ms after the last `didOpen` and publishes
one coherent `workshopCode`/`elementCount`/`publishDiagnostics` triple per
compile. The runner drains until the server is quiet (`QUIET_SECONDS`, default
3 s) and records only the LAST compile triple — deterministic because the open
set is explicit and fixed per observation. A session that drops mid-stream
(transient container failure) is retried; the recorded triple is unchanged.

The corpus runner re-run twice produces byte-identical `results.json`; running
it without `--update` fails with `OSTW_ORACLE_DRIFT` when the recorded
evidence no longer reproduces under the pinned reference. That drift check is
a manual maintainer command, not a Wright CI gate: the upstream-reference CI
job was removed in #177, and future oracle reproducibility workflows belong to
`del-rs` (del-rs#49, tracked in Wright by #182). Wright CI still consumes the
recorded evidence through the #119 compile differential; it does not re-derive
it. `accept`/`reject` is derived from the final `elementCount` (`>= 0` means
the reference produced Workshop code; `-1` means it reported errors).
