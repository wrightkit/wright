//! The request vocabulary of the tool service: the owned query surface plus
//! agent-oriented operations, as they arrive on every transport.

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
    /// Every rule, optionally narrowed by an inline selection (`name`,
    /// `rule`, `file`, `max`; #531). `content` adds each selected rule's
    /// canonical content (ADR-0024).
    Rules {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
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
}
