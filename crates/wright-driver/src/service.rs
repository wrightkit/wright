//! [`ToolService`] exposes Wright's compile/check/analyze/query workflows and
//! agent-oriented semantic queries over stable public contracts, reusing the
//! driver session. It is transport-neutral: the `serve` transport adapters
//! (#60) and the embedding API are thin mappings over the same operations,
//! so behavior is testable in-process without a transport.
//!
//! Capability/version negotiation is provided by [`Capabilities`]; cost and
//! resource inspection ([`ToolRequest::CostEstimate`]) consumes the
//! Wright-owned generated-resource semantics established by the `wright-bench`
//! harness (emitted bytes, canonical program counts, action/rule counts) and
//! distinguishes exact counts from static findings.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::diag::Diagnostic;
use crate::input;
use crate::result::{AnalyzeResult, CheckResult, CompileResult, Envelope, InspectResult};
use crate::{CompilerSession, Loaded, RESULT_CONTRACT};
use wright_analyzer::canonical::{
    BLOCK_KINDS, ReferenceKind, SemanticIndex, SemanticService, SymbolKind,
};
/// A structured tool error.
pub use wright_analyzer::service::ErrorInfo as ToolErrorInfo;
/// A tool response: a structured owned result or a structured error.
pub use wright_analyzer::service::Response as ToolResponse;
use wright_analyzer::service::{Request, Response};

/// The tool-service name and version.
pub const SERVICE_NAME: &str = "wright-tool-service";
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const AGENT_CONTRACT: &str = "wright-agent/v1";

/// Every operation `capabilities.operations` advertises, in contract order —
/// the catalog client tool definitions are emitted from (#535).
pub const OPERATIONS: &[&str] = &[
    "capabilities",
    "project",
    "rules",
    "symbols",
    "references",
    "usage",
    "cfg",
    "findings",
    "persistentObjects",
    "lint",
    "lintRules",
    "callGraph",
    "costEstimate",
    "targetMetadata",
    "compile",
    "check",
    "analyze",
    "inspect",
    "validateEditTransaction",
    "semanticRename",
    "providerSemanticRename",
    "providerValidateEdit",
    "lookup",
];

/// A semantic query target on the agent contract (#429): a numeric id or a
/// declared name.
///
/// Numeric ids keep their established meaning — the semantic index's symbol
/// id for `references`/`usage`, the rule index for `cfg` — so existing
/// numeric requests are unchanged. A name resolves against the loaded
/// program's semantic index: unmatched or ambiguous names produce a
/// structured error, never a guess.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
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
    /// Compile the loaded project.
    Compile,
    /// Check the loaded project.
    Check,
    /// Analyze the loaded project.
    Analyze {
        /// Return the brief summary form (#532): counts, the
        /// highest-priority items, and how to expand.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        brief: bool,
    },
    /// Inspect the loaded project.
    Inspect {
        /// Return the brief summary form (#532).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        brief: bool,
    },
    /// The loaded canonical program summary (origin, files, counts, findings).
    Project,
    /// Every rule, optionally narrowed by an inline selection (`name`,
    /// `file`, `max`; #531).
    Rules {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<usize>,
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
    },
    /// References to a symbol, addressed by its numeric id or its name
    /// (#429); optionally narrowed by an inline selection (`kind`, `rule`,
    /// `file`, `max`; #531).
    References {
        symbol: Address,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rule: Option<Address>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<usize>,
    },
    /// Usage counts for a symbol, addressed by its numeric id or its name
    /// (#429).
    Usage { symbol: Address },
    /// The control-flow graph of one rule, addressed by its numeric index or
    /// its name (#429); optionally narrowed by an inline selection (`kind`,
    /// `max`; #531).
    Cfg {
        rule: Address,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<usize>,
    },
    /// Every static-analysis finding, optionally narrowed by an inline
    /// selection (`severity`, `rule`, `file`, `max`; #430).
    Findings(crate::select::FindingSelection),
    /// Persistent Workshop object facts, separate from lint diagnostics.
    PersistentObjects,
    /// Lint findings plus per-rule id/effective severity and the effective
    /// configuration (#98); `lintRules` serves full rule metadata (#431).
    /// Optionally narrowed by an inline selection (#430), and optionally
    /// returned in the brief summary form (#532).
    Lint {
        #[serde(flatten)]
        selection: crate::select::FindingSelection,
        /// Return the brief summary form (#532).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        brief: bool,
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
    },
    /// Generated-resource cost estimates (exact counts + findings),
    /// optionally narrowed by an inline selection (#430).
    CostEstimate(crate::select::FindingSelection),
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
    /// Request a semantic rename through the shared refactoring contract
    /// (#129/#130/#434): returns the validated exact-range transaction or
    /// structured refusal diagnostics. On raw Workshop input the target is
    /// a `symbol` (numeric id or declared name, as `references`/`usage`
    /// address them) or a `source`/`line`/`col` position, and occurrences
    /// rewrite through `workshop-rs` identifier provenance. Source
    /// languages route to `providerSemanticRename`. Wright
    /// proposes/validates; applying edits to disk is an explicit consumer
    /// responsibility.
    SemanticRename {
        /// The current text of every source the rename may edit, keyed by
        /// the same source identities the target names. Optional (#472):
        /// when absent the service reads the loaded input's current text
        /// from disk; when present it stays the caller-side precondition
        /// text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<std::collections::BTreeMap<String, String>>,
        target: crate::edit::RenameTarget,
    },
    /// Provider-driven semantic rename (#139): rename target resolution and
    /// edit generation route through the LPP `rename` capability of the
    /// provider configured for `language_id`; the resulting source edits are
    /// wrapped in Wright's own transaction (identity/version preconditions,
    /// deterministic ordering, overlap checks, atomic preview) and validated
    /// through the provider's project semantics (`lpp/validateEdits` per
    /// edited document, then `lpp/check` over the edited project) before
    /// success. Provider refusals, unsupported capabilities, stale sources,
    /// and semantic validation failures are structured refusals with no
    /// partial edit set; there is no fallback to textual search/replace.
    #[serde(rename = "providerSemanticRename")]
    ProviderSemanticRename {
        /// The opaque language id of the configured provider.
        language_id: String,
        /// The document set the rename is computed against (the provider's
        /// view: text, language id, version). Optional (#548): when omitted,
        /// the session's loaded project supplies it — every member file read
        /// from disk at `version` 0, keyed by `file://` URI. An omitted set
        /// therefore reads the program the way the other program-reading
        /// operations do.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        documents: Option<wright_lpp::DocumentSet>,
        /// The URI (a key of `documents`) in which `position` is
        /// interpreted.
        position_document_uri: String,
        /// The position of the symbol to rename (0-based LSP conventions).
        position: wright_lpp::Position,
        /// The new name; the provider validates it against the language's
        /// identifier rules.
        new_name: String,
        /// The project the documents belong to (informational).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project_root: Option<String>,
        /// The caller's current text for every source the rename may edit,
        /// keyed by document URI (the identity/version precondition view).
        /// Optional (#548): defaults to each document's text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sources: Option<std::collections::BTreeMap<String, String>>,
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
        /// Entry kind filter: `action`, `value`, `event`, `enumMember`, or
        /// `setting`.
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
}

