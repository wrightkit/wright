# Wright Tooling Contract

Wright owns cross-language product tooling over semantic information supplied by the owning implementations and canonical Workshop.

## Tooling surfaces

Wright owns:

- unified check/diagnostic orchestration;
- lint and static analysis;
- semantic inspect/query surfaces;
- validated source-edit/refactoring transactions;
- agent and embedding workflows;
- CI/machine-readable presentation;
- editor-neutral language services and LSP;
- Workshop stability/cost analysis;
- unified compile/conversion UX where the owning implementations support it.

Language-specific parsing, type/source semantics, runtime/compiler lowering, and reconstruction remain in their owners.

## Semantic inputs

Prefer stable semantic identities, structured diagnostics, provenance, and query contracts from the owning implementation. Do not build lint/analysis rules on parser/provider internals when a stable semantic boundary exists.

Where tooling requires canonical Workshop meaning, compose owner information with `workshop-rs` rather than adding a Wright-owned shadow semantic model.

## Source edits

The default mutation model is:

```text
semantic understanding
   ↓
validated edit intent
   ↓
source-owned ranges/provenance
   ↓
transaction-safe edits to original source
   ↓
re-check / re-analyze
```

Normal source tooling does not require full-file regeneration. Preserve comments/trivia/formatting/unchanged structure where practical. Unsupported or unsafe edits fail explicitly; do not silently degrade to textual search/replace when semantic correctness is required.

## Shared services

CLI, LSP, agent/MCP-style adapters, embedding, and CI should reuse common Wright-owned semantic/query/edit services rather than each implementing language-specific logic independently.

Machine-readable contracts are versioned and deterministic when declared. Presentation layers must not alter semantic outcomes.

## Lint and analysis evidence

Rules must distinguish exact semantic facts, static metrics, heuristics, and runtime evidence. Heuristics remain labeled and false-positive controlled. Wright must not upgrade a heuristic or community claim into a correctness diagnostic without sufficient evidence.

## Compilation and conversion

Wright owns the product UX, not the source-language compiler semantics.

Source→Workshop combines source-owner semantics/lowering with `workshop-rs` canonical validation/emission. Workshop→OPY/DEL reconstruction belongs to the respective source implementation. Direct OPY↔DEL translation is optional and should not drive architecture for symmetry.

A command may exist before every source construct is supported; capability claims must follow owner + integration evidence.