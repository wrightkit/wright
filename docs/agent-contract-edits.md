# Agent contract: statement edits

Context: for consumers of `editStatements` on `wright-agent/v1`. The rest
of the contract is in [agent-contract.md](agent-contract.md).

## Statement edits (ADR-0029)

`editStatements` (#855) is the structured form of the raw Workshop edit:
the caller names positions in a rule's `conditions`/`actions` lists and
carries `workshop-rs/rule-content-v1` statement documents — the same node
shape `rules` returns — instead of composing Workshop source by hand.
`workshop-rs` owns node conversion and canonical emission (locale
spellings, literals, indentation, comments, section creation); Wright owns
addressing, freshness, the assembled transaction, and its validation.

* `edits[]` is a list of statement edits, each `{rule, list, index,
  change, ...}`. `rule` is the rule index or declared name — a numeric
  index must carry the request's `snapshot` generation like every other
  numeric address (#595). `list` is `conditions` or `actions`. `index` is
  the flat position `rules` reports: for `insert` the position the new
  statements go before (`index == list length` appends); for `replace`
  and `delete` the statement to change. `insert` carries `statements` (a
  non-empty list of `rule-content-v1` documents — an `if`, its body, and
  its `end` go in one insert), `replace` carries `statement`, `delete`
  carries neither. Every address names a position in the program *before
  any edit in the request*; two edits at the same address refuse
  `edit-duplicate-address`.
* `inputIdentity` is the freshness precondition: the identity of the
  program the addresses were read from — `project`'s reported value, or
  the identity of the supplied `sources` text. A mismatch refuses
  `edit-stale-source`. `sources` is optional with the same semantics as
  `validateEditTransaction` (#472): when supplied, the program the
  addresses name is loaded from that text (keyed by the input's source
  spelling); when omitted, the disk-loaded program serves.
* On success the result carries the assembled `EditTransaction` — source
  edits a caller can apply through its own write path — plus the
  `validateEditTransaction` payload (`ok`, `exit`, `diagnostics`, and
  `preview` when the transaction validates). Nothing is written and the
  served program is never mutated.
* Raw Workshop input only: OPY refuses `edit-requires-provider` naming
  `providerValidateEdit`, and kinds without a shipped provider keep their
  `source-provider-unavailable` refusal — both reported inside the result
  payload like a `validateEditTransaction` refusal.
