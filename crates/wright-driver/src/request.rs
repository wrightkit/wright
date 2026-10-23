use serde::{Deserialize, Serialize};

/// A semantic query target on the agent contract (#429): a numeric id or a
/// declared name.
///
/// Numeric ids keep their established meaning — the semantic index's symbol
/// id for `references`/`usage`, the rule index for `cfg`/`rules` — and are
/// scoped to the program generation that issued them (#595): a request
/// carrying a numeric address must also carry that response's `snapshot`
/// value, or the request is refused `stale-id`. A name resolves against the
/// loaded program's semantic index: unmatched or ambiguous names produce a
/// structured error, never a guess, and `snapshot` is ignored for them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, expecting = "a numeric id or a declared name string")]
pub enum Address {
    /// The numeric id assigned by the loaded program's semantic addressing.
    Id(u32),
    /// The declared name (`rule("name")`, variable, or subroutine name).
    Name(String),
}

impl From<u32> for Address {
    fn from(id: u32) -> Self {
        Self::Id(id)
    }
}

impl From<String> for Address {
    fn from(name: String) -> Self {
        Self::Name(name)
    }
}

impl From<&str> for Address {
    fn from(name: &str) -> Self {
        Self::Name(name.to_string())
    }
}

/// A tool request: the owned query surface plus agent-oriented operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum ToolRequest {
    /// Service identity, contract, and supported operations.
    Capabilities,
    /// Compile the loaded project and write the program to a file
    /// (ADR-0025): `output` or `<entry stem>.ow` in the working directory.
    /// An existing file is replaced only with `force`; the input is never
    /// overwritten.
    Compile {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<std::path::PathBuf>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        force: bool,
        /// Output locale (#759): `wright compile --to`. `None` emits the
        /// loaded input's locale; a locale the catalog does not declare is
        /// a structured usage error, and a declared locale missing mappings
        /// fails explicitly per the workshop-rs emission contract.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
    },
    /// Check the loaded project.
    Check {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        all: bool,
    },
    /// Analyze the loaded project; optionally narrowed by an inline
    /// selection (`severity`, `rule`, `file`, `max`; #635) applied to
    /// `facts.risks`; presentation returns the essential report.
    Analyze {
        #[serde(flatten)]
        selection: crate::select::FindingSelection,
        /// Return every item in the presentation lists.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        all: bool,
    },
    /// Inspect the loaded project.
    Inspect {
        /// Return every item in the presentation lists.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        all: bool,
    },
    /// The loaded canonical program summary (origin, files, counts, findings).
    Project,
    /// Every rule, optionally narrowed by an inline selection (`rule`,
    /// `file`, `max`; #531). `content` adds each selected rule's canonical
    /// content (ADR-0024).
    Rules {
        /// The removed `name` selection (ADR-0035): a request still
        /// carrying it is refused `malformed-request` rather than silently
        /// ignored. The value deserializes untouched so the refusal comes
        /// from the service — identical on every transport, including the
        /// CLI's in-process request — instead of depending on transport
        /// deserialization.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rule: Option<Address>,
        /// The program generation a numeric `rule` was issued under (#595):
        /// required with a numeric address, refused `stale-id` when absent
        /// or mismatched, and ignored for a declared name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<usize>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        content: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        all: bool,
    },
    /// Symbols, optionally filtered by `kind` and narrowed by an inline
    /// selection (`file`, `max`; #531).
    Symbols {
        #[serde(default)]
        kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<usize>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        all: bool,
    },
    /// References to a symbol, addressed by its numeric id or its name
    /// (#429); optionally narrowed by an inline selection (`kind`, `rule`,
    /// `file`, `max`; #531).
    References {
        symbol: Address,
        /// The program generation a numeric `symbol`/`rule` was issued
        /// under (#595): required with a numeric address, refused
        /// `stale-id` when absent or mismatched, and ignored when every
        /// address in the request is a declared name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rule: Option<Address>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<usize>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        all: bool,
    },
    /// Usage counts for a symbol, addressed by its numeric id or its name
    /// (#429).
    Usage {
        symbol: Address,
        /// The program generation a numeric `symbol` was issued under
        /// (#595): required with a numeric address, refused `stale-id` when
        /// absent or mismatched, and ignored for a declared name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot: Option<u64>,
    },
    /// The control-flow graph of one rule, addressed by its numeric index or
    /// its name (#429); optionally narrowed by an inline selection (`kind`,
    /// `max`; #531).
    Cfg {
        rule: Address,
        /// The program generation a numeric `rule` was issued under (#595):
        /// required with a numeric address, refused `stale-id` when absent
        /// or mismatched, and ignored for a declared name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<usize>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        all: bool,
    },
    /// Every static-analysis finding, optionally narrowed by an inline
    /// selection (`severity`, `rule`, `file`, `max`; #430).
    Findings {
        #[serde(flatten)]
        selection: crate::select::FindingSelection,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        all: bool,
    },
    /// Persistent Workshop object facts, separate from lint diagnostics.
    PersistentObjects {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        all: bool,
    },
    /// Lint findings plus per-rule id/effective severity and the effective
    /// configuration (#98); `lintRules` serves full rule metadata (#431).
    /// Optionally narrowed by an inline selection (#430), and optionally
    /// returned in the essential presentation form.
    Lint {
        #[serde(flatten)]
        selection: crate::select::FindingSelection,
        /// Return every item in the presentation lists.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        all: bool,
    },
    /// The registered lint rules with full metadata and the effective lint
    /// configuration.
    LintRules,
    /// The subroutine call graph (caller rules → callee subroutines),
    /// optionally narrowed by an inline selection (`caller`, `callee`,
    /// `max`; #531).
    CallGraph {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caller: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        callee: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<usize>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        all: bool,
    },
    /// Generated-resource cost estimates (exact counts + findings),
    /// optionally narrowed by an inline selection (#430). An optional
    /// `rule` selects one declared rule name and returns that rule's
    /// element-cost breakdown (#724).
    CostEstimate {
        #[serde(flatten)]
        selection: crate::select::FindingSelection,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rule: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        all: bool,
    },
    /// Target/catalog metadata (actions, values, events, enum domains).
    TargetMetadata,
    /// Validate and preview a caller-supplied source-edit transaction
    /// against the session's project (#130): atomic all-or-nothing
    /// semantics, structured refusal diagnostics, no filesystem writes.
    /// Raw Workshop input validates by reparsing the edited sources through
    /// `workshop-rs` (#434); source languages route to
    /// `providerValidateEdit`.
    #[serde(rename = "validateEditTransaction")]
    ValidateEdit {
        /// The current text of every source the transaction touches, keyed
        /// by the same source identities the edits carry. Optional (#472):
        /// when absent the service reads the current text of the files the
        /// transaction names from disk; when present it stays the
        /// caller-side precondition text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<std::collections::BTreeMap<String, String>>,
        transaction: crate::edit::EditTransaction,
    },
    /// Provider-driven edit validation (#139): validate a caller-proposed
    /// source-edit transaction against the provider's project semantics
    /// (`lpp/validateEdits` per edited document, then `lpp/check` over the
    /// edited project) before any application. The transaction must carry
    /// document URIs as source identities and the identity of the text the
    /// edits were computed against; stale, malformed, or semantically
    /// invalid transactions refuse with no partial edit set.
    #[serde(rename = "providerValidateEdit")]
    ProviderValidateEdit {
        /// The opaque language id of the configured provider.
        language_id: String,
        /// The unmodified project as the provider sees it. Optional (#548):
        /// when omitted, the session's loaded project supplies it — every
        /// member file read from disk at `version` 0, keyed by `file://` URI.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        documents: Option<wright_lpp::DocumentSet>,
        /// The caller-proposed transaction (Wright-owned edit contract).
        transaction: crate::edit::EditTransaction,
        /// The caller's current text for every edited source, keyed by
        /// document URI (the identity/version precondition view). Optional
        /// (#548): defaults to each document's text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<std::collections::BTreeMap<String, String>>,
        /// The project the documents belong to (informational).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project_root: Option<String>,
    },
    /// Structured statement edits for raw Workshop input (ADR-0029, #855):
    /// insert, replace, or delete a rule's `conditions`/`actions` entries
    /// carrying `workshop-rs/rule-content-v1` statement documents. The
    /// service resolves every address against the program before any edit,
    /// delegates content conversion and source-edit generation to
    /// `workshop-rs`, assembles the owner edits into an `EditTransaction`,
    /// and validates it through `validateEditTransaction`. Nothing is
    /// written and the loaded program is never mutated.
    #[serde(rename = "editStatements")]
    EditStatements {
        /// The freshness precondition (ADR-0029): must equal the
        /// `inputIdentity` of the program the addresses were read from —
        /// the value `project` reports, or the identity of the supplied
        /// `sources` text. A mismatch refuses `edit-stale-source`.
        #[serde(rename = "inputIdentity")]
        input_identity: String,
        /// The caller's current text for the loaded input source, with the
        /// same optional semantics as `validateEditTransaction` (#472):
        /// when supplied the program the addresses name is loaded from
        /// that text, which is also the precondition text the assembled
        /// edits verify against; when absent the disk-loaded program
        /// serves.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<std::collections::BTreeMap<String, String>>,
        /// The program generation a numeric `rule` address was issued
        /// under (#595): required when any edit carries one, refused
        /// `stale-id` when absent or mismatched, and ignored when every
        /// `rule` is a declared name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot: Option<u64>,
        /// The statement edits, every one addressed against the program
        /// before any edit in this request applies. Two edits at the same
        /// statement address refuse `edit-duplicate-address`.
        edits: Vec<StatementEditRequest>,
    },
    /// Resolve a free-text name guess — a display name, a near spelling, or
    /// a guess — to the owner's accepted vocabulary (ADR-0021, #529):
    /// spellings, enum domains and members, settings keys, and
    /// Wright-rendered callable signatures. Answers from the language
    /// vocabulary alone: the loaded program is never required, consulted,
    /// or triggered to load.
    Lookup {
        /// `workshop` or `opy`, the ids `capabilities.languages` reports.
        language: String,
        /// Free text: a display name, a near spelling, or a guess.
        #[serde(default)]
        query: Option<String>,
        /// Entry kind filter — any kind either owner emits (#563):
        /// `action`, `value`, `event`, `enumMember`, `enum`, `setting`,
        /// `parameter`, `settingPath`, `memberAction`, `memberValue`,
        /// `structural`, or `operator`. A kind one language never emits
        /// answers `[]` there.
        #[serde(default)]
        kind: Option<String>,
        /// The identity of an enum domain, a callable, or a settings path
        /// prefix; the result lists its members, parameters, or children.
        #[serde(default)]
        within: Option<String>,
        /// The locale used to read Workshop display names.
        #[serde(default)]
        locale: Option<String>,
        /// Maximum entries: default 3, maximum 10.
        #[serde(default)]
        limit: Option<usize>,
    },
    /// List the owner's Workshop vocabulary of one kind in owner order, one
    /// page at a time (ADR-0031), for editors that show every choice.
    /// Answers from the language vocabulary alone: the loaded program is
    /// never required, consulted, or triggered to load.
    #[serde(rename = "listEntries")]
    ListEntries {
        /// `workshop`; any other language id answers `unavailable` or is
        /// refused.
        language: String,
        /// `action`, `value`, `event`, `enum`, or `enumMember`.
        kind: String,
        /// An enum domain identity; required for `enumMember` and refused
        /// for every other kind.
        #[serde(default)]
        within: Option<String>,
        /// The locale used to read Workshop display names.
        #[serde(default)]
        locale: Option<String>,
        /// Entries per page: default 100, maximum 500.
        #[serde(default, rename = "pageSize")]
        page_size: Option<usize>,
        /// The `nextCursor` of the previous page; omitted for the first.
        #[serde(default)]
        cursor: Option<String>,
    },
    /// Simulate the loaded project under `athena` (ADR-0033): a thin pass
    /// to the simulator's `run_text`/`run_scenario` entry points over the
    /// effective program's canonical Workshop text — a source-language
    /// project's provider-compiled artifact included — returning the trace
    /// unchanged inside a `wright-result/v1` envelope. `athena` owns
    /// execution semantics and the trace contract; `complete`,
    /// `not_modeled`, and `missing_input` outcomes are all trace content —
    /// the request is refused only when it cannot be served at all (a stale
    /// `inputIdentity`, a malformed `scenario` or `seconds`, or a project
    /// that does not load).
    #[serde(rename = "simulate")]
    Simulate {
        /// The freshness precondition (ADR-0033): must equal the
        /// `inputIdentity` of the program to simulate — the value `project`
        /// reports, or the identity of the supplied `sources` text. A
        /// mismatch refuses `edit-stale-source` and runs nothing.
        #[serde(rename = "inputIdentity")]
        input_identity: String,
        /// The caller's current text for the loaded input source, with the
        /// same optional semantics as `validateEditTransaction` (#472): when
        /// supplied the program is loaded from that text, which is also the
        /// precondition `inputIdentity` verifies against; when absent the
        /// disk-loaded input's own text is simulated.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<std::collections::BTreeMap<String, String>>,
        /// The simulated seconds for a run without a scenario — `athena`'s
        /// `Options.seconds`. Required when `scenario` is absent, and refused
        /// when one is present because the scenario owns the simulated time.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seconds: Option<f64>,
        /// An `athena` scenario value (its `docs/scenario-format.md`),
        /// passed through unchanged — Wright supplies no defaults for what
        /// it omits, and `scenario.seconds` supplies the simulated time.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scenario: Option<serde_json::Value>,
    },
}

