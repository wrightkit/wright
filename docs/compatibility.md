# Wright Compatibility Contract

Status: accepted baseline (semantic compatibility priority, ADR-0008/ADR-0009)
Scope: measurable compatibility claims for the Wright tooling and compiler core

Compatibility is a claim about a named input corpus, reference version,
configuration, comparison method, and evidence set. A successful build, an HTTP
health check, or similar infrastructure evidence does not establish compiler
compatibility.

## Reference boundary

Wright owns tooling and orchestration, not the durable source-language
implementations. `opy-rs` owns OPY language semantics, `del-rs` owns the
DEL/OSTW-compatible implementation, and `workshop-rs` owns canonical Workshop
semantics and WIR. During the migration described by ADR-0009, Wright still
contains the in-repo `wright-opy` / `wright-ostw` migration frontends and the
`wright-workshop` re-export adapter until their planned cutovers complete.
Wright therefore keeps regression coverage for those current integration paths,
but upstream OverPy and OSTW compilers/language services remain compatibility
oracles and behavior references rather than production or default-CI runtime
dependencies.

Live reference acquisition and authoritative language compatibility evidence
belong to the language owner:

- `opy-rs` owns the pinned OverPy oracle, OPY corpus, oracle runner,
  differential expectations, and OPY support matrix;
- `del-rs` is the durable owner for pinned OSTW reference evidence and
  DEL/OSTW compatibility claims;
- Wright may keep immutable recorded snapshots required by its current
  migration/provider/product regressions, but those snapshots do not make
  Wright the semantic or oracle owner.

Project-level provenance for upstream implementations referenced by Wright is
recorded in
[`compatibility/upstream-references.md`](compatibility/upstream-references.md)
while the corresponding language repositories maintain their authoritative
language-side provenance and evidence.

The reference for every compatibility result must record:

* the reference version or immutable source revision;
* the corpus or fixture identifier and input hash;
* Wright's version or commit and relevant configuration;
* target/runtime versions when execution is involved;
* the normalization and comparison method; and
* the result, including unsupported or inconclusive outcomes.

## Compatibility levels

The levels below measure different contracts. They are cumulative for a single
claim only when the evidence explicitly covers the same corpus and supported
subset; passing a lower level never implies passing a higher one.

### S: syntax compatibility

Wright and the reference agree on whether each corpus input is accepted by the
supported frontend boundary, and accepted inputs are classified into the same
documented supported or unsupported subset.

Minimum evidence:

* the input identity and reference result are recorded;
* accept/reject outcomes are compared; and
* unsupported cases have an explicit Wright diagnostic or documented reason.

S-level evidence does not prove that accepted programs have the same meaning
or output.

### D: diagnostic compatibility

For inputs that are rejected or otherwise diagnosed, Wright reports the same
documented diagnostic category and relevant source region as the reference, or
records an intentional, reviewed difference. Stable diagnostic codes and
locations are normative; human-readable wording is not required to be byte for
byte identical unless a fixture explicitly says so.

Minimum evidence includes structured diagnostic comparisons and malformed or
unsupported-input cases, not only successful examples.

### N: normalized-output compatibility

For accepted inputs, Wright's produced artifact is compared after applying a
versioned normalization procedure that removes only documented presentation or
volatile differences. Normalization must not erase semantic operations,
references, control flow, or values whose differences affect behavior.

The comparison result must identify the normalizer version and either provide a
canonical artifact hash or a reviewable diff. Exact source-text equality is
neither required nor sufficient unless the output contract specifically makes
it normative.

### E: semantic compatibility

Wright and the reference exhibit equivalent observable behavior for a defined
scenario set, target/runtime, and execution environment. The scenario set must
state its treatment of randomness, time, external state, errors, and other
sources of nondeterminism.

Minimum evidence includes repeatable scenario results, the environment and
runtime identity, and an explanation for any intentionally unobservable or
implementation-defined behavior. N-level output comparison alone is not
E-level evidence.

## Compatibility priority

The four-level framework (S/D/N/E) measures different contracts. Their
**priority order** is:

> **E (observable semantics) > D (diagnostics) > S (syntax) > N (text output)**

Byte-identical output, identical temporary-variable allocation, identical
optimizer output, or identical formatting are not goals unless a difference
affects:

- observable Workshop or game behavior;
- valid Workshop syntax or native Workshop round-trip behavior;
- source or tooling contracts; or
- an explicitly documented compatibility surface.

N-level evidence remains a useful regression-detection tool, but
presentation-only N-level differences must not automatically create
implementation work. See [ADR-0008](adr/0008-tooling-first-semantic-platform.md).

## Release gates

Every release or milestone claim should state the highest level it covers and
the exact corpus scope. The gates are:

| Gate | Required evidence | Does not prove |
| --- | --- | --- |
| S | Syntax corpus with recorded reference outcomes | diagnostics, output, or behavior |
| D | Structured diagnostic fixtures, including failure cases | output or behavior |
| N | Versioned normalizer and reviewable canonical comparisons | runtime behavior |
| E | Repeatable behavior scenarios with target/runtime identity | compatibility outside the scenario set |

A claim is blocked, not passed, when its corpus, reference version, comparison
method, or environment is missing. Inconclusive and unsupported cases must be
reported rather than counted as successes.

## Fixture and corpus rules

Wright's [`../compatibility/`](../compatibility/) directory contains consumer
regression evidence for the language/integration paths still present in this
repository. Its historical `oracle.json` files are immutable recorded reference
results, not a Wright-owned live oracle.

Run the repository-local integrity checks with:

```sh
python3 -m unittest discover -s compatibility/tests
```

These checks verify fixture identity, source hashes, expected status, recorded
output hashes, and provenance without installing an upstream language runtime.

Reference evidence changes must begin in the owning language repository. For
OPY, refresh and review the pinned OverPy evidence in `wrightkit/opy-rs`, then
import only the immutable result needed by a Wright consumer regression. The
legacy OSTW reference harness currently remaining under
`compatibility/ostw/` is migration state and must move to `wrightkit/del-rs`
before Wright removes it.

Fixtures containing third-party code, generated output, or user data require a
redistribution and provenance review before being committed. When a fixture
cannot be redistributed, the owning repository should record a generator, hash,
or acquisition instruction instead of shipping the content here.

## Open questions

The following remain unresolved until the relevant implementation exists:

* the final Wright-side evidence retained after the OPY and DEL provider
  cutovers;
* the machine-readable diagnostic schema and stable code registry;
* the canonical Workshop output normalizer and its versioning policy;
* the target/runtime used for semantic scenarios; and
* the corpus licensing and local-generation process for future Wright-owned
  product scenarios.

These questions must not be silently answered by a compatibility shortcut.
Record a decision in `docs/adr/` when implementation evidence is available.

## Related decisions

* [ADR-0002: Compatibility strategy](adr/0002-compatibility-strategy.md)
* [ADR-0003: IR boundary](adr/0003-ir-boundary.md)
* [ADR-0004: OverPy licensing and clean-room boundary](adr/0004-overpy-licensing-boundary.md)
* [ADR-0008: Tooling-first semantic platform rebaseline](adr/0008-tooling-first-semantic-platform.md)
* [ADR-0009: Language ownership and licensing boundaries](adr/0009-language-ownership-licensing-boundaries.md)
