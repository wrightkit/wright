# Wright Compatibility Contract

Status: accepted baseline for v0.1
Scope: measurable compatibility claims for the OverPy-compatible core

Compatibility is a claim about a named input corpus, reference version,
configuration, comparison method, and evidence set. A successful build, an HTTP
health check, or similar infrastructure evidence does not establish compiler
compatibility.

## Reference boundary

Until a later native frontend milestone, existing OverPy is the `.opy`
frontend/parser and the reference oracle for supported behavior. Wright may
invoke or compare with that oracle through isolated compatibility tooling. The
Rust core remains independently implemented and must not expose OverPy's
internal representation as its API.

The reference for every compatibility result must record:

* the OverPy version or immutable source revision;
* the corpus or fixture identifier and input hash;
* Wright's version or commit and relevant configuration;
* target/runtime versions when execution is involved;
* the normalization and comparison method; and
* the result, including unsupported or inconclusive outcomes.

## Compatibility levels

The levels below measure different contracts. They are cumulative for a single
claim only when the evidence explicitly covers the same corpus and supported
subset; passing a lower level never implies passing a higher one.

### S — syntax compatibility

Wright and the reference agree on whether each corpus input is accepted by the
supported frontend boundary, and accepted inputs are classified into the same
documented supported or unsupported subset.

Minimum evidence:

* the input identity and reference result are recorded;
* accept/reject outcomes are compared; and
* unsupported cases have an explicit Wright diagnostic or documented reason.

S-level evidence does not prove that accepted programs have the same meaning
or output.

### D — diagnostic compatibility

For inputs that are rejected or otherwise diagnosed, Wright reports the same
documented diagnostic category and relevant source region as the reference, or
records an intentional, reviewed difference. Stable diagnostic codes and
locations are normative; human-readable wording is not required to be byte for
byte identical unless a fixture explicitly says so.

Minimum evidence includes structured diagnostic comparisons and malformed or
unsupported-input cases, not only successful examples.

### N — normalized-output compatibility

For accepted inputs, Wright's produced artifact is compared after applying a
versioned normalization procedure that removes only documented presentation or
volatile differences. Normalization must not erase semantic operations,
references, control flow, or values whose differences affect behavior.

The comparison result must identify the normalizer version and either provide a
canonical artifact hash or a reviewable diff. Exact source-text equality is
neither required nor sufficient unless the output contract specifically makes
it normative.

### E — semantic compatibility

Wright and the reference exhibit equivalent observable behavior for a defined
scenario set, target/runtime, and execution environment. The scenario set must
state its treatment of randomness, time, external state, errors, and other
sources of nondeterminism.

Minimum evidence includes repeatable scenario results, the environment and
runtime identity, and an explanation for any intentionally unobservable or
implementation-defined behavior. N-level output comparison alone is not
E-level evidence.

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

Compatibility fixtures are executable evidence. The repository layout,
metadata schema, pinned oracle, and runner commands are defined in
[`compatibility/README.md`](compatibility/README.md). Each fixture should make
its scope visible and should avoid relying on unrecorded local state. Its
manifest includes at least:

```text
fixture id
input hash and provenance
reference identity
Wright identity
target/runtime identity, when applicable
compatibility level
normalizer/scenario version, when applicable
expected result
```

Run `python3 compatibility/run_oracle.py` to verify normalized snapshots. Use
`--update` only for an intentional oracle update that is reviewed with its
fixture and provenance changes.

Fixtures containing third-party code, generated output, or user data require a
redistribution and provenance review before being committed. When a fixture
cannot be redistributed, the repository may record a generator, hash, or local
acquisition instruction instead of shipping the content.

## Open questions

The following remain unresolved until the relevant implementation exists:

* the supported OverPy version range and extension policy;
* the machine-readable diagnostic schema and stable code registry;
* the canonical Workshop output normalizer and its versioning policy;
* the target/runtime used for semantic scenarios; and
* the corpus licensing and local-generation process.

These questions must not be silently answered by a compatibility shortcut.
Record a decision in `docs/adr/` when implementation evidence is available.

## Related decisions

* [ADR-0002: Compatibility strategy](docs/adr/0002-compatibility-strategy.md)
* [ADR-0003: IR boundary](docs/adr/0003-ir-boundary.md)
* [ADR-0004: OverPy licensing and clean-room boundary](docs/adr/0004-overpy-licensing-boundary.md)