/// The rule statement list a statement edit addresses (ADR-0029) — the
/// same two lists `rule-content-v1` exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StatementList {
    /// The rule's `conditions` section (`rule.conditions`).
    Conditions,
    /// The rule's `actions` section (`rule.actions`).
    Actions,
}

impl StatementList {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Conditions => "conditions",
            Self::Actions => "actions",
        }
    }
}

/// One statement-level edit against a rule's `conditions` or `actions`
/// list (ADR-0029). `rule` + `list` + `index` is the statement address: a
/// position in the program before any edit in the request applies, the
/// same flat action index space `rules`, `cfg`, `references`, and
/// findings share.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatementEditRequest {
    /// The rule holding the list: its numeric index in the program
    /// (scoped to the request's `snapshot` generation, #595) or its
    /// declared name.
    pub rule: Address,
    /// The statement list inside the rule.
    pub list: StatementList,
    /// For `insert`, the index the new statements are placed before
    /// (`index == list length` appends); for `replace`/`delete`, the
    /// index of the statement to change.
    pub index: usize,
    /// The change to make at the address.
    #[serde(flatten)]
    pub change: StatementEditChange,
}

/// The change a statement edit makes (ADR-0029). Payload nodes are
/// `workshop-rs/rule-content-v1` statement documents — `{"value": …}`
/// for a condition, `{"call": id, "args": […]}` for an action; either may
/// carry `"disabled": true`, and `span`/`spans` keys are output-only and
/// refused by the owner.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "camelCase")]
pub enum StatementEditChange {
    /// Insert the `rule-content-v1` statements, in order, before `index`.
    Insert {
        /// The non-empty list of statement documents. Statements that
        /// belong at one position — an `if`, its body, and its `end` —
        /// go in one `insert`.
        statements: Vec<serde_json::Value>,
    },
    /// Replace the statement at `index`, keeping its leading comments.
    Replace {
        /// The replacement statement document.
        statement: serde_json::Value,
    },
    /// Delete the statement at `index` with its attached comments.
    Delete,
}
