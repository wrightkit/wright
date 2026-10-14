# Development priorities

Real user workflows are the primary evidence. When `wright check`, `lint`, or
`analyze` fails on a real project:

1. reproduce the failure;
2. identify the owning layer;
3. fix source-language semantics in `opy-rs` / `deltin-rs`, canonical Workshop
   semantics in `workshop-rs`, or integration/tooling behavior in Wright;
4. keep the full-project regression and add a minimized test where practical;
5. do not substitute architecture cleanup or support-matrix bookkeeping for a
   usable workflow.

Architecture work remains important at public/versioned contracts, repository
ownership boundaries, provenance/source-edit correctness, and dependency
direction. Most internal structure is revisable implementation detail.

Ownership and dependency direction are defined in
[`../architecture/ownership.md`](../architecture/ownership.md).
