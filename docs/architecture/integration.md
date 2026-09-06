# Wright Language Integration Contract

Wright integrates independently usable language/Workshop implementations into one product surface. Integration must preserve owner semantics rather than reimplement them.

## Integration flow

For a source-language workflow:

```text
user-selected source / entry
   ↓
Wright integration seam
   ↓
owning implementation
   ├─ project/source discovery
   ├─ syntax + semantic diagnostics/queries
   └─ source-specific compiler/lowering
          ↓
      workshop-rs canonical Workshop
          ↓
      Wright tooling / presentation / CI
```

Raw Workshop enters through `workshop-rs` directly.

Wright may consume native Rust APIs and/or LPP depending on the product boundary. Provider is an integration role, not repository identity and not semantic ownership.

## Contract preservation

Integration must preserve structured diagnostics, source provenance, semantic identities, and explicit unsupported/failure states available from the owner.

Do not silently:

- fall back to upstream OverPy/OSTW runtimes for a declared first-party workflow;
- reinterpret unknown source constructs inside Wright;
- duplicate Workshop catalog/WIR/settings/localization data;
- invent Wright-only OPY/DEL syntax;
- turn provider transport data into a new semantic authority.

A temporary adapter representation may exist during migration, but its current shape is implementation reality rather than durable architecture unless explicitly promoted to a versioned contract.

## Failure routing

Classify the owner before changing code:

- OPY syntax/preprocessing/semantic/compiler/reconstruction gap → `opy-rs`;
- DEL/OSTW project/type/runtime/compiler/reconstruction gap → `deltin-rs`;
- canonical Workshop parser/WIR/catalog/settings/localization/validation/emission gap → `workshop-rs`;
- LPP framing/version/conformance gap → `language-provider-protocol`;
- Wright orchestration, lint/analysis/edit/agent/CI/LSP/presentation integration gap → Wright.

If the owner implementation is correct but Wright misuses or loses its contract, fix Wright. If the owner capability does not exist, do not fabricate it in the integration layer.

## Real-project verification

An integration fix is not complete merely because a unit/provider test passes. Re-run the real workflow that exposed the failure where applicable and preserve project/revision/path provenance.

For language-specific claims, verify both the owner-side capability and the Wright integration path. A Wright-level green result cannot upgrade an owner support claim by itself.

## Process boundary

LPP is a neutral process/data contract. Conformance proves protocol behavior, not semantic completeness. Provider versions, current result mapping strategy, and migration/cutover state are dynamic implementation facts and do not belong in this durable contract.