/// The capability/version contract of the service.
#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub name: String,
    pub version: String,
    pub contract: String,
    pub agent_contract: String,
    pub operations: Vec<String>,
    /// The committed-schema `$defs` entry describing each advertised
    /// operation's result (#532): `"<op>": "#/$defs/<Op>Result"`, so a
    /// caller learns an operation's result shape without a second document.
    pub result_schemas: std::collections::BTreeMap<String, String>,
    pub languages: Vec<String>,
    pub profiles: Vec<String>,
}

/// The session-aware tool service.
pub struct ToolService<'a> {
    session: &'a mut CompilerSession,
    /// The current program snapshot, when one exists. The service may exist
    /// without one (#512): a configured project that cannot load at
    /// construction is deferred to the first program-reading request rather
    /// than failing the service, so `None` is the unloaded state — never an
    /// empty placeholder program.
    loaded: Option<Loaded>,
    /// The shared semantic service over `loaded` — `Some` exactly when a
    /// snapshot is held.
    semantic: Option<Arc<SemanticService<'static>>>,
    lint_semantic: Option<SemanticService<'static>>,
    /// The disk state `loaded` was read from, recomputed before every
    /// program-reading request so a long-lived session observes edits, file
    /// additions, and removals (#471). `None` while no snapshot is held.
    fingerprint: Option<input::DiskFingerprint>,
    /// Whether the symbol ids a client holds were issued by the current
    /// program (#471): a reload clears it until a `symbols` listing or an
    /// `ambiguous-symbol` refusal names the current space.
    symbol_ids_current: bool,
    /// The rule-index counterpart of `symbol_ids_current`, re-established by
    /// `rules` and `ambiguous-rule` (#471).
    rule_ids_current: bool,
    /// How many times an observed disk change reloaded the program — the
    /// observable hook for "unchanged inputs are not reloaded" (#471).
    reloads: usize,
    /// `true` while the service sits in the invalidated state after a
    /// failed refresh (#512): the next successful load is a reload — an
    /// observed change brought the project back — rather than the deferred
    /// initial load.
    invalidated: bool,
}

