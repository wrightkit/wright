# OPY Semantic Compatibility Manifest — Specification

Status: accepted specification (planning) — machine-readable compatibility
contract for the proactive OPY baseline (#106)
Scope: the smallest useful Wright-owned representation for builtin
actions/values, member functions, signatures, parameter enum domains, enum
members, and source aliases; reference-validated and consumed by the native
frontend. This document specifies the manifest; the generator/ingestion
implementation is a separate bounded child issue and is not implemented here.

## Purpose and boundary

The investigation (#106 inventory) concluded that a machine-readable,
Wright-owned manifest is justified: Wright's parse surface already exceeds its
semantic/compile surface, and the residual `unknown-action`/`unknown-value`/
`unsupported-member` emission gaps are catalog-coverage gaps, not grammar gaps.
The manifest replaces the hardcoded `KNOWN_ENUMS` table in
`crates/wright-opy/src/lower.rs` with data and gives the frontend a single,
reference-validated source for:

* builtin actions and values (generic and member);
* member-function metadata (receiver + argument signatures);
* signatures, argument names/order, and defaults;
* parameter enum domains (`Invis`, `Status`, `Transform`, `Throttle`, …);
* enum members per domain;
* source aliases (old function names, contextual aliases such as
  `ChaseReeval`).

It is **language-compatibility metadata**, distinct from:

* `crates/wright-workshop/src/catalog/data/catalog.json` — the Workshop
  emission/localization layer (en-US spellings, emitter output); the manifest
  links to it by canonical id rather than duplicating spellings; and
* issue #96's runtime content registry (heroes/maps/abilities content data,
  extension boundaries, independent version identities) — deferred; this
  investigation found no architecture trigger that requires reopening #96.

## Data model (schema v1 sketch)

```jsonc
{
  "schemaVersion": 1,
  "reference": {
    "name": "overpy",
    "version": "9.7.10",
    "contentCommit": "889d974",
    "integrity": "sha512-oX17nauJcPTaKIrRFY/rD0Rl8atqFUVv9Hg2TKH+A68/fC8+ZO344Mkd1A/Y0oOVp1hr5tktMBjzMEDDnMEYUw=="
  },
  "functions": [
    {
      "id": "chaseOverTime",              // manifest id; Wright-owned spelling
      "kind": "action",                   // action | value | memberAction | memberValue
      "params": [
        { "name": "variable", "type": "Variable" },
        { "name": "destination", "type": ["float", "Vector"] },
        { "name": "duration", "type": "float", "default": null },
        { "name": "reevaluation", "type": "enum", "domain": "ChaseTimeReeval" }
      ],
      "catalogId": "chaseOverTime"        // link to workshop catalog.json when emission is supported
    }
  ],
  "enumDomains": [
    {
      "domain": "ChaseTimeReeval",
      "members": ["NONE", "DESTINATION_AND_DURATION"],
      "emission": { "catalogDomain": "ChaseTimeReeval", "memberSpelling": "upstream-canonical" }
    }
  ],
  "aliases": [
    { "source": "stopChasingVariable", "target": "stopChasing", "kind": "functionAlias" },
    { "source": "ChaseReeval", "target": "contextual", "kind": "callContextAlias",
      "context": { "call": "chase", "arg": "reevaluation", "rate": "ChaseRateReeval", "duration": "ChaseTimeReeval" } }
  ],
  "provenance": {
    "generator": "wright-opy-catalog-gen",   // planned; not yet implemented
    "reviewed": true,
    "license": "AGPL-3.0-or-later"
  }
}
```

Entries carry the minimal semantic data the frontend needs to resolve names,
check arity, resolve enum domains, and lower; they deliberately omit upstream
description/localization text.

## Data provenance and licensing rule

The manifest is **Wright-authored data validated against observed oracle
behavior** — the same path used by the existing chase-enums fixtures and the
`wright-catalog-gen` pipeline. It must not be produced by mechanically
converting OverPy's GPL-3.0 TypeScript data files (`src/data/*.ts`) into the
manifest: ADR-0004 and `docs/licensing.md` forbid importing OverPy
implementation details into the core, and observed behavior through documented
compatibility tests is the permitted input. Every entry (or generated batch)
records the reference probe fixture/hash that validates it.

## Validation rules (planned pipeline)

* `check` — schema validation, duplicate/colliding ids, colliding or missing
  aliases, undeclared enum members, and entries lacking oracle evidence all
  fail deterministically (mirroring `wright-catalog-gen`).
* `build` — deterministic canonical rewrite; re-running is byte-idempotent.
* Reference validation — systematic probe inputs run against the pinned
  oracle record accept/reject, normalized emission, and diagnostics; Wright
  runs the same probes and compares at the S/D level (see the validation
  strategy in the #106 planning comment).

## Consumers

* `wright-opy` — name/member/enum resolution, arity and signature checks,
  `KNOWN_ENUMS` absorption, earlier resolution of unknown-action/value errors
  (addressing the diagnostic-provenance limitation);
* `wright-workshop` — canonical-id linkage to the emission catalog;
* differential and systematic reference tests;
* documentation, agents, and future release metadata can consume the same
  declared boundary.

## Non-goals

* Implementing the generator/ingestion pipeline in this issue (bounded child
  issue, pending PM review).
* A runtime-downloadable or hot-updating content registry (#96).
* Workshop content data (heroes/abilities/maps) as manifest entries.
* Preserving upstream implementation structure for its own sake.