impl<'a> ToolService<'a> {
    /// Build the service over a session.
    ///
    /// A session whose configuration changed after construction is refused
    /// with `session-config-changed` (#511): the service never adopts state
    /// derived from a superseded configuration.
    ///
    /// The configured project is loaded eagerly when it can be, but a load
    /// failure does not fail construction (#512): the service exists without
    /// a program snapshot, project-independent operations (`capabilities`,
    /// `targetMetadata`, the `provider*` mutations carrying their own
    /// document set) stay available, and each program-reading request
    /// retries the load — surfacing the loader's structured diagnostic
    /// until the project heals.
    #[hotpath::measure]
    pub fn new(session: &'a mut CompilerSession) -> Result<ToolService<'a>, Diagnostic> {
        session.verify_fixed_config()?;
        let mut service = ToolService {
            session,
            loaded: None,
            semantic: None,
            lint_semantic: None,
            fingerprint: None,
            symbol_ids_current: true,
            rule_ids_current: true,
            reloads: 0,
            invalidated: false,
        };
        if let Ok(loaded) = service.session.load() {
            service.adopt(loaded);
        }
        Ok(service)
    }

    /// Reloads caused by observed disk changes (#471). An input whose
    /// fingerprint has not changed is never reloaded, so a stable value
    /// across requests proves the session kept serving the same snapshot.
    /// The deferred initial load (#512) is not a reload.
    pub fn reload_count(&self) -> usize {
        self.reloads
    }

    /// The current program snapshot (origin, input identity, canonical
    /// program), or `None` while the configured project cannot load (#512).
    /// The first successful program-reading request populates it.
    pub fn loaded(&self) -> Option<&Loaded> {
        self.loaded.as_ref()
    }

    /// Full semantic-service builds behind this service's session (#513) —
    /// the observable hook proving repeated requests share the loaded
    /// snapshot's index rather than rebuilding it.
    pub fn semantic_build_count(&self) -> usize {
        self.session.semantic_build_count()
    }

    /// The capability/version contract.
    pub fn capabilities(&self) -> Capabilities {
        let operations: Vec<String> = OPERATIONS.iter().map(|op| op.to_string()).collect();
        // The committed schema names every operation's result definition
        // `<Op>Result` — `capabilities` surfaces the map so a caller learns
        // result shapes without a second document (#532).
        let result_schemas = operations
            .iter()
            .map(|op| {
                let mut pascal = op.clone();
                if let Some(first) = pascal.get_mut(0..1) {
                    first.make_ascii_uppercase();
                }
                (op.clone(), format!("#/$defs/{pascal}Result"))
            })
            .collect();
        Capabilities {
            name: SERVICE_NAME.to_string(),
            version: SERVICE_VERSION.to_string(),
            contract: RESULT_CONTRACT.to_string(),
            agent_contract: AGENT_CONTRACT.to_string(),
            operations,
            result_schemas,
            languages: vec!["opy".to_string(), "workshop".to_string()],
            profiles: vec![
                crate::Profile::Off.as_str().to_string(),
                crate::Profile::Compat.as_str().to_string(),
                crate::Profile::Aggressive.as_str().to_string(),
            ],
        }
    }

    /// Handle one tool request, returning a structured owned response.
    ///
    /// A request that reads the loaded program requires a valid current
    /// snapshot at request time (#471, #512): the deferred initial load runs
    /// here when the service was built over an unloadable project, and a
    /// changed input fingerprint reloads the session, so a long-lived
    /// session serves the project as it exists now. Either failure is the
    /// request's structured refusal — the previous snapshot is never served
    /// silently. `capabilities` reports service metadata, `targetMetadata`
    /// the static catalog, and `provider*` operations carrying their own
    /// document set consult neither — a `provider*` request that omits its
    /// document set derives it from the loaded project (#548) and so
    /// triggers the load like any other program-reading operation.
    pub fn handle(&mut self, request: &ToolRequest) -> ToolResponse {
        if Self::reads_program(request) {
            if let Err(error) = self.refresh() {
                return ToolResponse::Error { error };
            }
            if let Some(error) = self.stale_id(request) {
                return ToolResponse::Error { error };
            }
        }
        let response = hotpath::measure_block!(request_label(request), self.dispatch(request));
        self.note_id_space(request, &response);
        response
    }

    /// Whether the request consults the loaded program (#471). A provider
    /// operation carrying its own document set does not; one that omits it
    /// derives the set from the loaded project (#548) and so reads the
    /// program like every other program-reading operation.
    fn reads_program(request: &ToolRequest) -> bool {
        !matches!(
            request,
            ToolRequest::Capabilities
                | ToolRequest::TargetMetadata
                | ToolRequest::ProviderSemanticRename {
                    documents: Some(_),
                    ..
                }
                | ToolRequest::ProviderValidateEdit {
                    documents: Some(_),
                    ..
                }
                | ToolRequest::Lookup { .. }
        )
    }

    /// Ensure a valid current snapshot backs a program-reading request
    /// (#471, #512). With no snapshot the request performs the deferred
    /// initial load; with one, a changed disk fingerprint reloads it. A
    /// failed attempt surfaces the loader's structured diagnostic and
    /// invalidates the served snapshot — the previous program is never
    /// served, even if the input is restored to identical bytes — so the
    /// next request retries the same resolution and a repaired project is
    /// adopted without restarting the service.
    fn refresh(&mut self) -> Result<(), ToolErrorInfo> {
        let Some(loaded) = &self.loaded else {
            // The deferred initial load, or recovery after an invalidated
            // snapshot (#512). Recovery counts as a reload — an observed
            // disk change brought the project back — and keeps the dropped
            // snapshot's id space stale until it is re-observed.
            let loaded = self.session.load().map_err(Self::loader_error)?;
            self.adopt(loaded);
            if self.invalidated {
                self.reloads += 1;
                self.invalidated = false;
            }
            return Ok(());
        };
        let fingerprint = input::disk_fingerprint(&self.session.config, &loaded.input);
        if self.fingerprint.as_ref() == Some(&fingerprint) {
            return Ok(());
        }
        let loaded = match self.session.reload() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.invalidate();
                return Err(Self::loader_error(diagnostic));
            }
        };
        self.adopt(loaded);
        self.reloads += 1;
        // The reloaded program's numeric ids are a different space: ids
        // issued before the reload refuse `stale-id` until a `symbols` or
        // `rules` listing — or an `ambiguous-*` refusal, which already names
        // the current candidates — re-establishes them.
        self.symbol_ids_current = false;
        self.rule_ids_current = false;
        Ok(())
    }

    /// Drop the served snapshot after a failed refresh (#512). The next
    /// program-reading request must load again — restoring the input's exact
    /// previous bytes must not fingerprint-match the pre-failure snapshot
    /// back to life — and ids issued by the dropped program refuse
    /// `stale-id` until the new space is observed.
    fn invalidate(&mut self) {
        self.loaded = None;
        self.semantic = None;
        self.lint_semantic = None;
        self.fingerprint = None;
        self.symbol_ids_current = false;
        self.rule_ids_current = false;
        self.invalidated = true;
    }

    /// The loader's diagnostic carried through the request's structured
    /// refusal unchanged.
    fn loader_error(diagnostic: Diagnostic) -> ToolErrorInfo {
        ToolErrorInfo {
            code: diagnostic.code,
            message: diagnostic.message,
        }
    }

    /// Replace the loaded snapshot and rebuild the semantic services over it.
    fn adopt(&mut self, loaded: Loaded) {
        let semantic = self.session.shared_semantic(&loaded);
        self.lint_semantic = (!self.session.config.lint.rules.is_empty())
            .then(|| semantic.with_lint_config(self.session.config.lint.clone()));
        self.semantic = Some(semantic);
        self.fingerprint = Some(input::disk_fingerprint(&self.session.config, &loaded.input));
        self.loaded = Some(loaded);
    }

    /// The current snapshot — present whenever a program-reading request
    /// reaches `dispatch`, because `refresh` succeeded first.
    fn snapshot(&self) -> &Loaded {
        self.loaded
            .as_ref()
            .expect("the refresh gate guarantees a snapshot")
    }

    /// The document set a provider mutation runs against (#548): the
    /// caller's set when supplied, otherwise the loaded project's members —
    /// each member read from disk at `version` 0 and keyed by `file://` URI,
    /// so the provider sees the same project the session serves. The
    /// omitted-set path only runs after the refresh gate, so `snapshot` is
    /// guaranteed; a member that is not a readable disk file is a structured
    /// refusal, never a silently partial set.
    fn provider_documents(
        &self,
        language_id: &str,
        documents: &Option<wright_lpp::DocumentSet>,
    ) -> Result<wright_lpp::DocumentSet, ToolErrorInfo> {
        if let Some(documents) = documents {
            return Ok(documents.clone());
        }
        let loaded = self.snapshot();
        let mut set = wright_lpp::DocumentSet::new();
        for member in loaded.source_files.iter() {
            let (uri, path) = provider_member(member, &loaded.input.cwd)?;
            let text = std::fs::read_to_string(&path).map_err(|error| ToolErrorInfo {
                code: "provider-document-unreadable".to_string(),
                message: format!(
                    "cannot read loaded project member '{}' for the provider document set: {error}",
                    path.display()
                ),
            })?;
            set.insert(
                uri.clone(),
                wright_lpp::Document {
                    uri,
                    language_id: language_id.to_string(),
                    version: 0,
                    text,
                },
            );
        }
        Ok(set)
    }

    /// The semantic service over the current snapshot — built in `adopt`.
    fn semantic(&self) -> &SemanticService<'static> {
        self.semantic
            .as_deref()
            .expect("the refresh gate guarantees a snapshot")
    }

    /// Refuse a numeric id that was issued by an earlier program (#471):
    /// `stale-id` until the client observes the current space. Name
    /// addressing resolves against the loaded index and is always fresh.
    fn stale_id(&self, request: &ToolRequest) -> Option<ToolErrorInfo> {
        let stale = match request {
            ToolRequest::References { symbol, .. } | ToolRequest::Usage { symbol } => {
                matches!(symbol, Address::Id(_)) && !self.symbol_ids_current
            }
            ToolRequest::Cfg { rule, .. } => {
                matches!(rule, Address::Id(_)) && !self.rule_ids_current
            }
            ToolRequest::SemanticRename { target, .. } => {
                matches!(target.symbol, Some(Address::Id(_))) && !self.symbol_ids_current
            }
            _ => false,
        };
        stale.then(|| ToolErrorInfo {
            code: "stale-id".to_string(),
            message: "the loaded project changed on disk; numeric symbol ids and rule indexes issued before the reload are no longer valid — list `symbols` or `rules` again, or address by name".to_string(),
        })
    }

    /// A request that names the current id space re-establishes it (#471): a
    /// successful `symbols`/`rules` listing, or an `ambiguous-*` refusal that
    /// already reports the current candidates. `semanticRename` reports its
    /// `ambiguous-symbol` as a result diagnostic rather than a refusal.
    fn note_id_space(&mut self, request: &ToolRequest, response: &ToolResponse) {
        match response {
            ToolResponse::Ok { result } => match request {
                ToolRequest::Symbols { .. } => self.symbol_ids_current = true,
                ToolRequest::Rules { .. } => self.rule_ids_current = true,
                ToolRequest::SemanticRename { .. }
                    if diagnostics_list_code(result, "ambiguous-symbol") =>
                {
                    self.symbol_ids_current = true;
                }
                _ => {}
            },
            ToolResponse::Error { error } => match error.code.as_str() {
                "ambiguous-symbol" => self.symbol_ids_current = true,
                "ambiguous-rule" => self.rule_ids_current = true,
                _ => {}
            },
        }
    }

    fn dispatch(&mut self, request: &ToolRequest) -> ToolResponse {
        match request {
            ToolRequest::Capabilities => ToolResponse::Ok {
                result: serde_json::to_value(self.capabilities()).expect("capabilities serialize"),
            },
            ToolRequest::Compile => {
                let result = serde_json::to_value(self.session.compile())
                    .expect("compile result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Check => {
                let result =
                    serde_json::to_value(self.session.check()).expect("check result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Analyze { brief } => {
                let loaded = self.snapshot().clone();
                let semantic = Arc::clone(
                    self.semantic
                        .as_ref()
                        .expect("the refresh gate guarantees a snapshot"),
                );
                let mut result =
                    serde_json::to_value(self.session.analyze_loaded(loaded, &semantic))
                        .expect("analyze result serializes");
                if *brief {
                    result["result"] = crate::brief::analyze(&result["result"]);
                }
                ToolResponse::Ok { result }
            }
            ToolRequest::Inspect { brief } => {
                let loaded = self.snapshot().clone();
                let semantic = Arc::clone(
                    self.semantic
                        .as_ref()
                        .expect("the refresh gate guarantees a snapshot"),
                );
                let mut result =
                    serde_json::to_value(self.session.inspect_loaded(loaded, &semantic))
                        .expect("inspect result serializes");
                if *brief {
                    result["result"] = crate::brief::inspect(&result["result"]);
                }
                ToolResponse::Ok { result }
            }
            ToolRequest::Project => self.ok(self.project()),
            ToolRequest::Rules { name, file, max } => self.rules(name, file, max),
            ToolRequest::Symbols { kind, file, max } => {
                self.symbols(kind.as_deref(), file.as_deref(), *max)
            }
            ToolRequest::References {
                symbol,
                kind,
                rule,
                file,
                max,
            } => self.references(
                symbol,
                kind.as_deref(),
                rule.as_ref(),
                file.as_deref(),
                *max,
            ),
            ToolRequest::Usage { symbol } => match self.symbol_address(symbol) {
                Ok(symbol) => self.semantic_query(Request::GetUsage { symbol }),
                Err(error) => ToolResponse::Error { error },
            },
            ToolRequest::Cfg { rule, kind, max } => match self.rule_address(rule) {
                Ok(rule) => self.cfg(rule, kind.as_deref(), *max),
                Err(error) => ToolResponse::Error { error },
            },
            ToolRequest::Findings(selection) => self.findings(selection),
            ToolRequest::PersistentObjects => self.persistent_objects(),
            ToolRequest::Lint { selection, brief } => self.lint(selection, *brief),
            ToolRequest::LintRules => self.configured_semantic().handle(&Request::LintRules),
            ToolRequest::CallGraph {
                caller,
                callee,
                max,
            } => self.call_graph(caller.as_deref(), callee.as_deref(), *max),
            ToolRequest::CostEstimate(selection) => self.cost_estimate(selection),
            ToolRequest::TargetMetadata => self.ok(self.target_metadata()),
            ToolRequest::ValidateEdit {
                sources,
                transaction,
            } => self.ok(serde_json::to_value(
                self.session
                    .validate_edit_transaction(sources.as_ref(), transaction),
            )
            .expect("serializes")),
            ToolRequest::SemanticRename { sources, target } => {
                let rename = self.session.semantic_rename(sources.as_ref(), target);
                self.ok(serde_json::to_value(rename).expect("serializes"))
            }
            ToolRequest::ProviderSemanticRename {
                language_id,
                documents,
                position_document_uri,
                position,
                new_name,
                project_root,
                sources,
            } => {
                let documents = match self.provider_documents(language_id, documents) {
                    Ok(documents) => documents,
                    Err(error) => return ToolResponse::Error { error },
                };
                let req = crate::provider_edit::ProviderRenameRequest {
                    position_document_uri: position_document_uri.clone(),
                    position: *position,
                    new_name: new_name.clone(),
                    project_root: project_root.clone(),
                    sources: sources
                        .clone()
                        .unwrap_or_else(|| document_sources(&documents)),
                    documents,
                };
                self.ok(
                    serde_json::to_value(self.run_provider_flow(language_id, |p| {
                        crate::provider_edit::semantic_rename(p, &req)
                    }))
                    .expect("serializes"),
                )
            }
            ToolRequest::ProviderValidateEdit {
                language_id,
                documents,
                transaction,
                sources,
                project_root,
            } => {
                let documents = match self.provider_documents(language_id, documents) {
                    Ok(documents) => documents,
                    Err(error) => return ToolResponse::Error { error },
                };
                let req = crate::provider_edit::ProviderValidateRequest {
                    transaction: transaction.clone(),
                    sources: sources
                        .clone()
                        .unwrap_or_else(|| document_sources(&documents)),
                    project_root: project_root.clone(),
                    documents,
                };
                self.ok(
                    serde_json::to_value(self.run_provider_flow(language_id, |p| {
                        crate::provider_edit::validate_transaction(p, &req)
                    }))
                    .expect("serializes"),
                )
            }
            ToolRequest::Lookup {
                language,
                query,
                kind,
                within,
                locale,
                limit,
            } => crate::lookup::lookup(
                self.session,
                language,
                query.as_deref(),
                kind.as_deref(),
                within.as_deref(),
                locale.as_deref(),
                *limit,
            ),
        }
    }

    /// Compile through the shared session pipeline.
    ///
    /// A failed refresh invalidated the service snapshot and emptied the
    /// session's cache, so `compile`'s own load attempt re-surfaces the
    /// reload diagnostic as this envelope's refusal; the stale snapshot is
    /// never consulted (#471, #512).
    pub fn compile(&mut self) -> Envelope<CompileResult> {
        let _ = self.refresh();
        self.session.compile()
    }

    /// Check through the shared session pipeline.
    ///
    /// The same refresh contract as [`Self::compile`] applies (#471).
    pub fn check(&mut self) -> Envelope<CheckResult> {
        let _ = self.refresh();
        self.session.check()
    }

    /// Analyze through the shared session pipeline.
    ///
    /// The same refresh contract as [`Self::inspect`] applies (#471): a
    /// failed refresh cannot delegate to the stale snapshot, so the
    /// session's own load surfaces the refusal (#513 reuses the held
    /// semantic service on the fresh path).
    pub fn analyze(&mut self) -> Envelope<AnalyzeResult> {
        if self.refresh().is_err() {
            return self.session.analyze();
        }
        let loaded = self.snapshot().clone();
        let semantic = Arc::clone(
            self.semantic
                .as_ref()
                .expect("the refresh gate guarantees a snapshot"),
        );
        self.session.analyze_loaded(loaded, &semantic)
    }

    /// Inspect through the shared session pipeline.
    ///
    /// `inspect` renders through `self.semantic`, so a failed refresh cannot
    /// delegate to the stale snapshot the way `compile` can — the session's
    /// own load surfaces the same failure as the envelope's refusal (#471).
    pub fn inspect(&mut self) -> Envelope<InspectResult> {
        if self.refresh().is_err() {
            return self.session.inspect();
        }
        let loaded = self.snapshot().clone();
        let semantic = Arc::clone(
            self.semantic
                .as_ref()
                .expect("the refresh gate guarantees a snapshot"),
        );
        self.session.inspect_loaded(loaded, &semantic)
    }

    /// Spawn the LPP provider client for `language_id` through the session's
    /// provider registry (#142).
    ///
    /// Tooling consumes provider capabilities through the transport-neutral
    /// `wright_lpp::LanguageProvider` seam; process and JSON-RPC details
    /// stay inside `wright-lpp`. Unconfigured languages and missing
    /// capabilities refuse explicitly.
    pub fn language_provider(
        &self,
        language_id: &str,
    ) -> Result<Box<dyn wright_lpp::LanguageProvider>, wright_lpp::ProviderError> {
        self.session.language_provider(language_id)
    }

    /// Run a provider-driven mutation flow (#139) through the session's
    /// shared provider lifecycle path.
    fn run_provider_flow(
        &self,
        language_id: &str,
        flow: impl FnOnce(
            &mut dyn wright_lpp::LanguageProvider,
        ) -> crate::provider_edit::ProviderMutation,
    ) -> crate::provider_edit::ProviderMutation {
        self.session.run_provider_flow(
            language_id,
            &wright_lpp::ClientInfo {
                name: SERVICE_NAME.to_string(),
                version: SERVICE_VERSION.to_string(),
            },
            flow,
        )
    }

    fn ok(&self, result: serde_json::Value) -> ToolResponse {
        ToolResponse::Ok { result }
    }

    /// `findings`: every static-analysis finding with resolved span paths.
    ///
    /// The tool/agent surface resolves `span.path` exactly like the CLI
    /// `analyze`/`lint` workflows, so one file identity holds per finding
    /// across every surface (#102). A selection narrows the reported set
    /// (#430): without selection fields the result stays the bare finding
    /// array; with them it becomes `{"findings": [...], "selection": {...}}`
    /// so truncation is always visible.
    fn findings(&self, selection: &crate::select::FindingSelection) -> ToolResponse {
        if let Some(error) = self.selection_error(selection) {
            return error;
        }
        match self.semantic_query_with_resolved_span_paths(Request::GetFindings) {
            ToolResponse::Ok { result } => {
                let mut findings = match result {
                    serde_json::Value::Array(findings) => findings,
                    _ => Vec::new(),
                };
                let loaded = self.snapshot();
                crate::fix::attach_fixes(
                    &mut findings,
                    self.semantic().findings(),
                    loaded,
                    self.session.catalog(),
                    &CompilerSession::locale_for(loaded),
                );
                let (findings, outcome) = selection
                    .apply_findings(findings, &crate::select::file_bases(&self.snapshot().input));
                match outcome {
                    Some(outcome) => self.ok(json!({ "findings": findings, "selection": outcome })),
                    None => self.ok(serde_json::Value::Array(findings)),
                }
            }
            other => other,
        }
    }

    /// `persistentObjects`: persistent-object facts with resolved source paths.
    fn persistent_objects(&self) -> ToolResponse {
        self.semantic_query_with_resolved_span_paths(Request::GetPersistentObjects)
    }

    /// Resolve a symbol `Address` to its numeric id (#429). A name resolves
    /// through the shared semantic index; unmatched and ambiguous names are
    /// the analyzer's structured `unknown-symbol`/`ambiguous-symbol` errors.
    fn symbol_address(&self, address: &Address) -> Result<u32, ToolErrorInfo> {
        match address {
            Address::Id(id) => Ok(*id),
            Address::Name(name) => self
                .semantic()
                .resolve_symbol(name)
                .map(|symbol| symbol.id.index() as u32),
        }
    }

    /// Resolve a rule `Address` to its numeric index (#429).
    fn rule_address(&self, address: &Address) -> Result<u32, ToolErrorInfo> {
        match address {
            Address::Id(id) => Ok(*id),
            Address::Name(name) => self.semantic().resolve_rule(name).map(|rule| rule as u32),
        }
    }

    fn semantic_query_with_resolved_span_paths(&self, request: Request) -> ToolResponse {
        match self.semantic_query(request) {
            ToolResponse::Ok { mut result } => {
                crate::session::resolve_span_paths(&mut result, self.snapshot());
                ToolResponse::Ok { result }
            }
            other => other,
        }
    }

    /// Run one semantic query over the loaded program.
    fn semantic_query(&self, request: Request) -> ToolResponse {
        self.semantic().handle(&request)
    }

    fn configured_semantic(&self) -> &SemanticService<'static> {
        self.lint_semantic
            .as_ref()
            .unwrap_or_else(|| self.semantic())
    }

    /// A selection naming an unknown rule id is a usage error, never a
    /// silent empty result (#430).
    fn selection_error(&self, selection: &crate::select::FindingSelection) -> Option<ToolResponse> {
        selection
            .validate(self.session.lint_registry())
            .err()
            .map(invalid_selection)
    }

    /// Bound one list-shaped semantic operation's result (#531): without
    /// selection fields the previous bare array is returned unchanged;
    /// with them the result becomes `{<key>: [...], "selection": {...}}`
    /// reporting the pre-selection `total` and the count `max` withheld.
    fn select_result(
        &self,
        key: &str,
        result: serde_json::Value,
        file: Option<&str>,
        keep: impl Fn(&serde_json::Value) -> bool,
        max: Option<usize>,
        selected: bool,
    ) -> ToolResponse {
        let items = match result {
            serde_json::Value::Array(items) => items,
            _ => Vec::new(),
        };
        let (kept, outcome) = crate::select::select_list(
            items,
            file,
            &crate::select::file_bases(&self.snapshot().input),
            json_span_path,
            keep,
            max,
        );
        if !selected {
            return self.ok(serde_json::Value::Array(kept));
        }
        self.ok(json!({ key: kept, "selection": outcome }))
    }

    /// `rules`: every rule with resolved span paths, like `symbols` — a
    /// `file` id without its path is unusable to a caller. The selection
    /// fields are `name` (a declared rule name), `file`, and `max` (#531).
    fn rules(
        &self,
        name: &Option<String>,
        file: &Option<String>,
        max: &Option<usize>,
    ) -> ToolResponse {
        if let Some(name) = name {
            if !self
                .snapshot()
                .program
                .rules
                .iter()
                .any(|rule| rule.name == *name)
            {
                return invalid_selection(format!("unknown rule '{name}'"));
            }
        }
        let selected = name.is_some() || file.is_some() || max.is_some();
        let response = self.semantic_query_with_resolved_span_paths(Request::ListRules);
        match response {
            ToolResponse::Ok { result } => self.select_result(
                "rules",
                result,
                file.as_deref(),
                |rule| {
                    name.as_deref()
                        .is_none_or(|name| rule["name"].as_str() == Some(name))
                },
                *max,
                selected,
            ),
            other => other,
        }
    }

    /// `symbols`: every symbol with resolved span paths. `kind` predates
    /// #531 — a request carrying only `kind` keeps the previous bare-array
    /// shape, still filtered; the `file`/`max` selection fields wrap the
    /// result in `{"symbols": [...], "selection": {...}}` with the
    /// pre-selection total.
    fn symbols(&self, kind: Option<&str>, file: Option<&str>, max: Option<usize>) -> ToolResponse {
        if let Some(kind) = kind {
            if !SymbolKind::ALL.iter().any(|known| known.as_str() == kind) {
                return invalid_selection(format!(
                    "unknown symbol kind '{kind}' (expected one of: {})",
                    SymbolKind::ALL
                        .iter()
                        .map(|known| known.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        match self.semantic_query_with_resolved_span_paths(Request::ListSymbols { kind: None }) {
            ToolResponse::Ok { result } => self.select_result(
                "symbols",
                result,
                file,
                |symbol| kind.is_none_or(|kind| symbol["kind"].as_str() == Some(kind)),
                max,
                file.is_some() || max.is_some(),
            ),
            other => other,
        }
    }

    /// `references`: every reference to one symbol. The selection fields
    /// are `kind` (a reference kind), `rule` (a rule address narrowing to
    /// references inside that rule), `file`, and `max` (#531).
    fn references(
        &self,
        symbol: &Address,
        kind: Option<&str>,
        rule: Option<&Address>,
        file: Option<&str>,
        max: Option<usize>,
    ) -> ToolResponse {
        if let Some(kind) = kind {
            if !ReferenceKind::ALL
                .iter()
                .any(|known| known.as_str() == kind)
            {
                return invalid_selection(format!(
                    "unknown reference kind '{kind}' (expected one of: {})",
                    ReferenceKind::ALL
                        .iter()
                        .map(|known| known.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        let rule_id = match rule {
            Some(address) => match self.rule_address(address) {
                Ok(id) if (id as usize) < self.snapshot().program.rules.len() => Some(id),
                Ok(id) => return invalid_selection(format!("unknown rule {id}")),
                Err(error) => return invalid_selection(error.message),
            },
            None => None,
        };
        match self.symbol_address(symbol) {
            Ok(symbol) => {
                match self
                    .semantic_query_with_resolved_span_paths(Request::FindReferences { symbol })
                {
                    ToolResponse::Ok { result } => self.select_result(
                        "references",
                        result,
                        file,
                        |reference| {
                            kind.is_none_or(|kind| reference["kind"].as_str() == Some(kind))
                                && rule_id.is_none_or(|id| {
                                    reference["rule"].as_u64() == Some(u64::from(id))
                                })
                        },
                        max,
                        kind.is_some() || rule.is_some() || file.is_some() || max.is_some(),
                    ),
                    other => other,
                }
            }
            Err(error) => ToolResponse::Error { error },
        }
    }

    /// `cfg`: one rule's control-flow graph. The selection fields are
    /// `kind` (a block kind) and `max` (#531): they filter and bound
    /// `blocks` — which keep their original `id`s, so `successors` still
    /// address the full graph — and add a `selection` member with the
    /// pre-selection total.
    fn cfg(&self, rule: u32, kind: Option<&str>, max: Option<usize>) -> ToolResponse {
        if let Some(kind) = kind {
            if !BLOCK_KINDS.contains(&kind) {
                return invalid_selection(format!(
                    "unknown cfg block kind '{kind}' (expected one of: {})",
                    BLOCK_KINDS.join(", ")
                ));
            }
        }
        match self.semantic_query(Request::GetCfg { rule }) {
            ToolResponse::Ok { mut result } => {
                if kind.is_none() && max.is_none() {
                    return self.ok(result);
                }
                let blocks = result["blocks"].as_array().cloned().unwrap_or_default();
                let (kept, outcome) = crate::select::select_list(
                    blocks,
                    None,
                    &[],
                    |_| None,
                    |block| kind.is_none_or(|kind| block["kind"].as_str() == Some(kind)),
                    max,
                );
                result["blocks"] = serde_json::Value::Array(kept);
                result["selection"] = serde_json::to_value(outcome).expect("selection serializes");
                self.ok(result)
            }
            other => other,
        }
    }

    /// `callGraph`: caller rules → callee subroutines. The selection fields
    /// are `caller` (a declared rule name), `callee` (a declared subroutine
    /// name), and `max` (#531).
    fn call_graph(
        &self,
        caller: Option<&str>,
        callee: Option<&str>,
        max: Option<usize>,
    ) -> ToolResponse {
        let program = &self.snapshot().program;
        if let Some(caller) = caller {
            if !program.rules.iter().any(|rule| rule.name == caller) {
                return invalid_selection(format!("unknown caller rule '{caller}'"));
            }
        }
        if let Some(callee) = callee {
            if !program
                .subroutines
                .iter()
                .any(|subroutine| subroutine.name == callee)
            {
                return invalid_selection(format!("unknown callee subroutine '{callee}'"));
            }
        }
        let edges = program
            .rules
            .iter()
            .flat_map(|rule| {
                rule.actions.iter().filter_map(move |action| match action {
                    workshop_rs::Action::CallSubroutine { subroutine } => {
                        Some(json!({ "caller": rule.name, "callee": subroutine }))
                    }
                    _ => None,
                })
            })
            .collect::<Vec<_>>();
        self.select_result(
            "edges",
            serde_json::Value::Array(edges),
            None,
            |edge| {
                caller.is_none_or(|caller| edge["caller"].as_str() == Some(caller))
                    && callee.is_none_or(|callee| edge["callee"].as_str() == Some(callee))
            },
            max,
            caller.is_some() || callee.is_some() || max.is_some(),
        )
    }

    /// `lint`: per-rule id and effective severity, effective configuration,
    /// and findings over the loaded program through the same semantic-service
    /// path as the CLI `lint` workflow (no duplicated rule execution, #98).
    /// Full rule metadata is served once by `lintRules` rather than inlined
    /// into every `lint` response (#431). A `selection` member records the
    /// true total and withheld count when the request selected a subset
    /// (#430); a `brief` request returns the summary form over the selected
    /// set instead (#532).
    fn lint(&self, selection: &crate::select::FindingSelection, brief: bool) -> ToolResponse {
        if let Some(error) = self.selection_error(selection) {
            return error;
        }
        let service = self.configured_semantic();
        let lint_rules = match service.handle(&Request::LintRules) {
            Response::Ok { result } => result,
            Response::Error { .. } => serde_json::json!({}),
        };
        let mut findings = match service.handle(&Request::GetFindings) {
            Response::Ok { result } => result,
            Response::Error { .. } => serde_json::json!([]),
        };
        crate::session::resolve_span_paths(&mut findings, self.snapshot());
        let mut findings = match findings {
            serde_json::Value::Array(findings) => findings,
            _ => Vec::new(),
        };
        let loaded = self.snapshot();
        crate::fix::attach_fixes(
            &mut findings,
            service.findings(),
            loaded,
            self.session.catalog(),
            &CompilerSession::locale_for(loaded),
        );
        let (findings, outcome) =
            selection.apply_findings(findings, &crate::select::file_bases(&self.snapshot().input));
        let mut result = json!({
            "inputIdentity": self.snapshot().input.identity,
            "rules": crate::result::compact_lint_rules(lint_rules.get("rules")),
            "config": lint_rules.get("config").cloned().unwrap_or_else(|| json!({})),
            "findings": findings,
            "skipped": lint_rules.get("skipped").cloned().unwrap_or_else(|| json!([])),
        });
        if let Some(outcome) = outcome {
            result["selection"] = serde_json::to_value(outcome).expect("selection serializes");
        }
        if brief {
            result = crate::brief::lint(&result);
        }
        self.ok(result)
    }

    /// Program summary with origin and source identity.
    fn project(&self) -> serde_json::Value {
        let loaded = self.snapshot();
        let index = SemanticIndex::build(&loaded.program);
        let findings = wright_analyzer::canonical::analyze(
            &loaded.program,
            &wright_analyzer::registry::LintConfig::default(),
        );
        json!({
            "origin": { "kind": loaded.origin.kind, "locale": loaded.origin.locale },
            "inputIdentity": loaded.input.identity,
            "files": loaded.source_files.len().max(1),
            "globalVariables": loaded.program.global_variables.len(),
            "playerVariables": loaded.program.player_variables.len(),
            "subroutines": loaded.program.subroutines.len(),
            "rules": loaded.program.rules.len(),
            "symbols": index.symbols().count(),
            "findings": findings.len(),
        })
    }

    /// `costEstimate`: exact generated-resource counts plus static findings.
    /// The findings subset honors the shared selection (#430); findings carry
    /// no span, so a `file` selection matches nothing here.
    fn cost_estimate(&self, selection: &crate::select::FindingSelection) -> ToolResponse {
        if let Some(error) = self.selection_error(selection) {
            return error;
        }
        let loaded = self.snapshot();
        let locale = CompilerSession::locale_for(loaded);
        let text = workshop_rs::emitter::emit(&loaded.program, self.session.catalog(), &locale)
            .unwrap_or_default();
        let waits = loaded
            .program
            .rules
            .iter()
            .flat_map(|r| &r.actions)
            .filter(|a| matches!(a, workshop_rs::Action::Call { name, .. } if name == "wait"))
            .count();
        let findings = wright_analyzer::canonical::analyze(
            &loaded.program,
            &wright_analyzer::registry::LintConfig::default(),
        );
        let findings = findings
            .iter()
            .map(|f| {
                json!({
                    "code": f.code,
                    "severity": f.severity.as_str(),
                    "message": f.message,
                })
            })
            .collect::<Vec<_>>();
        let (findings, outcome) =
            selection.apply_findings(findings, &crate::select::file_bases(&loaded.input));
        let mut result = json!({
            "exact": {
                "emittedBytes": text.len(),
                "programActions": loaded.program.rules.iter().map(|r| r.actions.len()).sum::<usize>(),
                "programRules": loaded.program.rules.len(),
                "waitActions": waits,
            },
            "findings": findings,
            "kind": {
                "exact": "exact target-resource counts",
                "findings": "static/heuristic execution indicators",
                "performance": "compiler-host performance is measured by wright-bench, not in-process",
            },
        });
        if let Some(outcome) = outcome {
            result["selection"] = serde_json::to_value(outcome).expect("selection serializes");
        }
        self.ok(result)
    }

    fn target_metadata(&self) -> serde_json::Value {
        let catalog = self.session.catalog();
        json!({
            "catalogVersion": catalog.catalog_version(),
            "locales": catalog.locales().iter().map(|l| l.to_string()).collect::<Vec<_>>(),
            "actions": catalog.entries_of(workshop_rs::catalog::Kind::Action).count(),
            "values": catalog.entries_of(workshop_rs::catalog::Kind::Value).count(),
            "events": catalog.entries_of(workshop_rs::catalog::Kind::Event).count(),
            "operators": catalog.entries_of(workshop_rs::catalog::Kind::Operator).count(),
            "enumDomains": catalog.enum_domains().map(|domain| json!({
                "domain": domain.domain,
                "members": domain.members.iter().map(|m| m.member.clone()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }
}

/// An unknown selection/filter value is a structured usage error, never a
/// silent empty result (#430, #531).
fn invalid_selection(message: String) -> ToolResponse {
    ToolResponse::Error {
        error: ToolErrorInfo {
            code: "invalid-selection".to_string(),
            message,
        },
    }
}

/// The `span.path` a resolved semantic item carries, for `file` matching.
fn json_span_path(item: &serde_json::Value) -> Option<&str> {
    item.get("span")
        .and_then(|span| span.get("path"))
        .and_then(serde_json::Value::as_str)
}

/// The identity/version precondition view a defaulted `sources` mirrors
/// (#548): every document's own text under its URI.
fn document_sources(
    documents: &wright_lpp::DocumentSet,
) -> std::collections::BTreeMap<String, String> {
    documents
        .values()
        .map(|document| (document.uri.clone(), document.text.clone()))
        .collect()
}

/// A loaded project member as `(file:// URI, absolute disk path)` (#548): a
/// URI spelling keeps its identity and converts back to its path; a bare
/// path absolutizes against the session's input cwd and converts to a
/// `file://` URI. A member that is not a disk file refuses — the defaulted
/// document set covers only sources the session read from disk.
fn provider_member(
    member: &str,
    cwd: &std::path::Path,
) -> Result<(String, std::path::PathBuf), ToolErrorInfo> {
    // A Windows drive-absolute spelling (`C:\...`, `C:/...`) must be handled
    // before URI parsing: `url::Url::parse` accepts it as a URI whose scheme
    // is the drive letter, and `to_file_path` then refuses. The provider
    // emits these spellings on Windows hosts; treat them as disk paths on
    // every host so the refusal contract stays about URI kind, not host OS.
    if let Some(pair) = windows_drive_member(member) {
        return Ok(pair);
    }
    if let Ok(url) = url::Url::parse(member) {
        let path = url.to_file_path().map_err(|()| ToolErrorInfo {
            code: "provider-document-uri".to_string(),
            message: format!(
                "loaded project member '{member}' is not a disk file and cannot serve the \
                 provider document set"
            ),
        })?;
        return Ok((url.to_string(), path));
    }
    let path = {
        let path = std::path::PathBuf::from(member);
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    };
    url::Url::from_file_path(&path)
        .map(|url| (url.to_string(), path))
        .map_err(|()| ToolErrorInfo {
            code: "provider-document-uri".to_string(),
            message: format!(
                "loaded project member '{member}' cannot be expressed as a file:// URI"
            ),
        })
}

/// `C:\dir\file.opy` or `C:/dir/file.opy` → `(file:///C:/dir/file.opy, path)`.
/// The path is percent-encoded before parsing so literal `#`, `%`, `?`, and
/// friends are not reinterpreted as URL syntax — `Url::parse` alone would
/// treat `#` as a fragment delimiter and `%xx` as existing escapes.
fn windows_drive_member(member: &str) -> Option<(String, std::path::PathBuf)> {
    const PATH_ENCODE: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'#')
        .add(b'%')
        .add(b'<')
        .add(b'>')
        .add(b'?')
        .add(b'`')
        .add(b'{')
        .add(b'|')
        .add(b'}')
        .add(b'^');
    let bytes = member.as_bytes();
    let drive_absolute = bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    if !drive_absolute {
        return None;
    }
    let path = std::path::PathBuf::from(member);
    let normalized = member.replace('\\', "/");
    let encoded = percent_encoding::utf8_percent_encode(&normalized, PATH_ENCODE);
    let url = url::Url::parse(&format!("file:///{encoded}")).ok()?;
    Some((url.to_string(), path))
}

/// The hotpath measurement label for one request's dispatch.
#[cfg_attr(not(feature = "hotpath"), allow(dead_code))]
fn request_label(request: &ToolRequest) -> &'static str {
    match request {
        ToolRequest::Capabilities => "req::capabilities",
        ToolRequest::Compile => "req::compile",
        ToolRequest::Check => "req::check",
        ToolRequest::Analyze { .. } => "req::analyze",
        ToolRequest::Inspect { .. } => "req::inspect",
        ToolRequest::Project => "req::project",
        ToolRequest::Rules { .. } => "req::rules",
        ToolRequest::Symbols { .. } => "req::symbols",
        ToolRequest::References { .. } => "req::references",
        ToolRequest::Usage { .. } => "req::usage",
        ToolRequest::Cfg { .. } => "req::cfg",
        ToolRequest::Findings(_) => "req::findings",
        ToolRequest::PersistentObjects => "req::persistentObjects",
        ToolRequest::Lint { .. } => "req::lint",
        ToolRequest::LintRules => "req::lintRules",
        ToolRequest::CallGraph { .. } => "req::callGraph",
        ToolRequest::CostEstimate(_) => "req::costEstimate",
        ToolRequest::TargetMetadata => "req::targetMetadata",
        ToolRequest::ValidateEdit { .. } => "req::validateEdit",
        ToolRequest::SemanticRename { .. } => "req::semanticRename",
        ToolRequest::ProviderSemanticRename { .. } => "req::providerSemanticRename",
        ToolRequest::ProviderValidateEdit { .. } => "req::providerValidateEdit",
        ToolRequest::Lookup { .. } => "req::lookup",
    }
}

/// Whether a result payload's `diagnostics` include `code` (#471): the
/// `semanticRename` refusal surface embeds `ambiguous-symbol` this way.
fn diagnostics_list_code(result: &serde_json::Value, code: &str) -> bool {
    result
        .get("diagnostics")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|diagnostics| {
            diagnostics
                .iter()
                .any(|d| d.get("code").and_then(serde_json::Value::as_str) == Some(code))
        })
}

#[cfg(test)]
mod tests {
    use super::provider_member;
    use std::path::{Path, PathBuf};

    #[test]
    fn windows_drive_member_is_a_disk_path_not_a_uri_scheme() {
        // `url::Url::parse` accepts `C:\...` as a URI with scheme `c`; the
        // provider member helper must classify it as a filesystem path first.
        let (uri, path) = provider_member("C:\\project\\main.opy", Path::new("/cwd"))
            .expect("windows drive member resolves");
        assert_eq!(uri, "file:///C:/project/main.opy");
        assert_eq!(path, PathBuf::from("C:\\project\\main.opy"));

        let (uri, _) = provider_member("D:/work/lib.opy", Path::new("/cwd"))
            .expect("forward-slash drive member resolves");
        assert_eq!(uri, "file:///D:/work/lib.opy");

        // Literal `#`/`%`/`?` in the path are encoded, not parsed as URL
        // syntax, and decode back to the same spelling.
        for (member, uri) in [
            ("C:\\project#1\\main.opy", "file:///C:/project%231/main.opy"),
            (
                "C:\\project%20name\\main.opy",
                "file:///C:/project%2520name/main.opy",
            ),
        ] {
            let (produced, _) =
                provider_member(member, Path::new("/cwd")).expect("member resolves");
            assert_eq!(produced, uri);
            let url = url::Url::parse(&produced).expect("uri parses");
            let decoded = percent_encoding::percent_decode_str(url.path())
                .decode_utf8()
                .expect("utf8");
            assert_eq!(decoded, format!("/{}", member.replace('\\', "/")));
        }
    }

    #[test]
    fn file_uri_member_keeps_its_spelling() {
        let (uri, path) =
            provider_member("file:///project/main.opy", Path::new("/cwd")).expect("file URI");
        assert_eq!(uri, "file:///project/main.opy");
        assert_eq!(path, PathBuf::from("/project/main.opy"));
    }

    #[test]
    fn non_file_uri_member_refuses() {
        let error = provider_member("untitled:main.opy", Path::new("/cwd"))
            .expect_err("non-file scheme refuses");
        assert_eq!(error.code, "provider-document-uri");
    }

    #[test]
    fn relative_member_resolves_against_input_cwd() {
        let (uri, path) = provider_member("src/lib.opy", Path::new("/project"))
            .expect("relative member resolves");
        assert_eq!(path, PathBuf::from("/project/src/lib.opy"));
        assert_eq!(uri, "file:///project/src/lib.opy");
    }
}
